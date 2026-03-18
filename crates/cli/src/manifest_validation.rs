use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::{FileMetadataDigest, ManifestBuilder};
use crate::languages::semantic_chunk_language_for_path;
use crate::settings::SemanticRuntimeConfig;
use crate::storage::{ManifestEntry, ManifestMetadataEntry, RepositoryManifestSnapshot, Storage};

pub(crate) fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
    system_time
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
}

pub(crate) fn validate_manifest_digests_for_root(
    root: &Path,
    file_digests: &[FileMetadataDigest],
) -> Option<Vec<FileMetadataDigest>> {
    if file_digests.is_empty() {
        let live_entries = ManifestBuilder::default()
            .build_metadata_with_diagnostics(root)
            .ok()?
            .entries;
        if !live_entries.is_empty() {
            return None;
        }
    }

    let mut validated = Vec::with_capacity(file_digests.len());
    for digest in file_digests {
        let path = if digest.path.is_absolute() {
            digest.path.clone()
        } else {
            root.join(&digest.path)
        };
        if !path.starts_with(root) {
            return None;
        }

        let metadata = fs::metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() != digest.size_bytes {
            return None;
        }

        let mtime_ns = metadata.modified().ok().and_then(system_time_to_unix_nanos);
        if mtime_ns != digest.mtime_ns {
            return None;
        }

        validated.push(FileMetadataDigest {
            path,
            size_bytes: metadata.len(),
            mtime_ns,
        });
    }

    Some(validated)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ValidatedManifestCandidateCacheEntry {
    Dirty,
    Ready {
        snapshot_id: String,
        digests: Arc<Vec<FileMetadataDigest>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ValidatedManifestCandidateCacheKey {
    root: PathBuf,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ValidatedManifestCandidateCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub dirty_bypasses: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValidatedManifestCandidateCacheLookup {
    Hit(Arc<Vec<FileMetadataDigest>>),
    Miss,
    Dirty,
}

#[derive(Debug, Default)]
pub struct ValidatedManifestCandidateCache {
    entries: BTreeMap<ValidatedManifestCandidateCacheKey, ValidatedManifestCandidateCacheEntry>,
    stats: ValidatedManifestCandidateCacheStats,
}

impl ValidatedManifestCandidateCache {
    pub(crate) fn lookup(
        &mut self,
        root: &Path,
        snapshot_id: &str,
    ) -> ValidatedManifestCandidateCacheLookup {
        let key = Self::cache_key(root);
        match self.entries.get(&key) {
            Some(ValidatedManifestCandidateCacheEntry::Ready {
                snapshot_id: cached_snapshot_id,
                digests,
            }) if cached_snapshot_id == snapshot_id => {
                self.stats.hits += 1;
                ValidatedManifestCandidateCacheLookup::Hit(digests.clone())
            }
            Some(ValidatedManifestCandidateCacheEntry::Dirty) => {
                self.stats.dirty_bypasses += 1;
                ValidatedManifestCandidateCacheLookup::Dirty
            }
            Some(ValidatedManifestCandidateCacheEntry::Ready { .. }) => {
                self.entries.remove(&key);
                self.stats.misses += 1;
                ValidatedManifestCandidateCacheLookup::Miss
            }
            None => {
                self.stats.misses += 1;
                ValidatedManifestCandidateCacheLookup::Miss
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn store_validated(
        &mut self,
        root: &Path,
        snapshot_id: &str,
        digests: &[FileMetadataDigest],
    ) {
        self.store_validated_shared(root, snapshot_id, Arc::new(digests.to_vec()));
    }

    pub(crate) fn store_validated_shared(
        &mut self,
        root: &Path,
        snapshot_id: &str,
        digests: Arc<Vec<FileMetadataDigest>>,
    ) {
        self.entries.insert(
            Self::cache_key(root),
            ValidatedManifestCandidateCacheEntry::Ready {
                snapshot_id: snapshot_id.to_owned(),
                digests,
            },
        );
    }

    pub(crate) fn invalidate_root(&mut self, root: &Path) {
        self.entries.remove(&Self::cache_key(root));
    }

    pub(crate) fn mark_dirty_root(&mut self, root: &Path) {
        self.entries.insert(
            Self::cache_key(root),
            ValidatedManifestCandidateCacheEntry::Dirty,
        );
    }

    pub(crate) fn is_dirty_root(&self, root: &Path) -> bool {
        matches!(
            self.entries.get(&Self::cache_key(root)),
            Some(ValidatedManifestCandidateCacheEntry::Dirty)
        )
    }

    fn cache_key(root: &Path) -> ValidatedManifestCandidateCacheKey {
        ValidatedManifestCandidateCacheKey {
            root: root.to_path_buf(),
        }
    }

    #[cfg(test)]
    pub(crate) fn stats(&self) -> ValidatedManifestCandidateCacheStats {
        self.stats
    }

    #[cfg(test)]
    pub(crate) fn has_entry_for_root(&self, root: &Path) -> bool {
        self.entries.contains_key(&Self::cache_key(root))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedManifestSnapshot {
    pub snapshot_id: String,
    pub digests: Vec<FileMetadataDigest>,
}

#[derive(Debug, Clone)]
pub(crate) struct SharedValidatedManifestSnapshot {
    pub snapshot_id: String,
    pub digests: Arc<Vec<FileMetadataDigest>>,
}

pub(crate) fn latest_validated_manifest_snapshot(
    storage: &Storage,
    repository_id: &str,
    root: &Path,
    cache: Option<&Arc<RwLock<ValidatedManifestCandidateCache>>>,
) -> Option<ValidatedManifestSnapshot> {
    let shared = latest_validated_manifest_snapshot_shared(storage, repository_id, root, cache)?;
    Some(ValidatedManifestSnapshot {
        snapshot_id: shared.snapshot_id,
        digests: shared.digests.as_ref().clone(),
    })
}

pub(crate) fn latest_validated_manifest_snapshot_shared(
    storage: &Storage,
    repository_id: &str,
    root: &Path,
    cache: Option<&Arc<RwLock<ValidatedManifestCandidateCache>>>,
) -> Option<SharedValidatedManifestSnapshot> {
    let latest = storage
        .load_latest_manifest_metadata_for_repository(repository_id)
        .ok()??;
    let snapshot_id = latest.snapshot_id.clone();

    if let Some(cache) = cache {
        let lookup = cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lookup(root, &snapshot_id);
        match lookup {
            ValidatedManifestCandidateCacheLookup::Hit(digests) => {
                if validate_manifest_digests_for_root(root, digests.as_ref()).is_some() {
                    return Some(SharedValidatedManifestSnapshot {
                        snapshot_id,
                        digests,
                    });
                }

                cache
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .invalidate_root(root);
                return None;
            }
            ValidatedManifestCandidateCacheLookup::Dirty => return None,
            ValidatedManifestCandidateCacheLookup::Miss => {}
        }
    }

    let snapshot_digests = manifest_digests_from_metadata_entries(&latest.entries);
    let validated_digests = Arc::new(validate_manifest_digests_for_root(root, &snapshot_digests)?);

    if let Some(cache) = cache {
        cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .store_validated_shared(root, &snapshot_id, Arc::clone(&validated_digests));
    }

    Some(SharedValidatedManifestSnapshot {
        snapshot_id,
        digests: validated_digests,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositoryManifestFreshness {
    MissingSnapshot,
    StaleSnapshot,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositorySemanticFreshness {
    Disabled,
    MissingManifestSnapshot,
    StaleManifestSnapshot,
    NoEligibleEntries,
    MissingForActiveModel,
    Ready,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepositorySemanticTarget {
    pub provider: String,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepositoryFreshnessStatus {
    pub snapshot_id: Option<String>,
    pub manifest_entry_count: Option<usize>,
    pub manifest: RepositoryManifestFreshness,
    pub semantic: RepositorySemanticFreshness,
    pub validated_manifest_digests: Option<Vec<FileMetadataDigest>>,
    pub semantic_target: Option<RepositorySemanticTarget>,
}

impl RepositoryFreshnessStatus {
    pub(crate) fn should_refresh_watch(&self) -> bool {
        matches!(
            self.manifest,
            RepositoryManifestFreshness::MissingSnapshot
                | RepositoryManifestFreshness::StaleSnapshot
        ) || matches!(
            self.semantic,
            RepositorySemanticFreshness::MissingForActiveModel
        )
    }

    pub(crate) fn watch_reason(&self) -> &'static str {
        match self.manifest {
            RepositoryManifestFreshness::MissingSnapshot => "missing_manifest_snapshot",
            RepositoryManifestFreshness::StaleSnapshot => "stale_manifest_snapshot",
            RepositoryManifestFreshness::Ready => match self.semantic {
                RepositorySemanticFreshness::Disabled => "manifest_valid",
                RepositorySemanticFreshness::MissingManifestSnapshot => "missing_manifest_snapshot",
                RepositorySemanticFreshness::StaleManifestSnapshot => "stale_manifest_snapshot",
                RepositorySemanticFreshness::NoEligibleEntries => {
                    "manifest_valid_no_semantic_eligible_entries"
                }
                RepositorySemanticFreshness::MissingForActiveModel => {
                    "semantic_snapshot_missing_for_active_model"
                }
                RepositorySemanticFreshness::Ready => "manifest_and_semantic_snapshot_valid",
            },
        }
    }
}

pub(crate) fn manifest_digests_from_entries(entries: &[ManifestEntry]) -> Vec<FileMetadataDigest> {
    entries
        .iter()
        .map(|entry| FileMetadataDigest {
            path: entry.path.clone().into(),
            size_bytes: entry.size_bytes,
            mtime_ns: entry.mtime_ns,
        })
        .collect()
}

fn manifest_digests_from_metadata_entries(
    entries: &[ManifestMetadataEntry],
) -> Vec<FileMetadataDigest> {
    entries
        .iter()
        .map(|entry| FileMetadataDigest {
            path: entry.path.clone().into(),
            size_bytes: entry.size_bytes,
            mtime_ns: entry.mtime_ns,
        })
        .collect()
}

pub(crate) fn validate_manifest_snapshot_for_root(
    root: &Path,
    snapshot: &RepositoryManifestSnapshot,
) -> Option<Vec<FileMetadataDigest>> {
    let digests = manifest_digests_from_entries(&snapshot.entries);
    validate_manifest_digests_for_root(root, &digests)
}

pub(crate) fn repository_freshness_status<F>(
    storage: &Storage,
    repository_id: &str,
    root: &Path,
    semantic_runtime: &SemanticRuntimeConfig,
    should_ignore_path: F,
) -> FriggResult<RepositoryFreshnessStatus>
where
    F: Fn(&Path) -> bool,
{
    let latest = storage.load_latest_manifest_for_repository(repository_id)?;
    let Some(snapshot) = latest else {
        return Ok(RepositoryFreshnessStatus {
            snapshot_id: None,
            manifest_entry_count: None,
            manifest: RepositoryManifestFreshness::MissingSnapshot,
            semantic: if semantic_runtime.enabled {
                RepositorySemanticFreshness::MissingManifestSnapshot
            } else {
                RepositorySemanticFreshness::Disabled
            },
            validated_manifest_digests: None,
            semantic_target: None,
        });
    };

    let snapshot_id = snapshot.snapshot_id.clone();
    let manifest_entry_count = Some(snapshot.entries.len());
    let Some(validated_manifest_digests) = validate_manifest_snapshot_for_root(root, &snapshot)
    else {
        return Ok(RepositoryFreshnessStatus {
            snapshot_id: Some(snapshot_id),
            manifest_entry_count,
            manifest: RepositoryManifestFreshness::StaleSnapshot,
            semantic: if semantic_runtime.enabled {
                RepositorySemanticFreshness::StaleManifestSnapshot
            } else {
                RepositorySemanticFreshness::Disabled
            },
            validated_manifest_digests: None,
            semantic_target: None,
        });
    };

    if !semantic_runtime.enabled {
        return Ok(RepositoryFreshnessStatus {
            snapshot_id: Some(snapshot_id),
            manifest_entry_count,
            manifest: RepositoryManifestFreshness::Ready,
            semantic: RepositorySemanticFreshness::Disabled,
            validated_manifest_digests: Some(validated_manifest_digests),
            semantic_target: None,
        });
    }

    semantic_runtime
        .validate()
        .map_err(|err| FriggError::InvalidInput(format!("{err}")))?;
    let provider = semantic_runtime.provider.ok_or_else(|| {
        FriggError::Internal("semantic runtime provider missing after validation".to_owned())
    })?;
    let model = semantic_runtime.normalized_model().ok_or_else(|| {
        FriggError::Internal("semantic runtime model missing after validation".to_owned())
    })?;
    let semantic_target = RepositorySemanticTarget {
        provider: provider.as_str().to_owned(),
        model: model.to_owned(),
    };

    let has_semantic_eligible_entries = snapshot.entries.iter().any(|entry| {
        let path = Path::new(&entry.path);
        !should_ignore_path(path) && semantic_chunk_language_for_path(path).is_some()
    });
    if !has_semantic_eligible_entries {
        return Ok(RepositoryFreshnessStatus {
            snapshot_id: Some(snapshot_id),
            manifest_entry_count,
            manifest: RepositoryManifestFreshness::Ready,
            semantic: RepositorySemanticFreshness::NoEligibleEntries,
            validated_manifest_digests: Some(validated_manifest_digests),
            semantic_target: Some(semantic_target),
        });
    }

    let has_rows = storage.has_semantic_embeddings_for_repository_snapshot_model(
        repository_id,
        &snapshot.snapshot_id,
        &semantic_target.provider,
        &semantic_target.model,
    )?;

    Ok(RepositoryFreshnessStatus {
        snapshot_id: Some(snapshot.snapshot_id),
        manifest_entry_count,
        manifest: RepositoryManifestFreshness::Ready,
        semantic: if has_rows {
            RepositorySemanticFreshness::Ready
        } else {
            RepositorySemanticFreshness::MissingForActiveModel
        },
        validated_manifest_digests: Some(validated_manifest_digests),
        semantic_target: Some(semantic_target),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("frigg-manifest-validation-{test_name}-{unique}"))
    }

    fn cleanup_workspace(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("temp manifest-validation workspace should remove");
        }
    }

    fn seed_manifest_snapshot(
        storage: &Storage,
        repository_id: &str,
        snapshot_id: &str,
        file_path: &Path,
    ) -> FriggResult<()> {
        let metadata = fs::metadata(file_path).map_err(|err| {
            FriggError::Internal(format!(
                "failed to stat manifest-validation fixture '{}': {err}",
                file_path.display()
            ))
        })?;
        storage.upsert_manifest(
            repository_id,
            snapshot_id,
            &[ManifestEntry {
                path: file_path.display().to_string(),
                sha256: "fixture-sha".to_owned(),
                size_bytes: metadata.len(),
                mtime_ns: metadata.modified().ok().and_then(system_time_to_unix_nanos),
            }],
        )
    }

    #[test]
    fn latest_validated_manifest_snapshot_shared_reuses_cached_digest_arc() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("shared-cache-hit");
        fs::create_dir_all(&workspace_root)
            .expect("manifest-validation workspace root should be creatable");
        let file_path = workspace_root.join("src.rs");
        fs::write(&file_path, "fn alpha() {}\n")
            .expect("manifest-validation fixture file should be writable");

        let db_path = workspace_root.join(".frigg/provenance.db");
        fs::create_dir_all(
            db_path
                .parent()
                .expect("manifest-validation db path should have a parent"),
        )
        .expect("manifest-validation db parent should be creatable");
        let storage = Storage::new(&db_path);
        storage.initialize()?;
        seed_manifest_snapshot(&storage, "repo-001", "snapshot-001", &file_path)?;

        let cache = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
        let first = latest_validated_manifest_snapshot_shared(
            &storage,
            "repo-001",
            &workspace_root,
            Some(&cache),
        )
        .expect("first shared manifest validation should succeed");
        let second = latest_validated_manifest_snapshot_shared(
            &storage,
            "repo-001",
            &workspace_root,
            Some(&cache),
        )
        .expect("second shared manifest validation should hit cache");

        assert!(Arc::ptr_eq(&first.digests, &second.digests));
        let stats = cache
            .read()
            .expect("validated manifest candidate cache should not be poisoned")
            .stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 1);

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[test]
    fn latest_validated_manifest_snapshot_shared_respects_dirty_root_bypass() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("shared-dirty-bypass");
        fs::create_dir_all(&workspace_root)
            .expect("manifest-validation workspace root should be creatable");
        let file_path = workspace_root.join("src.rs");
        fs::write(&file_path, "fn beta() {}\n")
            .expect("manifest-validation fixture file should be writable");

        let db_path = workspace_root.join(".frigg/provenance.db");
        fs::create_dir_all(
            db_path
                .parent()
                .expect("manifest-validation db path should have a parent"),
        )
        .expect("manifest-validation db parent should be creatable");
        let storage = Storage::new(&db_path);
        storage.initialize()?;
        seed_manifest_snapshot(&storage, "repo-001", "snapshot-001", &file_path)?;

        let cache = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
        assert!(
            latest_validated_manifest_snapshot_shared(
                &storage,
                "repo-001",
                &workspace_root,
                Some(&cache),
            )
            .is_some(),
            "initial shared manifest validation should populate cache"
        );
        cache
            .write()
            .expect("validated manifest candidate cache should not be poisoned")
            .mark_dirty_root(&workspace_root);

        assert!(
            latest_validated_manifest_snapshot_shared(
                &storage,
                "repo-001",
                &workspace_root,
                Some(&cache),
            )
            .is_none(),
            "dirty roots should bypass shared manifest snapshot reuse"
        );
        let stats = cache
            .read()
            .expect("validated manifest candidate cache should not be poisoned")
            .stats();
        assert_eq!(stats.dirty_bypasses, 1);

        cleanup_workspace(&workspace_root);
        Ok(())
    }
}
