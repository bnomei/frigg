use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use crate::storage::{PathRelationProjection, PathSurfaceTermProjection};

use super::super::intent::HybridRankingIntent;
use super::super::lexical_channel::{
    HybridPathWitnessQueryContext, hybrid_path_witness_recall_score,
    hybrid_path_witness_recall_score_for_projection,
};
use super::super::overlay_projection::{
    PathOverlayBoost, StoredEntrypointSurfaceProjection,
    accumulate_companion_surface_overlay_boosts, accumulate_relation_overlay_boosts,
    accumulate_test_subject_overlay_boosts, entrypoint_surface_overlay_boost,
};
use super::super::path_witness_projection::{
    GenericWitnessSurfaceFamily, StoredPathWitnessProjection, generic_surface_families_from_bits,
    generic_surface_family_from_name,
};
use super::super::path_witness_search::PathWitnessCandidate;
use super::super::types::{HybridPathWitnessProjectionCacheKey, RepositoryCandidateUniverse};

use super::{ProjectedGraphAdjacentRelation, ProjectedGraphContext, ProjectionStoreService};

impl ProjectionStoreService {
    pub(in crate::searcher) fn load_projected_graph_context_for_repository(
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

    pub(in crate::searcher) fn projected_path_witness_candidates_for_repository(
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

    pub(in crate::searcher) fn overlay_boosts_for_repository(
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

    pub(crate) fn coverage_hint_keys_for_repositories(
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
                    if !coverage_row_relevant_relation_kind(&row.relation_kind) {
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

    pub(crate) fn best_path_witness_anchor_for_repository(
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

fn coverage_row_relevant_relation_kind(relation_kind: &str) -> bool {
    relation_kind == "companion_surface"
}

fn live_path_witness_projections_for_repository(
    repository: &RepositoryCandidateUniverse,
) -> Arc<Vec<StoredPathWitnessProjection>> {
    Arc::new(
        repository
            .candidates
            .iter()
            .map(|candidate| StoredPathWitnessProjection::from_path(&candidate.relative_path))
            .collect(),
    )
}
