//! Process-wide workspace catalog keyed by canonical repository root.
//!
//! Separates the public path-derived `repository_id` from the `runtime_repository_id` that owns
//! durable SQLite rows. Existing indexes, including loaded portable caches, supply the runtime
//! partition so search, indexing, watch leases, and cache invalidation never split one workspace
//! across two identities. Per-session adoption refcounts let those resources outlive a session.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::domain::model::stable_repository_id_for_root;
use crate::storage::{Storage, resolve_provenance_db_path};

/// Workspace identity and storage routing shared by MCP sessions and background tasks.
///
/// `repository_id` is the public path-derived alias; `runtime_repository_id` is the durable
/// partition key used for SQLite, indexing, watch leases, and process caches.
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
    /// Seeds startup roots while preferring an existing database's sole durable partition.
    ///
    /// The configured id is used only when storage has no readable repository row.
    pub(crate) fn from_startup_repositories<I>(repositories: I) -> Self
    where
        I: IntoIterator<Item = (String, String, String)>,
    {
        let mut registry = Self::default();
        for (configured_repository_id, display_name, root_path) in repositories {
            let root = PathBuf::from(&root_path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&root_path));
            let repository_id = stable_repository_id_for_root(&root).0;
            let runtime_repository_id =
                stored_repository_id_for_root(&root).unwrap_or(configured_repository_id);
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

    /// Every workspace currently registered (startup + ephemeral attaches).
    pub(crate) fn known_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.workspaces.clone()
    }

    /// Workspaces that originated from process config rather than late attach.
    pub(crate) fn startup_workspaces(&self) -> Vec<AttachedWorkspace> {
        self.workspaces
            .iter()
            .filter(|workspace| {
                self.startup_repository_ids
                    .contains(&workspace.repository_id)
            })
            .cloned()
            .collect()
    }

    /// True when `repository_id` (stable or runtime) is a startup-configured root.
    pub(crate) fn is_startup_repository_id(&self, repository_id: &str) -> bool {
        self.workspaces
            .iter()
            .find(|workspace| {
                workspace.repository_id == repository_id
                    || workspace.runtime_repository_id == repository_id
            })
            .is_some_and(|workspace| {
                self.startup_repository_ids
                    .contains(&workspace.repository_id)
            })
    }

    /// Lookup by stable public `repository_id` (alias of `workspace_by_any_repository_id`).
    pub(crate) fn workspace_by_repository_id(
        &self,
        repository_id: &str,
    ) -> Option<AttachedWorkspace> {
        self.workspace_by_any_repository_id(repository_id)
    }

    /// Lookup by stable or runtime repository id used across watch/index tasks.
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

    /// Inserts a workspace under a canonical root, or returns the existing entry for that root.
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

    /// Catalogs a dynamically attached root without forking an existing storage partition.
    ///
    /// The second result is true when this call created the registry entry.
    pub(crate) fn get_or_insert(&mut self, canonical_root: PathBuf) -> (AttachedWorkspace, bool) {
        let display_name = display_name_for_root(&canonical_root);
        let repository_id = stable_repository_id_for_root(&canonical_root).0;
        let runtime_repository_id =
            stored_repository_id_for_root(&canonical_root).unwrap_or_else(|| repository_id.clone());
        let already_known = self.by_canonical_root.contains_key(&canonical_root);
        let workspace = self.insert_with_repository_id(
            canonical_root,
            repository_id.clone(),
            runtime_repository_id,
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

    /// Active MCP sessions that currently have this repository adopted.
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

/// Reads the sole durable partition so registry fallback cannot create a second repository row.
///
/// Missing storage and inspection failures return `None`; startup/readiness gates remain
/// responsible for surfacing incompatible or corrupt databases before they are used.
fn stored_repository_id_for_root(root: &Path) -> Option<String> {
    let db_path = storage_db_path_for_root(root);
    if !db_path.is_file() {
        return None;
    }
    Storage::new(db_path).sole_repository_id().ok().flatten()
}

#[cfg(test)]
mod portable_cache_tests {
    #![allow(clippy::panic)]

    use super::*;
    use crate::storage::{Storage, ensure_provenance_db_parent_dir};
    use uuid::Uuid;

    #[test]
    fn dynamic_workspace_reuses_the_stored_repository_partition() {
        let root = std::env::temp_dir().join(format!("frigg-cache-registry-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("create workspace");
        let db_path = ensure_provenance_db_parent_dir(&root).expect("resolve storage path");
        let storage = Storage::new(&db_path);
        storage.initialize().expect("initialize storage");
        storage
            .upsert_repository("repo-001", Path::new("/different/checkout"), "fixture")
            .expect("seed repository");

        let canonical_root = root.canonicalize().expect("canonicalize workspace");
        let (workspace, inserted) = WorkspaceRegistry::default().get_or_insert(canonical_root);
        assert!(inserted);
        assert_eq!(workspace.runtime_repository_id, "repo-001");
        assert_ne!(workspace.repository_id, workspace.runtime_repository_id);

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn startup_workspace_reuses_the_stored_repository_partition() {
        let root = std::env::temp_dir().join(format!("frigg-cache-startup-{}", Uuid::now_v7()));
        std::fs::create_dir_all(&root).expect("create workspace");
        let db_path = ensure_provenance_db_parent_dir(&root).expect("resolve storage path");
        let storage = Storage::new(&db_path);
        storage.initialize().expect("initialize storage");
        storage
            .upsert_repository(
                "loaded-partition",
                Path::new("/different/checkout"),
                "fixture",
            )
            .expect("seed repository");

        let registry = WorkspaceRegistry::from_startup_repositories([(
            "configured-partition".to_owned(),
            "fixture".to_owned(),
            root.display().to_string(),
        )]);
        let workspace = registry
            .startup_workspaces()
            .into_iter()
            .next()
            .expect("startup workspace");
        assert_eq!(workspace.runtime_repository_id, "loaded-partition");

        let _ = std::fs::remove_dir_all(root);
    }
}
