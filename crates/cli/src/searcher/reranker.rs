use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};

use super::HybridRankedEvidence;
use super::intent::HybridRankingIntent;
use super::path_witness_projection::{
    GenericWitnessSurfaceFamily, StoredPathWitnessProjection,
    generic_surface_families_for_projection,
};
use super::policy::{
    PolicyQueryContext, SelectionCandidate, SelectionFacts, SelectionState,
    hybrid_selection_score_from_context,
};
use super::surfaces::{
    HybridSourceClass, is_bench_support_path, is_entrypoint_runtime_path,
    is_runtime_config_artifact_path, is_test_support_path,
};

pub(super) type CoverageProjectionHintMap =
    BTreeMap<(String, String), Vec<(GenericWitnessSurfaceFamily, String)>>;

#[derive(Debug, Clone, Copy, PartialEq)]
struct DiversificationHeapEntry {
    score: f32,
    evidence_rank: usize,
    candidate_revision: usize,
    index: usize,
}

impl Eq for DiversificationHeapEntry {}

impl Ord for DiversificationHeapEntry {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.evidence_rank.cmp(&self.evidence_rank))
            .then_with(|| self.candidate_revision.cmp(&other.candidate_revision))
            .then_with(|| other.index.cmp(&self.index))
    }
}

impl PartialOrd for DiversificationHeapEntry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn hybrid_ranked_evidence_order(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
) -> std::cmp::Ordering {
    right
        .blended_score
        .total_cmp(&left.blended_score)
        .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
        .then_with(|| right.witness_score.total_cmp(&left.witness_score))
        .then_with(|| right.graph_score.total_cmp(&left.graph_score))
        .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
        .then(left.document.cmp(&right.document))
        .then(left.excerpt.cmp(&right.excerpt))
}

pub(super) fn diversify_hybrid_ranked_evidence(
    ranked: Vec<HybridRankedEvidence>,
    limit: usize,
    query_text: &str,
) -> Vec<HybridRankedEvidence> {
    if limit == 0 || ranked.is_empty() {
        return Vec::new();
    }

    let intent = HybridRankingIntent::from_query(query_text);
    let query_context = PolicyQueryContext::new(&intent, query_text);
    let mut state = SelectionState::default();
    let mut candidates = ranked
        .into_iter()
        .map(|evidence| Some(SelectionCandidate::new(evidence, &intent, &query_context)))
        .collect::<Vec<_>>();
    let evidence_ranks = evidence_ranks_for_candidates(&candidates);
    let mut selected = Vec::with_capacity(limit.min(candidates.len()));
    let mut heap = BinaryHeap::with_capacity(candidates.len());
    let mut candidate_revisions = vec![0usize; candidates.len()];

    for index in 0..candidates.len() {
        push_diversification_candidate(
            &mut heap,
            &candidates,
            &candidate_revisions,
            &evidence_ranks,
            index,
            &intent,
            &query_context,
            &state,
        );
    }

    while selected.len() < limit {
        let Some(entry) = heap.pop() else {
            break;
        };
        let Some(_) = candidates[entry.index].as_ref() else {
            continue;
        };
        if entry.candidate_revision != candidate_revisions[entry.index] {
            continue;
        }

        let chosen = candidates[entry.index]
            .take()
            .expect("live heap candidate should still be present");
        state.observe(&chosen);
        refresh_diversification_heap(
            &mut heap,
            &candidates,
            &mut candidate_revisions,
            &evidence_ranks,
            &chosen,
            &intent,
            &query_context,
            &state,
        );
        selected.push(chosen.evidence);
    }

    selected
}

fn push_diversification_candidate(
    heap: &mut BinaryHeap<DiversificationHeapEntry>,
    candidates: &[Option<SelectionCandidate>],
    candidate_revisions: &[usize],
    evidence_ranks: &[usize],
    index: usize,
    intent: &HybridRankingIntent,
    query_context: &PolicyQueryContext,
    state: &SelectionState,
) {
    let Some(candidate) = candidates[index].as_ref() else {
        return;
    };
    heap.push(DiversificationHeapEntry {
        score: hybrid_selection_score(candidate, intent, query_context, state),
        evidence_rank: evidence_ranks[index],
        candidate_revision: candidate_revisions[index],
        index,
    });
}

