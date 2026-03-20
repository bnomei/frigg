use super::super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet};
use super::super::super::super::facts::SelectionFacts;
use super::super::super::super::predicates::selection as pred;
use super::super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn first_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    (state.seen_count == 0).then_some(PolicyEffect::Add(0.24))
}

fn first_support_or_test_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    (state.seen_count == 0).then_some(PolicyEffect::Add(0.10))
}

fn identifier_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        0.30
    } else {
        0.16
    }))
}

fn fixtures_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.42
    } else {
        -0.24
    }))
}

fn python_entrypoint_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let state = ctx;

    Some(PolicyEffect::Add(if intent.wants_python_witnesses {
        if state.seen_count == 0 { 0.26 } else { 0.14 }
    } else if state.seen_count == 0 {
        -0.16
    } else {
        -0.08
    }))
}

fn python_config_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let state = ctx;

    Some(PolicyEffect::Add(if intent.wants_python_workspace_config {
        if state.seen_count == 0 { 0.18 } else { 0.10 }
    } else if state.seen_count == 0 {
        -0.18
    } else {
        -0.10
    }))
}

fn python_test_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let state = ctx;

    Some(PolicyEffect::Add(if intent.wants_python_witnesses {
        if state.seen_count == 0 { 0.28 } else { 0.12 }
    } else if state.seen_count == 0 {
        -0.22
    } else {
        -0.12
    }))
}

fn loose_python_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.18
    } else {
        -0.10
    }))
}

fn path_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    if candidate.path_overlap == 0 {
        return None;
    }

    let delta = match candidate.class {
        HybridSourceClass::Runtime => {
            if candidate.path_overlap == 1 {
                0.10
            } else {
                0.18
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if candidate.path_overlap == 1 {
                0.08
            } else {
                0.14
            }
        }
        HybridSourceClass::Documentation | HybridSourceClass::Readme => {
            if candidate.path_overlap == 1 {
                0.02
            } else {
                0.06
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.runtime.first_runtime_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::class_is_runtime_leaf(),
            pred::seen_count_is_zero_leaf(),
        ]),
        first_runtime_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.first_support_or_test_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::seen_count_is_zero_leaf(),
            ],
            &[pred::class_is_support_leaf(), pred::class_is_tests_leaf()],
            &[],
        ),
        first_support_or_test_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.identifier_anchor_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::excerpt_has_exact_identifier_anchor_leaf(),
            ],
            &[
                pred::class_is_runtime_leaf(),
                pred::class_is_support_leaf(),
                pred::class_is_tests_leaf(),
            ],
            &[],
        ),
        identifier_anchor_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.fixtures_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::class_is_fixtures_leaf(),
        ]),
        fixtures_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.python_entrypoint_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_entrypoint_runtime_leaf(),
        ]),
        python_entrypoint_adjustment,
    ),
    ScoreRule::when(
        "selection.runtime.python_config_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_runtime_config_leaf(),
        ]),
        python_config_adjustment,
    ),
    ScoreRule::when(
        "selection.runtime.python_test_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_test_witness_leaf(),
        ]),
        python_test_adjustment,
    ),
    ScoreRule::when(
        "selection.runtime.loose_python_test_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_loose_python_test_module_leaf(),
        ]),
        loose_python_test_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.path_overlap_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::path_overlap_leaf(),
        ]),
        path_overlap_bonus,
    ),
];

pub(super) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);
