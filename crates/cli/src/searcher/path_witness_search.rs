use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use crate::domain::{FriggResult, model::TextMatch};

use super::intent::HybridRankingIntent;
use super::lexical_channel::{
    HybridPathWitnessQueryContext, best_path_witness_anchor_in_file,
    hybrid_path_witness_recall_score,
};
use super::policy;
use super::query_terms::{hybrid_excerpt_has_build_flow_anchor, hybrid_query_overlap_terms};
use super::types::{
    NormalizedSearchFilters, RepositoryCandidateUniverse, SearchCandidateFile,
    SearchCandidateUniverse,
};
use super::{
    SearchExecutionOutput, SearchFilters, SearchTextQuery, TextSearcher, normalize_search_filters,
};

#[derive(Debug)]
pub(super) struct PathWitnessCandidate {
    pub(super) score: f32,
    pub(super) repository_id: String,
    pub(super) rel_path: String,
    pub(super) path: PathBuf,
    pub(super) witness_provenance_ids: Vec<String>,
}

#[derive(Debug)]
struct BoundedPathWitnessFrontier {
    limit: usize,
    candidates: Vec<PathWitnessCandidate>,
}

#[derive(Debug, Clone, Copy)]
struct OverlaySeedCandidateRef<'a> {
    score: f32,
    has_overlay: bool,
    candidate: &'a SearchCandidateFile,
}

#[derive(Debug)]
struct BoundedOverlaySeedFrontier<'a> {
    limit: usize,
    candidates: Vec<OverlaySeedCandidateRef<'a>>,
}