fn refresh_diversification_heap(
    heap: &mut BinaryHeap<DiversificationHeapEntry>,
    candidates: &[Option<SelectionCandidate>],
    candidate_revisions: &mut [usize],
    evidence_ranks: &[usize],
    chosen: &SelectionCandidate,
    intent: &HybridRankingIntent,
    query_context: &PolicyQueryContext,
    state: &SelectionState,
) {
    for (index, candidate) in candidates.iter().enumerate() {
        let Some(candidate) = candidate.as_ref() else {
            continue;
        };
        if !diversification_candidate_score_changed(chosen, candidate) {
            continue;
        }
        candidate_revisions[index] = candidate_revisions[index].saturating_add(1);
        heap.push(DiversificationHeapEntry {
            score: hybrid_selection_score(candidate, intent, query_context, state),
            evidence_rank: evidence_ranks[index],
            candidate_revision: candidate_revisions[index],
            index,
        });
    }
}

fn diversification_candidate_score_changed(
    chosen: &SelectionCandidate,
    candidate: &SelectionCandidate,
) -> bool {
    if chosen.evidence.document.path == candidate.evidence.document.path {
        return false;
    }

    let chosen_path = chosen.evidence.document.path.as_str();
    let candidate_path = candidate.evidence.document.path.as_str();
    if chosen.static_features.class == HybridSourceClass::Runtime
        || is_entrypoint_runtime_path(chosen_path)
        || is_runtime_config_artifact_path(chosen_path)
    {
        return true;
    }

    chosen.static_features.class == candidate.static_features.class
        || (chosen.static_features.is_ci_workflow && candidate.static_features.is_ci_workflow)
        || (chosen.static_features.is_example_support
            && candidate.static_features.is_example_support)
        || (is_bench_support_path(chosen_path) && is_bench_support_path(candidate_path))
        || (is_plain_test_support_path(chosen_path) && is_plain_test_support_path(candidate_path))
        || (chosen.static_features.is_repo_root_runtime_config_artifact
            && candidate
                .static_features
                .is_repo_root_runtime_config_artifact)
        || (chosen.static_features.is_typescript_runtime_module_index
            && candidate.static_features.is_typescript_runtime_module_index)
}

fn is_plain_test_support_path(path: &str) -> bool {
    is_test_support_path(path) && !is_bench_support_path(path)
}

fn evidence_ranks_for_candidates(candidates: &[Option<SelectionCandidate>]) -> Vec<usize> {
    let mut indices = candidates
        .iter()
        .enumerate()
        .filter_map(|(index, candidate)| candidate.as_ref().map(|_| index))
        .collect::<Vec<_>>();
    indices.sort_by(|left, right| {
        let left = candidates[*left]
            .as_ref()
            .expect("evidence-rank candidates should exist");
        let right = candidates[*right]
            .as_ref()
            .expect("evidence-rank candidates should exist");
        hybrid_ranked_evidence_order(&left.evidence, &right.evidence)
    });

    let mut ranks = vec![0; candidates.len()];
    for (rank, index) in indices.into_iter().enumerate() {
        ranks[index] = rank;
    }
    ranks
}

fn hybrid_selection_score(
    candidate: &SelectionCandidate,
    intent: &HybridRankingIntent,
    query_context: &PolicyQueryContext,
    state: &SelectionState,
) -> f32 {
    let ctx = SelectionFacts::from_candidate(candidate, intent, query_context, state);

    hybrid_selection_score_from_context(&ctx)
}

fn is_coverage_backed(
    entry: &HybridRankedEvidence,
    coverage_hints: &CoverageProjectionHintMap,
) -> bool {
    entry.witness_score > 0.0
        || !entry.witness_sources.is_empty()
        || coverage_hints.contains_key(&coverage_hint_key(entry))
}

fn coverage_hint_key(entry: &HybridRankedEvidence) -> (String, String) {
    (
        entry.document.repository_id.clone(),
        entry.document.path.clone(),
    )
}

