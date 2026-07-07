//! Navigation helper caches that are not query-answer caches.
//!
//! Caches heuristic reference and related navigation helper state; invalidated on repository
//! manifest refresh, not per query answer.

use super::*;
use crate::mcp::server::runtime_cache::serialized_value_estimated_bytes;

impl FriggMcpServer {
    pub(super) fn invalidate_repository_navigation_caches(&self, repository_id: &str) {
        self.cache_state
            .heuristic_reference_cache_epoch
            .fetch_add(1, Ordering::Relaxed);
        let mut heuristic_reference_cache = self
            .cache_state
            .heuristic_reference_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = heuristic_reference_cache.len();
        heuristic_reference_cache.retain(|key, _| key.repository_id != repository_id);
        self.record_runtime_cache_event(
            RuntimeCacheFamily::HeuristicReference,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(heuristic_reference_cache.len()),
        );
    }

    pub(super) fn cached_heuristic_references(
        &self,
        cache_key: &HeuristicReferenceCacheKey,
    ) -> Option<CachedHeuristicReferences> {
        let cached = self
            .cache_state
            .heuristic_reference_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::HeuristicReference,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn cache_heuristic_references(
        &self,
        cache_key: HeuristicReferenceCacheKey,
        references: Vec<HeuristicReference>,
        source_files_discovered: usize,
        source_read_diagnostics_count: usize,
        source_files_loaded: usize,
        source_bytes_loaded: u64,
    ) {
        self.cache_heuristic_references_observing_epoch(
            cache_key,
            references,
            source_files_discovered,
            source_read_diagnostics_count,
            source_files_loaded,
            source_bytes_loaded,
            None,
        );
    }

    pub(super) fn cache_heuristic_references_observing_epoch(
        &self,
        cache_key: HeuristicReferenceCacheKey,
        references: Vec<HeuristicReference>,
        source_files_discovered: usize,
        source_read_diagnostics_count: usize,
        source_files_loaded: usize,
        source_bytes_loaded: u64,
        cache_epoch: Option<u64>,
    ) {
        if let Some(cache_epoch) = cache_epoch {
            if self
                .cache_state
                .heuristic_reference_cache_epoch
                .load(Ordering::Relaxed)
                != cache_epoch
            {
                return;
            }
        }

        let mut cache = self
            .cache_state
            .heuristic_reference_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedHeuristicReferences {
                    references: Arc::new(references),
                    source_files_discovered,
                    source_read_diagnostics_count,
                    source_files_loaded,
                    source_bytes_loaded,
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::HeuristicReference,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_budget(
            RuntimeCacheFamily::HeuristicReference,
            &mut cache,
            |_, entry| {
                serialized_value_estimated_bytes(entry.references.as_ref())
                    .saturating_add(entry.source_files_loaded.saturating_mul(32))
            },
        );
    }
}
