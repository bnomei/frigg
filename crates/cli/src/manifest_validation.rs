//! Manifest freshness validation and watch-time reuse of validated digest snapshots.
//!
//! Rebuilds lightweight metadata digests for candidate paths and caches validated snapshots so
//! hybrid search can trust manifest-backed universes without repeating full content walks.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
#[cfg(test)]
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::{
    FileMetadataDigest, ManifestBuildDiagnostic, ManifestBuilder, ManifestMetadataBuildOutput,
};
use crate::languages::semantic_chunk_language_for_path;
use crate::settings::SemanticRuntimeConfig;
use crate::storage::{ManifestEntry, ManifestMetadataEntry, RepositoryManifestSnapshot, Storage};

#[cfg(test)]
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
    let live_output = ManifestBuilder::default()
        .build_metadata_with_diagnostics(root)
        .ok()?;
    validate_manifest_digests_against_live_output(root, file_digests, live_output)
}

fn validate_manifest_digests_against_live_output(
    root: &Path,
    file_digests: &[FileMetadataDigest],
    live_output: ManifestMetadataBuildOutput,
) -> Option<Vec<FileMetadataDigest>> {
    let snapshot_digests = normalized_manifest_digests_for_root(root, file_digests)?;
    if manifest_diagnostics_invalidate_snapshot(root, &snapshot_digests, &live_output.diagnostics) {
        return None;
    }
    let live_digests = normalized_manifest_digests_for_root(root, &live_output.entries)?;

    (snapshot_digests == live_digests).then_some(snapshot_digests)
}

fn normalized_manifest_digests_for_root(
    root: &Path,
    file_digests: &[FileMetadataDigest],
) -> Option<Vec<FileMetadataDigest>> {
    let mut normalized = Vec::with_capacity(file_digests.len());
    for digest in file_digests {
        let path = normalize_manifest_path_for_root(root, &digest.path)?;
        normalized.push(FileMetadataDigest {
            path,
            size_bytes: digest.size_bytes,
            mtime_ns: digest.mtime_ns,
        });
    }
    normalized.sort_by(file_metadata_digest_order);
    Some(normalized)
}

fn manifest_diagnostics_invalidate_snapshot(
    root: &Path,
    snapshot_digests: &[FileMetadataDigest],
    diagnostics: &[ManifestBuildDiagnostic],
) -> bool {
    if diagnostics.is_empty() {
        return false;
    }

    let snapshot_paths = snapshot_digests
        .iter()
        .map(|digest| digest.path.clone())
        .collect::<BTreeSet<_>>();
    diagnostics.iter().any(|diagnostic| {
        let Some(path) = diagnostic.path.as_deref() else {
            return true;
        };
        let Some(path) = normalize_manifest_path_for_root(root, path) else {
            return true;
        };
        snapshot_paths.contains(&path)
    })
}

fn normalize_manifest_path_for_root(root: &Path, path: &Path) -> Option<PathBuf> {
    let normalized = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    normalized.starts_with(root).then_some(normalized)
}

fn file_metadata_digest_order(
    left: &FileMetadataDigest,
    right: &FileMetadataDigest,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.size_bytes.cmp(&right.size_bytes))
        .then(left.mtime_ns.cmp(&right.mtime_ns))
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

/// Cache hit/miss counters for validated manifest digest reuse.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ValidatedManifestCandidateCacheStats {
    pub hits: usize,
    pub misses: usize,
    pub dirty_bypasses: usize,
}

/// Outcome of a validated manifest digest cache lookup for a workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ValidatedManifestCandidateCacheLookup {
    Hit(Arc<Vec<FileMetadataDigest>>),
    Miss,
    Dirty,
}

/// In-memory cache of manifest digests validated against the live workspace root.
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
    ) -> bool {
        let key = Self::cache_key(root);
        if matches!(
            self.entries.get(&key),
            Some(ValidatedManifestCandidateCacheEntry::Dirty)
        ) {
            return false;
        }
        self.entries.insert(
            key,
            ValidatedManifestCandidateCacheEntry::Ready {
                snapshot_id: snapshot_id.to_owned(),
                digests,
            },
        );
        true
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

/// Manifest snapshot with digests confirmed against the live workspace filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ValidatedManifestSnapshot {
    pub snapshot_id: String,
    pub digests: Vec<FileMetadataDigest>,
}

