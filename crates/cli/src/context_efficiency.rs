//! Shared context-efficiency helpers for optional response metadata and local summaries.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use crate::storage::RepositoryManifestMetadataSnapshot;

pub(crate) const CONTEXT_EFFICIENCY_LOG_ENV: &str = "FRIGG_CONTEXT_EFFICIENCY_LOG";
const MANIFEST_METADATA_CACHE_LIMIT: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ManifestMetadataCacheKey {
    storage_identity: String,
    repository_id: String,
    snapshot_id: String,
}

impl ManifestMetadataCacheKey {
    pub(crate) fn new(
        storage_identity: impl Into<String>,
        repository_id: impl Into<String>,
        snapshot_id: impl Into<String>,
    ) -> Self {
        Self {
            storage_identity: storage_identity.into(),
            repository_id: repository_id.into(),
            snapshot_id: snapshot_id.into(),
        }
    }

    pub(crate) fn from_storage_path(
        storage_path: &Path,
        repository_id: &str,
        snapshot_id: &str,
    ) -> Self {
        Self::new(
            storage_path.to_string_lossy().into_owned(),
            repository_id.to_owned(),
            snapshot_id.to_owned(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestMetadataSummary {
    pub(crate) repository_id: String,
    pub(crate) snapshot_id: String,
    pub(crate) indexed_readable_files: usize,
    pub(crate) indexed_readable_bytes: u64,
    pub(crate) min_mtime_ns: Option<u64>,
    pub(crate) max_mtime_ns: Option<u64>,
    pub(crate) path_size_bytes: BTreeMap<String, u64>,
}

impl ManifestMetadataSummary {
    pub(crate) fn from_snapshot(snapshot: &RepositoryManifestMetadataSnapshot) -> Self {
        let mut indexed_readable_bytes = 0_u64;
        let mut min_mtime_ns = None;
        let mut max_mtime_ns = None;
        let mut path_size_bytes = BTreeMap::new();

        for entry in &snapshot.entries {
            indexed_readable_bytes = indexed_readable_bytes.saturating_add(entry.size_bytes);
            if let Some(mtime_ns) = entry.mtime_ns {
                min_mtime_ns =
                    Some(min_mtime_ns.map_or(mtime_ns, |current: u64| current.min(mtime_ns)));
                max_mtime_ns =
                    Some(max_mtime_ns.map_or(mtime_ns, |current: u64| current.max(mtime_ns)));
            }
            path_size_bytes.insert(entry.path.clone(), entry.size_bytes);
        }

        Self {
            repository_id: snapshot.repository_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            indexed_readable_files: snapshot.entries.len(),
            indexed_readable_bytes,
            min_mtime_ns,
            max_mtime_ns,
            path_size_bytes,
        }
    }

    pub(crate) fn returned_unique_file_bytes<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> u64 {
        let unique_paths = paths.into_iter().collect::<BTreeSet<_>>();
        unique_paths
            .into_iter()
            .filter_map(|path| self.path_size_bytes.get(path))
            .copied()
            .fold(0_u64, u64::saturating_add)
    }
}

#[derive(Debug, Default)]
struct ManifestMetadataCache {
    insertion_order: VecDeque<ManifestMetadataCacheKey>,
    entries: BTreeMap<ManifestMetadataCacheKey, ManifestMetadataSummary>,
}

impl ManifestMetadataCache {
    fn get(&self, key: &ManifestMetadataCacheKey) -> Option<ManifestMetadataSummary> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: ManifestMetadataCacheKey, value: ManifestMetadataSummary) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key, value);
            return;
        }

        self.insertion_order.push_back(key.clone());
        self.entries.insert(key, value);
        while self.entries.len() > MANIFEST_METADATA_CACHE_LIMIT {
            let Some(oldest_key) = self.insertion_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest_key);
        }
    }
}

static MANIFEST_METADATA_CACHE: OnceLock<Mutex<ManifestMetadataCache>> = OnceLock::new();

fn manifest_metadata_cache() -> &'static Mutex<ManifestMetadataCache> {
    MANIFEST_METADATA_CACHE.get_or_init(|| Mutex::new(ManifestMetadataCache::default()))
}

pub(crate) fn cached_manifest_metadata_summary(
    key: &ManifestMetadataCacheKey,
) -> Option<ManifestMetadataSummary> {
    manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned")
        .get(key)
}

pub(crate) fn store_manifest_metadata_summary(
    key: ManifestMetadataCacheKey,
    value: ManifestMetadataSummary,
) {
    manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned")
        .insert(key, value);
}

