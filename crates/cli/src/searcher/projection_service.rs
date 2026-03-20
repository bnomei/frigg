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

#[derive(Clone, Default)]
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
