use super::*;
use crate::mcp::server::runtime_cache::serialized_value_estimated_bytes;

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
            Ok(_) => match storage.verify() {
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

    pub(in crate::mcp::server) fn cached_repository_summary(
        &self,
        repository_id: &str,
    ) -> Option<RepositorySummary> {
        let cache = self
            .cache_state
            .repository_summary_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.get(repository_id)?;
        (entry.generated_at.elapsed() <= Self::REPOSITORY_SUMMARY_CACHE_TTL)
            .then(|| entry.summary.clone())
    }

    pub(in crate::mcp::server) fn cache_repository_summary(
        &self,
        repository_id: &str,
        summary: &RepositorySummary,
    ) {
        let mut cache = self
            .cache_state
            .repository_summary_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(
            repository_id.to_owned(),
            CachedRepositorySummary {
                summary: summary.clone(),
                generated_at: Instant::now(),
            },
        );
        self.trim_runtime_cache_to_budget(
            RuntimeCacheFamily::RepositorySummary,
            &mut cache,
            |_, entry| serialized_value_estimated_bytes(&entry.summary),
        );
    }

    pub(in crate::mcp::server) fn invalidate_repository_summary_cache(&self, repository_id: &str) {
        self.cache_state
            .repository_summary_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(repository_id);
    }

    pub(in crate::mcp::server) fn repository_summary(
        &self,
        workspace: &AttachedWorkspace,
    ) -> RepositorySummary {
        let dirty_root = self.workspace_has_dirty_root(workspace);
        if !dirty_root
            && let Some(summary) = self.cached_repository_summary(&workspace.repository_id)
        {
            return summary;
        }
        if dirty_root {
            self.invalidate_repository_summary_cache(&workspace.repository_id);
        }

        let storage = Self::workspace_storage_summary(workspace);
        let health = self.workspace_index_health_summary(workspace, &storage);
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
        let summary = RepositorySummary {
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
            health: Some(health),
        };
        if !dirty_root {
            self.cache_repository_summary(&workspace.repository_id, &summary);
        }
        summary
    }
}
