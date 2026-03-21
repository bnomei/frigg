use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::domain::model::stable_repository_id_for_root;
use crate::storage::resolve_provenance_db_path;

#[derive(Debug, Clone)]
pub(crate) struct AttachedWorkspace {
    pub repository_id: String,
    pub runtime_repository_id: String,
    pub display_name: String,
    pub root: PathBuf,
    pub db_path: PathBuf,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct WorkspaceRegistry {
    workspaces: Vec<AttachedWorkspace>,
    by_canonical_root: BTreeMap<PathBuf, usize>,
    active_session_counts: BTreeMap<String, usize>,
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
            registry.insert_with_repository_id(
                root,
                repository_id,
                runtime_repository_id,
                display_name,
            );
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

    pub(crate) fn get_or_insert(&mut self, canonical_root: PathBuf) -> AttachedWorkspace {
        let display_name = display_name_for_root(&canonical_root);
        let repository_id = stable_repository_id_for_root(&canonical_root).0;
        self.insert_with_repository_id(
            canonical_root,
            repository_id.clone(),
            repository_id,
            display_name,
        )
    }

    pub(crate) fn mark_session_adopted(&mut self, repository_id: &str) -> usize {
        let count = self
            .active_session_counts
            .entry(repository_id.to_owned())
            .or_insert(0);
        *count = count.saturating_add(1);
        *count
    }

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