impl TextSearcher {
    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) fn search_path_witness_recall_with_filters(
        &self,
        query_text: &str,
        filters: &SearchFilters,
        limit: usize,
        intent: &HybridRankingIntent,
    ) -> FriggResult<SearchExecutionOutput> {
        if limit == 0 || !intent.wants_path_witness_recall() {
            return Ok(SearchExecutionOutput::default());
        }

        let normalized_filters = normalize_search_filters(filters.clone())?;
        let empty_query = SearchTextQuery {
            query: String::new(),
            path_regex: None,
            limit,
        };
        let candidate_universe = self.build_candidate_universe(&empty_query, &normalized_filters);
        self.search_path_witness_recall_in_universe(
            query_text,
            &candidate_universe,
            &normalized_filters,
            limit,
            intent,
        )
    }

    pub(super) fn search_path_witness_recall_in_universe(
        &self,
        query_text: &str,
        candidate_universe: &SearchCandidateUniverse,
        filters: &NormalizedSearchFilters,
        limit: usize,
        intent: &HybridRankingIntent,
    ) -> FriggResult<SearchExecutionOutput> {
        let frontier = policy::plan_path_witness_frontier(intent, limit);
        let top_k = frontier.top_k;
        let materialized_limit = frontier.materialized_limit;
        let query_context = HybridPathWitnessQueryContext::from_query_text(query_text);
        let build_flow_overlap_terms = intent
            .wants_entrypoint_build_flow
            .then(|| hybrid_query_overlap_terms(query_text));
        let mut frontier_candidates = BoundedPathWitnessFrontier::new(top_k);
        let base_repositories = candidate_universe
            .repositories
            .iter()
            .map(|repository| (repository.repository_id.clone(), repository))
            .collect::<BTreeMap<_, _>>();
        let candidate_universe =
            self.candidate_universe_with_hidden_workflows(candidate_universe, filters, intent);
        for repository in &candidate_universe.repositories {
            let repository_candidates = self
                .projected_path_witness_candidates_for_repository(
                    repository,
                    base_repositories.get(&repository.repository_id).copied(),
                    intent,
                    &query_context,
                )
                .unwrap_or_else(|| {
                    repository
                        .candidates
                        .iter()
                        .filter_map(|candidate| {
                            let score = hybrid_path_witness_recall_score(
                                &candidate.relative_path,
                                intent,
                                &query_context,
                            )?;
                            Some(PathWitnessCandidate {
                                score,
                                repository_id: repository.repository_id.clone(),
                                rel_path: candidate.relative_path.clone(),
                                path: candidate.absolute_path.clone(),
                                witness_provenance_ids: Vec::new(),
                            })
                        })
                        .collect::<Vec<_>>()
                });
            for candidate in repository_candidates {
                frontier_candidates.offer(candidate);
            }
        }

        let mut matches = Vec::with_capacity(materialized_limit);
        for candidate in frontier_candidates
            .into_sorted_vec()
            .into_iter()
            .take(materialized_limit)
        {
            let PathWitnessCandidate {
                score,
                repository_id,
                rel_path,
                path,
                witness_provenance_ids,
            } = candidate;
            let projected_anchor = base_repositories
                .get(&repository_id)
                .and_then(|repository| {
                    self.projection_store_service
                        .best_path_witness_anchor_for_repository(
                            repository,
                            &rel_path,
                            &query_context,
                        )
                });
            let projected_needs_build_anchor_upgrade =
                projected_anchor.as_ref().is_some_and(|(_, excerpt)| {
                    build_flow_overlap_terms
                        .as_ref()
                        .is_some_and(|terms| !hybrid_excerpt_has_build_flow_anchor(excerpt, terms))
                });
            let file_anchor = if projected_anchor.is_none() || projected_needs_build_anchor_upgrade
            {
                best_path_witness_anchor_in_file(&rel_path, &path, &query_context)
            } else {
                None
            };
            let preferred_file_anchor = file_anchor.as_ref().filter(|(_, excerpt)| {
                build_flow_overlap_terms
                    .as_ref()
                    .is_some_and(|terms| hybrid_excerpt_has_build_flow_anchor(excerpt, terms))
            });
            let (line, excerpt) = preferred_file_anchor
                .cloned()
                .or(projected_anchor)
                .or(file_anchor)
                .unwrap_or_else(|| (1, rel_path.clone()));
            matches.push(TextMatch {
                match_id: None,
                repository_id,
                path: rel_path,
                line,
                column: 1,
                excerpt,
                witness_score_hint_millis: Some(path_witness_score_hint_millis(score)),
                witness_provenance_ids: (!witness_provenance_ids.is_empty())
                    .then_some(witness_provenance_ids),
            });
        }

        Ok(SearchExecutionOutput {
            total_matches: matches.len(),
            matches,
            diagnostics: candidate_universe.diagnostics,
            lexical_backend: None,
            lexical_backend_note: None,
        })
    }

    pub(super) fn projected_path_witness_candidates_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        base_repository: Option<&RepositoryCandidateUniverse>,
        intent: &HybridRankingIntent,
        query_context: &HybridPathWitnessQueryContext,
    ) -> Option<Vec<PathWitnessCandidate>> {
        self.projection_store_service
            .projected_path_witness_candidates_for_repository(
                repository,
                base_repository,
                intent,
                query_context,
            )
    }

    pub(super) fn build_overlay_aware_path_witness_seed_universe(
        &self,
        candidate_universe: &SearchCandidateUniverse,
        filters: &NormalizedSearchFilters,
        intent: &HybridRankingIntent,
        query_context: &HybridPathWitnessQueryContext,
        lexical_limit: usize,
    ) -> Option<SearchCandidateUniverse> {
        let per_repository_limit = lexical_limit.saturating_div(2).saturating_add(4).max(10);
        let overlay_reserve = overlay_seed_reserve_slots(intent, per_repository_limit);
        let expanded_universe =
            self.candidate_universe_with_hidden_workflows(candidate_universe, filters, intent);
        let mut repositories = Vec::new();
        for repository in &expanded_universe.repositories {
            let base_repository = candidate_universe
                .repositories
                .iter()
                .find(|candidate| candidate.repository_id == repository.repository_id);
            let overlay_boosts_by_path = self
                .projection_store_service
                .overlay_boosts_for_repository(repository, base_repository, intent, query_context);
            let mut scored = BoundedOverlaySeedFrontier::new(per_repository_limit);
            let mut overlay_scored = BoundedOverlaySeedFrontier::new(per_repository_limit);
            for candidate in &repository.candidates {
                let Some(scored_candidate) = ({
                    let base_score = hybrid_path_witness_recall_score(
                        &candidate.relative_path,
                        intent,
                        query_context,
                    );
                    let overlay_boost = overlay_boosts_by_path
                        .get(&candidate.relative_path)
                        .cloned()
                        .unwrap_or_default();
                    match base_score
                        .map(|score| score + overlay_boost.bonus_score())
                        .or_else(|| {
                            (overlay_boost.bonus_millis > 0).then_some(overlay_boost.bonus_score())
                        }) {
                        Some(score) => Some(OverlaySeedCandidateRef {
                            score,
                            has_overlay: overlay_boost.bonus_millis > 0,
                            candidate,
                        }),
                        None => None,
                    }
                }) else {
                    continue;
                };
                scored.offer(scored_candidate);
                if scored_candidate.has_overlay {
                    overlay_scored.offer(scored_candidate);
                }
            }
            let scored = scored.into_sorted_vec();
            if scored.is_empty() {
                continue;
            }
            let overlay_scored = overlay_scored.into_sorted_vec();
            let mut candidates = Vec::<SearchCandidateFile>::new();
            let mut selected_paths = BTreeSet::<String>::new();
            let base_take = per_repository_limit.saturating_sub(overlay_reserve);
            for scored_candidate in scored.iter().take(base_take) {
                let candidate = scored_candidate.candidate;
                selected_paths.insert(candidate.relative_path.clone());
                candidates.push(SearchCandidateFile {
                    relative_path: candidate.relative_path.clone(),
                    absolute_path: candidate.absolute_path.clone(),
                });
            }
            if overlay_reserve > 0 {
                for scored_candidate in &overlay_scored {
                    let candidate = scored_candidate.candidate;
                    if !selected_paths.insert(candidate.relative_path.clone()) {
                        continue;
                    }
                    candidates.push(SearchCandidateFile {
                        relative_path: candidate.relative_path.clone(),
                        absolute_path: candidate.absolute_path.clone(),
                    });
                    if candidates.len() >= per_repository_limit {
                        break;
                    }
                }
            }
            if candidates.len() < per_repository_limit {
                for scored_candidate in scored {
                    let candidate = scored_candidate.candidate;
                    if !selected_paths.insert(candidate.relative_path.clone()) {
                        continue;
                    }
                    candidates.push(SearchCandidateFile {
                        relative_path: candidate.relative_path.clone(),
                        absolute_path: candidate.absolute_path.clone(),
                    });
                    if candidates.len() >= per_repository_limit {
                        break;
                    }
                }
            }
            repositories.push(RepositoryCandidateUniverse {
                repository_id: repository.repository_id.clone(),
                root: repository.root.clone(),
                snapshot_id: repository.snapshot_id.clone(),
                candidates,
            });
        }
        if repositories.is_empty() {
            return None;
        }

        Some(SearchCandidateUniverse {
            repositories,
            diagnostics: expanded_universe.diagnostics,
        })
    }
}

