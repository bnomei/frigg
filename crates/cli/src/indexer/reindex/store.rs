use std::path::{Path, PathBuf};

use crate::domain::FriggResult;
use crate::storage::Storage;

use super::super::manifest::{file_digest_to_manifest_entry, manifest_entry_to_file_digest};
use super::super::{FileDigest, RepositoryManifest};

#[derive(Debug, Clone)]
/// Thin indexer-facing wrapper around storage that speaks in repository manifest terms instead of
/// raw persistence tables.
pub struct ManifestStore {
    storage: Storage,
}

impl ManifestStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            storage: Storage::new(db_path),
        }
    }

    pub fn initialize(&self) -> FriggResult<()> {
        self.storage.initialize()
    }

    pub fn upsert_repository(
        &self,
        repository_id: &str,
        root_path: &Path,
        display_name: &str,
    ) -> FriggResult<()> {
        self.storage
            .upsert_repository(repository_id, root_path, display_name)
    }

    pub(crate) fn initialize_for_reindex(&self, semantic_enabled: bool) -> FriggResult<()> {
        if semantic_enabled {
            self.storage.initialize()
        } else {
            self.storage.initialize_without_vector_store()
        }
    }

    pub fn persist_snapshot_manifest(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        entries: &[FileDigest],
    ) -> FriggResult<()> {
        let manifest_entries = entries
            .iter()
            .map(file_digest_to_manifest_entry)
            .collect::<Vec<_>>();
        self.storage
            .upsert_manifest(repository_id, snapshot_id, &manifest_entries)
    }

    pub fn load_snapshot_manifest(&self, snapshot_id: &str) -> FriggResult<Vec<FileDigest>> {
        self.storage
            .load_manifest_for_snapshot(snapshot_id)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(manifest_entry_to_file_digest)
                    .collect()
            })
    }

    pub fn load_latest_manifest_for_repository(
        &self,
        repository_id: &str,
    ) -> FriggResult<Option<RepositoryManifest>> {
        self.storage
            .load_latest_manifest_for_repository(repository_id)
            .map(|snapshot| {
                snapshot.map(|snapshot| RepositoryManifest {
                    repository_id: snapshot.repository_id,
                    snapshot_id: snapshot.snapshot_id,
                    entries: snapshot
                        .entries
                        .into_iter()
                        .map(manifest_entry_to_file_digest)
                        .collect(),
                })
            })
    }

    pub fn delete_snapshot(&self, snapshot_id: &str) -> FriggResult<()> {
        self.storage.delete_snapshot(snapshot_id)
    }
}
