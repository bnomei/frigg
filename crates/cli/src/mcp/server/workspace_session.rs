//! Per-session workspace adoption, default-repository selection, watch lease refcounting, and
//! session-local `result_handle` storage for `read_match`.

use super::workspace::{WorkspaceAttachRollbackGuard, WorkspaceResolutionGuard};
use super::*;

pub(super) struct WorkspaceAttachTargetOutcome {
    pub(super) response: WorkspaceAttachResponse,
    pub(super) rollback_guard: Option<WorkspaceAttachRollbackGuard>,
}

type WorkspaceTargetResolution = Result<
    (
        AttachedWorkspace,
        Option<String>,
        Option<WorkspaceResolveMode>,
        Option<WorkspaceResolutionGuard>,
    ),
    ErrorData,
>;

impl FriggMcpServer {
    pub(super) fn clone_for_new_session(&self) -> Self {
        Self {
            config: Arc::clone(&self.config),
            tool_router: self.tool_router.clone(),
            tool_surface_profile: self.tool_surface_profile,
            runtime_state: self.runtime_state.clone(),
            session_state: FriggMcpSessionState::new(
                Arc::clone(&self.runtime_state.workspace_registry),
                self.runtime_state.watch_runtime.clone(),
            ),
            cache_state: self.cache_state.clone(),
        }
    }

    /// Builds the watch-runtime callback that invalidates all repository-scoped MCP cache families.
    ///
    /// The callback accepts either stable or runtime repository ids, normalizes to the canonical
    /// `repository_id`, and also clears runtime-id projection entries when both ids differ.
    pub fn repository_cache_invalidation_callback(
        &self,
    ) -> crate::watch::RepositoryCacheInvalidationCallback {
        let server = self.clone();
        Arc::new(move |repository_id: &str| {
            let original_repository_id = repository_id.to_owned();
            let workspace = server
                .runtime_state
                .workspace_registry
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .workspace_by_any_repository_id(repository_id);
            let repository_id = workspace
                .as_ref()
                .map(|workspace| workspace.repository_id.as_str())
                .unwrap_or(repository_id);
            server.invalidate_repository_symbol_corpus_cache(repository_id);
            server.invalidate_repository_response_freshness_cache(repository_id);
            server
                .runtime_state
                .searcher_projection_store_service
                .invalidate_repository(repository_id);
            if original_repository_id != repository_id {
                server
                    .runtime_state
                    .searcher_projection_store_service
                    .invalidate_repository(&original_repository_id);
            }
            server.invalidate_repository_precise_generator_probe_cache(repository_id);
            server.scip_invalidate_repository_precise_generation_cache(repository_id);
            server.invalidate_repository_precise_graph_caches(repository_id);
            server.invalidate_repository_navigation_caches(repository_id);
        })
    }

    pub fn set_watch_runtime(&self, watch_runtime: Option<Arc<crate::watch::WatchRuntime>>) {
        let mut state = self
            .runtime_state
            .watch_runtime
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *state = watch_runtime;
    }

    /// Resolves a workspace target from either a path or repository id and installs rollback guards.
    ///
    /// Relative paths may be resolved against the current session root, while path-based resolution
    /// creates a pending workspace guard so failed attach attempts do not leave ephemeral roots
    /// incorrectly pruned or adopted.
    pub(super) fn resolve_workspace_target(
        &self,
        path: Option<&str>,
        repository_id: Option<&str>,
        resolve_mode: WorkspaceResolveMode,
    ) -> WorkspaceTargetResolution {
        match (path, repository_id) {
            (Some(path), None) => {
                if path.trim().is_empty() {
                    return Err(Self::invalid_params(
                        "workspace_attach.path must not be empty",
                        None,
                    ));
                }
                let path = Path::new(path);
                if path.is_relative() && Self::relative_attach_path_has_parent(path) {
                    return Err(Self::access_denied(
                        "workspace_attach.path must not contain parent directory components",
                        Some(json!({ "path": path.display().to_string() })),
                    ));
                }
                let resolved_from = if path.is_relative() {
                    match self.effective_attach_directory_relative_to_session_root(path) {
                        Some(resolved_from) => resolved_from,
                        None => Self::effective_attach_directory(path)?,
                    }
                } else {
                    Self::effective_attach_directory(path)?
                };
                let (root, resolution) = match resolve_mode {
                    WorkspaceResolveMode::GitRoot => match Self::find_git_root(&resolved_from) {
                        Some(git_root) => (git_root, WorkspaceResolveMode::GitRoot),
                        None => (resolved_from.clone(), WorkspaceResolveMode::Direct),
                    },
                    WorkspaceResolveMode::Direct => {
                        (resolved_from.clone(), WorkspaceResolveMode::Direct)
                    }
                };
                self.authorize_attach_root(&root)?;
                let (workspace, resolution_guard) = {
                    let mut registry = self
                        .runtime_state
                        .workspace_registry
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    let (workspace, _) = registry.get_or_insert(root);
                    registry.mark_workspace_pending(&workspace.repository_id);
                    let resolution_guard = WorkspaceResolutionGuard::new(
                        Arc::clone(&self.runtime_state.workspace_registry),
                        workspace.repository_id.clone(),
                    );
                    (workspace, Some(resolution_guard))
                };
                Ok((
                    workspace,
                    Some(resolved_from.display().to_string()),
                    Some(resolution),
                    resolution_guard,
                ))
            }
            (None, Some(repository_id)) => {
                if !self.is_visible_repository_id(repository_id) {
                    return Err(Self::resource_not_found(
                        "repository_id not found",
                        Some(json!({
                            "repository_id": repository_id,
                            "hint": "Only startup repositories and repositories adopted by this session are visible by id.",
                        })),
                    ));
                }
                let workspace = self
                    .runtime_state
                    .workspace_registry
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .workspace_by_repository_id(repository_id)
                    .ok_or_else(|| {
                        Self::resource_not_found(
                            "repository_id not found",
                            Some(json!({ "repository_id": repository_id })),
                        )
                    })?;
                Ok((workspace, None, None, None))
            }
            (Some(_), Some(_)) => Err(Self::invalid_params(
                "workspace target must provide either `path` or `repository_id`, not both",
                None,
            )),
            (None, None) => Err(Self::invalid_params(
                "workspace target requires either `path` or `repository_id`",
                None,
            )),
        }
    }

