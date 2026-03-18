use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection, Storage,
    SubtreeCoverageProjection, resolve_provenance_db_path,
};

use super::intent::HybridRankingIntent;
use super::lexical_channel::{
    HybridPathWitnessQueryContext, hybrid_path_witness_recall_score,
    hybrid_path_witness_recall_score_for_projection,
};
use super::overlay_projection::{
    PathOverlayBoost, StoredEntrypointSurfaceProjection,
    accumulate_companion_surface_overlay_boosts, accumulate_relation_overlay_boosts,
    accumulate_test_subject_overlay_boosts, entrypoint_surface_overlay_boost,
};
use super::path_witness_projection::{
    GenericWitnessSurfaceFamily, generic_surface_families_from_bits,
    generic_surface_family_from_name,
};
use super::retrieval_projection::{
    PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION, PATH_RELATION_PROJECTION_HEURISTIC_VERSION,
    PATH_SURFACE_TERM_PROJECTION_HEURISTIC_VERSION, RETRIEVAL_PROJECTION_FAMILY_PATH_ANCHOR_SKETCH,
    RETRIEVAL_PROJECTION_FAMILY_PATH_RELATION, RETRIEVAL_PROJECTION_FAMILY_PATH_SURFACE_TERM,
    RETRIEVAL_PROJECTION_FAMILY_SUBTREE_COVERAGE, SUBTREE_COVERAGE_PROJECTION_HEURISTIC_VERSION,
    augment_path_relation_projection_records_with_ast_relation_evidence,
    build_path_anchor_sketch_projection_records, build_path_relation_projection_records,
    build_path_surface_term_projection_records, build_subtree_coverage_projection_records,
    normalize_path_relation_projection_records,
};
use super::{
    ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION, HybridPathWitnessProjectionCacheKey,
    PATH_WITNESS_PROJECTION_HEURISTIC_VERSION, PathWitnessCandidate,
    RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE, RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS,
    RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT, RepositoryCandidateUniverse,
    StoredPathWitnessProjection, StoredTestSubjectProjection,
    TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION,
    build_entrypoint_surface_projection_records_from_paths,
    build_path_witness_projection_records_from_paths,
    build_test_subject_projection_records_from_paths, decode_entrypoint_surface_projection_records,
    decode_path_witness_projection_records, decode_test_subject_projection_records,
    normalize_repository_relative_path,
};

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

    #[cfg(test)]
    pub(super) fn load_or_build_path_witness_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredPathWitnessProjection>>> {
        self.load_path_witness_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Repairing,
        )
    }

    fn load_read_only_path_witness_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredPathWitnessProjection>>> {
        self.load_path_witness_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Retrieval,
        )
    }

    fn load_path_witness_projections_for_repository_with_mode(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        mode: ProjectionLoadMode,
    ) -> Option<Arc<Vec<StoredPathWitnessProjection>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .path_witness_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        let live_fallback = || {
            let candidate_paths = repository_candidate_paths(repository);
            if candidate_paths.is_empty() {
                return None;
            }

            let rows = build_path_witness_projection_records_from_paths(&candidate_paths).ok()?;
            let projections = Arc::new(decode_path_witness_projection_records(&rows).ok()?);
            if matches!(mode, ProjectionLoadMode::Repairing) {
                self.path_witness_cache
                    .write()
                    .ok()?
                    .insert(cache_key.clone(), Arc::clone(&projections));
            }
            Some(projections)
        };

        let Ok(db_path) = resolve_provenance_db_path(&repository.root) else {
            return live_fallback();
        };
        if !db_path.exists() {
            return live_fallback();
        }

        let storage = Storage::new(db_path);
        if let Some(head) = storage
            .load_retrieval_projection_head_for_repository_snapshot_family(
                &repository.repository_id,
                snapshot_id,
                RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS,
            )
            .ok()
            .flatten()
            .filter(|head| head.heuristic_version == PATH_WITNESS_PROJECTION_HEURISTIC_VERSION)
        {
            let rows = storage
                .load_path_witness_projections_for_repository_snapshot(
                    &repository.repository_id,
                    snapshot_id,
                )
                .ok()?;
            if rows.len() == head.row_count {
                let projections = Arc::new(decode_path_witness_projection_records(&rows).ok()?);
                self.path_witness_cache
                    .write()
                    .ok()?
                    .insert(cache_key, Arc::clone(&projections));
                return Some(projections);
            }
        }

        if matches!(mode, ProjectionLoadMode::Retrieval) {
            return live_fallback();
        }

        let expected_paths =
            projection_source_paths_for_repository(repository, &storage, snapshot_id);
        if expected_paths.is_empty() {
            return None;
        }

        let mut rows = storage
            .load_path_witness_projections_for_repository_snapshot(
                &repository.repository_id,
                snapshot_id,
            )
            .ok()?;
        let has_expected_rows = rows.len() == expected_paths.len()
            && rows
                .iter()
                .map(|row| row.path.as_str())
                .eq(expected_paths.iter().map(String::as_str));
        if !has_expected_rows {
            rows = build_path_witness_projection_records_from_paths(&expected_paths).ok()?;
            storage
                .replace_path_witness_projections_for_repository_snapshot(
                    &repository.repository_id,
                    snapshot_id,
                    &rows,
                )
                .ok()?;
        }

        let projections = Arc::new(decode_path_witness_projection_records(&rows).ok()?);
        self.path_witness_cache
            .write()
            .ok()?
            .insert(cache_key, Arc::clone(&projections));
        Some(projections)
    }

    #[cfg(test)]
    pub(super) fn load_or_build_test_subject_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredTestSubjectProjection>>> {
        self.load_test_subject_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Repairing,
        )
    }

    fn load_read_only_test_subject_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredTestSubjectProjection>>> {
        self.load_test_subject_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Retrieval,
        )
    }

    fn load_test_subject_projections_for_repository_with_mode(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        mode: ProjectionLoadMode,
    ) -> Option<Arc<Vec<StoredTestSubjectProjection>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .test_subject_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        let live_fallback = || {
            let candidate_paths = repository_candidate_paths(repository);
            if candidate_paths.is_empty() {
                return None;
            }
            let rows = build_test_subject_projection_records_from_paths(&candidate_paths).ok()?;
            let projections = Arc::new(decode_test_subject_projection_records(&rows).ok()?);
            if matches!(mode, ProjectionLoadMode::Repairing) {
                self.test_subject_cache
                    .write()
                    .ok()?
                    .insert(cache_key.clone(), Arc::clone(&projections));
            }
            Some(projections)
        };

        let Ok(db_path) = resolve_provenance_db_path(&repository.root) else {
            return matches!(mode, ProjectionLoadMode::Retrieval)
                .then(live_fallback)
                .flatten();
        };
        if !db_path.exists() {
            return matches!(mode, ProjectionLoadMode::Retrieval)
                .then(live_fallback)
                .flatten();
        }

        let storage = Storage::new(db_path);
        if let Some(head) = storage
            .load_retrieval_projection_head_for_repository_snapshot_family(
                &repository.repository_id,
                snapshot_id,
                RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT,
            )
            .ok()
            .flatten()
            .filter(|head| head.heuristic_version == TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION)
        {
            let rows = storage
                .load_test_subject_projections_for_repository_snapshot(
                    &repository.repository_id,
                    snapshot_id,
                )
                .ok()?;
            if rows.len() == head.row_count {
                let projections = Arc::new(decode_test_subject_projection_records(&rows).ok()?);
                self.test_subject_cache
                    .write()
                    .ok()?
                    .insert(cache_key, Arc::clone(&projections));
                return Some(projections);
            }
        }

        if matches!(mode, ProjectionLoadMode::Retrieval) {
            return live_fallback();
        }

        let expected_paths =
            projection_source_paths_for_repository(repository, &storage, snapshot_id);
        if expected_paths.is_empty() {
            return None;
        }
        let expected_rows =
            { build_test_subject_projection_records_from_paths(&expected_paths).ok()? };
        let mut rows = storage
            .load_test_subject_projections_for_repository_snapshot(
                &repository.repository_id,
                snapshot_id,
            )
            .ok()?;
        if rows != expected_rows {
            storage
                .replace_test_subject_projections_for_repository_snapshot(
                    &repository.repository_id,
                    snapshot_id,
                    &expected_rows,
                )
                .ok()?;
            rows = expected_rows;
        }

        let projections = Arc::new(decode_test_subject_projection_records(&rows).ok()?);
        self.test_subject_cache
            .write()
            .ok()?
            .insert(cache_key, Arc::clone(&projections));
        Some(projections)
    }

    #[cfg(test)]
    pub(super) fn load_or_build_entrypoint_surface_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredEntrypointSurfaceProjection>>> {
        self.load_entrypoint_surface_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Repairing,
        )
    }

    fn load_read_only_entrypoint_surface_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredEntrypointSurfaceProjection>>> {
        self.load_entrypoint_surface_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Retrieval,
        )
    }

    fn load_entrypoint_surface_projections_for_repository_with_mode(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        mode: ProjectionLoadMode,
    ) -> Option<Arc<Vec<StoredEntrypointSurfaceProjection>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .entrypoint_surface_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        let live_fallback = || {
            let candidate_paths = repository_candidate_paths(repository);
            if candidate_paths.is_empty() {
                return None;
            }
            let rows =
                build_entrypoint_surface_projection_records_from_paths(&candidate_paths).ok()?;
            let projections = Arc::new(decode_entrypoint_surface_projection_records(&rows).ok()?);
            if matches!(mode, ProjectionLoadMode::Repairing) {
                self.entrypoint_surface_cache
                    .write()
                    .ok()?
                    .insert(cache_key.clone(), Arc::clone(&projections));
            }
            Some(projections)
        };

        let Ok(db_path) = resolve_provenance_db_path(&repository.root) else {
            return matches!(mode, ProjectionLoadMode::Retrieval)
                .then(live_fallback)
                .flatten();
        };
        if !db_path.exists() {
            return matches!(mode, ProjectionLoadMode::Retrieval)
                .then(live_fallback)
                .flatten();
        }

        let storage = Storage::new(db_path);
        if let Some(head) = storage
            .load_retrieval_projection_head_for_repository_snapshot_family(
                &repository.repository_id,
                snapshot_id,
                RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE,
            )
            .ok()
            .flatten()
            .filter(|head| {
                head.heuristic_version == ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION
            })
        {
            let rows = storage
                .load_entrypoint_surface_projections_for_repository_snapshot(
                    &repository.repository_id,
                    snapshot_id,
                )
                .ok()?;
            if rows.len() == head.row_count {
                let projections =
                    Arc::new(decode_entrypoint_surface_projection_records(&rows).ok()?);
                self.entrypoint_surface_cache
                    .write()
                    .ok()?
                    .insert(cache_key, Arc::clone(&projections));
                return Some(projections);
            }
        }

        if matches!(mode, ProjectionLoadMode::Retrieval) {
            return live_fallback();
        }

        let expected_paths =
            projection_source_paths_for_repository(repository, &storage, snapshot_id);
        if expected_paths.is_empty() {
            return None;
        }
        let expected_rows =
            { build_entrypoint_surface_projection_records_from_paths(&expected_paths).ok()? };
        let mut rows = storage
            .load_entrypoint_surface_projections_for_repository_snapshot(
                &repository.repository_id,
                snapshot_id,
            )
            .ok()?;
        if rows != expected_rows {
            storage
                .replace_entrypoint_surface_projections_for_repository_snapshot(
                    &repository.repository_id,
                    snapshot_id,
                    &expected_rows,
                )
                .ok()?;
            rows = expected_rows;
        }

        let projections = Arc::new(decode_entrypoint_surface_projection_records(&rows).ok()?);
        self.entrypoint_surface_cache
            .write()
            .ok()?
            .insert(cache_key, Arc::clone(&projections));
        Some(projections)
    }

    #[allow(dead_code)]
    fn load_or_build_path_relation_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<PathRelationProjection>>> {
        self.load_path_relation_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Repairing,
        )
    }

    fn load_read_only_path_relation_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<PathRelationProjection>>> {
        self.load_path_relation_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Retrieval,
        )
    }

    fn load_path_relation_projections_for_repository_with_mode(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        mode: ProjectionLoadMode,
    ) -> Option<Arc<Vec<PathRelationProjection>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .path_relation_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        if let Ok(db_path) = resolve_provenance_db_path(&repository.root) {
            if db_path.exists() {
                let storage = Storage::new(db_path);
                if let Some(head) = storage
                    .load_retrieval_projection_head_for_repository_snapshot_family(
                        &repository.repository_id,
                        snapshot_id,
                        RETRIEVAL_PROJECTION_FAMILY_PATH_RELATION,
                    )
                    .ok()
                    .flatten()
                    .filter(|head| {
                        head.heuristic_version == PATH_RELATION_PROJECTION_HEURISTIC_VERSION
                    })
                {
                    let rows = storage
                        .load_path_relation_projections_for_repository_snapshot(
                            &repository.repository_id,
                            snapshot_id,
                        )
                        .ok()?;
                    if rows.len() == head.row_count {
                        let projections = Arc::new(rows);
                        self.path_relation_cache
                            .write()
                            .ok()?
                            .insert(cache_key.clone(), Arc::clone(&projections));
                        return Some(projections);
                    }
                }
            }
        }

        let path_witness = self.load_path_witness_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            mode,
        )?;
        let test_subject = self
            .load_test_subject_projections_for_repository_with_mode(repository, snapshot_id, mode)
            .unwrap_or_else(|| Arc::new(Vec::new()));
        let entrypoint_surface = self
            .load_entrypoint_surface_projections_for_repository_with_mode(
                repository,
                snapshot_id,
                mode,
            )
            .unwrap_or_else(|| Arc::new(Vec::new()));
        let source_paths = repository_candidate_paths(repository);
        let absolute_source_paths = source_paths
            .iter()
            .map(|path| repository.root.join(path))
            .collect::<Vec<_>>();
        let mut rows = build_path_relation_projection_records(
            path_witness.as_ref(),
            test_subject.as_ref(),
            entrypoint_surface.as_ref(),
        );
        augment_path_relation_projection_records_with_ast_relation_evidence(
            repository.root.as_path(),
            &absolute_source_paths,
            path_witness.as_ref(),
            &mut rows,
        );
        normalize_path_relation_projection_records(&mut rows);
        let projections = Arc::new(rows);
        if matches!(mode, ProjectionLoadMode::Repairing) {
            self.path_relation_cache
                .write()
                .ok()?
                .insert(cache_key, Arc::clone(&projections));
        }
        Some(projections)
    }

    #[allow(dead_code)]
    fn load_or_build_subtree_coverage_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<SubtreeCoverageProjection>>> {
        self.load_subtree_coverage_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Repairing,
        )
    }

    fn load_read_only_subtree_coverage_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<SubtreeCoverageProjection>>> {
        self.load_subtree_coverage_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Retrieval,
        )
    }

    fn load_subtree_coverage_projections_for_repository_with_mode(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        mode: ProjectionLoadMode,
    ) -> Option<Arc<Vec<SubtreeCoverageProjection>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .subtree_coverage_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        if let Ok(db_path) = resolve_provenance_db_path(&repository.root) {
            if db_path.exists() {
                let storage = Storage::new(db_path);
                if let Some(head) = storage
                    .load_retrieval_projection_head_for_repository_snapshot_family(
                        &repository.repository_id,
                        snapshot_id,
                        RETRIEVAL_PROJECTION_FAMILY_SUBTREE_COVERAGE,
                    )
                    .ok()
                    .flatten()
                    .filter(|head| {
                        head.heuristic_version == SUBTREE_COVERAGE_PROJECTION_HEURISTIC_VERSION
                    })
                {
                    let rows = storage
                        .load_subtree_coverage_projections_for_repository_snapshot(
                            &repository.repository_id,
                            snapshot_id,
                        )
                        .ok()?;
                    if rows.len() == head.row_count {
                        let projections = Arc::new(rows);
                        self.subtree_coverage_cache
                            .write()
                            .ok()?
                            .insert(cache_key.clone(), Arc::clone(&projections));
                        return Some(projections);
                    }
                }
            }
        }

        let path_witness = self.load_path_witness_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            mode,
        )?;
        let projections = Arc::new(build_subtree_coverage_projection_records(
            path_witness.as_ref(),
        ));
        if matches!(mode, ProjectionLoadMode::Repairing) {
            self.subtree_coverage_cache
                .write()
                .ok()?
                .insert(cache_key, Arc::clone(&projections));
        }
        Some(projections)
    }

    #[allow(dead_code)]
    fn load_or_build_path_surface_term_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<BTreeMap<String, PathSurfaceTermProjection>>> {
        self.load_path_surface_term_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Repairing,
        )
    }

    fn load_read_only_path_surface_term_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<BTreeMap<String, PathSurfaceTermProjection>>> {
        self.load_path_surface_term_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Retrieval,
        )
    }

    fn load_path_surface_term_projections_for_repository_with_mode(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        mode: ProjectionLoadMode,
    ) -> Option<Arc<BTreeMap<String, PathSurfaceTermProjection>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .path_surface_term_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        if let Ok(db_path) = resolve_provenance_db_path(&repository.root) {
            if db_path.exists() {
                let storage = Storage::new(db_path);
                if let Some(head) = storage
                    .load_retrieval_projection_head_for_repository_snapshot_family(
                        &repository.repository_id,
                        snapshot_id,
                        RETRIEVAL_PROJECTION_FAMILY_PATH_SURFACE_TERM,
                    )
                    .ok()
                    .flatten()
                    .filter(|head| {
                        head.heuristic_version == PATH_SURFACE_TERM_PROJECTION_HEURISTIC_VERSION
                    })
                {
                    let rows = storage
                        .load_path_surface_term_projections_for_repository_snapshot(
                            &repository.repository_id,
                            snapshot_id,
                        )
                        .ok()?;
                    if rows.len() == head.row_count {
                        let projections = Arc::new(
                            rows.into_iter()
                                .map(|row| (row.path.clone(), row))
                                .collect::<BTreeMap<_, _>>(),
                        );
                        self.path_surface_term_cache
                            .write()
                            .ok()?
                            .insert(cache_key.clone(), Arc::clone(&projections));
                        return Some(projections);
                    }
                }
            }
        }

        let path_witness = self.load_path_witness_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            mode,
        )?;
        let entrypoint_surface = self
            .load_entrypoint_surface_projections_for_repository_with_mode(
                repository,
                snapshot_id,
                mode,
            )
            .unwrap_or_else(|| Arc::new(Vec::new()));
        let projections = Arc::new(
            build_path_surface_term_projection_records(
                path_witness.as_ref(),
                entrypoint_surface.as_ref(),
            )
            .into_iter()
            .map(|row| (row.path.clone(), row))
            .collect::<BTreeMap<_, _>>(),
        );
        if matches!(mode, ProjectionLoadMode::Repairing) {
            self.path_surface_term_cache
                .write()
                .ok()?
                .insert(cache_key, Arc::clone(&projections));
        }
        Some(projections)
    }

    #[allow(dead_code)]
    fn load_or_build_path_anchor_sketches_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<BTreeMap<String, Vec<PathAnchorSketchProjection>>>> {
        self.load_path_anchor_sketches_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Repairing,
        )
    }

    fn load_read_only_path_anchor_sketches_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<BTreeMap<String, Vec<PathAnchorSketchProjection>>>> {
        self.load_path_anchor_sketches_for_repository_with_mode(
            repository,
            snapshot_id,
            ProjectionLoadMode::Retrieval,
        )
    }

    fn load_path_anchor_sketches_for_repository_with_mode(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        mode: ProjectionLoadMode,
    ) -> Option<Arc<BTreeMap<String, Vec<PathAnchorSketchProjection>>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .path_anchor_sketch_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        if let Ok(db_path) = resolve_provenance_db_path(&repository.root) {
            if db_path.exists() {
                let storage = Storage::new(db_path);
                if let Some(head) = storage
                    .load_retrieval_projection_head_for_repository_snapshot_family(
                        &repository.repository_id,
                        snapshot_id,
                        RETRIEVAL_PROJECTION_FAMILY_PATH_ANCHOR_SKETCH,
                    )
                    .ok()
                    .flatten()
                    .filter(|head| {
                        head.heuristic_version == PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION
                    })
                {
                    let rows = storage
                        .load_path_anchor_sketch_projections_for_repository_snapshot(
                            &repository.repository_id,
                            snapshot_id,
                        )
                        .ok()?;
                    if rows.len() == head.row_count {
                        let projections = Arc::new(group_anchor_sketches_by_path(rows));
                        self.path_anchor_sketch_cache
                            .write()
                            .ok()?
                            .insert(cache_key.clone(), Arc::clone(&projections));
                        return Some(projections);
                    }
                }
            }
        }

        let path_witness = self.load_path_witness_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            mode,
        )?;
        let path_surface_terms = self.load_path_surface_term_projections_for_repository_with_mode(
            repository,
            snapshot_id,
            mode,
        )?;
        let path_surface_terms = path_surface_terms.values().cloned().collect::<Vec<_>>();
        let projections = Arc::new(group_anchor_sketches_by_path(
            build_path_anchor_sketch_projection_records(
                &repository.root,
                path_witness.as_ref(),
                &path_surface_terms,
            ),
        ));
        if matches!(mode, ProjectionLoadMode::Repairing) {
            self.path_anchor_sketch_cache
                .write()
                .ok()?
                .insert(cache_key, Arc::clone(&projections));
        }
        Some(projections)
    }

    pub(super) fn load_projected_graph_context_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
    ) -> Option<ProjectedGraphContext> {
        let snapshot_id = repository.snapshot_id.as_deref()?;
        let relations =
            self.load_read_only_path_relation_projections_for_repository(repository, snapshot_id)?;
        Some(ProjectedGraphContext {
            adjacency_by_path: self.load_projected_graph_adjacency_for_repository(
                repository,
                snapshot_id,
                relations.as_ref(),
            )?,
            relations,
            surface_terms_by_path: self
                .load_read_only_path_surface_term_projections_for_repository(
                    repository,
                    snapshot_id,
                )?,
            anchors_by_path: self
                .load_read_only_path_anchor_sketches_for_repository(repository, snapshot_id)?,
        })
    }

    fn load_projected_graph_adjacency_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
        relations: &[PathRelationProjection],
    ) -> Option<Arc<BTreeMap<String, Vec<ProjectedGraphAdjacentRelation>>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .projected_graph_adjacency_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        let adjacency = Arc::new(build_projected_graph_adjacency_index(relations));
        self.projected_graph_adjacency_cache
            .write()
            .ok()?
            .insert(cache_key, Arc::clone(&adjacency));
        Some(adjacency)
    }

    pub(super) fn projected_path_witness_candidates_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        base_repository: Option<&RepositoryCandidateUniverse>,
        intent: &HybridRankingIntent,
        query_context: &HybridPathWitnessQueryContext,
    ) -> Option<Vec<PathWitnessCandidate>> {
        let base_repository = base_repository.unwrap_or(repository);
        let projections = if let Some(snapshot_id) = base_repository.snapshot_id.as_deref() {
            self.load_read_only_path_witness_projections_for_repository(
                base_repository,
                snapshot_id,
            )?
        } else {
            live_path_witness_projections_for_repository(base_repository)
        };
        let base_candidates_by_path = base_repository
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.relative_path.clone(),
                    candidate.absolute_path.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let projections_by_path = projections
            .iter()
            .map(|projection| (projection.path.clone(), projection))
            .collect::<BTreeMap<_, _>>();
        if base_candidates_by_path
            .keys()
            .any(|path| !projections_by_path.contains_key(path))
        {
            return None;
        }
        let surface_terms_by_path =
            base_repository
                .snapshot_id
                .as_deref()
                .and_then(|snapshot_id| {
                    self.load_read_only_path_surface_term_projections_for_repository(
                        base_repository,
                        snapshot_id,
                    )
                });

        let overlay_boosts_by_path = self.overlay_boosts_for_repository(
            repository,
            Some(base_repository),
            intent,
            query_context,
        );
        let mut scored = Vec::new();
        for (rel_path, path) in &base_candidates_by_path {
            let projection = projections_by_path.get(rel_path)?;
            let base_score = hybrid_path_witness_recall_score_for_projection(
                rel_path,
                projection,
                intent,
                query_context,
            );
            let (surface_term_bonus, mut surface_term_provenance_ids) = path_surface_term_bonus(
                surface_terms_by_path
                    .as_ref()
                    .and_then(|surface_terms| surface_terms.get(rel_path)),
                query_context,
            );
            let overlay_boost = overlay_boosts_by_path
                .get(rel_path)
                .cloned()
                .unwrap_or_default();
            let Some(score) = base_score
                .map(|score| score + surface_term_bonus + overlay_boost.bonus_score())
                .or_else(|| {
                    (overlay_boost.bonus_millis > 0 || surface_term_bonus > 0.0)
                        .then_some(surface_term_bonus + overlay_boost.bonus_score())
                })
            else {
                continue;
            };
            surface_term_provenance_ids.extend(overlay_boost.provenance_ids.clone());
            scored.push(PathWitnessCandidate {
                score,
                repository_id: repository.repository_id.clone(),
                rel_path: rel_path.clone(),
                path: path.clone(),
                witness_provenance_ids: surface_term_provenance_ids,
            });
        }

        for candidate in &repository.candidates {
            if base_candidates_by_path.contains_key(&candidate.relative_path) {
                continue;
            }
            let base_score =
                hybrid_path_witness_recall_score(&candidate.relative_path, intent, query_context);
            let (surface_term_bonus, mut surface_term_provenance_ids) = path_surface_term_bonus(
                surface_terms_by_path
                    .as_ref()
                    .and_then(|surface_terms| surface_terms.get(&candidate.relative_path)),
                query_context,
            );
            let overlay_boost = overlay_boosts_by_path
                .get(&candidate.relative_path)
                .cloned()
                .unwrap_or_default();
            let Some(score) = base_score
                .map(|score| score + surface_term_bonus + overlay_boost.bonus_score())
                .or_else(|| {
                    (overlay_boost.bonus_millis > 0 || surface_term_bonus > 0.0)
                        .then_some(surface_term_bonus + overlay_boost.bonus_score())
                })
            else {
                continue;
            };
            surface_term_provenance_ids.extend(overlay_boost.provenance_ids.clone());
            scored.push(PathWitnessCandidate {
                score,
                repository_id: repository.repository_id.clone(),
                rel_path: candidate.relative_path.clone(),
                path: candidate.absolute_path.clone(),
                witness_provenance_ids: surface_term_provenance_ids,
            });
        }

        Some(scored)
    }

    pub(super) fn overlay_boosts_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        base_repository: Option<&RepositoryCandidateUniverse>,
        intent: &HybridRankingIntent,
        query_context: &HybridPathWitnessQueryContext,
    ) -> BTreeMap<String, PathOverlayBoost> {
        let mut overlay_boosts_by_path = BTreeMap::<String, PathOverlayBoost>::new();
        let base_repository = base_repository.unwrap_or(repository);

        if intent.wants_tests || intent.wants_test_witness_recall {
            if let Some(snapshot_id) = base_repository.snapshot_id.as_deref() {
                if let Some(test_subject_projections) = self
                    .load_read_only_test_subject_projections_for_repository(
                        base_repository,
                        snapshot_id,
                    )
                {
                    for (path, boost) in accumulate_test_subject_overlay_boosts(
                        test_subject_projections.as_ref(),
                        intent,
                        query_context,
                    ) {
                        merge_path_overlay_boost(&mut overlay_boosts_by_path, path, boost);
                    }
                }
            }
        }

        if let Some(snapshot_id) = base_repository.snapshot_id.as_deref() {
            let relation_overlay_applied = match (
                self.load_read_only_path_relation_projections_for_repository(
                    base_repository,
                    snapshot_id,
                ),
                self.load_read_only_path_surface_term_projections_for_repository(
                    base_repository,
                    snapshot_id,
                ),
            ) {
                (Some(path_relations), Some(path_surface_terms)) => {
                    for (path, boost) in accumulate_relation_overlay_boosts(
                        path_relations.as_ref(),
                        path_surface_terms.as_ref(),
                        intent,
                        query_context,
                    ) {
                        merge_path_overlay_boost(&mut overlay_boosts_by_path, path, boost);
                    }
                    true
                }
                _ => false,
            };

            if !relation_overlay_applied {
                if let Some(path_witness_projections) = self
                    .load_read_only_path_witness_projections_for_repository(
                        base_repository,
                        snapshot_id,
                    )
                {
                    for (path, boost) in accumulate_companion_surface_overlay_boosts(
                        path_witness_projections.as_ref(),
                        intent,
                        query_context,
                    ) {
                        merge_path_overlay_boost(&mut overlay_boosts_by_path, path, boost);
                    }
                }
            }
        } else {
            let path_witness_projections =
                live_path_witness_projections_for_repository(base_repository);
            for (path, boost) in accumulate_companion_surface_overlay_boosts(
                path_witness_projections.as_ref(),
                intent,
                query_context,
            ) {
                merge_path_overlay_boost(&mut overlay_boosts_by_path, path, boost);
            }
        }

        let wants_entrypoint_overlay = intent.wants_entrypoint_build_flow
            || intent.wants_runtime_config_artifacts
            || intent.wants_ci_workflow_witnesses
            || intent.wants_scripts_ops_witnesses;
        if !wants_entrypoint_overlay {
            return overlay_boosts_by_path;
        }

        let mut stored_projection_paths = BTreeSet::<String>::new();
        if let Some(snapshot_id) = base_repository.snapshot_id.as_deref() {
            if let Some(entrypoint_surface_projections) = self
                .load_read_only_entrypoint_surface_projections_for_repository(
                    base_repository,
                    snapshot_id,
                )
            {
                for projection in entrypoint_surface_projections.iter() {
                    stored_projection_paths.insert(projection.path.clone());
                    if let Some(boost) =
                        entrypoint_surface_overlay_boost(projection, intent, query_context)
                    {
                        merge_path_overlay_boost(
                            &mut overlay_boosts_by_path,
                            projection.path.clone(),
                            boost,
                        );
                    }
                }
            }
        }

        for candidate in &repository.candidates {
            if stored_projection_paths.contains(&candidate.relative_path) {
                continue;
            }
            if let Some(projection) =
                StoredEntrypointSurfaceProjection::from_path(&candidate.relative_path)
            {
                if let Some(boost) =
                    entrypoint_surface_overlay_boost(&projection, intent, query_context)
                {
                    merge_path_overlay_boost(
                        &mut overlay_boosts_by_path,
                        candidate.relative_path.clone(),
                        boost,
                    );
                }
            }
        }

        overlay_boosts_by_path
    }

    pub(super) fn coverage_hint_keys_for_repositories(
        &self,
        repositories: &[RepositoryCandidateUniverse],
    ) -> BTreeMap<(String, String), Vec<(GenericWitnessSurfaceFamily, String)>> {
        let mut hints =
            BTreeMap::<(String, String), Vec<(GenericWitnessSurfaceFamily, String)>>::new();
        for repository in repositories {
            let Some(snapshot_id) = repository.snapshot_id.as_deref() else {
                continue;
            };
            let Some(rows) = self.load_read_only_subtree_coverage_projections_for_repository(
                repository,
                snapshot_id,
            ) else {
                continue;
            };
            for row in rows.iter() {
                let Some(family) = generic_surface_family_from_name(&row.family) else {
                    continue;
                };
                hints
                    .entry((repository.repository_id.clone(), row.exemplar_path.clone()))
                    .or_default()
                    .push((family, row.subtree_root.clone()));
            }

            if let Some(rows) = self
                .load_read_only_path_relation_projections_for_repository(repository, snapshot_id)
            {
                for row in rows.iter() {
                    if !Self::coverage_row_relevant_relation_kind(&row.relation_kind) {
                        continue;
                    }

                    let src_families = generic_surface_families_from_bits(row.src_family_bits);
                    let dst_families = generic_surface_families_from_bits(row.dst_family_bits);
                    let src_projection = StoredPathWitnessProjection::from_path(&row.src_path);
                    let dst_projection = StoredPathWitnessProjection::from_path(&row.dst_path);

                    if let Some(subtree_root) = src_projection.subtree_root.as_deref() {
                        for family in &dst_families {
                            hints
                                .entry((repository.repository_id.clone(), row.src_path.clone()))
                                .or_default()
                                .push((*family, subtree_root.to_owned()));
                        }
                    }

                    if let Some(subtree_root) = dst_projection.subtree_root.as_deref() {
                        for family in &src_families {
                            hints
                                .entry((repository.repository_id.clone(), row.dst_path.clone()))
                                .or_default()
                                .push((*family, subtree_root.to_owned()));
                        }
                    }
                }
            }
        }

        for values in hints.values_mut() {
            values.sort();
            values.dedup();
        }
        hints
    }

    fn coverage_row_relevant_relation_kind(relation_kind: &str) -> bool {
        relation_kind == "companion_surface"
    }

    pub(super) fn best_path_witness_anchor_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        rel_path: &str,
        query_context: &HybridPathWitnessQueryContext,
    ) -> Option<(usize, String)> {
        let snapshot_id = repository.snapshot_id.as_deref()?;
        let sketches =
            self.load_read_only_path_anchor_sketches_for_repository(repository, snapshot_id)?;
        let anchors = sketches.get(rel_path)?;
        anchors
            .iter()
            .filter_map(|anchor| {
                let overlap = anchor
                    .terms
                    .iter()
                    .filter(|term| {
                        query_context
                            .query_overlap_terms
                            .iter()
                            .any(|candidate| candidate == *term)
                    })
                    .count() as u32;
                let exact = query_context
                    .exact_terms
                    .iter()
                    .any(|term| anchor.excerpt.to_ascii_lowercase().contains(term.as_str()));
                let score = overlap
                    .saturating_mul(6)
                    .saturating_add(u32::from(exact).saturating_mul(8))
                    .saturating_add(anchor.score_hint.min(64) as u32);
                (score > 0).then_some((
                    score,
                    anchor.line,
                    anchor.excerpt.clone(),
                    anchor.anchor_rank,
                ))
            })
            .max_by(|left, right| {
                left.0
                    .cmp(&right.0)
                    .then_with(|| right.1.cmp(&left.1))
                    .then_with(|| right.3.cmp(&left.3))
            })
            .map(|(_, line, excerpt, _)| (line, excerpt))
    }
}

