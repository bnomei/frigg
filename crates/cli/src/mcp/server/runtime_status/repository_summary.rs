//! `RepositorySummary` assembly for list and workspace tools.

use super::*;

impl FriggMcpServer {
    pub(in crate::mcp::server) fn workspace_storage_summary(
        workspace: &AttachedWorkspace,
    ) -> WorkspaceStorageSummary {
        if !workspace.db_path.is_file() {
            return WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: false,
                initialized: false,
                index_state: WorkspaceStorageIndexState::MissingDb,
                error: None,
            };
        }

        let storage = Storage::new(&workspace.db_path);
        match storage.schema_version() {
            Ok(0) => WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: true,
                initialized: false,
                index_state: WorkspaceStorageIndexState::Uninitialized,
                error: None,
            },
            Ok(version) if version != crate::storage::latest_storage_schema_version() => {
                WorkspaceStorageSummary {
                    db_path: workspace.db_path.display().to_string(),
                    exists: true,
                    initialized: true,
                    index_state: WorkspaceStorageIndexState::Error,
                    error: Some(format!(
                        "storage schema is incompatible (found {version}, expected {}); delete '{}' and rerun `frigg init` or `frigg index` to regenerate storage",
                        crate::storage::latest_storage_schema_version(),
                        workspace.db_path.display()
                    )),
                }
            }
            Ok(_) => match storage.verify_relational_schema() {
                Ok(_) => {
                    match storage
                        .load_latest_manifest_for_repository(&workspace.runtime_repository_id)
                    {
                        Ok(Some(_)) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Ready,
                            error: None,
                        },
                        Ok(None) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Uninitialized,
                            error: None,
                        },
                        Err(err) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Error,
                            error: Some(err.to_string()),
                        },
                    }
                }
                Err(err) => WorkspaceStorageSummary {
                    db_path: workspace.db_path.display().to_string(),
                    exists: true,
                    initialized: true,
                    index_state: WorkspaceStorageIndexState::Error,
                    error: Some(err.to_string()),
                },
            },
            Err(err) => WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: true,
                initialized: false,
                index_state: WorkspaceStorageIndexState::Error,
                error: Some(err.to_string()),
            },
        }
    }

    fn repository_summary_from_parts(
        &self,
        workspace: &AttachedWorkspace,
        storage: WorkspaceStorageSummary,
        health: Option<WorkspaceIndexHealthSummary>,
    ) -> RepositorySummary {
        let session_adopted = self
            .session_state
            .inner
            .adopted_repository_ids
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&workspace.repository_id);
        let active_session_count = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_session_count(&workspace.repository_id);
        let watch = self
            .runtime_state
            .watch_runtime
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|runtime| runtime.lease_status(&workspace.runtime_repository_id))
            .unwrap_or_default();
        RepositorySummary {
            repository_id: workspace.repository_id.clone(),
            display_name: workspace.display_name.clone(),
            root_path: workspace.root.display().to_string(),
            session: RepositorySessionSummary {
                adopted: session_adopted,
                active_session_count,
            },
            watch: RepositoryWatchSummary {
                active: watch.active,
                lease_count: watch.lease_count,
            },
            storage: Some(storage),
            health,
        }
    }

    pub(in crate::mcp::server) fn repository_summary(
        &self,
        workspace: &AttachedWorkspace,
    ) -> RepositorySummary {
        let storage = Self::workspace_storage_summary(workspace);
        let health = self.workspace_index_health_summary(workspace, &storage);
        self.repository_summary_from_parts(workspace, storage, Some(health))
    }

    pub(in crate::mcp::server) fn public_repository_summary(
        &self,
        workspace: &AttachedWorkspace,
    ) -> RepositorySummary {
        let storage = Self::workspace_storage_summary(workspace);
        self.repository_summary_from_parts(workspace, storage, None)
    }
}
