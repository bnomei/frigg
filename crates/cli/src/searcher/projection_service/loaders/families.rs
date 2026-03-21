use std::collections::BTreeMap;
use std::sync::Arc;

use super::common::{group_anchor_sketches_by_path, projection_source_paths_for_repository};
use crate::searcher::RepositoryCandidateUniverse;
use crate::searcher::overlay_projection::{
    StoredEntrypointSurfaceProjection, StoredTestSubjectProjection,
    build_entrypoint_surface_projection_records_from_paths,
    build_test_subject_projection_records as build_test_subject_projection_records_from_paths,
    decode_entrypoint_surface_projection_records, decode_test_subject_projection_records,
};
use crate::searcher::path_witness_projection::{
    PATH_WITNESS_PROJECTION_HEURISTIC_VERSION, StoredPathWitnessProjection,
    build_path_witness_projection_records_from_paths, decode_path_witness_projection_records,
};
use crate::searcher::projection_service::{ProjectionLoadMode, ProjectionStoreService};
use crate::searcher::retrieval_projection::{
    ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION,
    PATH_ANCHOR_SKETCH_PROJECTION_HEURISTIC_VERSION, PATH_RELATION_PROJECTION_HEURISTIC_VERSION,
    PATH_SURFACE_TERM_PROJECTION_HEURISTIC_VERSION, RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE,
    RETRIEVAL_PROJECTION_FAMILY_PATH_ANCHOR_SKETCH, RETRIEVAL_PROJECTION_FAMILY_PATH_RELATION,
    RETRIEVAL_PROJECTION_FAMILY_PATH_SURFACE_TERM, RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS,
    RETRIEVAL_PROJECTION_FAMILY_SUBTREE_COVERAGE, RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT,
    SUBTREE_COVERAGE_PROJECTION_HEURISTIC_VERSION, TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION,
    augment_path_relation_projection_records_with_ast_relation_evidence,
    build_path_anchor_sketch_projection_records, build_path_relation_projection_records,
    build_path_surface_term_projection_records, build_subtree_coverage_projection_records,
    normalize_path_relation_projection_records,
};
use crate::searcher::types::HybridPathWitnessProjectionCacheKey;
use crate::storage::{
    PathAnchorSketchProjection, PathRelationProjection, PathSurfaceTermProjection, Storage,
    SubtreeCoverageProjection, resolve_provenance_db_path,
};