fn overlay_seed_reserve_slots(intent: &HybridRankingIntent, per_repository_limit: usize) -> usize {
    let mut reserve = 0;
    if intent.wants_runtime_witnesses {
        reserve += 1;
    }
    if intent.wants_tests || intent.wants_test_witness_recall {
        reserve += 1;
    }
    if intent.wants_entrypoint_build_flow
        || intent.wants_runtime_config_artifacts
        || intent.wants_ci_workflow_witnesses
        || intent.wants_scripts_ops_witnesses
    {
        reserve += 1;
    }

    reserve.min(per_repository_limit.saturating_sub(1)).min(2)
}

fn path_witness_score_hint_millis(score: f32) -> u32 {
    let millis = score.max(0.0).mul_add(1000.0, 0.0).round();
    if !millis.is_finite() {
        return u32::MAX;
    }
    millis.clamp(0.0, u32::MAX as f32) as u32
}

pub(super) fn path_witness_candidate_order(
    left: &PathWitnessCandidate,
    right: &PathWitnessCandidate,
) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.repository_id.cmp(&right.repository_id))
        .then_with(|| left.rel_path.cmp(&right.rel_path))
        .then_with(|| left.path.cmp(&right.path))
}

impl BoundedPathWitnessFrontier {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            candidates: Vec::with_capacity(limit),
        }
    }

    fn offer(&mut self, candidate: PathWitnessCandidate) {
        if self.limit == 0 {
            return;
        }
        if self.candidates.len() < self.limit {
            self.candidates.push(candidate);
            return;
        }
        let Some(worst_index) = self.worst_index() else {
            return;
        };
        if path_witness_candidate_order(&candidate, &self.candidates[worst_index]).is_lt() {
            self.candidates[worst_index] = candidate;
        }
    }

    fn into_sorted_vec(mut self) -> Vec<PathWitnessCandidate> {
        self.candidates.sort_by(path_witness_candidate_order);
        self.candidates
    }

    fn worst_index(&self) -> Option<usize> {
        let mut worst_index = None;
        for (index, candidate) in self.candidates.iter().enumerate() {
            match worst_index {
                Some(current)
                    if path_witness_candidate_order(&self.candidates[current], candidate)
                        .is_lt() =>
                {
                    worst_index = Some(index);
                }
                None => worst_index = Some(index),
                _ => {}
            }
        }
        worst_index
    }
}

impl<'a> BoundedOverlaySeedFrontier<'a> {
    fn new(limit: usize) -> Self {
        Self {
            limit,
            candidates: Vec::with_capacity(limit),
        }
    }

    fn offer(&mut self, candidate: OverlaySeedCandidateRef<'a>) {
        if self.limit == 0 {
            return;
        }
        if self.candidates.len() < self.limit {
            self.candidates.push(candidate);
            return;
        }
        let Some(worst_index) = self.worst_index() else {
            return;
        };
        if overlay_seed_candidate_order(candidate, self.candidates[worst_index]).is_lt() {
            self.candidates[worst_index] = candidate;
        }
    }

    fn into_sorted_vec(mut self) -> Vec<OverlaySeedCandidateRef<'a>> {
        self.candidates
            .sort_by(|left, right| overlay_seed_candidate_order(*left, *right));
        self.candidates
    }

    fn worst_index(&self) -> Option<usize> {
        let mut worst_index = None;
        for (index, candidate) in self.candidates.iter().enumerate() {
            match worst_index {
                Some(current)
                    if overlay_seed_candidate_order(self.candidates[current], *candidate)
                        .is_lt() =>
                {
                    worst_index = Some(index);
                }
                None => worst_index = Some(index),
                _ => {}
            }
        }
        worst_index
    }
}

fn overlay_seed_candidate_order(
    left: OverlaySeedCandidateRef<'_>,
    right: OverlaySeedCandidateRef<'_>,
) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| right.has_overlay.cmp(&left.has_overlay))
        .then_with(|| {
            left.candidate
                .relative_path
                .cmp(&right.candidate.relative_path)
        })
        .then_with(|| {
            left.candidate
                .absolute_path
                .cmp(&right.candidate.absolute_path)
        })
}
