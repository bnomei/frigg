use super::*;

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
            provenance_state: self.provenance_state.clone(),
        }
    }

    pub fn repository_cache_invalidation_callback(
        &self,
    ) -> crate::watch::RepositoryCacheInvalidationCallback {
        let server = self.clone();
        Arc::new(move |repository_id: &str| {
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
            server.invalidate_repository_summary_cache(repository_id);
            server.invalidate_repository_response_freshness_cache(repository_id);
            server.invalidate_repository_file_content_cache(repository_id);
            server
                .runtime_state
                .searcher_projection_store_service
                .invalidate_repository(repository_id);
            server.invalidate_repository_precise_generator_probe_cache(repository_id);
            server.scip_invalidate_repository_precise_generation_cache(repository_id);
            server.invalidate_repository_precise_graph_caches(repository_id);
            server.invalidate_repository_search_response_caches(repository_id);
            server.invalidate_repository_navigation_response_caches(repository_id);
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

    pub(super) fn resolve_workspace_target(
        &self,
        path: Option<&str>,
        repository_id: Option<&str>,
        resolve_mode: WorkspaceResolveMode,
    ) -> Result<
        (
            AttachedWorkspace,
            Option<String>,
            Option<WorkspaceResolveMode>,
        ),
        ErrorData,
    > {
        match (path, repository_id) {
            (Some(path), None) => {
                if path.trim().is_empty() {
                    return Err(Self::invalid_params(
                        "workspace_attach.path must not be empty",
                        None,
                    ));
                }
                let path = Path::new(path);
                let resolved_from = Self::effective_attach_directory(path)?;
                let (root, resolution) = match resolve_mode {
                    WorkspaceResolveMode::GitRoot => match Self::find_git_root(&resolved_from) {
                        Some(git_root) => (git_root, WorkspaceResolveMode::GitRoot),
                        None => (resolved_from.clone(), WorkspaceResolveMode::Direct),
                    },
                    WorkspaceResolveMode::Direct => {
                        (resolved_from.clone(), WorkspaceResolveMode::Direct)
                    }
                };
                let workspace = {
                    let mut registry = self
                        .runtime_state
                        .workspace_registry
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    registry.get_or_insert(root)
                };
                Ok((
                    workspace,
                    Some(resolved_from.display().to_string()),
                    Some(resolution),
                ))
            }
            (None, Some(repository_id)) => {
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
                Ok((workspace, None, None))
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

    pub(super) fn attach_workspace_target_internal(
        &self,
        path: Option<&str>,
        repository_id: Option<&str>,
        set_default: bool,
        resolve_mode: WorkspaceResolveMode,
    ) -> Result<WorkspaceAttachResponse, ErrorData> {
        let (workspace, resolved_from, resolution) =
            self.resolve_workspace_target(path, repository_id, resolve_mode)?;

        let newly_adopted = self.adopt_workspace(&workspace, set_default)?;

        self.runtime_state
            .validated_manifest_candidate_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .invalidate_root(&workspace.root);
        self.runtime_state
            .searcher_projection_store_service
            .invalidate_repository(&workspace.repository_id);
        self.invalidate_repository_symbol_corpus_cache(&workspace.repository_id);
        self.invalidate_repository_summary_cache(&workspace.repository_id);
        self.invalidate_repository_response_freshness_cache(&workspace.repository_id);
        self.invalidate_repository_file_content_cache(&workspace.repository_id);
        self.invalidate_repository_precise_generator_probe_cache(&workspace.repository_id);
        self.invalidate_repository_precise_graph_caches(&workspace.repository_id);
        self.invalidate_repository_search_response_caches(&workspace.repository_id);
        self.invalidate_repository_navigation_response_caches(&workspace.repository_id);
        self.maybe_refresh_workspace_semantic_snapshot(&workspace);

        let mut repository = self.repository_summary(&workspace);
        let storage = repository
            .storage
            .clone()
            .unwrap_or_else(|| Self::workspace_storage_summary(&workspace));
        repository.storage = None;
        self.maybe_spawn_workspace_runtime_prewarm(&workspace);
        let precise_generation_action =
            self.maybe_spawn_workspace_precise_generation_for_paths(&workspace, &[], &[]);
        let precise = self
            .workspace_precise_summary_for_workspace(&workspace, Some(precise_generation_action));
        let precise_lifecycle = self.workspace_precise_lifecycle_summary(
            &workspace,
            precise_generation_action,
            &precise,
            false,
            false,
        );

        Ok(WorkspaceAttachResponse {
            repository,
            resolved_from: resolved_from.unwrap_or_else(|| workspace.root.display().to_string()),
            resolution: resolution.unwrap_or(WorkspaceResolveMode::Direct),
            session_default: self.current_repository_id().as_deref()
                == Some(workspace.repository_id.as_str()),
            storage,
            action: if newly_adopted {
                WorkspaceAttachAction::AttachedFresh
            } else {
                WorkspaceAttachAction::ReusedWorkspace
            },
            precise,
            precise_lifecycle,
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
        self.attach_workspace_target_internal(Some(&owned_path), None, set_default, resolve_mode)
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
        [
            RuntimeTaskKind::ChangedReindex,
            RuntimeTaskKind::SemanticRefresh,
            RuntimeTaskKind::WorkspacePrepare,
            RuntimeTaskKind::WorkspaceReindex,
        ]
        .into_iter()
        .any(|kind| {
            registry.has_active_task_for_repository(kind, repository_id)
                || workspace.as_ref().is_some_and(|workspace| {
                    workspace.runtime_repository_id != repository_id
                        && registry
                            .has_active_task_for_repository(kind, &workspace.runtime_repository_id)
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
    ) -> (FriggConfig, BTreeMap<String, String>) {
        let scoped_config = FriggConfig {
            workspace_roots: scoped_workspaces
                .iter()
                .map(|workspace| workspace.root.clone())
                .collect(),
            ..(*self.config).clone()
        };
        let repository_id_map = scoped_config
            .repositories()
            .into_iter()
            .zip(scoped_workspaces.iter())
            .map(|(temporary, actual)| (temporary.repository_id.0, actual.repository_id.clone()))
            .collect::<BTreeMap<_, _>>();
        (scoped_config, repository_id_map)
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
            self.known_workspaces()
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

        for (repository_id, root_canonical) in roots {
            let candidate = if requested.is_absolute() {
                requested.clone()
            } else {
                root_canonical.join(&requested)
            };

            match candidate.canonicalize() {
                Ok(candidate_canonical) => {
                    if !candidate_canonical.starts_with(&root_canonical) {
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
                            Self::relative_display_path(&root_canonical, &candidate_canonical);
                        return Ok((repository_id, candidate_canonical, display_path));
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    if Self::candidate_within_root(&candidate, &root_canonical)? {
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