impl ProjectionStoreService {
    #[cfg(test)]
    pub(crate) fn load_or_build_path_witness_projections_for_repository(
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

    pub(crate) fn load_read_only_path_witness_projections_for_repository(
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
            let candidate_paths = repository
                .candidates
                .iter()
                .map(|candidate| candidate.relative_path.clone())
                .collect::<Vec<_>>();
            if candidate_paths.is_empty() {
                return None;
            }

            let rows = build_path_witness_projection_records_from_paths(&candidate_paths).ok()?;
            let projections = Arc::new(decode_path_witness_projection_records(&rows).ok()?);
            if matches!(mode, ProjectionLoadMode::Repairing) {
                self.insert_cached_projection_entry(
                    &self.path_witness_cache,
                    cache_key.clone(),
                    Arc::clone(&projections),
                )?;
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
                self.insert_cached_projection_entry(
                    &self.path_witness_cache,
                    cache_key,
                    Arc::clone(&projections),
                )?;
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
        self.insert_cached_projection_entry(
            &self.path_witness_cache,
            cache_key,
            Arc::clone(&projections),
        )?;
        Some(projections)
    }

    #[cfg(test)]
    pub(crate) fn load_or_build_test_subject_projections_for_repository(
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

    pub(crate) fn load_read_only_test_subject_projections_for_repository(
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
            let candidate_paths = repository
                .candidates
                .iter()
                .map(|candidate| candidate.relative_path.clone())
                .collect::<Vec<_>>();
            if candidate_paths.is_empty() {
                return None;
            }
            let rows = build_test_subject_projection_records_from_paths(&candidate_paths).ok()?;
            let projections = Arc::new(decode_test_subject_projection_records(&rows).ok()?);
            if matches!(mode, ProjectionLoadMode::Repairing) {
                self.insert_cached_projection_entry(
                    &self.test_subject_cache,
                    cache_key.clone(),
                    Arc::clone(&projections),
                )?;
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
                self.insert_cached_projection_entry(
                    &self.test_subject_cache,
                    cache_key,
                    Arc::clone(&projections),
                )?;
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
        self.insert_cached_projection_entry(
            &self.test_subject_cache,
            cache_key,
            Arc::clone(&projections),
        )?;
        Some(projections)
    }

    #[cfg(test)]
    pub(crate) fn load_or_build_entrypoint_surface_projections_for_repository(
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

    pub(crate) fn load_read_only_entrypoint_surface_projections_for_repository(
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
            let candidate_paths = repository
                .candidates
                .iter()
                .map(|candidate| candidate.relative_path.clone())
                .collect::<Vec<_>>();
            if candidate_paths.is_empty() {
                return None;
            }
            let rows =
                build_entrypoint_surface_projection_records_from_paths(&candidate_paths).ok()?;
            let projections = Arc::new(decode_entrypoint_surface_projection_records(&rows).ok()?);
            if matches!(mode, ProjectionLoadMode::Repairing) {
                self.insert_cached_projection_entry(
                    &self.entrypoint_surface_cache,
                    cache_key.clone(),
                    Arc::clone(&projections),
                )?;
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
                self.insert_cached_projection_entry(
                    &self.entrypoint_surface_cache,
                    cache_key,
                    Arc::clone(&projections),
                )?;
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
        self.insert_cached_projection_entry(
            &self.entrypoint_surface_cache,
            cache_key,
            Arc::clone(&projections),
        )?;
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

    pub(crate) fn load_read_only_path_relation_projections_for_repository(
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
                        self.insert_cached_projection_entry(
                            &self.path_relation_cache,
                            cache_key.clone(),
                            Arc::clone(&projections),
                        )?;
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
        let source_paths = repository
            .candidates
            .iter()
            .map(|candidate| candidate.relative_path.clone())
            .collect::<Vec<_>>();
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
            self.insert_cached_projection_entry(
                &self.path_relation_cache,
                cache_key,
                Arc::clone(&projections),
            )?;
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

    pub(crate) fn load_read_only_subtree_coverage_projections_for_repository(
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
                        self.insert_cached_projection_entry(
                            &self.subtree_coverage_cache,
                            cache_key.clone(),
                            Arc::clone(&projections),
                        )?;
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
            self.insert_cached_projection_entry(
                &self.subtree_coverage_cache,
                cache_key,
                Arc::clone(&projections),
            )?;
        }
        Some(projections)
    }

    #[allow(dead_code)]
    pub(crate) fn load_or_build_path_surface_term_projections_for_repository(
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

    pub(crate) fn load_read_only_path_surface_term_projections_for_repository(
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
                        self.insert_cached_projection_entry(
                            &self.path_surface_term_cache,
                            cache_key.clone(),
                            Arc::clone(&projections),
                        )?;
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
            self.insert_cached_projection_entry(
                &self.path_surface_term_cache,
                cache_key,
                Arc::clone(&projections),
            )?;
        }
        Some(projections)
    }

    #[allow(dead_code)]
    pub(crate) fn load_or_build_path_anchor_sketches_for_repository(
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

    pub(crate) fn load_read_only_path_anchor_sketches_for_repository(
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
                        self.insert_cached_projection_entry(
                            &self.path_anchor_sketch_cache,
                            cache_key.clone(),
                            Arc::clone(&projections),
                        )?;
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
            self.insert_cached_projection_entry(
                &self.path_anchor_sketch_cache,
                cache_key,
                Arc::clone(&projections),
            )?;
        }
        Some(projections)
    }
}