/// Shared validated manifest snapshot used to avoid cloning digest vectors on cache hits.
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
) -> FriggResult<Option<ValidatedManifestSnapshot>> {
    let Some(shared) =
        latest_validated_manifest_snapshot_shared(storage, repository_id, root, cache)?
    else {
        return Ok(None);
    };
    Ok(Some(ValidatedManifestSnapshot {
        snapshot_id: shared.snapshot_id,
        digests: shared.digests.as_ref().clone(),
    }))
}

pub(crate) fn latest_validated_manifest_snapshot_shared(
    storage: &Storage,
    repository_id: &str,
    root: &Path,
    cache: Option<&Arc<RwLock<ValidatedManifestCandidateCache>>>,
) -> FriggResult<Option<SharedValidatedManifestSnapshot>> {
    let Some(latest) = storage.load_latest_manifest_metadata_for_repository(repository_id)? else {
        return Ok(None);
    };
    let snapshot_id = latest.snapshot_id.clone();

    if let Some(cache) = cache {
        let lookup = cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .lookup(root, &snapshot_id);
        match lookup {
            ValidatedManifestCandidateCacheLookup::Hit(digests) => {
                if validate_manifest_digests_for_root(root, digests.as_ref()).is_some() {
                    return Ok(Some(SharedValidatedManifestSnapshot {
                        snapshot_id,
                        digests,
                    }));
                }

                cache
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .invalidate_root(root);
                return Ok(None);
            }
            ValidatedManifestCandidateCacheLookup::Dirty => return Ok(None),
            ValidatedManifestCandidateCacheLookup::Miss => {}
        }
    }

    let snapshot_digests = manifest_digests_from_metadata_entries(&latest.entries);
    let Some(validated_digests) = validate_manifest_digests_for_root(root, &snapshot_digests)
    else {
        return Ok(None);
    };
    let validated_digests = Arc::new(validated_digests);

    if let Some(cache) = cache {
        cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .store_validated_shared(root, &snapshot_id, Arc::clone(&validated_digests));
    }

    Ok(Some(SharedValidatedManifestSnapshot {
        snapshot_id,
        digests: validated_digests,
    }))
}

/// Return a cached validated snapshot without re-walking the workspace.
///
/// Callers must only use this when another runtime guard, such as an active
/// watch lease plus dirty-root invalidation, is responsible for cache freshness.
pub(crate) fn latest_cached_validated_manifest_snapshot_shared(
    storage: &Storage,
    repository_id: &str,
    root: &Path,
    cache: &Arc<RwLock<ValidatedManifestCandidateCache>>,
) -> FriggResult<Option<SharedValidatedManifestSnapshot>> {
    let Some(latest) = storage.load_latest_manifest_metadata_for_repository(repository_id)? else {
        return Ok(None);
    };
    let snapshot_id = latest.snapshot_id.clone();
    let lookup = cache
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .lookup(root, &snapshot_id);
    match lookup {
        ValidatedManifestCandidateCacheLookup::Hit(digests) => {
            Ok(Some(SharedValidatedManifestSnapshot {
                snapshot_id,
                digests,
            }))
        }
        ValidatedManifestCandidateCacheLookup::Dirty
        | ValidatedManifestCandidateCacheLookup::Miss => Ok(None),
    }
}

/// Manifest snapshot freshness relative to the live workspace root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositoryManifestFreshness {
    MissingSnapshot,
    StaleSnapshot,
    Ready,
}

/// Semantic corpus freshness for the active provider-model partition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RepositorySemanticFreshness {
    Disabled,
    MissingManifestSnapshot,
    StaleManifestSnapshot,
    NoEligibleEntries,
    MissingForActiveModel,
    Ready,
}

/// Active semantic embedding provider and model for freshness checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RepositorySemanticTarget {
    pub provider: String,
    pub model: String,
}

