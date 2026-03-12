use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn canonical_match(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Multiply(ctx.canonical_match_multiplier))
}

fn runtime_overlap_multiplier(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    ctx.wants_runtime_witnesses
        .then_some(PolicyEffect::Multiply(
            ctx.runtime_witness_path_overlap_multiplier,
        ))
}

fn build_flow_anchor_multiplier(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    ctx.excerpt_has_build_flow_anchor
        .then_some(PolicyEffect::Multiply(1.12))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.base.canonical_match",
        PolicyStage::SelectionBase,
        canonical_match,
    ),
    ScoreRule::new(
        "selection.base.runtime_overlap_multiplier",
        PolicyStage::SelectionBase,
        runtime_overlap_multiplier,
    ),
    ScoreRule::new(
        "selection.base.build_flow_anchor_multiplier",
        PolicyStage::SelectionBase,
        build_flow_anchor_multiplier,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rules(program, ctx, RULES);
}
