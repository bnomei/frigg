use std::collections::{BTreeMap, BTreeSet};

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

pub(super) type CoverageProjectionHintMap =
    BTreeMap<(String, String), Vec<(GenericWitnessSurfaceFamily, String)>>;

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
        let mut best_score = hybrid_selection_score(&remaining[0], &intent, &query_context, &state);

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
}