    fn effective_attach_directory_relative_to_session_root(&self, path: &Path) -> Option<PathBuf> {
        let workspace = self.current_workspace().or_else(|| {
            let attached_workspaces = self.attached_workspaces();
            (attached_workspaces.len() == 1).then(|| attached_workspaces[0].clone())
        });
        let workspace = workspace?;

        Self::effective_attach_directory(&workspace.root.join(path)).ok()
    }

    pub(super) fn workspace_by_repository_id(
        &self,
        repository_id: &str,
    ) -> Option<AttachedWorkspace> {
        self.runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(repository_id)
    }

    fn latest_repository_precise_generation_summary(
        &self,
        repository_id: &str,
    ) -> Option<WorkspacePreciseGenerationSummary> {
        Self::precise_generator_specs()
            .into_iter()
            .filter_map(|spec| {
                self.scip_cached_workspace_precise_generation(repository_id, spec.generator_id)
            })
            .max_by_key(|summary| summary.generated_at_ms)
    }

    fn active_repository_precise_generation_task(
        &self,
        repository_id: &str,
    ) -> Option<RuntimeTaskSummary> {
        self.runtime_state
            .runtime_task_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_tasks()
            .into_iter()
            .find(|task| {
                task.kind == RuntimeTaskKind::PreciseGenerate && task.repository_id == repository_id
            })
    }

    pub(super) fn workspace_precise_lifecycle_summary(
        &self,
        workspace: &AttachedWorkspace,
        generation_action: WorkspacePreciseGenerationAction,
        precise: &WorkspacePreciseSummary,
        waited_for_completion: bool,
        timed_out: bool,
    ) -> WorkspacePreciseLifecycleSummary {
        let active_task = self.active_repository_precise_generation_task(&workspace.repository_id);
        let last_generation =
            self.latest_repository_precise_generation_summary(&workspace.repository_id);
        let active_task_phase = active_task.as_ref().map(|task| task.phase.clone());
        let failure_class = precise.failure_class.or_else(|| {
            last_generation
                .as_ref()
                .and_then(|summary| summary.failure_class)
        });
        let failure_summary = precise.failure_summary.clone().or_else(|| {
            last_generation
                .as_ref()
                .and_then(|summary| summary.detail.clone())
        });
        let recommended_action = precise.recommended_action.or_else(|| {
            last_generation
                .as_ref()
                .and_then(|summary| summary.recommended_action)
        });
        let phase = if timed_out {
            WorkspacePreciseLifecyclePhase::Timeout
        } else if let Some(_task) = active_task.as_ref() {
            WorkspacePreciseLifecyclePhase::Running
        } else if let Some(summary) = last_generation.as_ref() {
            match summary.status {
                WorkspacePreciseGenerationStatus::Succeeded => {
                    WorkspacePreciseLifecyclePhase::Succeeded
                }
                WorkspacePreciseGenerationStatus::Failed
                | WorkspacePreciseGenerationStatus::Timeout => {
                    WorkspacePreciseLifecyclePhase::Failed
                }
                WorkspacePreciseGenerationStatus::MissingTool
                | WorkspacePreciseGenerationStatus::Unsupported => {
                    WorkspacePreciseLifecyclePhase::Unavailable
                }
                WorkspacePreciseGenerationStatus::NotConfigured
                | WorkspacePreciseGenerationStatus::Skipped => {
                    WorkspacePreciseLifecyclePhase::Skipped
                }
            }
        } else {
            match generation_action {
                WorkspacePreciseGenerationAction::Triggered => {
                    WorkspacePreciseLifecyclePhase::NotStarted
                }
                WorkspacePreciseGenerationAction::SkippedActiveTask => {
                    WorkspacePreciseLifecyclePhase::Running
                }
                WorkspacePreciseGenerationAction::SkippedNoWork
                | WorkspacePreciseGenerationAction::NotApplicable => {
                    WorkspacePreciseLifecyclePhase::Skipped
                }
            }
        };
        WorkspacePreciseLifecycleSummary {
            phase,
            waited_for_completion,
            generation_action,
            last_generation,
            active_task_phase,
            active_task,
            failure_class,
            failure_summary,
            recommended_action,
        }
    }

