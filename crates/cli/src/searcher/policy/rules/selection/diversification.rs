use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn first_build_workflow_bonus(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(4.0))
}

fn first_ci_workflow_bonus(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(0.16))
}

fn ci_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(
        -(if ctx.wants_runtime_config_artifacts {
            1.28
        } else {
            0.84
        } * ctx.seen_ci_workflows as f32),
    ))
}

fn ci_repo_root_penalty(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-0.22))
}

fn example_support_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_example_support == 0 {
        -0.36
    } else {
        -(0.86 * ctx.seen_example_support as f32)
    }))
}

fn mixed_query_first_example_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let best_overlap = ctx.path_overlap.max(ctx.specific_witness_path_overlap);
    let delta = match best_overlap {
        0 => {
            if ctx.has_exact_query_term_match {
                0.86
            } else {
                0.48
            }
        }
        1 => 1.18,
        _ => 1.72,
    };

    Some(PolicyEffect::Add(delta))
}

fn mixed_query_example_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-(1.00 * ctx.seen_example_support as f32)))
}

fn mixed_query_first_bench_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = match ctx.specific_witness_path_overlap {
        0 => 0.36,
        1 => 0.86,
        _ => 1.48,
    };

    Some(PolicyEffect::Add(delta))
}

fn mixed_query_bench_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
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
    if !ctx.has_exact_query_term_match && ctx.specific_witness_path_overlap == 0 {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.has_exact_query_term_match {
        1.44
    } else {
        0.84
    }))
}

fn mixed_query_plain_test_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(
        -(1.10 * ctx.seen_plain_test_support as f32),
    ))
}

fn typescript_index_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(
        if ctx.seen_typescript_runtime_module_indexes == 0 {
            0.96
        } else {
            0.36
        },
    ))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.diversification.first_build_workflow_bonus",
        PolicyStage::SelectionDiversification,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::is_entrypoint_build_workflow_leaf(),
                pred::seen_ci_workflows_is_zero_leaf(),
            ],
            &[
                pred::excerpt_has_build_flow_anchor_leaf(),
                pred::specific_witness_path_overlap_leaf(),
            ],
            &[],
        ),
        first_build_workflow_bonus,
    ),
    ScoreRule::when(
        "selection.diversification.first_ci_workflow_bonus",
        PolicyStage::SelectionDiversification,
        Predicate::new(
            &[
                pred::wants_runtime_config_or_entrypoint_build_flow_leaf(),
                pred::is_ci_workflow_leaf(),
                pred::seen_ci_workflows_is_zero_leaf(),
            ],
            &[
                pred::excerpt_has_build_flow_anchor_leaf(),
                pred::specific_witness_path_overlap_leaf(),
            ],
            &[],
        ),
        first_ci_workflow_bonus,
    ),
    ScoreRule::when(
        "selection.diversification.ci_repeat_penalty",
        PolicyStage::SelectionDiversification,
        Predicate::all(&[
            pred::wants_runtime_config_or_entrypoint_build_flow_leaf(),
            pred::is_ci_workflow_leaf(),
            pred::seen_ci_workflows_positive_leaf(),
        ]),
        ci_repeat_penalty,
    ),
    ScoreRule::when(
        "selection.diversification.ci_repo_root_penalty",
        PolicyStage::SelectionDiversification,
        Predicate::all(&[
            pred::wants_runtime_config_or_entrypoint_build_flow_leaf(),
            pred::is_ci_workflow_leaf(),
            pred::seen_ci_workflows_positive_leaf(),
            pred::has_seen_repo_root_runtime_config_leaf(),
        ]),
        ci_repo_root_penalty,
    ),
    ScoreRule::when(
        "selection.diversification.example_support_penalty",
        PolicyStage::SelectionDiversification,
        Predicate::new(
            &[
                pred::wants_runtime_config_or_entrypoint_build_flow_leaf(),
                pred::is_example_support_leaf(),
            ],
            &[],
            &[pred::wants_examples_leaf()],
        ),
        example_support_penalty,
    ),
    ScoreRule::when(
        "selection.diversification.mixed_query_first_example_bonus",
        PolicyStage::SelectionDiversification,
        Predicate::all(&[
            pred::wants_mixed_query_example_or_bench_leaf(),
            pred::is_example_support_leaf(),
            pred::seen_example_support_is_zero_leaf(),
        ]),
        mixed_query_first_example_bonus,
    ),
    ScoreRule::when(
        "selection.diversification.mixed_query_example_repeat_penalty",
        PolicyStage::SelectionDiversification,
        Predicate::new(
            &[
                pred::wants_mixed_query_example_or_bench_leaf(),
                pred::is_example_support_leaf(),
                pred::seen_example_support_positive_leaf(),
            ],
            &[],
            &[
                pred::path_overlap_leaf(),
                pred::has_exact_query_term_match_leaf(),
            ],
        ),
        mixed_query_example_repeat_penalty,
    ),
    ScoreRule::when(
        "selection.diversification.mixed_query_first_bench_bonus",
        PolicyStage::SelectionDiversification,
        Predicate::all(&[
            pred::wants_mixed_query_example_or_bench_leaf(),
            pred::is_bench_support_leaf(),
            pred::seen_bench_support_is_zero_leaf(),
        ]),
        mixed_query_first_bench_bonus,
    ),
    ScoreRule::when(
        "selection.diversification.mixed_query_bench_repeat_penalty",
        PolicyStage::SelectionDiversification,
        Predicate::all(&[
            pred::wants_mixed_query_example_or_bench_leaf(),
            pred::is_bench_support_leaf(),
            pred::seen_bench_support_positive_leaf(),
        ]),
        mixed_query_bench_repeat_penalty,
    ),
    ScoreRule::when(
        "selection.diversification.mixed_query_first_plain_test_bonus",
        PolicyStage::SelectionDiversification,
        Predicate::new(
            &[
                pred::wants_mixed_query_example_or_bench_leaf(),
                pred::is_test_support_leaf(),
                pred::seen_plain_test_support_is_zero_leaf(),
            ],
            &[],
            &[
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
            ],
        ),
        mixed_query_first_plain_test_bonus,
    ),
    ScoreRule::when(
        "selection.diversification.mixed_query_plain_test_repeat_penalty",
        PolicyStage::SelectionDiversification,
        Predicate::new(
            &[
                pred::wants_mixed_query_example_or_bench_leaf(),
                pred::is_test_support_leaf(),
                pred::seen_plain_test_support_positive_leaf(),
            ],
            &[],
            &[
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
                pred::has_exact_query_term_match_leaf(),
                pred::specific_witness_path_overlap_leaf(),
            ],
        ),
        mixed_query_plain_test_repeat_penalty,
    ),
    ScoreRule::when(
        "selection.diversification.typescript_index_bonus",
        PolicyStage::SelectionDiversification,
        Predicate::all(&[
            pred::wants_runtime_config_or_entrypoint_build_flow_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        typescript_index_bonus,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
