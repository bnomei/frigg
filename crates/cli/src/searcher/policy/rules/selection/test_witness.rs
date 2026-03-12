use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn class_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.class == HybridSourceClass::Tests).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 1.42 } else { 0.72 }),
    )
}

fn specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall
        || !ctx.is_test_support
        || ctx.specific_witness_path_overlap == 0
    {
        return None;
    }

    let delta = match ctx.specific_witness_path_overlap {
        1 => {
            if ctx.seen_count == 0 {
                2.10
            } else {
                1.02
            }
        }
        2 => {
            if ctx.seen_count == 0 {
                3.68
            } else {
                1.82
            }
        }
        _ => {
            if ctx.seen_count == 0 {
                4.72
            } else {
                2.34
            }
        }
    };

    Some(PolicyEffect::Add(delta))
}

fn exact_query_match_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && ctx.has_exact_query_term_match
        && !(ctx.wants_example_or_bench_witnesses && ctx.is_examples_rs))
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            3.2
        } else {
            1.8
        }))
}

fn support_path_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall
        || !ctx.is_test_support
        || ctx.is_example_support
        || ctx.is_bench_support
    {
        return None;
    }

    let delta = match ctx.path_overlap {
        0 | 1 => 0.0,
        2 => {
            if ctx.seen_count == 0 {
                0.34
            } else {
                0.18
            }
        }
        _ => {
            if ctx.seen_count == 0 {
                1.80
            } else {
                0.92
            }
        }
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn example_or_bench_context_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && ctx.wants_example_or_bench_witnesses
        && ctx.is_test_support
        && !ctx.is_example_support
        && !ctx.is_bench_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.42
        } else {
            0.24
        }))
}

fn example_support_priority_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && ctx.wants_example_or_bench_witnesses
        && ctx.is_example_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.10
        } else {
            0.58
        }))
}

fn bench_support_priority_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.wants_example_or_bench_witnesses && ctx.is_bench_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.28
        } else {
            0.66
        }))
}

fn support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && !ctx.wants_example_or_bench_witnesses && ctx.is_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.18
        } else {
            0.10
        }))
}

fn generic_examples_rs_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.wants_example_or_bench_witnesses && ctx.is_examples_rs)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -3.00
        } else {
            -1.50
        }))
}

fn cli_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && !ctx.wants_example_or_bench_witnesses
        && ctx.query_mentions_cli
        && ctx.is_cli_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.84
        } else {
            0.46
        }))
}

fn generic_test_penalty_under_examples_benches(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && ctx.wants_example_or_bench_witnesses
        && ctx.class == HybridSourceClass::Tests
        && !ctx.is_example_support
        && !ctx.is_bench_support
        && ctx.specific_witness_path_overlap == 0
        && !ctx.has_exact_query_term_match
        && (!ctx.is_cli_test_support || !ctx.query_mentions_cli)
        && !ctx.is_test_harness)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -1.20
        } else {
            -0.60
        }))
}

fn harness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.is_test_harness).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 1.10 } else { 0.60 },
    ))
}

fn python_test_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall || !ctx.is_python_test_witness {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.wants_python_witnesses {
        if ctx.seen_count == 0 { 0.34 } else { 0.18 }
    } else if ctx.seen_count == 0 {
        -0.28
    } else {
        -0.14
    }))
}

fn loose_python_test_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.is_loose_python_test_module).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 0.12 } else { 0.06 },
    ))
}

fn non_code_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.is_non_code_test_doc).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { -0.44 } else { -0.26 },
    ))
}

fn frontend_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.is_frontend_runtime_noise).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { -0.28 } else { -0.16 },
    ))
}

fn cli_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && ctx.query_mentions_cli
        && ctx.class == HybridSourceClass::Runtime
        && !ctx.is_cli_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.34
        } else {
            -0.20
        }))
}

fn cli_non_support_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && ctx.query_mentions_cli
        && ctx.class == HybridSourceClass::Tests
        && !ctx.is_cli_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.24
        } else {
            -0.12
        }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.tests.class_bonus",
        PolicyStage::SelectionTestWitness,
        class_bonus,
    ),
    ScoreRule::new(
        "selection.tests.specific_overlap_bonus",
        PolicyStage::SelectionTestWitness,
        specific_overlap_bonus,
    ),
    ScoreRule::new(
        "selection.tests.exact_query_match_bonus",
        PolicyStage::SelectionTestWitness,
        exact_query_match_bonus,
    ),
    ScoreRule::new(
        "selection.tests.support_path_overlap_bonus",
        PolicyStage::SelectionTestWitness,
        support_path_overlap_bonus,
    ),
    ScoreRule::new(
        "selection.tests.example_or_bench_context_bonus",
        PolicyStage::SelectionTestWitness,
        example_or_bench_context_bonus,
    ),
    ScoreRule::new(
        "selection.tests.example_support_priority_bonus",
        PolicyStage::SelectionTestWitness,
        example_support_priority_bonus,
    ),
    ScoreRule::new(
        "selection.tests.bench_support_priority_bonus",
        PolicyStage::SelectionTestWitness,
        bench_support_priority_bonus,
    ),
    ScoreRule::new(
        "selection.tests.support_bonus",
        PolicyStage::SelectionTestWitness,
        support_bonus,
    ),
    ScoreRule::new(
        "selection.tests.generic_examples_rs_penalty",
        PolicyStage::SelectionTestWitness,
        generic_examples_rs_penalty,
    ),
    ScoreRule::new(
        "selection.tests.cli_support_bonus",
        PolicyStage::SelectionTestWitness,
        cli_support_bonus,
    ),
    ScoreRule::new(
        "selection.tests.generic_test_penalty_under_examples_benches",
        PolicyStage::SelectionTestWitness,
        generic_test_penalty_under_examples_benches,
    ),
    ScoreRule::new(
        "selection.tests.harness_bonus",
        PolicyStage::SelectionTestWitness,
        harness_bonus,
    ),
    ScoreRule::new(
        "selection.tests.python_test_adjustment",
        PolicyStage::SelectionTestWitness,
        python_test_adjustment,
    ),
    ScoreRule::new(
        "selection.tests.loose_python_test_bonus",
        PolicyStage::SelectionTestWitness,
        loose_python_test_bonus,
    ),
    ScoreRule::new(
        "selection.tests.non_code_doc_penalty",
        PolicyStage::SelectionTestWitness,
        non_code_doc_penalty,
    ),
    ScoreRule::new(
        "selection.tests.frontend_noise_penalty",
        PolicyStage::SelectionTestWitness,
        frontend_noise_penalty,
    ),
    ScoreRule::new(
        "selection.tests.cli_runtime_penalty",
        PolicyStage::SelectionTestWitness,
        cli_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.tests.cli_non_support_test_penalty",
        PolicyStage::SelectionTestWitness,
        cli_non_support_test_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.wants_test_witness_recall {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
