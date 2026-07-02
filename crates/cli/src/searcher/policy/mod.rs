//! Hybrid retrieval policy: path quality multipliers, path witness recall, selection scoring,
//! post-selection guardrails, and frontier sizing. Facts are normalized once; predicates and DSL
//! rules apply stage-specific effects through the shared policy kernel.

mod dsl;
mod facts;
mod frontier;
mod kernel;
mod post_selection;
mod predicates;
mod rules;
mod trace;

use super::intent::HybridRankingIntent;
#[cfg(test)]
use super::path_witness_projection::StoredPathWitnessProjection;
use facts::PathQualityFacts;
pub(super) use facts::{
    PathWitnessFacts, PolicyQueryContext, SelectionCandidate, SelectionFacts, SelectionState,
};
pub(super) use frontier::plan_path_witness_frontier;
pub(crate) use post_selection::PostSelectionTrace;

/// Path-quality multiplier for a single path under the given retrieval intent.
pub(super) fn hybrid_path_quality_multiplier_with_intent(
    path: &str,
    intent: &HybridRankingIntent,
) -> f32 {
    let ctx = PathQualityFacts::from_path(path, intent);
    rules::path_quality::score(&ctx)
}

/// Path-witness recall score when facts pass eligibility; `None` when the path is ineligible.
pub(super) fn hybrid_path_witness_recall_score_from_context(ctx: &PathWitnessFacts) -> Option<f32> {
    rules::path_witness::score(ctx)
}

/// Selection policy score for one ranked candidate given facts and coverage state.
pub(super) fn hybrid_selection_score_from_context(ctx: &SelectionFacts) -> f32 {
    rules::selection::score(ctx)
}

/// Run post-selection guardrails on the final match list up to `limit`.
pub(super) fn apply_post_selection_guardrails(
    matches: Vec<super::HybridRankedEvidence>,
    candidate_pool: &[super::HybridRankedEvidence],
    witness_hits: &[super::HybridChannelHit],
    intent: &HybridRankingIntent,
    query_text: &str,
    lexical_only_mode: bool,
    limit: usize,
) -> Vec<super::HybridRankedEvidence> {
    let ctx = post_selection::PostSelectionContext::new_with_mode(
        intent,
        query_text,
        lexical_only_mode,
        limit,
        candidate_pool,
        witness_hits,
    );
    post_selection::apply(matches, &ctx)
}

pub(crate) fn apply_post_selection_guardrails_with_trace(
    matches: Vec<super::HybridRankedEvidence>,
    candidate_pool: &[super::HybridRankedEvidence],
    witness_hits: &[super::HybridChannelHit],
    intent: &HybridRankingIntent,
    query_text: &str,
    lexical_only_mode: bool,
    limit: usize,
) -> (Vec<super::HybridRankedEvidence>, Option<PostSelectionTrace>) {
    let ctx = post_selection::PostSelectionContext::new_with_trace_mode(
        intent,
        query_text,
        lexical_only_mode,
        limit,
        candidate_pool,
        witness_hits,
    );
    let final_matches = post_selection::apply(matches, &ctx);
    let trace = ctx.trace_snapshot();
    (final_matches, trace)
}

#[cfg(test)]
fn format_rule_trace(evaluation: crate::searcher::policy::trace::PolicyEvaluation) -> Vec<String> {
    evaluation
        .trace
        .map(|trace| {
            trace
                .rules
                .into_iter()
                .map(|rule| {
                    format!(
                        "{} {:?} {:?} {:.3}->{:.3}",
                        rule.rule_id, rule.predicate_ids, rule.effect, rule.before, rule.after
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn path_quality_rule_trace(path: &str, intent: &HybridRankingIntent) -> Vec<String> {
    let facts = PathQualityFacts::from_path(path, intent);
    format_rule_trace(rules::path_quality::evaluate(&facts, true))
}

#[cfg(test)]
pub(crate) fn path_witness_rule_trace(
    path: &str,
    intent: &HybridRankingIntent,
    query_text: &str,
) -> Vec<String> {
    let query_context = PolicyQueryContext::new(intent, query_text);
    let projection = StoredPathWitnessProjection::from_path(path);
    let facts = PathWitnessFacts::from_projection(path, &projection, intent, &query_context);
    rules::path_witness::evaluate(&facts, true)
        .map(format_rule_trace)
        .unwrap_or_default()
}

#[cfg(test)]
pub(crate) fn selection_rule_trace(
    entry: super::HybridRankedEvidence,
    selected_so_far: &[super::HybridRankedEvidence],
    intent: &HybridRankingIntent,
    query_text: &str,
) -> Vec<String> {
    let query_context = PolicyQueryContext::new(intent, query_text);
    let candidate = SelectionCandidate::new(entry, intent, &query_context);
    let state = SelectionState::from_selected(selected_so_far, intent, &query_context);
    let facts = SelectionFacts::from_candidate(&candidate, intent, &query_context, &state);
    format_rule_trace(rules::selection::evaluate(&facts, true))
}
