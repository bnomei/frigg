use super::HybridRankedEvidence;
use super::intent::HybridRankingIntent;
use super::policy::{
    SelectionCandidate, SelectionFacts, SelectionQueryContext, SelectionState,
    hybrid_selection_score_from_context,
};

pub(super) fn diversify_hybrid_ranked_evidence(
    ranked: Vec<HybridRankedEvidence>,
    limit: usize,
    query_text: &str,
) -> Vec<HybridRankedEvidence> {
    let intent = HybridRankingIntent::from_query(query_text);
    let query_context = SelectionQueryContext::new(&intent, query_text);
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
    query_context: &SelectionQueryContext,
    state: &SelectionState,
) -> f32 {
    let ctx = SelectionFacts::from_candidate(candidate, intent, query_context, state);

    hybrid_selection_score_from_context(&ctx)
}
fn hybrid_ranked_evidence_order(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
) -> std::cmp::Ordering {
    right
        .blended_score
        .total_cmp(&left.blended_score)
        .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
        .then_with(|| right.graph_score.total_cmp(&left.graph_score))
        .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
        .then(left.document.cmp(&right.document))
        .then(left.excerpt.cmp(&right.excerpt))
}