/// Combined manifest and semantic freshness snapshot used by watch and MCP refresh.
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
    let semantic = if !has_rows {
        RepositorySemanticFreshness::MissingForActiveModel
    } else {
        let health = storage.collect_semantic_storage_health_for_repository_model(
            repository_id,
            &semantic_target.provider,
            &semantic_target.model,
        )?;
        if health.vector_consistent {
            RepositorySemanticFreshness::Ready
        } else {
            RepositorySemanticFreshness::MissingForActiveModel
        }
    };

    Ok(RepositoryFreshnessStatus {
        snapshot_id: Some(snapshot.snapshot_id),
        manifest_entry_count,
        manifest: RepositoryManifestFreshness::Ready,
        semantic,
        validated_manifest_digests: Some(validated_manifest_digests),
        semantic_target: Some(semantic_target),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::ManifestDiagnosticKind;
    use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeProvider};
    use crate::storage::{SemanticChunkEmbeddingRecord, VECTOR_TABLE_NAME};
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
    fn validate_manifest_digests_ignores_diagnostics_outside_snapshot() {
        let workspace_root = temp_workspace_root("diagnostic-outside-snapshot");
        let snapshot_digests = vec![FileMetadataDigest {
            path: PathBuf::from("src/lib.rs"),
            size_bytes: 10,
            mtime_ns: Some(100),
        }];

        let validated = validate_manifest_digests_against_live_output(
            &workspace_root,
            &snapshot_digests,
            ManifestMetadataBuildOutput {
                entries: snapshot_digests.clone(),
                diagnostics: vec![ManifestBuildDiagnostic {
                    path: Some(workspace_root.join("generated/unreadable.rs")),
                    kind: ManifestDiagnosticKind::Read,
                    message: "permission denied".to_owned(),
                }],
            },
        )
        .expect("unrelated diagnostics should not stale an otherwise matching snapshot");

        assert_eq!(validated.len(), 1);
        assert_eq!(validated[0].path, workspace_root.join("src/lib.rs"));

        assert!(
            validate_manifest_digests_against_live_output(
                &workspace_root,
                &snapshot_digests,
                ManifestMetadataBuildOutput {
                    entries: snapshot_digests.clone(),
                    diagnostics: vec![ManifestBuildDiagnostic {
                        path: Some(workspace_root.join("src/lib.rs")),
                        kind: ManifestDiagnosticKind::Read,
                        message: "permission denied".to_owned(),
                    }],
                },
            )
            .is_none(),
            "diagnostics touching indexed snapshot paths must stay conservative"
        );
        assert!(
            validate_manifest_digests_against_live_output(
                &workspace_root,
                &snapshot_digests,
                ManifestMetadataBuildOutput {
                    entries: snapshot_digests.clone(),
                    diagnostics: vec![ManifestBuildDiagnostic {
                        path: None,
                        kind: ManifestDiagnosticKind::Walk,
                        message: "walk failed".to_owned(),
                    }],
                },
            )
            .is_none(),
            "unscoped walk diagnostics must stay conservative"
        );
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
        )?
        .expect("first shared manifest validation should succeed");
        let second = latest_validated_manifest_snapshot_shared(
            &storage,
            "repo-001",
            &workspace_root,
            Some(&cache),
        )?
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
    fn latest_validated_manifest_snapshot_shared_rejects_cached_snapshot_missing_new_file()
    -> FriggResult<()> {
        let workspace_root = temp_workspace_root("shared-cache-detects-new-file");
        fs::create_dir_all(&workspace_root)
            .expect("manifest-validation workspace root should be creatable");
        let file_path = workspace_root.join("src.rs");
        fs::write(&file_path, "fn cached() {}\n")
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
            )?
            .is_some(),
            "initial shared manifest validation should populate cache"
        );

        fs::write(workspace_root.join("new_file.rs"), "fn uncached() {}\n")
            .expect("new fixture file should be writable");

        assert!(
            latest_validated_manifest_snapshot_shared(
                &storage,
                "repo-001",
                &workspace_root,
                Some(&cache),
            )?
            .is_none(),
            "cached manifest snapshots must be rejected when live files are missing from the snapshot"
        );
        assert!(
            !cache
                .read()
                .expect("validated manifest candidate cache should not be poisoned")
                .has_entry_for_root(&workspace_root),
            "rejecting the stale cached snapshot should invalidate the cache entry"
        );

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
            )?
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
            )?
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

    #[test]
    fn latest_validated_manifest_snapshot_shared_propagates_storage_errors() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("shared-storage-error");
        fs::create_dir_all(workspace_root.join(".frigg"))
            .expect("manifest-validation db parent should be creatable");
        let db_path = workspace_root.join(".frigg/provenance.db");
        let conn = rusqlite::Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!("failed to create incompatible fixture db: {err}"))
        })?;
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );
            INSERT INTO schema_version (id, version, updated_at)
            VALUES (1, 0, CURRENT_TIMESTAMP);
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to seed incompatible fixture db: {err}"))
        })?;
        drop(conn);

        let storage = Storage::new(&db_path);
        let err =
            latest_validated_manifest_snapshot_shared(&storage, "repo-001", &workspace_root, None)
                .expect_err("schema-incompatible storage should be propagated");
        assert!(
            matches!(err, FriggError::StorageSchemaIncompatible { .. }),
            "expected typed storage schema error, got {err:?}"
        );

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    fn semantic_embedding_record(
        chunk_id: &str,
        repository_id: &str,
        snapshot_id: &str,
        path: &str,
    ) -> SemanticChunkEmbeddingRecord {
        SemanticChunkEmbeddingRecord {
            chunk_id: chunk_id.to_owned(),
            repository_id: repository_id.to_owned(),
            snapshot_id: snapshot_id.to_owned(),
            path: path.to_owned(),
            language: "rust".to_owned(),
            chunk_index: 0,
            start_line: 1,
            end_line: 10,
            provider: "openai".to_owned(),
            model: "text-embedding-3-small".to_owned(),
            trace_id: Some("trace-001".to_owned()),
            content_hash_blake3: format!("hash-{chunk_id}"),
            content_text: format!("fn {chunk_id}() {{}}"),
            embedding: vec![1.0, 0.0],
        }
    }

    #[test]
    fn repository_freshness_status_rejects_ready_when_vector_partition_drifts() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("repository-freshness-vector-drift");
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
        let repository_id = "repo-001";
        let snapshot_id = "snapshot-001";
        seed_manifest_snapshot(&storage, repository_id, snapshot_id, &file_path)?;
        storage.replace_semantic_embeddings_for_repository(
            repository_id,
            snapshot_id,
            "openai",
            "text-embedding-3-small",
            &[
                semantic_embedding_record("chunk-a", repository_id, snapshot_id, "src.rs"),
                semantic_embedding_record("chunk-b", repository_id, snapshot_id, "src.rs"),
            ],
        )?;

        let conn = rusqlite::Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!(
                "failed to open semantic vector corruption fixture db: {err}"
            ))
        })?;
        conn.execute(
            &format!(
                "DELETE FROM {VECTOR_TABLE_NAME} WHERE repository_id = ?1 AND provider = ?2 AND model = ?3 AND chunk_id = ?4"
            ),
            (repository_id, "openai", "text-embedding-3-small", "chunk-b"),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to corrupt semantic vector partition for freshness test: {err}"
            ))
        })?;
        conn.execute(
            &format!(
                r#"
                INSERT INTO {VECTOR_TABLE_NAME} (
                    embedding,
                    repository_id,
                    provider,
                    model,
                    language,
                    chunk_id
                )
                SELECT
                    embedding,
                    repository_id,
                    provider,
                    model,
                    language,
                    ?5
                FROM {VECTOR_TABLE_NAME}
                WHERE repository_id = ?1
                  AND provider = ?2
                  AND model = ?3
                  AND chunk_id = ?4
                "#
            ),
            (
                repository_id,
                "openai",
                "text-embedding-3-small",
                "chunk-a",
                "chunk-orphan",
            ),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to seed equal-count orphan semantic vector for freshness test: {err}"
            ))
        })?;
        drop(conn);

        assert!(
            storage.has_semantic_embeddings_for_repository_snapshot_model(
                repository_id,
                snapshot_id,
                "openai",
                "text-embedding-3-small",
            )?,
            "fixture should retain embedding rows while vector partition drifts"
        );
        let health = storage.collect_semantic_storage_health_for_repository_model(
            repository_id,
            "openai",
            "text-embedding-3-small",
        )?;
        assert_eq!(health.live_embedding_rows, health.live_vector_rows);
        assert!(!health.vector_consistent);

        let status = repository_freshness_status(
            &storage,
            repository_id,
            &workspace_root,
            &SemanticRuntimeConfig {
                enabled: true,
                provider: Some(SemanticRuntimeProvider::OpenAi),
                model: None,
                strict_mode: false,
            },
            |_| false,
        )?;
        assert_eq!(status.manifest, RepositoryManifestFreshness::Ready);
        assert_eq!(
            status.semantic,
            RepositorySemanticFreshness::MissingForActiveModel,
            "semantic freshness must not report Ready when vector partition is inconsistent"
        );
        assert!(status.should_refresh_watch());

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[test]
    fn store_validated_shared_does_not_clobber_dirty_root() {
        let workspace_root = temp_workspace_root("dirty-store-not-clobbered");
        let mut cache = ValidatedManifestCandidateCache::default();
        cache.mark_dirty_root(&workspace_root);

        let stored =
            cache.store_validated_shared(&workspace_root, "snapshot-001", Arc::new(Vec::new()));

        assert!(!stored, "dirty roots must reject ready snapshot stores");
        assert!(cache.is_dirty_root(&workspace_root));
        assert!(matches!(
            cache.lookup(&workspace_root, "snapshot-001"),
            ValidatedManifestCandidateCacheLookup::Dirty
        ));
    }
}
