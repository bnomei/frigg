use super::*;

impl FriggMcpSessionState {
    pub(super) fn new(
        workspace_registry: Arc<RwLock<WorkspaceRegistry>>,
        watch_runtime: Arc<RwLock<Option<Arc<crate::watch::WatchRuntime>>>>,
    ) -> Self {
        Self {
            inner: Arc::new(FriggMcpSessionStateInner {
                workspace_registry,
                watch_runtime,
                adopted_repository_ids: RwLock::new(BTreeSet::new()),
                session_default_repository_id: RwLock::new(None),
                result_handles: RwLock::new(SessionResultHandleCache::default()),
            }),
        }
    }
}

impl FriggMcpSessionStateInner {
    fn release_repository_id(&self, repository_id: &str) {
        self.workspace_registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .mark_session_released(repository_id);
        let runtime_repository_id = self
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(repository_id)
            .map(|workspace| workspace.runtime_repository_id)
            .unwrap_or_else(|| repository_id.to_owned());
        if let Some(watch_runtime) = self
            .watch_runtime
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .cloned()
        {
            watch_runtime.release_lease(&runtime_repository_id);
        }
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

    pub(super) fn adopt_workspace(
        &self,
        workspace: &AttachedWorkspace,
        set_default: bool,
    ) -> Result<bool, ErrorData> {
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
            {
                watch_runtime
                    .acquire_lease(workspace)
                    .map_err(Self::map_frigg_error)?;
            }
        }

        if set_default {
            self.set_current_repository_id(Some(workspace.repository_id.clone()));
        }

        Ok(newly_adopted)
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

        if self.current_repository_id().as_deref() == Some(repository_id) {
            self.set_current_repository_id(None);
        }
        self.session_state
            .inner
            .release_repository_id(repository_id);

        Ok(self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(repository_id))
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
        Self::resource_not_found(
            "no repositories are adopted for this session",
            Some(json!({
                "attached_repositories": [],
                "action": action,
                "hint": "call workspace_attach first or choose a repository_id from list_repositories",
            })),
        )
    }

    pub(super) fn attached_workspaces_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<AttachedWorkspace>, ErrorData> {
        let registry = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let adopted_repository_ids = self
            .session_state
            .inner
            .adopted_repository_ids
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .iter()
            .cloned()
            .collect::<Vec<_>>();

        if let Some(repository_id) = repository_id
            .map(str::to_owned)
            .or_else(|| self.current_repository_id())
        {
            if let Some(workspace) = registry.workspace_by_repository_id(&repository_id) {
                return Ok(vec![workspace]);
            }
            return Err(Self::resource_not_found(
                "repository_id not found",
                Some(json!({ "repository_id": repository_id })),
            ));
        }

        let workspaces = adopted_repository_ids
            .into_iter()
            .filter_map(|repository_id| registry.workspace_by_repository_id(&repository_id))
            .collect::<Vec<_>>();
        if workspaces.is_empty() {
            return Err(Self::no_attached_workspaces_error("workspace_attach"));
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
}