fn coverage_keys(
    entry: &HybridRankedEvidence,
    coverage_hints: &CoverageProjectionHintMap,
) -> Vec<(GenericWitnessSurfaceFamily, String)> {
    let projection = StoredPathWitnessProjection::from_path(&entry.document.path);
    let mut keys = projection
        .subtree_root
        .clone()
        .map(|subtree_root| {
            generic_surface_families_for_projection(&projection)
                .into_iter()
                .map(|family| (family, subtree_root.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if let Some(hinted) = coverage_hints.get(&coverage_hint_key(entry)) {
        keys.extend(hinted.iter().cloned());
    }
    keys.sort();
    keys.dedup();
    keys
}

pub(super) fn build_coverage_grouped_pool(
    grouped: Vec<HybridRankedEvidence>,
    selection_limit: usize,
    rank_limit: usize,
    coverage_hints: &CoverageProjectionHintMap,
) -> Vec<HybridRankedEvidence> {
    if grouped.len() <= rank_limit {
        return grouped;
    }

    let reserve = usize::min(8, usize::max(4, selection_limit)).min(rank_limit);
    if reserve == 0 {
        return grouped.into_iter().take(rank_limit).collect();
    }

    let base_take = rank_limit.saturating_sub(reserve);
    let mut baseline = grouped.iter().take(base_take).cloned().collect::<Vec<_>>();
    let mut represented = BTreeSet::<(GenericWitnessSurfaceFamily, String)>::new();
    let mut included_paths = baseline
        .iter()
        .map(|entry| entry.document.path.clone())
        .collect::<BTreeSet<_>>();

    for entry in &baseline {
        if is_coverage_backed(entry, coverage_hints) {
            represented.extend(coverage_keys(entry, coverage_hints));
        }
    }

    let mut preserved = Vec::new();
    for entry in grouped.iter().skip(base_take) {
        if preserved.len() >= reserve || !is_coverage_backed(entry, coverage_hints) {
            continue;
        }
        let keys = coverage_keys(entry, coverage_hints);
        if keys.is_empty() || keys.iter().all(|key| represented.contains(key)) {
            continue;
        }
        if !included_paths.insert(entry.document.path.clone()) {
            continue;
        }
        represented.extend(keys);
        preserved.push(entry.clone());
    }

    baseline.extend(preserved);
    for entry in grouped {
        if baseline.len() >= rank_limit {
            break;
        }
        if included_paths.insert(entry.document.path.clone()) {
            baseline.push(entry);
        }
    }

    baseline.sort_by(hybrid_ranked_evidence_order);
    baseline.truncate(rank_limit);
    baseline
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EvidenceAnchor, EvidenceAnchorKind, EvidenceDocumentRef};

    fn ranked(path: &str, blended_score: f32, witness_score: f32) -> HybridRankedEvidence {
        HybridRankedEvidence {
            document: EvidenceDocumentRef {
                repository_id: "repo-001".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            anchor: EvidenceAnchor::new(EvidenceAnchorKind::PathWitness, 1, 1, 1, 1),
            excerpt: path.to_owned(),
            blended_score,
            lexical_score: blended_score,
            witness_score,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: vec![format!("lexical:{path}")],
            witness_sources: (witness_score > 0.0)
                .then(|| vec![format!("witness:{path}")])
                .unwrap_or_default(),
            graph_sources: Vec::new(),
            semantic_sources: Vec::new(),
        }
    }

    fn diversify_hybrid_ranked_evidence_full_rescan(
        ranked: Vec<HybridRankedEvidence>,
        limit: usize,
        query_text: &str,
    ) -> Vec<HybridRankedEvidence> {
        let intent = HybridRankingIntent::from_query(query_text);
        let query_context = PolicyQueryContext::new(&intent, query_text);
        let mut state = SelectionState::default();
        let mut remaining = ranked
            .into_iter()
            .map(|evidence| SelectionCandidate::new(evidence, &intent, &query_context))
            .collect::<Vec<_>>();
        let mut selected = Vec::with_capacity(limit.min(remaining.len()));

        while selected.len() < limit && !remaining.is_empty() {
            let mut best_index = 0usize;
            let mut best_score =
                hybrid_selection_score(&remaining[0], &intent, &query_context, &state);

            for (index, candidate) in remaining.iter().enumerate().skip(1) {
                let score = hybrid_selection_score(candidate, &intent, &query_context, &state);
                if score.total_cmp(&best_score).is_gt()
                    || (score.total_cmp(&best_score).is_eq()
                        && hybrid_ranked_evidence_order(
                            &candidate.evidence,
                            &remaining[best_index].evidence,
                        )
                        .is_lt())
                {
                    best_index = index;
                    best_score = score;
                }
            }

            let chosen = remaining.swap_remove(best_index);
            state.observe(&chosen);
            selected.push(chosen.evidence);
        }

        selected
    }

    #[test]
    fn coverage_grouped_pool_preserves_unique_witness_backed_surface_keys() {
        let grouped = vec![
            ranked("README.md", 0.99, 0.0),
            ranked("packages/editor-ui/src/main.ts", 0.95, 0.8),
            ranked("packages/editor-ui/package.json", 0.94, 0.7),
            ranked("packages/worker/src/main.ts", 0.93, 0.75),
            ranked("packages/worker/package.json", 0.92, 0.65),
        ];

        let pool = build_coverage_grouped_pool(grouped, 1, 5, &CoverageProjectionHintMap::new());
        let paths = pool
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(paths.len(), 5);
        assert!(paths.contains(&"README.md"));
        assert!(paths.contains(&"packages/editor-ui/src/main.ts"));
        assert!(paths.contains(&"packages/editor-ui/package.json"));
        assert!(paths.contains(&"packages/worker/src/main.ts"));
        assert!(paths.contains(&"packages/worker/package.json"));
    }

    #[test]
    fn coverage_grouped_pool_stays_bounded_and_deterministic() {
        let grouped = vec![
            ranked("README.md", 0.99, 0.0),
            ranked("packages/editor-ui/src/main.ts", 0.95, 0.8),
            ranked("packages/editor-ui/package.json", 0.94, 0.7),
            ranked("packages/worker/src/main.ts", 0.93, 0.75),
            ranked("packages/worker/package.json", 0.92, 0.65),
            ranked("packages/editor-ui/src/secondary.ts", 0.91, 0.6),
        ];

        let left =
            build_coverage_grouped_pool(grouped.clone(), 2, 4, &CoverageProjectionHintMap::new());
        let right = build_coverage_grouped_pool(grouped, 2, 4, &CoverageProjectionHintMap::new());

        assert_eq!(left.len(), 4);
        assert_eq!(
            left.iter()
                .map(|entry| &entry.document.path)
                .collect::<Vec<_>>(),
            right
                .iter()
                .map(|entry| &entry.document.path)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn coverage_grouped_pool_preserves_projection_backed_exemplars_without_witness_scores() {
        let grouped = vec![
            ranked("README.md", 0.99, 0.0),
            ranked("packages/editor-ui/src/main.ts", 0.95, 0.8),
            ranked("packages/worker/package.json", 0.80, 0.0),
        ];
        let mut hints = CoverageProjectionHintMap::new();
        hints.insert(
            (
                "repo-001".to_owned(),
                "packages/worker/package.json".to_owned(),
            ),
            vec![(
                GenericWitnessSurfaceFamily::PackageSurface,
                "packages/worker".to_owned(),
            )],
        );

        let pool = build_coverage_grouped_pool(grouped, 1, 3, &hints);
        let paths = pool
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            paths.contains(&"packages/worker/package.json"),
            "projection-backed exemplar should be preserved even without direct witness score"
        );
    }

    #[test]
    fn diversification_matches_full_rescan_baseline_for_mixed_runtime_pool() {
        let ranked = vec![
            ranked("src/main.rs", 0.98, 0.2),
            ranked(".github/workflows/ci.yml", 0.97, 0.9),
            ranked("tests/unit/main_test.rs", 0.96, 0.3),
            ranked("docs/build_pipeline.md", 0.95, 0.1),
            ranked("package.json", 0.94, 0.0),
            ranked("README.md", 0.93, 0.0),
        ];

        let expected = diversify_hybrid_ranked_evidence_full_rescan(
            ranked.clone(),
            4,
            "build pipeline runtime tests",
        );
        let actual = diversify_hybrid_ranked_evidence(ranked, 4, "build pipeline runtime tests");

        assert_eq!(
            actual
                .iter()
                .map(|entry| entry.document.path.as_str())
                .collect::<Vec<_>>(),
            expected
                .iter()
                .map(|entry| entry.document.path.as_str())
                .collect::<Vec<_>>(),
            "lazy diversification should preserve the full-rescan selection order"
        );
    }

    #[test]
    fn diversification_remains_deterministic_under_equal_candidate_scores() {
        let ranked = vec![
            ranked("docs/build_notes_02.md", 1.0, 0.0),
            ranked("docs/build_notes_00.md", 1.0, 0.0),
            ranked("docs/build_notes_01.md", 1.0, 0.0),
            ranked(".github/workflows/ci.yml", 1.0, 0.0),
        ];

        let left = diversify_hybrid_ranked_evidence(ranked.clone(), 3, "build notes");
        let right = diversify_hybrid_ranked_evidence(ranked.clone(), 3, "build notes");
        let baseline = diversify_hybrid_ranked_evidence_full_rescan(ranked, 3, "build notes");

        let left_paths = left
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        let right_paths = right
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        let baseline_paths = baseline
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(left_paths, right_paths);
        assert_eq!(left_paths, baseline_paths);
    }
}