    pub(super) async fn wait_for_repository_precise_generation(
        &self,
        repository_id: &str,
        timeout: Duration,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let active = self
                .runtime_state
                .runtime_task_registry
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .has_active_task_for_repository(RuntimeTaskKind::PreciseGenerate, repository_id);
            if !active {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub(super) fn invalidate_workspace_index_runtime_caches(
        &self,
        workspace: &AttachedWorkspace,
        invalidate_precise_generation: bool,
    ) {
        self.runtime_state
            .validated_manifest_candidate_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .invalidate_root(&workspace.root);
        self.runtime_state
            .searcher_projection_store_service
            .invalidate_repository(&workspace.repository_id);
        if workspace.runtime_repository_id != workspace.repository_id {
            self.runtime_state
                .searcher_projection_store_service
                .invalidate_repository(&workspace.runtime_repository_id);
        }
        self.invalidate_repository_symbol_corpus_cache(&workspace.repository_id);
        self.invalidate_repository_response_freshness_cache(&workspace.repository_id);
        self.invalidate_repository_precise_generator_probe_cache(&workspace.repository_id);
        if invalidate_precise_generation {
            self.scip_invalidate_repository_precise_generation_cache(&workspace.repository_id);
        }
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        self.invalidate_repository_navigation_caches(&workspace.repository_id);
    }

    pub(in crate::mcp::server) fn active_repository_index_tasks(
        &self,
        repository_id: &str,
    ) -> Vec<RuntimeTaskSummary> {
        let workspace = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(repository_id);
        self.runtime_state
            .runtime_task_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_tasks()
            .into_iter()
            .filter(|task| {
                matches!(
                    task.kind,
                    RuntimeTaskKind::ChangedIndex
                        | RuntimeTaskKind::SemanticRefresh
                        | RuntimeTaskKind::WorkspacePrepare
                        | RuntimeTaskKind::WorkspaceIndex
                ) && (task.repository_id == repository_id
                    || workspace.as_ref().is_some_and(|workspace| {
                        workspace.runtime_repository_id != repository_id
                            && task.repository_id == workspace.runtime_repository_id
                    }))
            })
            .collect()
    }

    pub(in crate::mcp::server) fn runtime_task_repository_aliases(
        workspace: &AttachedWorkspace,
    ) -> Vec<&str> {
        if workspace.runtime_repository_id == workspace.repository_id {
            vec![workspace.repository_id.as_str()]
        } else {
            vec![
                workspace.repository_id.as_str(),
                workspace.runtime_repository_id.as_str(),
            ]
        }
    }

    pub(in crate::mcp::server) fn runtime_index_task_kinds() -> &'static [RuntimeTaskKind] {
        &[
            RuntimeTaskKind::ChangedIndex,
            RuntimeTaskKind::SemanticRefresh,
            RuntimeTaskKind::WorkspacePrepare,
            RuntimeTaskKind::WorkspaceIndex,
        ]
    }

    pub(in crate::mcp::server) fn try_start_repository_runtime_task(
        &self,
        workspace: &AttachedWorkspace,
        kind: RuntimeTaskKind,
        phase: impl Into<String>,
        detail: Option<String>,
    ) -> Result<RuntimeTaskGuard, Vec<RuntimeTaskSummary>> {
        RuntimeTaskGuard::try_start_if_no_active_for_any_repository(
            Arc::clone(&self.runtime_state.runtime_task_registry),
            Self::runtime_index_task_kinds(),
            &Self::runtime_task_repository_aliases(workspace),
            kind,
            workspace.repository_id.clone(),
            phase,
            detail,
        )
    }

    async fn wait_for_repository_index_work(&self, repository_id: &str, timeout: Duration) -> bool {
        let now = tokio::time::Instant::now();
        let deadline = now
            .checked_add(timeout)
            .unwrap_or_else(|| now + Duration::from_secs(60 * 60));
        loop {
            if self.active_repository_index_tasks(repository_id).is_empty() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    pub(super) fn workspace_index_readiness(
        &self,
        workspace: &AttachedWorkspace,
    ) -> (bool, bool, Option<String>) {
        let mut manifest_only_runtime = self.config.semantic_runtime.clone();
        manifest_only_runtime.enabled = false;
        let lexical_ready =
            match self.workspace_repository_freshness_status(workspace, &manifest_only_runtime) {
                Ok(status) => {
                    matches!(status.manifest, RepositoryManifestFreshness::Ready)
                        && !self.workspace_has_dirty_root(workspace)
                }
                Err(err) => return (false, false, Some(err)),
            };

        if !self.config.semantic_runtime.enabled {
            return (lexical_ready, true, None);
        }

        match self.workspace_repository_freshness_status(workspace, &self.config.semantic_runtime) {
            Ok(status) => {
                let semantic_ready = matches!(
                    status.semantic,
                    RepositorySemanticFreshness::Disabled
                        | RepositorySemanticFreshness::NoEligibleEntries
                        | RepositorySemanticFreshness::Ready
                );
                (lexical_ready, semantic_ready, None)
            }
            Err(err) => (lexical_ready, false, Some(err)),
        }
    }

    pub(super) fn workspace_index_lifecycle_summary(
        &self,
        workspace: &AttachedWorkspace,
        mode: WorkspaceAttachIndexMode,
        waited_for_completion: bool,
        timed_out: bool,
        action_taken: WorkspaceIndexAction,
        failure_summary: Option<String>,
    ) -> WorkspaceIndexLifecycleSummary {
        let (lexical_ready, semantic_ready, readiness_error) =
            self.workspace_index_readiness(workspace);
        let active_tasks = self.active_repository_index_tasks(&workspace.repository_id);
        let failure_summary = failure_summary.or(readiness_error);
        let phase = if lexical_ready && semantic_ready {
            WorkspaceIndexLifecyclePhase::Ready
        } else if timed_out {
            WorkspaceIndexLifecyclePhase::Timeout
        } else if failure_summary.is_some() || matches!(action_taken, WorkspaceIndexAction::Failed)
        {
            WorkspaceIndexLifecyclePhase::Failed
        } else if !active_tasks.is_empty()
            || matches!(action_taken, WorkspaceIndexAction::SkippedActiveTask)
        {
            WorkspaceIndexLifecyclePhase::Refreshing
        } else if matches!(action_taken, WorkspaceIndexAction::Queued) {
            WorkspaceIndexLifecyclePhase::RefreshQueued
        } else if matches!(action_taken, WorkspaceIndexAction::Unavailable) {
            WorkspaceIndexLifecyclePhase::Unavailable
        } else if matches!(action_taken, WorkspaceIndexAction::SkippedNoWork) {
            WorkspaceIndexLifecyclePhase::Stale
        } else {
            WorkspaceIndexLifecyclePhase::Skipped
        };
        let recommended_action = (!lexical_ready || !semantic_ready).then_some(match phase {
            WorkspaceIndexLifecyclePhase::Failed
            | WorkspaceIndexLifecyclePhase::Skipped
            | WorkspaceIndexLifecyclePhase::Stale
            | WorkspaceIndexLifecyclePhase::Timeout
            | WorkspaceIndexLifecyclePhase::Unavailable => WorkspaceRecommendedAction::RerunIndex,
            WorkspaceIndexLifecyclePhase::Ready
            | WorkspaceIndexLifecyclePhase::Refreshing
            | WorkspaceIndexLifecyclePhase::RefreshQueued => {
                WorkspaceRecommendedAction::UseHeuristicMode
            }
        });
        WorkspaceIndexLifecycleSummary {
            phase,
            mode,
            waited_for_completion,
            timed_out,
            action_taken,
            lexical_ready,
            semantic_ready,
            active_tasks,
            failure_summary,
            recommended_action,
        }
    }

    fn spawn_workspace_attach_index_refresh(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Result<
        tokio::task::JoinHandle<Result<crate::indexer::IndexSummary, String>>,
        Vec<RuntimeTaskSummary>,
    > {
        let task_guard = self.try_start_repository_runtime_task(
            workspace,
            RuntimeTaskKind::WorkspaceIndex,
            "attach_index_ensure",
            Some(format!("attach index ensure {}", workspace.root.display())),
        )?;
        let server = self.clone();
        let workspace = workspace.clone();
        let semantic_runtime = self.config.semantic_runtime.clone();
        Ok(tokio::task::spawn_blocking(move || {
            let result = (|| -> Result<crate::indexer::IndexSummary, String> {
                let db_path = ensure_provenance_db_parent_dir(&workspace.root)
                    .map_err(|err| err.to_string())?;
                let credentials = SemanticRuntimeCredentials::from_process_env();
                index_repository_with_runtime_config(
                    &workspace.runtime_repository_id,
                    &workspace.root,
                    &db_path,
                    IndexMode::ChangedOnly,
                    &semantic_runtime,
                    &credentials,
                )
                .map_err(|err| err.to_string())
            })();
            server.invalidate_workspace_index_runtime_caches(&workspace, true);
            let (status, detail) = match &result {
                Ok(_) => (RuntimeTaskStatus::Succeeded, None),
                Err(err) => (RuntimeTaskStatus::Failed, Some(err.clone())),
            };
            let mut task_guard = task_guard;
            task_guard.finish(status, detail);
            result
        }))
    }

    pub(super) async fn ensure_workspace_index_for_attach(
        &self,
        workspace: &AttachedWorkspace,
        wait_for_index: bool,
        timeout: Duration,
    ) -> (
        WorkspaceIndexLifecycleSummary,
        Option<crate::indexer::IndexSummary>,
    ) {
        let started_at = tokio::time::Instant::now();
        let (lexical_ready, semantic_ready, readiness_error) =
            self.workspace_index_readiness(workspace);
        if lexical_ready && semantic_ready {
            return (
                self.workspace_index_lifecycle_summary(
                    workspace,
                    WorkspaceAttachIndexMode::Ensure,
                    wait_for_index,
                    false,
                    WorkspaceIndexAction::SkippedNoWork,
                    None,
                ),
                None,
            );
        }
        if let Some(error) = readiness_error {
            return (
                self.workspace_index_lifecycle_summary(
                    workspace,
                    WorkspaceAttachIndexMode::Ensure,
                    wait_for_index,
                    false,
                    WorkspaceIndexAction::Failed,
                    Some(error),
                ),
                None,
            );
        }
        if self.repository_has_active_runtime_work(&workspace.repository_id) {
            if !wait_for_index {
                return (
                    self.workspace_index_lifecycle_summary(
                        workspace,
                        WorkspaceAttachIndexMode::Ensure,
                        false,
                        false,
                        WorkspaceIndexAction::SkippedActiveTask,
                        None,
                    ),
                    None,
                );
            }
            let remaining_timeout = timeout
                .checked_sub(started_at.elapsed())
                .unwrap_or(Duration::ZERO);
            if remaining_timeout.is_zero() {
                return (
                    self.workspace_index_lifecycle_summary(
                        workspace,
                        WorkspaceAttachIndexMode::Ensure,
                        true,
                        true,
                        WorkspaceIndexAction::SkippedActiveTask,
                        None,
                    ),
                    None,
                );
            }
            if !self
                .wait_for_repository_index_work(&workspace.repository_id, remaining_timeout)
                .await
            {
                return (
                    self.workspace_index_lifecycle_summary(
                        workspace,
                        WorkspaceAttachIndexMode::Ensure,
                        true,
                        true,
                        WorkspaceIndexAction::SkippedActiveTask,
                        None,
                    ),
                    None,
                );
            }
            let (lexical_ready, semantic_ready, readiness_error) =
                self.workspace_index_readiness(workspace);
            if lexical_ready && semantic_ready {
                return (
                    self.workspace_index_lifecycle_summary(
                        workspace,
                        WorkspaceAttachIndexMode::Ensure,
                        true,
                        false,
                        WorkspaceIndexAction::SkippedActiveTask,
                        None,
                    ),
                    None,
                );
            }
            if let Some(error) = readiness_error {
                return (
                    self.workspace_index_lifecycle_summary(
                        workspace,
                        WorkspaceAttachIndexMode::Ensure,
                        true,
                        false,
                        WorkspaceIndexAction::Failed,
                        Some(error),
                    ),
                    None,
                );
            }
        }

        let handle = loop {
            match self.spawn_workspace_attach_index_refresh(workspace) {
                Ok(handle) => break handle,
                Err(_) if !wait_for_index => {
                    return (
                        self.workspace_index_lifecycle_summary(
                            workspace,
                            WorkspaceAttachIndexMode::Ensure,
                            false,
                            false,
                            WorkspaceIndexAction::SkippedActiveTask,
                            None,
                        ),
                        None,
                    );
                }
                Err(_) => {
                    let remaining_timeout = timeout
                        .checked_sub(started_at.elapsed())
                        .unwrap_or(Duration::ZERO);
                    if remaining_timeout.is_zero()
                        || !self
                            .wait_for_repository_index_work(
                                &workspace.repository_id,
                                remaining_timeout,
                            )
                            .await
                    {
                        return (
                            self.workspace_index_lifecycle_summary(
                                workspace,
                                WorkspaceAttachIndexMode::Ensure,
                                true,
                                true,
                                WorkspaceIndexAction::SkippedActiveTask,
                                None,
                            ),
                            None,
                        );
                    }
                    let (lexical_ready, semantic_ready, readiness_error) =
                        self.workspace_index_readiness(workspace);
                    if lexical_ready && semantic_ready {
                        return (
                            self.workspace_index_lifecycle_summary(
                                workspace,
                                WorkspaceAttachIndexMode::Ensure,
                                true,
                                false,
                                WorkspaceIndexAction::SkippedActiveTask,
                                None,
                            ),
                            None,
                        );
                    }
                    if let Some(error) = readiness_error {
                        return (
                            self.workspace_index_lifecycle_summary(
                                workspace,
                                WorkspaceAttachIndexMode::Ensure,
                                true,
                                false,
                                WorkspaceIndexAction::Failed,
                                Some(error),
                            ),
                            None,
                        );
                    }
                }
            }
        };
        if !wait_for_index {
            return (
                self.workspace_index_lifecycle_summary(
                    workspace,
                    WorkspaceAttachIndexMode::Ensure,
                    false,
                    false,
                    WorkspaceIndexAction::Queued,
                    None,
                ),
                None,
            );
        }

        let remaining_timeout = timeout
            .checked_sub(started_at.elapsed())
            .unwrap_or(Duration::ZERO);
        if remaining_timeout.is_zero() {
            return (
                self.workspace_index_lifecycle_summary(
                    workspace,
                    WorkspaceAttachIndexMode::Ensure,
                    true,
                    true,
                    WorkspaceIndexAction::SkippedActiveTask,
                    None,
                ),
                None,
            );
        }

        match tokio::time::timeout(remaining_timeout, handle).await {
            Ok(Ok(Ok(summary))) => {
                let (lexical_ready, semantic_ready, readiness_error) =
                    self.workspace_index_readiness(workspace);
                let failure_summary = if lexical_ready && semantic_ready {
                    None
                } else {
                    readiness_error.or_else(|| {
                        Some(
                            "attach index refresh completed but index freshness is not ready"
                                .to_owned(),
                        )
                    })
                };
                (
                    self.workspace_index_lifecycle_summary(
                        workspace,
                        WorkspaceAttachIndexMode::Ensure,
                        true,
                        false,
                        if failure_summary.is_some() {
                            WorkspaceIndexAction::Failed
                        } else {
                            WorkspaceIndexAction::Refreshed
                        },
                        failure_summary,
                    ),
                    Some(summary),
                )
            }
            Ok(Ok(Err(err))) => (
                self.workspace_index_lifecycle_summary(
                    workspace,
                    WorkspaceAttachIndexMode::Ensure,
                    true,
                    false,
                    WorkspaceIndexAction::Failed,
                    Some(err),
                ),
                None,
            ),
            Ok(Err(err)) => (
                self.workspace_index_lifecycle_summary(
                    workspace,
                    WorkspaceAttachIndexMode::Ensure,
                    true,
                    false,
                    WorkspaceIndexAction::Failed,
                    Some(format!("attach index refresh task join failure: {err}")),
                ),
                None,
            ),
            Err(_) => (
                self.workspace_index_lifecycle_summary(
                    workspace,
                    WorkspaceAttachIndexMode::Ensure,
                    true,
                    true,
                    WorkspaceIndexAction::SkippedActiveTask,
                    None,
                ),
                None,
            ),
        }
    }

    pub(super) fn attach_workspace_target_internal(
        &self,
        path: Option<&str>,
        repository_id: Option<&str>,
        set_default: bool,
        resolve_mode: WorkspaceResolveMode,
        index_mode: WorkspaceAttachIndexMode,
    ) -> Result<WorkspaceAttachTargetOutcome, ErrorData> {
        let (workspace, resolved_from, resolution, _resolution_guard) =
            self.resolve_workspace_target(path, repository_id, resolve_mode)?;
        let previous_default_repository_id = self.current_repository_id();
        let mut rollback_guard = self.workspace_attach_path_rollback_guard(
            path,
            previous_default_repository_id,
            &workspace,
            set_default,
        );

        let adoption = self.adopt_workspace(&workspace, set_default)?;
        if adoption.newly_adopted
            && let Some(guard) = rollback_guard.as_mut()
        {
            guard.mark_created_adoption();
        }

        self.invalidate_workspace_index_runtime_caches(&workspace, false);

        let mut repository = self.public_repository_summary(&workspace);
        let storage = repository
            .storage
            .clone()
            .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
        repository.storage = None;
        let index_action = match index_mode {
            WorkspaceAttachIndexMode::Ensure => WorkspaceIndexAction::SkippedNoWork,
            WorkspaceAttachIndexMode::Defer => {
                self.maybe_spawn_workspace_runtime_prewarm(&workspace);
                WorkspaceIndexAction::Queued
            }
        };
        let (precise_generation_action, precise) =
            if matches!(index_mode, WorkspaceAttachIndexMode::Ensure) {
                (
                    WorkspacePreciseGenerationAction::NotApplicable,
                    WorkspacePreciseSummary {
                        state: WorkspacePreciseState::Unavailable,
                        failure_tool: None,
                        failure_class: None,
                        failure_summary: None,
                        recommended_action: None,
                        generation_action: Some(WorkspacePreciseGenerationAction::NotApplicable),
                    },
                )
            } else {
                let generation_action =
                    self.maybe_spawn_workspace_precise_generation_for_paths(&workspace, &[], &[]);
                let precise = self
                    .workspace_precise_summary_for_workspace(&workspace, Some(generation_action));
                (generation_action, precise)
            };
        let precise_lifecycle = self.workspace_precise_lifecycle_summary(
            &workspace,
            precise_generation_action,
            &precise,
            false,
            false,
        );

        Ok(WorkspaceAttachTargetOutcome {
            rollback_guard,
            response: WorkspaceAttachResponse {
                repository,
                resolved_from: resolved_from
                    .unwrap_or_else(|| workspace.root.display().to_string()),
                resolution: resolution.unwrap_or(WorkspaceResolveMode::Direct),
                session_default: self.current_repository_id().as_deref()
                    == Some(workspace.repository_id.as_str()),
                storage,
                action: if adoption.newly_adopted {
                    WorkspaceAttachAction::AttachedFresh
                } else {
                    WorkspaceAttachAction::ReusedWorkspace
                },
                precise,
                precise_lifecycle,
                index_lifecycle: self.workspace_index_lifecycle_summary(
                    &workspace,
                    index_mode,
                    false,
                    false,
                    index_action,
                    None,
                ),
            },
        })
    }

    #[cfg(test)]
    pub(super) fn attach_workspace_internal(
        &self,
        path: &Path,
        set_default: bool,
        resolve_mode: WorkspaceResolveMode,
    ) -> Result<WorkspaceAttachResponse, ErrorData> {
        let owned_path = path.display().to_string();
        self.attach_workspace_target_internal(
            Some(&owned_path),
            None,
            set_default,
            resolve_mode,
            WorkspaceAttachIndexMode::Defer,
        )
        .map(|outcome| {
            if let Some(guard) = outcome.rollback_guard {
                guard.disarm();
            }
            outcome.response
        })
    }

    pub(super) fn repository_has_active_runtime_work(&self, repository_id: &str) -> bool {
        let workspace = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(repository_id);
        let registry = self
            .runtime_state
            .runtime_task_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Self::runtime_index_task_kinds().iter().any(|kind| {
            registry.has_active_task_for_repository(*kind, repository_id)
                || workspace.as_ref().is_some_and(|workspace| {
                    workspace.runtime_repository_id != repository_id
                        && registry
                            .has_active_task_for_repository(*kind, &workspace.runtime_repository_id)
                })
        })
    }

    pub(super) fn repository_has_active_watch_lease(&self, repository_id: &str) -> bool {
        let workspace = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_any_repository_id(repository_id);
        workspace
            .as_ref()
            .and_then(|workspace| {
                self.runtime_state
                    .watch_runtime
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .as_ref()
                    .map(|runtime| {
                        runtime
                            .lease_status(&workspace.runtime_repository_id)
                            .active
                    })
            })
            .unwrap_or(false)
    }

    pub(super) fn scoped_search_config(
        &self,
        scoped_workspaces: &[AttachedWorkspace],
    ) -> (FriggConfig, Vec<String>, BTreeMap<String, String>) {
        let scoped_config = FriggConfig {
            workspace_roots: scoped_workspaces
                .iter()
                .map(|workspace| workspace.root.clone())
                .collect(),
            ..(*self.config).clone()
        };
        let runtime_repository_ids = scoped_workspaces
            .iter()
            .map(|workspace| workspace.runtime_repository_id.clone())
            .collect::<Vec<_>>();
        let mut repository_id_map = BTreeMap::new();
        for (temporary, actual) in scoped_config
            .repositories()
            .into_iter()
            .zip(scoped_workspaces.iter())
        {
            repository_id_map.insert(temporary.repository_id.0, actual.repository_id.clone());
            repository_id_map.insert(
                actual.runtime_repository_id.clone(),
                actual.repository_id.clone(),
            );
        }
        (scoped_config, runtime_repository_ids, repository_id_map)
    }

    pub(super) fn canonicalize_existing_ancestor(
        path: &Path,
    ) -> Result<Option<PathBuf>, ErrorData> {
        for ancestor in path.ancestors() {
            match ancestor.canonicalize() {
                Ok(canonical) => return Ok(Some(canonical)),
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    return Err(Self::internal(
                        format!(
                            "failed to canonicalize ancestor {}: {err}",
                            ancestor.display()
                        ),
                        None,
                    ));
                }
            }
        }

        Ok(None)
    }

    pub(super) fn candidate_within_root(
        candidate: &Path,
        root_canonical: &Path,
    ) -> Result<bool, ErrorData> {
        let Some(ancestor) = Self::canonicalize_existing_ancestor(candidate)? else {
            return Ok(false);
        };

        Ok(ancestor.starts_with(root_canonical))
    }

    pub(super) fn resolve_file_path(
        &self,
        params: &ReadFileParams,
    ) -> Result<(String, PathBuf, String), ErrorData> {
        let requested = PathBuf::from(&params.path);
        let roots = if requested.is_absolute() && params.repository_id.is_none() {
            self.attached_workspaces()
                .into_iter()
                .map(|workspace| (workspace.repository_id, workspace.root))
                .collect::<Vec<_>>()
        } else {
            self.roots_for_repository(params.repository_id.as_deref())?
        }
        .into_iter()
        .map(|(repository_id, root)| {
            let root_canonical = root.canonicalize().map_err(|err| {
                Self::internal(
                    format!("failed to canonicalize root {}: {err}", root.display()),
                    None,
                )
            })?;
            Ok((repository_id, root_canonical))
        })
        .collect::<Result<Vec<_>, ErrorData>>()?;

        let mut saw_workspace_candidate = false;
        let mut matches = Vec::new();

        for (repository_id, root_canonical) in &roots {
            let candidate = if requested.is_absolute() {
                requested.clone()
            } else {
                root_canonical.join(&requested)
            };

            match candidate.canonicalize() {
                Ok(candidate_canonical) => {
                    if !candidate_canonical.starts_with(root_canonical) {
                        continue;
                    }
                    saw_workspace_candidate = true;

                    let metadata = fs::metadata(&candidate_canonical).map_err(|err| {
                        Self::internal(
                            format!(
                                "failed to stat file {}: {err}",
                                candidate_canonical.display()
                            ),
                            None,
                        )
                    })?;
                    if metadata.is_file() {
                        let display_path =
                            Self::relative_display_path(root_canonical, &candidate_canonical);
                        matches.push((repository_id.clone(), candidate_canonical, display_path));
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if Self::candidate_within_root(&candidate, root_canonical)? {
                        saw_workspace_candidate = true;
                    }
                }
                Err(err) => {
                    return Err(Self::internal(
                        format!("failed to canonicalize file {}: {err}", candidate.display()),
                        None,
                    ));
                }
            }
        }

        if matches.len() > 1 && !requested.is_absolute() && params.repository_id.is_none() {
            return Err(Self::invalid_params(
                "relative path is ambiguous across adopted repositories; pass repository_id",
                Some(serde_json::json!({
                    "path": params.path,
                    "repository_ids": matches
                        .iter()
                        .map(|(repository_id, _, _)| repository_id)
                        .collect::<Vec<_>>(),
                })),
            ));
        }
        if let Some(resolved) = matches.into_iter().next() {
            return Ok(resolved);
        }

        if saw_workspace_candidate {
            return Err(Self::resource_not_found(
                "file not found",
                Some(serde_json::json!({ "path": params.path })),
            ));
        }

        Err(Self::access_denied(
            "path is outside workspace roots",
            Some(serde_json::json!({ "path": params.path })),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "frigg-scoped-search-config-{test_name}-{nonce}-{}",
            std::process::id()
        ))
    }

    #[test]
    fn scoped_search_config_uses_runtime_ids_for_search_and_public_ids_for_results() {
        let root = temp_workspace_root("runtime-id");
        fs::create_dir_all(&root).expect("scoped search fixture root should be creatable");
        let public_repository_id = crate::domain::model::stable_repository_id_for_root(&root).0;
        let runtime_repository_id = format!("{public_repository_id}-runtime");
        let workspace = AttachedWorkspace {
            repository_id: public_repository_id.clone(),
            runtime_repository_id: runtime_repository_id.clone(),
            display_name: "runtime-id".to_owned(),
            root: root.clone(),
            db_path: root.join(".frigg/storage.sqlite3"),
        };
        let config = FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid");
        let server = FriggMcpServer::new_with_runtime_options(config, false);

        let (scoped_config, runtime_repository_ids, repository_id_map) =
            server.scoped_search_config(std::slice::from_ref(&workspace));

        assert_eq!(scoped_config.repositories()[0].repository_id.0, "repo-001");
        assert_eq!(runtime_repository_ids, vec![runtime_repository_id.clone()]);
        assert_eq!(
            repository_id_map.get("repo-001"),
            Some(&public_repository_id)
        );
        assert_eq!(
            repository_id_map.get(&runtime_repository_id),
            Some(&public_repository_id)
        );

        let searcher =
            server.runtime_text_searcher_with_repository_ids(scoped_config, runtime_repository_ids);
        assert_eq!(
            searcher.repositories()[0].repository_id.0,
            runtime_repository_id
        );

        let _ = fs::remove_dir_all(root);
    }
}
