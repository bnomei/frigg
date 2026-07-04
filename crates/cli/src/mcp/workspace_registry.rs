//! Process-wide workspace catalog keyed by canonical repository root.
//!
//! Tracks stable `repository_id` values, SQLite storage paths, and per-session adoption
//! refcounts so watch leases and cache invalidation can outlive individual MCP sessions.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::domain::model::stable_repository_id_for_root;
use crate::storage::resolve_provenance_db_path;

/// One workspace known to the process-wide registry with stable and runtime repository ids.
#[derive(Debug, Clone)]
pub(crate) struct AttachedWorkspace {
    pub repository_id: String,
    pub runtime_repository_id: String,
    pub display_name: String,
    pub root: PathBuf,
    pub db_path: PathBuf,
}

/// Process-wide workspace catalog keyed by canonical repository root.
#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceRegistry {
    workspaces: Vec<AttachedWorkspace>,
    by_canonical_root: BTreeMap<PathBuf, usize>,
    startup_repository_ids: BTreeSet<String>,
    active_session_counts: BTreeMap<String, usize>,
    pending_workspace_counts: BTreeMap<String, usize>,
}

impl WorkspaceRegistry {
    pub(crate) fn from_startup_repositories<I>(repositories: I) -> Self
    where
        I: IntoIterator<Item = (String, String, String)>,
    {
        let mut registry = Self::default();
        for (runtime_repository_id, display_name, root_path) in repositories {
            let root = PathBuf::from(&root_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&root_path));
            let repository_id = stable_repository_id_for_root(&root).0;
            let workspace = registry.insert_with_repository_id(
                root,
                repository_id,
                runtime_repository_id,
                display_name,
            );
            registry
                .startup_repository_ids
                .insert(workspace.repository_id.clone());
        }
        registry
    }

    pub(crate) fn known_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.workspaces.clone()
    }

    pub(crate) fn workspace_by_repository_id(
        &self,
        repository_id: &str,
    ) -> Option<AttachedWorkspace> {
        self.workspace_by_any_repository_id(repository_id)
    }

    pub(crate) fn workspace_by_any_repository_id(
        &self,
        repository_id: &str,
    ) -> Option<AttachedWorkspace> {
        self.workspaces
            .iter()
            .find(|workspace| {
                workspace.repository_id == repository_id
                    || workspace.runtime_repository_id == repository_id
            })
            .cloned()
    }

    pub(crate) fn insert_with_repository_id(
        &mut self,
        canonical_root: PathBuf,
        repository_id: String,
        runtime_repository_id: String,
        display_name: String,
    ) -> AttachedWorkspace {
        if let Some(index) = self.by_canonical_root.get(&canonical_root).copied() {
            return self.workspaces[index].clone();
        }

        let workspace = AttachedWorkspace {
            db_path: storage_db_path_for_root(&canonical_root),
            repository_id,
            runtime_repository_id,
            display_name,
            root: canonical_root.clone(),
        };
        self.by_canonical_root
            .insert(canonical_root, self.workspaces.len());
        self.workspaces.push(workspace.clone());
        workspace
    }

    pub(crate) fn get_or_insert(&mut self, canonical_root: PathBuf) -> (AttachedWorkspace, bool) {
        let display_name = display_name_for_root(&canonical_root);
        let repository_id = stable_repository_id_for_root(&canonical_root).0;
        let already_known = self.by_canonical_root.contains_key(&canonical_root);
        let workspace = self.insert_with_repository_id(
            canonical_root,
            repository_id.clone(),
            repository_id,
            display_name,
        );
        (workspace, !already_known)
    }

    /// Marks a resolved workspace as pending while attach/index setup decides whether to adopt it.
    pub(crate) fn mark_workspace_pending(&mut self, repository_id: &str) -> usize {
        let count = self
            .pending_workspace_counts
            .entry(repository_id.to_owned())
            .or_insert(0);
        *count = count.saturating_add(1);
        *count
    }

    /// Releases a pending workspace guard after attach succeeds, rolls back, or errors.
    pub(crate) fn mark_workspace_pending_released(&mut self, repository_id: &str) -> usize {
        let Some(count) = self.pending_workspace_counts.get_mut(repository_id) else {
            return 0;
        };
        *count = count.saturating_sub(1);
        let remaining = *count;
        if remaining == 0 {
            self.pending_workspace_counts.remove(repository_id);
        }
        remaining
    }

    /// Pending guard count used to keep ephemeral workspaces alive until resolution finishes.
    pub(crate) fn pending_workspace_count(&self, repository_id: &str) -> usize {
        self.pending_workspace_counts
            .get(repository_id)
            .copied()
            .unwrap_or(0)
    }

    /// Increment active-session refcount so watch leases can be shared across MCP sessions.
    pub(crate) fn mark_session_adopted(&mut self, repository_id: &str) -> usize {
        let count = self
            .active_session_counts
            .entry(repository_id.to_owned())
            .or_insert(0);
        *count = count.saturating_add(1);
        *count
    }

    /// Decrement active-session refcount and drop tracking when no sessions remain adopted.
    pub(crate) fn mark_session_released(&mut self, repository_id: &str) -> usize {
        let Some(count) = self.active_session_counts.get_mut(repository_id) else {
            return 0;
        };
        *count = count.saturating_sub(1);
        let remaining = *count;
        if remaining == 0 {
            self.active_session_counts.remove(repository_id);
        }
        remaining
    }

    /// Removes an ephemeral workspace only after no startup, pending, or adopted session references remain.
    pub(crate) fn prune_inactive_ephemeral_workspace(
        &mut self,
        repository_id: &str,
    ) -> Option<AttachedWorkspace> {
        let index = self.workspaces.iter().position(|workspace| {
            workspace.repository_id == repository_id
                || workspace.runtime_repository_id == repository_id
        })?;
        let canonical_repository_id = self.workspaces[index].repository_id.clone();
        if self.active_session_count(&canonical_repository_id) > 0
            || self.pending_workspace_count(&canonical_repository_id) > 0
            || self
                .startup_repository_ids
                .contains(&canonical_repository_id)
        {
            return None;
        }

        let workspace = self.workspaces.remove(index);
        self.by_canonical_root.retain(|_, stored_index| {
            if *stored_index == index {
                false
            } else {
                if *stored_index > index {
                    *stored_index -= 1;
                }
                true
            }
        });
        self.active_session_counts.remove(&workspace.repository_id);
        Some(workspace)
    }

    pub(crate) fn active_session_count(&self, repository_id: &str) -> usize {
        self.active_session_counts
            .get(repository_id)
            .copied()
            .unwrap_or(0)
    }
}

fn display_name_for_root(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| root.display().to_string())
}

fn storage_db_path_for_root(root: &Path) -> PathBuf {
    resolve_provenance_db_path(root).unwrap_or_else(|_| {
        root.join(crate::storage::PROVENANCE_STORAGE_DIR)
            .join(crate::storage::PROVENANCE_STORAGE_DB_FILE)
    })
}
