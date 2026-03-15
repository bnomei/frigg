mod dsl;
mod facts;
mod frontier;
mod kernel;
mod post_selection;
mod predicates;
mod rules;
mod trace;

use super::intent::HybridRankingIntent;
use facts::PathQualityFacts;
pub(super) use facts::{
    PathWitnessFacts, PolicyQueryContext, SelectionCandidate, SelectionFacts, SelectionState,
};
pub(super) use frontier::plan_path_witness_frontier;

pub(super) fn hybrid_path_quality_multiplier_with_intent(
    path: &str,
    intent: &HybridRankingIntent,
) -> f32 {
    let ctx = PathQualityFacts::from_path(path, intent);
    rules::path_quality::score(&ctx)
}

pub(super) fn hybrid_path_witness_recall_score_from_context(ctx: &PathWitnessFacts) -> Option<f32> {
    rules::path_witness::score(ctx)
}

pub(super) fn hybrid_selection_score_from_context(ctx: &SelectionFacts) -> f32 {
    rules::selection::score(ctx)
}

pub(super) fn apply_post_selection_guardrails(
    matches: Vec<super::HybridRankedEvidence>,
    candidate_pool: &[super::HybridRankedEvidence],
    witness_hits: &[super::HybridChannelHit],
    intent: &HybridRankingIntent,
    query_text: &str,
    limit: usize,
) -> Vec<super::HybridRankedEvidence> {
    let ctx = post_selection::PostSelectionContext::new(
        intent,
        query_text,
        limit,
        candidate_pool,
        witness_hits,
    );
    post_selection::apply(matches, &ctx)
}
