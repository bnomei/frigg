//! Workspace lifecycle tools: attach, detach, prepare, index, and index/precise readiness waits.

use super::*;

impl FriggMcpSessionState {
    pub(super) fn new(
        workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
        watch_runtime: Arc<RwLock<Option<Arc<crate::watch::WatchRuntime>>>>,
    ) -> Self {
        Self {
            inner: Arc::new(FriggMcpSessionStateInner {
                display_session_id: Uuid::now_v7().simple().to_string(),
                workspace_registry,
                watch_runtime,
                adopted_repository_ids: RwLock::new(BTreeSet::new()),
                workspace_attach_states: RwLock::new(BTreeMap::new()),
                session_default_repository_id: RwLock::new(None),
                result_handles: RwLock::new(SessionResultHandleCache::default()),
            }),
        }
    }

    pub(super) fn display_session_id(&self) -> String {
        self.inner.display_session_id.clone()
    }
}

impl FriggMcpSessionStateInner {
    fn release_repository_id(&self, repository_id: &str) {
        let (remaining_sessions, runtime_repository_id) = {
            let mut registry = self
                .workspace_registry
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let runtime_repository_id = registry
                .workspace_by_repository_id(repository_id)
                .map(|workspace| workspace.runtime_repository_id)
                .unwrap_or_else(|| repository_id.to_owned());
            let remaining_sessions = registry.mark_session_released(repository_id);
            (remaining_sessions, runtime_repository_id)
        };
        if let Some(watch_runtime) = self
            .watch_runtime
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .cloned()
        {
            watch_runtime.release_lease(&runtime_repository_id);
        }
        if remaining_sessions == 0 {
            self.workspace_registry
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .prune_inactive_ephemeral_workspace(repository_id);
        }
    }
}

#[derive(Debug)]
pub(super) struct WorkspaceAdoption {
    pub(super) newly_adopted: bool,
}

pub(super) struct WorkspaceResolutionGuard {
    workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
    repository_id: String,
    active: bool,
}

impl WorkspaceResolutionGuard {
    pub(super) fn new(
        workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
        repository_id: String,
    ) -> Self {
        Self {
            workspace_registry,
            repository_id,
            active: true,
        }
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let mut registry = self
            .workspace_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        registry.mark_workspace_pending_released(&self.repository_id);
        registry.prune_inactive_ephemeral_workspace(&self.repository_id);
    }
}

impl Drop for WorkspaceResolutionGuard {
    fn drop(&mut self) {
        self.release();
    }
}

pub(super) struct WorkspaceAttachRollbackGuard {
    session_state: FriggMcpSessionState,
    repository_id: String,
    previous_default_repository_id: Option<String>,
    set_default: bool,
    created_adoption: bool,
    active: bool,
}

impl WorkspaceAttachRollbackGuard {
    fn new(
        session_state: FriggMcpSessionState,
        repository_id: String,
        previous_default_repository_id: Option<String>,
        set_default: bool,
    ) -> Self {
        {
            let mut states = session_state
                .inner
                .workspace_attach_states
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let state = states.entry(repository_id.clone()).or_default();
            state.in_flight = state.in_flight.saturating_add(1);
        }

        Self {
            session_state,
            repository_id,
            previous_default_repository_id,
            set_default,
            created_adoption: false,
            active: true,
        }
    }

    pub(super) fn mark_created_adoption(&mut self) {
        self.created_adoption = true;
    }

    pub(super) fn disarm(mut self) {
        self.finish(true);
    }

    fn finish(&mut self, completed: bool) {
        if !self.active {
            return;
        }
        self.active = false;

        let (rollback_previous_default_repository_id, restore_previous_default_repository_id) = {
            let mut states = self
                .session_state
                .inner
                .workspace_attach_states
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let state = states.entry(self.repository_id.clone()).or_default();
            state.in_flight = state.in_flight.saturating_sub(1);

            if completed {
                state.completed = true;
                state.rollback_requested = false;
                state.rollback_previous_default_repository_id = None;
                if self.set_default {
                    state.default_confirmed = true;
                    state.default_restore_requested = false;
                    state.default_restore_previous_default_repository_id = None;
                }
            } else if self.created_adoption {
                if !state.completed {
                    state.rollback_requested = true;
                    state.rollback_previous_default_repository_id =
                        self.previous_default_repository_id.clone();
                }
                if self.set_default && !state.default_confirmed {
                    state.default_restore_requested = true;
                    state.default_restore_previous_default_repository_id =
                        self.previous_default_repository_id.clone();
                }
            }

            let should_rollback =
                !state.completed && state.rollback_requested && state.in_flight == 0;
            let rollback_previous_default_repository_id =
                should_rollback.then(|| state.rollback_previous_default_repository_id.clone());
            let should_restore_default = state.completed
                && state.default_restore_requested
                && !state.default_confirmed
                && state.in_flight == 0;
            let restore_previous_default_repository_id = should_restore_default
                .then(|| state.default_restore_previous_default_repository_id.clone());
            if state.in_flight == 0
                && (state.completed || should_rollback || !state.rollback_requested)
            {
                states.remove(&self.repository_id);
            }
            (
                rollback_previous_default_repository_id,
                restore_previous_default_repository_id,
            )
        };

        if let Some(previous_default_repository_id) = rollback_previous_default_repository_id {
            self.rollback_adoption(previous_default_repository_id);
        } else if let Some(previous_default_repository_id) = restore_previous_default_repository_id
        {
            self.restore_previous_default(previous_default_repository_id);
        }
    }