fn merge_path_overlay_boost(
    boosts_by_path: &mut BTreeMap<String, PathOverlayBoost>,
    path: String,
    boost: PathOverlayBoost,
) {
    boosts_by_path.entry(path).or_default().merge(boost);
}

fn projection_source_paths_for_repository(
    repository: &RepositoryCandidateUniverse,
    storage: &Storage,
    snapshot_id: &str,
) -> Vec<String> {
    let candidate_paths = repository_candidate_paths(repository);
    let mut manifest_paths = storage
        .load_manifest_for_snapshot(snapshot_id)
        .ok()
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| {
                    normalize_repository_relative_path(&repository.root, Path::new(&entry.path))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    manifest_paths.sort();
    manifest_paths.dedup();

    if manifest_paths.is_empty()
        || candidate_paths
            .iter()
            .any(|path| manifest_paths.binary_search(path).is_err())
    {
        return candidate_paths;
    }

    manifest_paths
}

fn repository_candidate_paths(repository: &RepositoryCandidateUniverse) -> Vec<String> {
    let mut candidate_paths = repository
        .candidates
        .iter()
        .map(|candidate| candidate.relative_path.clone())
        .collect::<Vec<_>>();
    candidate_paths.sort();
    candidate_paths.dedup();
    candidate_paths
}

fn live_path_witness_projections_for_repository(
    repository: &RepositoryCandidateUniverse,
) -> Arc<Vec<StoredPathWitnessProjection>> {
    Arc::new(
        repository_candidate_paths(repository)
            .into_iter()
            .map(|path| StoredPathWitnessProjection::from_path(&path))
            .collect(),
    )
}

fn group_anchor_sketches_by_path(
    rows: Vec<PathAnchorSketchProjection>,
) -> BTreeMap<String, Vec<PathAnchorSketchProjection>> {
    let mut grouped = BTreeMap::<String, Vec<PathAnchorSketchProjection>>::new();
    for row in rows {
        grouped.entry(row.path.clone()).or_default().push(row);
    }
    for anchors in grouped.values_mut() {
        anchors.sort_by(|left, right| {
            left.anchor_rank
                .cmp(&right.anchor_rank)
                .then(left.line.cmp(&right.line))
        });
    }
    grouped
}

fn build_projected_graph_adjacency_index(
    relations: &[PathRelationProjection],
) -> BTreeMap<String, Vec<ProjectedGraphAdjacentRelation>> {
    let mut adjacency = BTreeMap::<String, Vec<ProjectedGraphAdjacentRelation>>::new();
    for (relation_index, relation) in relations.iter().enumerate() {
        adjacency
            .entry(relation.src_path.clone())
            .or_default()
            .push(ProjectedGraphAdjacentRelation {
                direction_rank: 0,
                target_path: relation.dst_path.clone(),
                relation_index,
            });
        adjacency
            .entry(relation.dst_path.clone())
            .or_default()
            .push(ProjectedGraphAdjacentRelation {
                direction_rank: 1,
                target_path: relation.src_path.clone(),
                relation_index,
            });
    }

    for entries in adjacency.values_mut() {
        entries.sort_by(|left, right| {
            let left_relation = &relations[left.relation_index];
            let right_relation = &relations[right.relation_index];
            projected_graph_relation_order_key(left_relation)
                .cmp(&projected_graph_relation_order_key(right_relation))
                .reverse()
                .then(left.direction_rank.cmp(&right.direction_rank))
                .then(left.target_path.cmp(&right.target_path))
        });
    }

    adjacency
}

fn projected_graph_relation_order_key(relation: &PathRelationProjection) -> (usize, &str, &str) {
    (
        relation.score_hint,
        relation.relation_kind.as_str(),
        relation.evidence_source.as_str(),
    )
}

fn path_surface_term_bonus(
    projection: Option<&PathSurfaceTermProjection>,
    query_context: &HybridPathWitnessQueryContext,
) -> (f32, Vec<String>) {
    let Some(projection) = projection else {
        return (0.0, Vec::new());
    };
    let weighted_overlap = query_context
        .query_overlap_terms
        .iter()
        .filter_map(|term| projection.term_weights.get(term))
        .map(|weight| *weight as u32)
        .sum::<u32>()
        .min(20);
    let exact_term_match = query_context.exact_terms.iter().any(|term| {
        projection
            .exact_terms
            .iter()
            .any(|candidate| candidate == term)
    });
    let bonus = weighted_overlap as f32 * 0.01 + if exact_term_match { 0.08 } else { 0.0 };
    if bonus == 0.0 {
        return (0.0, Vec::new());
    }

    let mut provenance_ids = vec![format!("projection:path_surface_term:{}", projection.path)];
    if exact_term_match {
        provenance_ids.push(format!(
            "projection:path_surface_term:exact:{}",
            projection.path
        ));
    }
    (bonus, provenance_ids)
}
