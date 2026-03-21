//! Snapshot-scoped decoded retrieval projection caches.
//!
//! The projection service keeps decoded projection families in process-wide caches so repeated
//! search and navigation requests can reuse them across requests. These caches are intentionally
//! bounded and repository-invalidated to keep long-lived MCP servers memory-stable.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection,
    SubtreeCoverageProjection,
};

use super::overlay_projection::StoredEntrypointSurfaceProjection;
use super::overlay_projection::StoredTestSubjectProjection;
use super::path_witness_projection::StoredPathWitnessProjection;
use super::types::HybridPathWitnessProjectionCacheKey;

mod loaders;
mod query;

const PROJECTION_SERVICE_CACHE_MAX_ENTRIES: usize = 16;

fn trim_projection_cache<K, V>(cache: &mut BTreeMap<K, V>)
where
    K: Ord,
{
    while cache.len() > PROJECTION_SERVICE_CACHE_MAX_ENTRIES {
        let _ = cache.pop_first();
    }
}

#[allow(clippy::type_complexity)]
#[derive(Clone, Default)]
/// Shared decoded projection cache service reused by request-scoped searchers.
pub(crate) struct ProjectionStoreService {
    path_witness_cache: Arc<
        RwLock<
            BTreeMap<HybridPathWitnessProjectionCacheKey, Arc<Vec<StoredPathWitnessProjection>>>,
        >,
    >,
    test_subject_cache: Arc<
        RwLock<
            BTreeMap<HybridPathWitnessProjectionCacheKey, Arc<Vec<StoredTestSubjectProjection>>>,
        >,
    >,
    entrypoint_surface_cache: Arc<
        RwLock<
            BTreeMap<
                HybridPathWitnessProjectionCacheKey,
                Arc<Vec<StoredEntrypointSurfaceProjection>>,
            >,
        >,
    >,
    path_relation_cache: Arc<
        RwLock<BTreeMap<HybridPathWitnessProjectionCacheKey, Arc<Vec<PathRelationProjection>>>>,
    >,
    subtree_coverage_cache: Arc<
        RwLock<BTreeMap<HybridPathWitnessProjectionCacheKey, Arc<Vec<SubtreeCoverageProjection>>>>,
    >,
    path_surface_term_cache: Arc<
        RwLock<
            BTreeMap<
                HybridPathWitnessProjectionCacheKey,
                Arc<BTreeMap<String, PathSurfaceTermProjection>>,
            >,
        >,
    >,
    path_anchor_sketch_cache: Arc<
        RwLock<
            BTreeMap<
                HybridPathWitnessProjectionCacheKey,
                Arc<BTreeMap<String, Vec<PathAnchorSketchProjection>>>,
            >,
        >,
    >,
    projected_graph_adjacency_cache: Arc<
        RwLock<
            BTreeMap<
                HybridPathWitnessProjectionCacheKey,
                Arc<BTreeMap<String, Vec<ProjectedGraphAdjacentRelation>>>,
            >,
        >,
    >,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectedGraphAdjacentRelation {
    pub direction_rank: u8,
    pub target_path: String,
    pub relation_index: usize,
}

#[derive(Clone)]
/// Read-only projected graph view assembled from snapshot-scoped relation and surface families.
pub(super) struct ProjectedGraphContext {
    pub relations: Arc<Vec<PathRelationProjection>>,
    pub surface_terms_by_path: Arc<BTreeMap<String, PathSurfaceTermProjection>>,
    pub anchors_by_path: Arc<BTreeMap<String, Vec<PathAnchorSketchProjection>>>,
    pub adjacency_by_path: Arc<BTreeMap<String, Vec<ProjectedGraphAdjacentRelation>>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjectionLoadMode {
    Retrieval,
    Repairing,
}

impl ProjectionStoreService {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn insert_cached_projection_entry<V>(
        &self,
        cache: &Arc<RwLock<BTreeMap<HybridPathWitnessProjectionCacheKey, Arc<V>>>>,
        cache_key: HybridPathWitnessProjectionCacheKey,
        value: Arc<V>,
    ) -> Option<()> {
        let mut cache = cache.write().ok()?;
        cache.insert(cache_key, value);
        trim_projection_cache(&mut cache);
        Some(())
    }

    pub(crate) fn invalidate_repository(&self, repository_id: &str) {
        let retain_repository =
            |key: &HybridPathWitnessProjectionCacheKey| key.repository_id != repository_id;

        if let Ok(mut cache) = self.path_witness_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
        if let Ok(mut cache) = self.test_subject_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
        if let Ok(mut cache) = self.entrypoint_surface_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
        if let Ok(mut cache) = self.path_relation_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
        if let Ok(mut cache) = self.subtree_coverage_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
        if let Ok(mut cache) = self.path_surface_term_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
        if let Ok(mut cache) = self.path_anchor_sketch_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
        if let Ok(mut cache) = self.projected_graph_adjacency_cache.write() {
            cache.retain(|key, _| retain_repository(key));
        }
    }

    #[cfg(test)]
    pub(crate) fn entrypoint_surface_cache_len(&self) -> usize {
        self.entrypoint_surface_cache
            .read()
            .map(|cache| cache.len())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn path_witness_cache_len(&self) -> usize {
        self.path_witness_cache
            .read()
            .map(|cache| cache.len())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(crate) fn projected_graph_adjacency_cache_len(&self) -> usize {
        self.projected_graph_adjacency_cache
            .read()
            .map(|cache| cache.len())
            .unwrap_or_default()
    }
}
