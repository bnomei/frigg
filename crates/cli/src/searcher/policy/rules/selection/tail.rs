use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn identifier_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (best_overlap(ctx) >= 2).then_some(PolicyEffect::Add(0.18))
}

fn identifier_anchor_small_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (best_overlap(ctx) == 1).then_some(PolicyEffect::Add(0.08))
}

fn missing_identifier_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (best_overlap(ctx) == 0).then_some(PolicyEffect::Add(-0.14))
}

fn best_overlap(ctx: &SelectionFacts) -> usize {
    ctx.path_overlap.max(ctx.excerpt_overlap)
}

const RUNTIME_SUPPORT_TESTS_ANY: &[super::super::super::dsl::PredicateLeaf<SelectionFacts>] = &[
    pred::class_is_runtime_leaf(),
    pred::class_is_support_leaf(),
    pred::class_is_tests_leaf(),
];

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.tail.identifier_anchor_bonus",
        PolicyStage::SelectionTail,
        Predicate::all(&[
            pred::query_has_identifier_anchor_leaf(),
            pred::wants_runtime_or_entrypoint_build_flow_leaf(),
        ]),
        identifier_anchor_bonus,
    ),
    ScoreRule::when(
        "selection.tail.identifier_anchor_small_bonus",
        PolicyStage::SelectionTail,
        Predicate::all(&[
            pred::query_has_identifier_anchor_leaf(),
            pred::wants_runtime_or_entrypoint_build_flow_leaf(),
        ]),
        identifier_anchor_small_bonus,
    ),
    ScoreRule::when(
        "selection.tail.missing_identifier_anchor_penalty",
        PolicyStage::SelectionTail,
        Predicate::new(
            &[
                pred::query_has_identifier_anchor_leaf(),
                pred::wants_runtime_or_entrypoint_build_flow_leaf(),
            ],
            RUNTIME_SUPPORT_TESTS_ANY,
            &[pred::is_entrypoint_runtime_leaf()],
        ),
        missing_identifier_anchor_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
