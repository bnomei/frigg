use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::{Arc, RwLock};

use crate::storage::{Storage, resolve_provenance_db_path};

use super::intent::HybridRankingIntent;
use super::lexical_channel::{
    HybridPathWitnessQueryContext, hybrid_path_witness_recall_score,
    hybrid_path_witness_recall_score_for_projection,
};
use super::overlay_projection::{
    PathOverlayBoost, StoredEntrypointSurfaceProjection, accumulate_test_subject_overlay_boosts,
    entrypoint_surface_overlay_boost,
};
use super::{
    HybridPathWitnessProjectionCacheKey, PathWitnessCandidate, RepositoryCandidateUniverse,
    StoredPathWitnessProjection, StoredTestSubjectProjection,
    build_entrypoint_surface_projection_records_from_paths,
    build_path_witness_projection_records_from_paths,
    build_test_subject_projection_records_from_paths, decode_entrypoint_surface_projection_records,
    decode_path_witness_projection_records, decode_test_subject_projection_records,
    normalize_repository_relative_path,
};

#[derive(Default)]
pub(super) struct ProjectionStoreService {
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
}

impl ProjectionStoreService {
    pub(super) fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub(super) fn entrypoint_surface_cache_len(&self) -> usize {
        self.entrypoint_surface_cache
            .read()
            .map(|cache| cache.len())
            .unwrap_or_default()
    }

    #[cfg(test)]
    pub(super) fn path_witness_cache_len(&self) -> usize {
        self.path_witness_cache
            .read()
            .map(|cache| cache.len())
            .unwrap_or_default()
    }

    pub(super) fn load_or_build_path_witness_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
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

        let db_path = resolve_provenance_db_path(&repository.root).ok()?;
        if !db_path.exists() {
            return None;
        }

        let storage = Storage::new(db_path);
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

    pub(super) fn load_or_build_test_subject_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
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

        let db_path = resolve_provenance_db_path(&repository.root).ok()?;
        if !db_path.exists() {
            return None;
        }

        let storage = Storage::new(db_path);
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

    pub(super) fn load_or_build_entrypoint_surface_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
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

        let db_path = resolve_provenance_db_path(&repository.root).ok()?;
        if !db_path.exists() {
            return None;
        }

        let storage = Storage::new(db_path);
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

    pub(super) fn projected_path_witness_candidates_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        base_repository: Option<&RepositoryCandidateUniverse>,
        intent: &HybridRankingIntent,
        query_context: &HybridPathWitnessQueryContext,
    ) -> Option<Vec<PathWitnessCandidate>> {
        let base_repository = base_repository?;
        let snapshot_id = base_repository.snapshot_id.as_deref()?;
        let projections = self
            .load_or_build_path_witness_projections_for_repository(base_repository, snapshot_id)?;
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
            let overlay_boost = overlay_boosts_by_path
                .get(rel_path)
                .cloned()
                .unwrap_or_default();
            let Some(score) = base_score
                .map(|score| score + overlay_boost.bonus_score())
                .or_else(|| {
                    (overlay_boost.bonus_millis > 0).then_some(overlay_boost.bonus_score())
                })
            else {
                continue;
            };
            scored.push(PathWitnessCandidate {
                score,
                repository_id: repository.repository_id.clone(),
                rel_path: rel_path.clone(),
                path: path.clone(),
                witness_provenance_ids: overlay_boost.provenance_ids,
            });
        }

        for candidate in &repository.candidates {
            if base_candidates_by_path.contains_key(&candidate.relative_path) {
                continue;
            }
            let base_score =
                hybrid_path_witness_recall_score(&candidate.relative_path, intent, query_context);
            let overlay_boost = overlay_boosts_by_path
                .get(&candidate.relative_path)
                .cloned()
                .unwrap_or_default();
            let Some(score) = base_score
                .map(|score| score + overlay_boost.bonus_score())
                .or_else(|| {
                    (overlay_boost.bonus_millis > 0).then_some(overlay_boost.bonus_score())
                })
            else {
                continue;
            };
            scored.push(PathWitnessCandidate {
                score,
                repository_id: repository.repository_id.clone(),
                rel_path: candidate.relative_path.clone(),
                path: candidate.absolute_path.clone(),
                witness_provenance_ids: overlay_boost.provenance_ids,
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
                    .load_or_build_test_subject_projections_for_repository(
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
                .load_or_build_entrypoint_surface_projections_for_repository(
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