    fn rollback_adoption(&self, previous_default_repository_id: Option<String>) {
        let previous_default_repository_id = {
            let mut adopted = self
                .session_state
                .inner
                .adopted_repository_ids
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if !adopted.remove(&self.repository_id) {
                return;
            }
            previous_default_repository_id
                .as_ref()
                .filter(|repository_id| adopted.contains(repository_id.as_str()))
                .cloned()
        };

        {
            let mut current = self
                .session_state
                .inner
                .session_default_repository_id
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if current.as_deref() == Some(self.repository_id.as_str()) {
                *current = previous_default_repository_id;
            }
        }

        self.session_state
            .inner
            .release_repository_id(&self.repository_id);
    }

    fn restore_previous_default(&self, previous_default_repository_id: Option<String>) {
        let previous_default_repository_id = {
            let adopted = self
                .session_state
                .inner
                .adopted_repository_ids
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            previous_default_repository_id
                .as_ref()
                .filter(|repository_id| adopted.contains(repository_id.as_str()))
                .cloned()
        };

        let mut current = self
            .session_state
            .inner
            .session_default_repository_id
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if current.as_deref() == Some(self.repository_id.as_str()) {
            *current = previous_default_repository_id;
        }
    }
}

impl Drop for WorkspaceAttachRollbackGuard {
    fn drop(&mut self) {
        self.finish(false);
    }
}

impl Drop for FriggMcpSessionStateInner {
    fn drop(&mut self) {
        let adopted_repository_ids = self
            .adopted_repository_ids
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        for repository_id in adopted_repository_ids {
            self.release_repository_id(&repository_id);
        }
    }
}

