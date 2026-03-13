use super::*;

impl FriggMcpServer {
    pub(super) fn attached_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .attached_workspaces()
    }

    pub(super) fn current_repository_id(&self) -> Option<String> {
        self.session_default_repository_id
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    pub(super) fn set_current_repository_id(&self, repository_id: Option<String>) {
        let mut current = self
            .session_default_repository_id
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *current = repository_id;
    }

    pub(super) fn current_workspace(&self) -> Option<AttachedWorkspace> {
        let repository_id = self.current_repository_id()?;
        self.workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .workspace_by_repository_id(&repository_id)
    }

    pub(super) fn no_attached_workspaces_error(action: &str) -> ErrorData {
        Self::resource_not_found(
            "no repositories are attached for this session",
            Some(json!({
                "attached_repositories": [],
                "action": action,
                "hint": "call workspace_attach first or provide --workspace-root at startup",
            })),
        )
    }

    pub(super) fn effective_repository_id(&self, repository_id: Option<&str>) -> Option<String> {
        repository_id
            .map(str::to_owned)
            .or_else(|| self.current_repository_id())
    }

    pub(super) fn attached_workspaces_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Result<Vec<AttachedWorkspace>, ErrorData> {
        let registry = self
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());

        if let Some(repository_id) = self.effective_repository_id(repository_id) {
            if let Some(workspace) = registry.workspace_by_repository_id(&repository_id) {
                return Ok(vec![workspace]);
            }
            return Err(Self::resource_not_found(
                "repository_id not found",
                Some(json!({ "repository_id": repository_id })),
            ));
        }

        let workspaces = registry.attached_workspaces();
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
