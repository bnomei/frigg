use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};
fn entrypoint_prefers_entrypoint_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_runtime)
        .then_some(PolicyEffect::Multiply(1.92))
}

fn entrypoint_prefers_build_workflows(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_build_workflow)
        .then_some(PolicyEffect::Multiply(2.20))
}

fn entrypoint_prefers_ci_workflows(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && !ctx.is_entrypoint_build_workflow && ctx.is_ci_workflow)
        .then_some(PolicyEffect::Multiply(1.36))
}

fn entrypoint_prefers_typescript_index(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_typescript_runtime_module_index)
        .then_some(PolicyEffect::Multiply(1.16))
}

fn entrypoint_prefers_runtime_config_artifacts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.18))
}

fn entrypoint_prefers_repo_root_runtime_config_artifacts(
    ctx: &PathQualityFacts,
) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_repo_root_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.28))
}

fn entrypoint_penalizes_example_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_example_support)
        .then_some(PolicyEffect::Multiply(0.62))
}

fn entrypoint_penalizes_bench_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_bench_support)
        .then_some(PolicyEffect::Multiply(0.72))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "entrypoint.prefers_entrypoint_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_runtime_leaf(),
        ]),
        entrypoint_prefers_entrypoint_runtime,
    ),
    ScoreRule::when(
        "entrypoint.prefers_build_workflows",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_build_workflow_leaf(),
        ]),
        entrypoint_prefers_build_workflows,
    ),
    ScoreRule::when(
        "entrypoint.prefers_ci_workflows",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_ci_workflow_leaf(),
        ]),
        entrypoint_prefers_ci_workflows,
    ),
    ScoreRule::when(
        "entrypoint.prefers_typescript_index",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        entrypoint_prefers_typescript_index,
    ),
    ScoreRule::when(
        "entrypoint.prefers_runtime_config_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_runtime_config_artifact_leaf(),
        ]),
        entrypoint_prefers_runtime_config_artifacts,
    ),
    ScoreRule::when(
        "entrypoint.prefers_repo_root_runtime_config_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        entrypoint_prefers_repo_root_runtime_config_artifacts,
    ),
    ScoreRule::when(
        "entrypoint.penalizes_example_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_example_support_leaf(),
        ]),
        entrypoint_penalizes_example_support,
    ),
    ScoreRule::when(
        "entrypoint.penalizes_bench_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_bench_support_leaf(),
        ]),
        entrypoint_penalizes_bench_support,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
