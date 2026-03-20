use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn runtime_config_prefers_artifacts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.40))
}

fn runtime_config_prefers_repo_root_artifacts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_root_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.38))
}

fn runtime_config_prefers_entrypoint_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_entrypoint_runtime)
        .then_some(PolicyEffect::Multiply(1.72))
}

fn runtime_config_prefers_typescript_index(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_typescript_runtime_module_index)
        .then_some(PolicyEffect::Multiply(1.22))
}

fn runtime_config_prefers_python_config(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_python_runtime_config)
        .then_some(PolicyEffect::Multiply(1.36))
}

fn runtime_config_penalizes_docs_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Multiply(if ctx.is_root_readme {
        0.62
    } else {
        0.74
    }))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "runtime_config.prefers_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_runtime_config_artifact_leaf(),
        ]),
        runtime_config_prefers_artifacts,
    ),
    ScoreRule::when(
        "runtime_config.prefers_repo_root_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        runtime_config_prefers_repo_root_artifacts,
    ),
    ScoreRule::when(
        "runtime_config.prefers_entrypoint_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_runtime_leaf(),
        ]),
        runtime_config_prefers_entrypoint_runtime,
    ),
    ScoreRule::when(
        "runtime_config.prefers_typescript_index",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        runtime_config_prefers_typescript_index,
    ),
    ScoreRule::when(
        "runtime_config.prefers_python_config",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_python_runtime_config_leaf(),
        ]),
        runtime_config_prefers_python_config,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_docs_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_runtime_config_artifacts_leaf()]),
        runtime_config_penalizes_docs_readme,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
