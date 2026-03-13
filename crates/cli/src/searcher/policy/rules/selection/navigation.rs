use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn runtime_bonus(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(0.28))
}

fn mcp_runtime_navigation_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        0.96
    } else {
        0.42
    }))
}

fn reference_without_runtime_penalty(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-0.28))
}

fn mcp_reference_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.runtime_seen == 0 {
        -0.72
    } else {
        -0.24
    }))
}

fn reference_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-0.14 * ctx.seen_count as f32))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.navigation.runtime_bonus",
        PolicyStage::SelectionNavigation,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::is_navigation_runtime_leaf(),
            pred::seen_count_is_zero_leaf(),
        ]),
        runtime_bonus,
    ),
    ScoreRule::when(
        "selection.navigation.mcp_runtime_bonus",
        PolicyStage::SelectionNavigation,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::wants_mcp_runtime_surface_leaf(),
            pred::is_navigation_runtime_leaf(),
        ]),
        mcp_runtime_navigation_bonus,
    ),
    ScoreRule::when(
        "selection.navigation.reference_without_runtime_penalty",
        PolicyStage::SelectionNavigation,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::is_navigation_reference_doc_leaf(),
            pred::runtime_seen_is_zero_leaf(),
        ]),
        reference_without_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.navigation.mcp_reference_penalty",
        PolicyStage::SelectionNavigation,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::wants_mcp_runtime_surface_leaf(),
            pred::is_navigation_reference_doc_leaf(),
        ]),
        mcp_reference_penalty,
    ),
    ScoreRule::when(
        "selection.navigation.reference_repeat_penalty",
        PolicyStage::SelectionNavigation,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::is_navigation_reference_doc_leaf(),
            pred::seen_count_positive_leaf(),
        ]),
        reference_repeat_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