pub(crate) fn is_truthy_env_value(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !matches!(normalized.as_str(), "" | "0" | "false" | "no" | "off")
}

pub(crate) fn context_efficiency_log_enabled() -> bool {
    std::env::var(CONTEXT_EFFICIENCY_LOG_ENV)
        .map(|value| is_truthy_env_value(&value))
        .unwrap_or(false)
}

pub(crate) fn need_context_efficiency(include_context_efficiency: Option<bool>) -> bool {
    need_context_efficiency_with_log_state(
        include_context_efficiency,
        context_efficiency_log_enabled(),
    )
}

pub(crate) fn need_context_efficiency_with_log_state(
    include_context_efficiency: Option<bool>,
    context_efficiency_log_enabled: bool,
) -> bool {
    include_context_efficiency == Some(true) || context_efficiency_log_enabled
}

#[cfg(test)]
pub(crate) fn clear_manifest_metadata_cache_for_tests() {
    let mut cache = manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned");
    *cache = ManifestMetadataCache::default();
}

#[cfg(test)]
pub(crate) fn manifest_metadata_cache_entry_count_for_tests() -> usize {
    manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned")
        .entries
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ManifestMetadataEntry, RepositoryManifestMetadataSnapshot};

    #[test]
    fn context_efficiency_guard_requires_response_flag_or_env_log() {
        assert!(!need_context_efficiency_with_log_state(None, false));
        assert!(!need_context_efficiency_with_log_state(Some(false), false));
        assert!(need_context_efficiency_with_log_state(Some(true), false));
        assert!(need_context_efficiency_with_log_state(None, true));
        assert!(need_context_efficiency_with_log_state(Some(false), true));
    }

    #[test]
    fn context_efficiency_env_truthiness_is_explicitly_false_for_common_false_values() {
        for value in ["", "0", "false", "False", " no ", "OFF"] {
            assert!(!is_truthy_env_value(value), "{value:?} should be false");
        }
        for value in ["1", "true", "yes", "on", "anything-else"] {
            assert!(is_truthy_env_value(value), "{value:?} should be true");
        }
    }

    #[test]
    fn context_efficiency_manifest_summary_derives_totals_and_path_sizes() {
        let snapshot = RepositoryManifestMetadataSnapshot {
            repository_id: "repo-1".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
            entries: vec![
                ManifestMetadataEntry {
                    path: "src/a.rs".to_owned(),
                    size_bytes: 10,
                    mtime_ns: Some(300),
                },
                ManifestMetadataEntry {
                    path: "src/b.rs".to_owned(),
                    size_bytes: 25,
                    mtime_ns: Some(100),
                },
                ManifestMetadataEntry {
                    path: "README.md".to_owned(),
                    size_bytes: 5,
                    mtime_ns: None,
                },
            ],
        };

        let summary = ManifestMetadataSummary::from_snapshot(&snapshot);
        assert_eq!(summary.indexed_readable_files, 3);
        assert_eq!(summary.indexed_readable_bytes, 40);
        assert_eq!(summary.min_mtime_ns, Some(100));
        assert_eq!(summary.max_mtime_ns, Some(300));
        assert_eq!(
            summary.returned_unique_file_bytes(["src/a.rs", "src/a.rs", "README.md"]),
            15
        );
    }

    #[test]
    fn context_efficiency_manifest_cache_is_bounded_fifo() {
        clear_manifest_metadata_cache_for_tests();
        for index in 0..(MANIFEST_METADATA_CACHE_LIMIT + 3) {
            let key = ManifestMetadataCacheKey::new("storage", "repo", format!("snapshot-{index}"));
            let summary = ManifestMetadataSummary {
                repository_id: "repo".to_owned(),
                snapshot_id: format!("snapshot-{index}"),
                indexed_readable_files: index,
                indexed_readable_bytes: index as u64,
                min_mtime_ns: None,
                max_mtime_ns: None,
                path_size_bytes: BTreeMap::new(),
            };
            store_manifest_metadata_summary(key, summary);
        }

        assert_eq!(
            manifest_metadata_cache_entry_count_for_tests(),
            MANIFEST_METADATA_CACHE_LIMIT
        );
        assert!(
            cached_manifest_metadata_summary(&ManifestMetadataCacheKey::new(
                "storage",
                "repo",
                "snapshot-0"
            ))
            .is_none()
        );
        assert!(
            cached_manifest_metadata_summary(&ManifestMetadataCacheKey::new(
                "storage",
                "repo",
                format!("snapshot-{}", MANIFEST_METADATA_CACHE_LIMIT + 2)
            ))
            .is_some()
        );
        clear_manifest_metadata_cache_for_tests();
    }
}
