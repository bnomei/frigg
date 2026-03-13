use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn canonical_match(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Multiply(ctx.canonical_match_multiplier))
}

fn runtime_overlap_multiplier(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Multiply(
        ctx.runtime_witness_path_overlap_multiplier,
    ))
}

fn build_flow_anchor_multiplier(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Multiply(1.12))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.base.canonical_match",
        PolicyStage::SelectionBase,
        Predicate::ALWAYS,
        canonical_match,
    ),
    ScoreRule::when(
        "selection.base.runtime_overlap_multiplier",
        PolicyStage::SelectionBase,
        Predicate::all(&[pred::wants_runtime_witnesses_leaf()]),
        runtime_overlap_multiplier,
    ),
    ScoreRule::when(
        "selection.base.build_flow_anchor_multiplier",
        PolicyStage::SelectionBase,
        Predicate::all(&[pred::excerpt_has_build_flow_anchor_leaf()]),
        build_flow_anchor_multiplier,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
