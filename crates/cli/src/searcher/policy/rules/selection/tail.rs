use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn identifier_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let best_overlap = ctx.path_overlap.max(ctx.excerpt_overlap);
    (ctx.query_has_identifier_anchor
        && (ctx.wants_runtime_witnesses || ctx.wants_entrypoint_build_flow)
        && best_overlap >= 2)
        .then_some(PolicyEffect::Add(0.18))
}

fn identifier_anchor_small_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let best_overlap = ctx.path_overlap.max(ctx.excerpt_overlap);
    (ctx.query_has_identifier_anchor
        && (ctx.wants_runtime_witnesses || ctx.wants_entrypoint_build_flow)
        && best_overlap == 1)
        .then_some(PolicyEffect::Add(0.08))
}

fn missing_identifier_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let best_overlap = ctx.path_overlap.max(ctx.excerpt_overlap);
    (ctx.query_has_identifier_anchor
        && (ctx.wants_runtime_witnesses || ctx.wants_entrypoint_build_flow)
        && best_overlap == 0
        && matches!(
            ctx.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        )
        && !ctx.is_entrypoint_runtime)
        .then_some(PolicyEffect::Add(-0.14))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.tail.identifier_anchor_bonus",
        PolicyStage::SelectionTail,
        identifier_anchor_bonus,
    ),
    ScoreRule::new(
        "selection.tail.identifier_anchor_small_bonus",
        PolicyStage::SelectionTail,
        identifier_anchor_small_bonus,
    ),
    ScoreRule::new(
        "selection.tail.missing_identifier_anchor_penalty",
        PolicyStage::SelectionTail,
        missing_identifier_anchor_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.query_has_identifier_anchor
        || !(ctx.wants_runtime_witnesses || ctx.wants_entrypoint_build_flow)
    {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