impl FriggMcpServer {
    pub(super) fn known_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .known_workspaces()
    }

    pub(super) fn startup_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .startup_workspaces()
    }

    pub(super) fn auto_adoptable_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.startup_workspaces()
    }

    pub(super) fn visible_workspaces(&self) -> Vec<AttachedWorkspace> {
        let mut visible = BTreeMap::new();
        {
            let registry = self
                .runtime_state
                .workspace_registry
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for workspace in registry.startup_workspaces() {
                visible.insert(workspace.repository_id.clone(), workspace);
            }
            for repository_id in self
                .session_state
                .inner
                .adopted_repository_ids
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .iter()
            {
                if let Some(workspace) = registry.workspace_by_repository_id(repository_id) {
                    visible.insert(workspace.repository_id.clone(), workspace);
                }
            }
        }
        visible.into_values().collect()
    }

    pub(super) fn attached_workspaces(&self) -> Vec<AttachedWorkspace> {
        let adopted_repository_ids = self
            .session_state
            .inner
            .adopted_repository_ids
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>();
        let registry = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        adopted_repository_ids
            .into_iter()
            .filter_map(|repository_id| registry.workspace_by_repository_id(&repository_id))
            .collect()
    }

    pub(super) fn current_repository_id(&self) -> Option<String> {
        self.session_state
            .inner
            .session_default_repository_id
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn set_current_repository_id(&self, repository_id: Option<String>) {
        let mut current = self
            .session_state
            .inner
            .session_default_repository_id
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = repository_id;
    }

    pub(super) fn is_visible_repository_id(&self, repository_id: &str) -> bool {
        let registry = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if registry.is_startup_repository_id(repository_id) {
            return true;
        }
        let Some(workspace) = registry.workspace_by_any_repository_id(repository_id) else {
            return false;
        };
        self.session_state
            .inner
            .adopted_repository_ids
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&workspace.repository_id)
    }

    pub(super) fn adopt_workspace(
        &self,
        workspace: &AttachedWorkspace,
        set_default: bool,
    ) -> Result<WorkspaceAdoption, ErrorData> {
        let newly_adopted = {
            let mut adopted = self
                .session_state
                .inner
                .adopted_repository_ids
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            adopted.insert(workspace.repository_id.clone())
        };

        if newly_adopted {
            self.runtime_state
                .workspace_registry
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .mark_session_adopted(&workspace.repository_id);
            if let Some(watch_runtime) = self
                .runtime_state
                .watch_runtime
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .as_ref()
                .cloned()
                && let Err(err) = watch_runtime
                    .acquire_lease(workspace)
                    .map_err(Self::map_frigg_error)
            {
                {
                    let mut adopted = self
                        .session_state
                        .inner
                        .adopted_repository_ids
                        .write()
                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                    adopted.remove(&workspace.repository_id);
                }
                self.session_state
                    .inner
                    .release_repository_id(&workspace.repository_id);
                return Err(err);
            }
        }

        if set_default {
            self.set_current_repository_id(Some(workspace.repository_id.clone()));
        }

        Ok(WorkspaceAdoption { newly_adopted })
    }

    pub(super) fn workspace_attach_path_rollback_guard(
        &self,
        path: Option<&str>,
        previous_default_repository_id: Option<String>,
        workspace: &AttachedWorkspace,
        set_default: bool,
    ) -> Option<WorkspaceAttachRollbackGuard> {
        path?;

        Some(WorkspaceAttachRollbackGuard::new(
            self.session_state.clone(),
            workspace.repository_id.clone(),
            previous_default_repository_id,
            set_default,
        ))
    }

    pub(super) fn detach_workspace(
        &self,
        repository_id: &str,
    ) -> Result<Option<AttachedWorkspace>, ErrorData> {
        let removed = {
            let mut adopted = self
                .session_state
                .inner
                .adopted_repository_ids
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            adopted.remove(repository_id)
        };
        if !removed {
            return Ok(None);
        }
        self.session_state
            .inner
            .workspace_attach_states
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(repository_id);

        if self.current_repository_id().as_deref() == Some(repository_id) {
            self.set_current_repository_id(None);
        }
        let detached_workspace = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(repository_id);
        self.session_state
            .inner
            .release_repository_id(repository_id);

        Ok(detached_workspace)
    }

    pub(super) fn current_workspace(&self) -> Option<AttachedWorkspace> {
        let repository_id = self.current_repository_id()?;
        self.runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(&repository_id)
    }

    pub(super) fn no_attached_workspaces_error(action: &str) -> ErrorData {
        let attach_path = std::env::current_dir()
            .ok()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<repo root or any file inside it>".to_owned());
        Self::resource_not_found(
            "no repositories are adopted for this session",
            Some(json!({
                "attached_repositories": [],
                "action": action,
                "hint": "Call workspace with next_params.path, then retry.",
                "next_tool": "workspace",
                "next_params": { "path": attach_path },
            })),
        )
    }

    pub(super) fn attached_workspaces_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<AttachedWorkspace>, ErrorData> {
        let adopted_repository_ids = self
            .session_state
            .inner
            .adopted_repository_ids
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>();

        if let Some(repository_id) = repository_id.map(str::to_owned) {
            let workspace = {
                let registry = self
                    .runtime_state
                    .workspace_registry
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                registry.workspace_by_repository_id(&repository_id)
            };
            let Some(workspace) = workspace else {
                return Err(Self::resource_not_found(
                    "repository_id not found",
                    Some(json!({ "repository_id": repository_id })),
                ));
            };

            let is_adopted = adopted_repository_ids
                .iter()
                .any(|id| id == &workspace.repository_id);
            if is_adopted {
                return Ok(vec![workspace]);
            }
            return Err(Self::resource_not_found(
                "repository_id is not adopted for this session",
                Some(json!({
                    "repository_id": repository_id,
                    "attached_repositories": adopted_repository_ids,
                    "hint": "Call workspace with next_params, then retry.",
                    "next_tool": "workspace",
                    "next_params": { "repository_id": workspace.repository_id },
                })),
            ));
        }

        if let Some(repository_id) = self.current_repository_id() {
            let registry = self
                .runtime_state
                .workspace_registry
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let Some(workspace) = registry.workspace_by_repository_id(&repository_id) else {
                return Err(Self::resource_not_found(
                    "repository_id not found",
                    Some(json!({ "repository_id": repository_id })),
                ));
            };
            if adopted_repository_ids
                .iter()
                .any(|id| id == &workspace.repository_id)
            {
                return Ok(vec![workspace]);
            }
            return Err(Self::resource_not_found(
                "current repository is not adopted for this session",
                Some(json!({
                    "repository_id": repository_id,
                    "attached_repositories": adopted_repository_ids,
                    "hint": "Call workspace with next_params, then retry.",
                    "next_tool": "workspace",
                    "next_params": { "repository_id": repository_id },
                })),
            ));
        }

        if adopted_repository_ids.is_empty() {
            let auto_adoptable_workspaces = self.auto_adoptable_workspaces();
            if let [workspace] = auto_adoptable_workspaces.as_slice() {
                self.adopt_workspace(workspace, true)?;
                return Ok(vec![workspace.clone()]);
            }
            if let Ok(current_dir) = std::env::current_dir() {
                let current_dir = current_dir.display().to_string();
                if let Ok((workspace, _, _, _)) = self.resolve_workspace_target(
                    Some(&current_dir),
                    None,
                    WorkspaceResolveMode::GitRoot,
                ) {
                    self.adopt_workspace(&workspace, true)?;
                    return Ok(vec![workspace]);
                }
            }
        }

        let registry = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let workspaces = adopted_repository_ids
            .into_iter()
            .filter_map(|repository_id| registry.workspace_by_repository_id(&repository_id))
            .collect::<Vec<_>>();
        if workspaces.is_empty() {
            return Err(Self::no_attached_workspaces_error("workspace"));
        }

        Ok(workspaces)
    }

    pub(super) fn roots_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<(String, PathBuf)>, ErrorData> {
        Ok(self
            .attached_workspaces_for_repository(repository_id)?
            .into_iter()
            .map(|workspace| (workspace.repository_id, workspace.root))
            .collect())
    }

    pub(super) fn effective_attach_directory(path: &Path) -> Result<PathBuf, ErrorData> {
        if path.exists() {
            let metadata = fs::metadata(path).map_err(|err| {
                Self::invalid_params(
                    format!("failed to inspect attach path {}: {err}", path.display()),
                    Some(json!({ "path": path.display().to_string() })),
                )
            })?;
            let directory = if metadata.is_dir() {
                path.to_path_buf()
            } else {
                path.parent().map(Path::to_path_buf).ok_or_else(|| {
                    Self::invalid_params(
                        "workspace_attach path has no parent directory",
                        Some(json!({ "path": path.display().to_string() })),
                    )
                })?
            };
            return directory.canonicalize().map_err(|err| {
                Self::invalid_params(
                    format!(
                        "failed to canonicalize attach path {}: {err}",
                        directory.display()
                    ),
                    Some(json!({ "path": path.display().to_string() })),
                )
            });
        }

        Self::canonicalize_existing_ancestor(path)?.ok_or_else(|| {
            Self::invalid_params(
                "workspace_attach path does not exist and has no existing ancestor",
                Some(json!({ "path": path.display().to_string() })),
            )
        })
    }

    pub(super) fn find_git_root(start: &Path) -> Option<PathBuf> {
        start.ancestors().find_map(|ancestor| {
            ancestor
                .join(".git")
                .exists()
                .then(|| ancestor.to_path_buf())
        })
    }

    pub(super) fn relative_attach_path_has_parent(path: &Path) -> bool {
        path.components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    }

    pub(super) fn authorized_attach_roots(&self) -> Vec<PathBuf> {
        let mut roots = BTreeSet::new();
        for workspace in self.startup_workspaces() {
            roots.insert(workspace.root);
        }
        for workspace in self.attached_workspaces() {
            roots.insert(workspace.root);
        }
        if let Ok(current_dir) = std::env::current_dir() {
            let current_root = Self::find_git_root(&current_dir).unwrap_or(current_dir);
            roots.insert(
                current_root
                    .canonicalize()
                    .unwrap_or_else(|_| current_root.to_path_buf()),
            );
        }
        roots.into_iter().collect()
    }

    pub(super) fn authorize_attach_root(&self, root: &Path) -> Result<(), ErrorData> {
        let root = root.canonicalize().map_err(|err| {
            Self::invalid_params(
                format!(
                    "failed to canonicalize attach root {}: {err}",
                    root.display()
                ),
                Some(json!({ "path": root.display().to_string() })),
            )
        })?;
        let authorized_roots = self.authorized_attach_roots();
        if authorized_roots
            .iter()
            .any(|authorized_root| root.starts_with(authorized_root))
        {
            return Ok(());
        }

        Err(Self::access_denied(
            "workspace attach path is outside authorized workspace roots",
            Some(json!({
                "path": root.display().to_string(),
                "authorized_roots": authorized_roots
                    .iter()
                    .map(|root| root.display().to_string())
                    .collect::<Vec<_>>(),
            })),
        ))
    }
}
