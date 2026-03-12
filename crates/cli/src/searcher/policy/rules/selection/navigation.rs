use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.is_navigation_runtime && ctx.seen_count == 0)
        .then_some(PolicyEffect::Add(0.28))
}

fn mcp_runtime_navigation_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.wants_mcp_runtime_surface && ctx.is_navigation_runtime)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.96
        } else {
            0.42
        }))
}

fn reference_without_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.is_navigation_reference_doc && ctx.runtime_seen == 0)
        .then_some(PolicyEffect::Add(-0.28))
}

fn mcp_reference_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks
        && ctx.wants_mcp_runtime_surface
        && ctx.is_navigation_reference_doc)
        .then_some(PolicyEffect::Add(if ctx.runtime_seen == 0 {
            -0.72
        } else {
            -0.24
        }))
}

fn reference_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.is_navigation_reference_doc && ctx.seen_count > 0)
        .then_some(PolicyEffect::Add(-0.14 * ctx.seen_count as f32))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.navigation.runtime_bonus",
        PolicyStage::SelectionNavigation,
        runtime_bonus,
    ),
    ScoreRule::new(
        "selection.navigation.mcp_runtime_bonus",
        PolicyStage::SelectionNavigation,
        mcp_runtime_navigation_bonus,
    ),
    ScoreRule::new(
        "selection.navigation.reference_without_runtime_penalty",
        PolicyStage::SelectionNavigation,
        reference_without_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.navigation.mcp_reference_penalty",
        PolicyStage::SelectionNavigation,
        mcp_reference_penalty,
    ),
    ScoreRule::new(
        "selection.navigation.reference_repeat_penalty",
        PolicyStage::SelectionNavigation,
        reference_repeat_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.wants_navigation_fallbacks {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
