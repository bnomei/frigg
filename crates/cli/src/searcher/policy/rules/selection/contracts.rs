use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn exact_identifier_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        0.78
    } else {
        0.38
    }))
}

fn runtime_support_tests_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        0.18
    } else {
        0.08
    }))
}

fn missing_identifier_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        -0.46
    } else {
        -0.24
    }))
}

const CONTRACT_QUERY_ALL: &[super::super::super::dsl::PredicateLeaf<SelectionFacts>] = &[
    pred::query_has_exact_terms_leaf(),
    pred::wants_contractish_leaf(),
];

const CONTRACT_RUNTIME_SUPPORT_TESTS_ANY: &[super::super::super::dsl::PredicateLeaf<
    SelectionFacts,
>] = &[
    pred::class_is_runtime_leaf(),
    pred::class_is_support_leaf(),
    pred::class_is_tests_leaf(),
];

const CONTRACT_RELEVANT_CLASS_ANY: &[super::super::super::dsl::PredicateLeaf<SelectionFacts>] = &[
    pred::class_is_runtime_leaf(),
    pred::class_is_support_leaf(),
    pred::class_is_tests_leaf(),
    pred::class_is_documentation_leaf(),
    pred::class_is_readme_leaf(),
    pred::class_is_fixtures_leaf(),
];

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.contracts.exact_identifier_bonus",
        PolicyStage::SelectionContracts,
        Predicate::all(&[
            pred::query_has_exact_terms_leaf(),
            pred::wants_contractish_leaf(),
            pred::excerpt_has_exact_identifier_anchor_leaf(),
        ]),
        exact_identifier_bonus,
    ),
    ScoreRule::when(
        "selection.contracts.runtime_support_tests_bonus",
        PolicyStage::SelectionContracts,
        Predicate::new(
            &[
                pred::query_has_exact_terms_leaf(),
                pred::wants_contractish_leaf(),
                pred::excerpt_has_exact_identifier_anchor_leaf(),
            ],
            CONTRACT_RUNTIME_SUPPORT_TESTS_ANY,
            &[],
        ),
        runtime_support_tests_bonus,
    ),
    ScoreRule::when(
        "selection.contracts.missing_identifier_penalty",
        PolicyStage::SelectionContracts,
        Predicate::new(
            CONTRACT_QUERY_ALL,
            CONTRACT_RELEVANT_CLASS_ANY,
            &[pred::excerpt_has_exact_identifier_anchor_leaf()],
        ),
        missing_identifier_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
