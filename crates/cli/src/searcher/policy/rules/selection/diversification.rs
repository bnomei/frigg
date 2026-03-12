use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn first_build_workflow_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_entrypoint_build_workflow
        && ctx.seen_ci_workflows == 0)
        .then_some(PolicyEffect::Add(4.0))
}

fn first_ci_workflow_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.is_ci_workflow
        && (ctx.wants_runtime_config_artifacts || ctx.wants_entrypoint_build_flow)
        && ctx.seen_ci_workflows == 0)
        .then_some(PolicyEffect::Add(0.16))
}

fn ci_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.is_ci_workflow
        && (ctx.wants_runtime_config_artifacts || ctx.wants_entrypoint_build_flow)
        && ctx.seen_ci_workflows > 0)
        .then_some(PolicyEffect::Add(
            -(if ctx.wants_runtime_config_artifacts {
                1.28
            } else {
                0.84
            } * ctx.seen_ci_workflows as f32),
        ))
}

fn ci_repo_root_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.is_ci_workflow
        && (ctx.wants_runtime_config_artifacts || ctx.wants_entrypoint_build_flow)
        && ctx.seen_ci_workflows > 0
        && ctx.seen_repo_root_runtime_configs > 0)
        .then_some(PolicyEffect::Add(-0.22))
}

fn example_support_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.is_example_support
        && !ctx.wants_examples
        && (ctx.wants_runtime_config_artifacts || ctx.wants_entrypoint_build_flow))
        .then_some(PolicyEffect::Add(if ctx.seen_example_support == 0 {
            -0.36
        } else {
            -(0.86 * ctx.seen_example_support as f32)
        }))
}

fn mixed_query_first_bench_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall
        || !ctx.wants_example_or_bench_witnesses
        || !ctx.is_bench_support
        || ctx.seen_bench_support > 0
    {
        return None;
    }

    let delta = match ctx.specific_witness_path_overlap {
        0 => 0.36,
        1 => 0.86,
        _ => 1.48,
    };

    Some(PolicyEffect::Add(delta))
}

fn mixed_query_bench_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall
        || !ctx.wants_example_or_bench_witnesses
        || !ctx.is_bench_support
        || ctx.seen_bench_support == 0
    {
        return None;
    }

    let per_seen = if ctx.specific_witness_path_overlap >= 2 {
        0.72
    } else {
        1.40
    };

    Some(PolicyEffect::Add(
        -(per_seen * ctx.seen_bench_support as f32),
    ))
}

fn mixed_query_first_plain_test_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall
        || !ctx.wants_example_or_bench_witnesses
        || !ctx.is_test_support
        || ctx.is_example_support
        || ctx.is_bench_support
        || ctx.seen_plain_test_support > 0
        || (!ctx.has_exact_query_term_match && ctx.specific_witness_path_overlap == 0)
    {
        return None;
    }

    let delta = if ctx.has_exact_query_term_match {
        1.44
    } else {
        0.84
    };

    Some(PolicyEffect::Add(delta))
}

fn mixed_query_plain_test_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall
        || !ctx.wants_example_or_bench_witnesses
        || !ctx.is_test_support
        || ctx.is_example_support
        || ctx.is_bench_support
        || ctx.seen_plain_test_support == 0
        || ctx.has_exact_query_term_match
        || ctx.specific_witness_path_overlap > 0
    {
        return None;
    }

    Some(PolicyEffect::Add(
        -(1.10 * ctx.seen_plain_test_support as f32),
    ))
}

fn typescript_index_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.is_typescript_runtime_module_index
        && (ctx.wants_runtime_config_artifacts || ctx.wants_entrypoint_build_flow))
        .then_some(PolicyEffect::Add(
            if ctx.seen_typescript_runtime_module_indexes == 0 {
                0.96
            } else {
                0.36
            },
        ))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.diversification.first_build_workflow_bonus",
        PolicyStage::SelectionDiversification,
        first_build_workflow_bonus,
    ),
    ScoreRule::new(
        "selection.diversification.first_ci_workflow_bonus",
        PolicyStage::SelectionDiversification,
        first_ci_workflow_bonus,
    ),
    ScoreRule::new(
        "selection.diversification.ci_repeat_penalty",
        PolicyStage::SelectionDiversification,
        ci_repeat_penalty,
    ),
    ScoreRule::new(
        "selection.diversification.ci_repo_root_penalty",
        PolicyStage::SelectionDiversification,
        ci_repo_root_penalty,
    ),
    ScoreRule::new(
        "selection.diversification.example_support_penalty",
        PolicyStage::SelectionDiversification,
        example_support_penalty,
    ),
    ScoreRule::new(
        "selection.diversification.mixed_query_first_bench_bonus",
        PolicyStage::SelectionDiversification,
        mixed_query_first_bench_bonus,
    ),
    ScoreRule::new(
        "selection.diversification.mixed_query_bench_repeat_penalty",
        PolicyStage::SelectionDiversification,
        mixed_query_bench_repeat_penalty,
    ),
    ScoreRule::new(
        "selection.diversification.mixed_query_first_plain_test_bonus",
        PolicyStage::SelectionDiversification,
        mixed_query_first_plain_test_bonus,
    ),
    ScoreRule::new(
        "selection.diversification.mixed_query_plain_test_repeat_penalty",
        PolicyStage::SelectionDiversification,
        mixed_query_plain_test_repeat_penalty,
    ),
    ScoreRule::new(
        "selection.diversification.typescript_index_bonus",
        PolicyStage::SelectionDiversification,
        typescript_index_bonus,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rules(program, ctx, RULES);
}
