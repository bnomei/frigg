use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn class_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        1.42
    } else {
        0.72
    }))
}

fn specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    if candidate.specific_witness_path_overlap == 0 {
        return None;
    }

    let delta = match candidate.specific_witness_path_overlap {
        1 => {
            if state.seen_count == 0 {
                2.10
            } else {
                1.02
            }
        }
        2 => {
            if state.seen_count == 0 {
                3.68
            } else {
                1.82
            }
        }
        _ => {
            if state.seen_count == 0 {
                4.72
            } else {
                2.34
            }
        }
    };

    Some(PolicyEffect::Add(delta))
}

fn exact_query_match_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let candidate = ctx;
    let state = ctx;
    (candidate.has_exact_query_term_match
        && !(intent.wants_example_or_bench_witnesses && candidate.is_examples_rs))
        .then_some(PolicyEffect::Add(if state.seen_count == 0 {
            3.2
        } else {
            1.8
        }))
}

fn support_path_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    if candidate.is_example_support || candidate.is_bench_support {
        return None;
    }

    let delta = match candidate.path_overlap {
        0 | 1 => 0.0,
        2 => {
            if state.seen_count == 0 {
                0.34
            } else {
                0.18
            }
        }
        _ => {
            if state.seen_count == 0 {
                1.80
            } else {
                0.92
            }
        }
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn example_or_bench_context_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    (!candidate.is_example_support && !candidate.is_bench_support).then_some(PolicyEffect::Add(
        if state.seen_count == 0 { 0.42 } else { 0.24 },
    ))
}

fn example_support_priority_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        1.10
    } else {
        0.58
    }))
}

fn bench_support_priority_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        1.28
    } else {
        0.66
    }))
}

fn support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        0.18
    } else {
        0.10
    }))
}

fn generic_examples_rs_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -3.00
    } else {
        -1.50
    }))
}

fn cli_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        0.84
    } else {
        0.46
    }))
}

fn generic_test_penalty_under_examples_benches(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    let query = ctx;
    (candidate.class == HybridSourceClass::Tests
        && !candidate.is_example_support
        && !candidate.is_bench_support
        && candidate.specific_witness_path_overlap == 0
        && !candidate.has_exact_query_term_match
        && (!candidate.is_cli_test_support || !query.query_mentions_cli)
        && !candidate.is_test_harness)
        .then_some(PolicyEffect::Add(if state.seen_count == 0 {
            -1.20
        } else {
            -0.60
        }))
}

fn harness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        1.10
    } else {
        0.60
    }))
}

fn python_test_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let state = ctx;

    Some(PolicyEffect::Add(if intent.wants_python_witnesses {
        if state.seen_count == 0 { 0.34 } else { 0.18 }
    } else if state.seen_count == 0 {
        -0.28
    } else {
        -0.14
    }))
}

fn loose_python_test_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        0.12
    } else {
        0.06
    }))
}

fn non_code_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.44
    } else {
        -0.26
    }))
}

fn frontend_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.28
    } else {
        -0.16
    }))
}

fn cli_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    (!candidate.is_cli_test_support).then_some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.34
    } else {
        -0.20
    }))
}

fn cli_non_support_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    (!candidate.is_cli_test_support).then_some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.24
    } else {
        -0.12
    }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.tests.class_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        class_bonus,
    ),
    ScoreRule::when(
        "selection.tests.specific_overlap_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_test_support_leaf(),
            pred::specific_witness_path_overlap_leaf(),
        ]),
        specific_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.tests.exact_query_match_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::has_exact_query_term_match_leaf(),
        ]),
        exact_query_match_bonus,
    ),
    ScoreRule::when(
        "selection.tests.support_path_overlap_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_test_support_leaf(),
        ]),
        support_path_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.tests.example_or_bench_context_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_test_support_leaf(),
        ]),
        example_or_bench_context_bonus,
    ),
    ScoreRule::when(
        "selection.tests.example_support_priority_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_example_support_leaf(),
        ]),
        example_support_priority_bonus,
    ),
    ScoreRule::when(
        "selection.tests.bench_support_priority_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_bench_support_leaf(),
        ]),
        bench_support_priority_bonus,
    ),
    ScoreRule::when(
        "selection.tests.support_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_test_support_leaf(),
        ]),
        support_bonus,
    ),
    ScoreRule::when(
        "selection.tests.generic_examples_rs_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_examples_rs_leaf(),
        ]),
        generic_examples_rs_penalty,
    ),
    ScoreRule::when(
        "selection.tests.cli_support_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::query_mentions_cli_leaf(),
            pred::is_cli_test_support_leaf(),
        ]),
        cli_support_bonus,
    ),
    ScoreRule::when(
        "selection.tests.generic_test_penalty_under_examples_benches",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        generic_test_penalty_under_examples_benches,
    ),
    ScoreRule::when(
        "selection.tests.harness_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_test_harness_leaf(),
        ]),
        harness_bonus,
    ),
    ScoreRule::when(
        "selection.tests.python_test_adjustment",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_python_test_witness_leaf(),
        ]),
        python_test_adjustment,
    ),
    ScoreRule::when(
        "selection.tests.loose_python_test_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_loose_python_test_module_leaf(),
        ]),
        loose_python_test_bonus,
    ),
    ScoreRule::when(
        "selection.tests.non_code_doc_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_non_code_test_doc_leaf(),
        ]),
        non_code_doc_penalty,
    ),
    ScoreRule::when(
        "selection.tests.frontend_noise_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_frontend_runtime_noise_leaf(),
        ]),
        frontend_noise_penalty,
    ),
    ScoreRule::when(
        "selection.tests.cli_runtime_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::query_mentions_cli_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        cli_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.tests.cli_non_support_test_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::query_mentions_cli_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        cli_non_support_test_penalty,
    ),
];

const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
