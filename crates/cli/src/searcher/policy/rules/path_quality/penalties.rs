use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn examples_or_bench_penalizes_non_support_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses
        && ctx.class == HybridSourceClass::Tests
        && !ctx.is_example_support
        && !ctx.is_bench_support)
        .then_some(PolicyEffect::Multiply(if ctx.wants_test_witness_recall {
            0.92
        } else {
            0.68
        }))
}

fn runtime_witness_penalizes_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.penalize_generic_runtime_docs
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Multiply(0.48))
}

fn runtime_witness_penalizes_generic_runtime_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.penalize_generic_runtime_docs
        && ctx.is_generic_runtime_witness_doc)
        .then_some(PolicyEffect::Multiply(0.34))
}

fn runtime_witness_penalizes_repo_metadata(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.is_repo_metadata && !ctx.is_python_runtime_config)
        .then_some(PolicyEffect::Multiply(0.08))
}

fn runtime_witness_penalizes_example_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_example_support)
        .then_some(PolicyEffect::Multiply(0.32))
}

fn runtime_witness_penalizes_root_repo_metadata(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.is_repo_metadata
        && ctx.path_depth <= 1
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.42))
}

fn runtime_witness_penalizes_root_docs_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.path_depth <= 1
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Multiply(0.56))
}

fn runtime_witness_penalizes_ci_workflow_noise(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.is_ci_workflow
        && !ctx.wants_ci_workflow_witnesses
        && !ctx.wants_entrypoint_build_flow
        && !ctx.wants_scripts_ops_witnesses)
        .then_some(PolicyEffect::Multiply(0.08))
}

fn runtime_witness_penalizes_frontend_runtime_noise(
    ctx: &PathQualityFacts,
) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_frontend_runtime_noise)
        .then_some(PolicyEffect::Multiply(0.42))
}

fn runtime_config_penalizes_test_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_test_support && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.58))
}

fn runtime_config_penalizes_generic_runtime_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_generic_runtime_witness_doc
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.28))
}

fn runtime_config_penalizes_repo_metadata(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_metadata && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.10))
}

fn runtime_config_penalizes_ci_workflow_noise(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_ci_workflow && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.08))
}

fn runtime_config_penalizes_example_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_example_support
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.26))
}

fn entrypoint_penalizes_reference_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_reference_doc)
        .then_some(PolicyEffect::Multiply(0.72))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "examples_or_bench.penalizes_non_support_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        examples_or_bench_penalizes_non_support_tests,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::penalize_generic_runtime_docs_leaf(),
        ]),
        runtime_witness_penalizes_docs,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_generic_runtime_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::penalize_generic_runtime_docs_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        runtime_witness_penalizes_generic_runtime_docs,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_repo_metadata",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        runtime_witness_penalizes_repo_metadata,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_example_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_example_support_leaf(),
        ]),
        runtime_witness_penalizes_example_support,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_root_repo_metadata",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        runtime_witness_penalizes_root_repo_metadata,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_root_docs_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_runtime_witnesses_leaf()]),
        runtime_witness_penalizes_root_docs_readme,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_ci_workflow_noise",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_ci_workflow_leaf(),
        ]),
        runtime_witness_penalizes_ci_workflow_noise,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_frontend_runtime_noise",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_runtime_witnesses_leaf()]),
        runtime_witness_penalizes_frontend_runtime_noise,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_test_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_test_support_leaf(),
        ]),
        runtime_config_penalizes_test_support,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_generic_runtime_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        runtime_config_penalizes_generic_runtime_docs,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_repo_metadata",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        runtime_config_penalizes_repo_metadata,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_ci_workflow_noise",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_ci_workflow_leaf(),
        ]),
        runtime_config_penalizes_ci_workflow_noise,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_example_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_example_support_leaf(),
        ]),
        runtime_config_penalizes_example_support,
    ),
    ScoreRule::when(
        "entrypoint.penalizes_reference_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_reference_doc_leaf(),
        ]),
        entrypoint_penalizes_reference_docs,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
