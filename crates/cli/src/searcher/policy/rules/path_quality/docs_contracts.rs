use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn docs_prefers_documentation_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_docs
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation
                | HybridSourceClass::ErrorContracts
                | HybridSourceClass::ToolContracts
                | HybridSourceClass::BenchmarkDocs
        ))
    .then_some(PolicyEffect::Multiply(1.36))
}

fn readme_prefers_readme_class(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_readme && ctx.class == HybridSourceClass::Readme)
        .then_some(PolicyEffect::Multiply(1.15))
}

fn readme_prefers_root_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_readme && ctx.is_root_readme).then_some(PolicyEffect::Multiply(1.45))
}

fn onboarding_prefers_readme_class(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_onboarding && ctx.class == HybridSourceClass::Readme)
        .then_some(PolicyEffect::Multiply(1.85))
}

fn onboarding_prefers_root_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_onboarding && ctx.is_root_readme).then_some(PolicyEffect::Multiply(1.25))
}

fn onboarding_prefers_documentation_class(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_onboarding && ctx.class == HybridSourceClass::Documentation)
        .then_some(PolicyEffect::Multiply(1.15))
}

fn contracts_prefers_contract_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_contracts
        && matches!(
            ctx.class,
            HybridSourceClass::ErrorContracts | HybridSourceClass::ToolContracts
        ))
    .then_some(PolicyEffect::Multiply(1.55))
}

fn error_taxonomy_prefers_error_contracts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy && ctx.class == HybridSourceClass::ErrorContracts)
        .then_some(PolicyEffect::Multiply(1.95))
}

fn error_taxonomy_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.18))
}

fn error_taxonomy_prefers_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy && ctx.class == HybridSourceClass::Tests)
        .then_some(PolicyEffect::Multiply(1.26))
}

fn error_taxonomy_penalizes_docs_readme_specs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme | HybridSourceClass::Specs
        ))
    .then_some(PolicyEffect::Multiply(0.78))
}

fn tool_contracts_prefers_tool_contracts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_tool_contracts && ctx.class == HybridSourceClass::ToolContracts)
        .then_some(PolicyEffect::Multiply(2.10))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "docs.prefers_documentation_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_docs_leaf()]),
        docs_prefers_documentation_classes,
    ),
    ScoreRule::when(
        "readme.prefers_readme_class",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_readme_leaf(), pred::class_is_readme_leaf()]),
        readme_prefers_readme_class,
    ),
    ScoreRule::when(
        "readme.prefers_root_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_readme_leaf(), pred::is_root_readme_leaf()]),
        readme_prefers_root_readme,
    ),
    ScoreRule::when(
        "onboarding.prefers_readme_class",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_onboarding_leaf(), pred::class_is_readme_leaf()]),
        onboarding_prefers_readme_class,
    ),
    ScoreRule::when(
        "onboarding.prefers_root_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_onboarding_leaf(), pred::is_root_readme_leaf()]),
        onboarding_prefers_root_readme,
    ),
    ScoreRule::when(
        "onboarding.prefers_documentation_class",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_onboarding_leaf(),
            pred::class_is_documentation_leaf(),
        ]),
        onboarding_prefers_documentation_class,
    ),
    ScoreRule::when(
        "contracts.prefers_contract_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_contracts_leaf()]),
        contracts_prefers_contract_classes,
    ),
    ScoreRule::when(
        "error_taxonomy.prefers_error_contracts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_error_taxonomy_leaf(),
            pred::class_is_error_contracts_leaf(),
        ]),
        error_taxonomy_prefers_error_contracts,
    ),
    ScoreRule::when(
        "error_taxonomy.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_error_taxonomy_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        error_taxonomy_prefers_runtime,
    ),
    ScoreRule::when(
        "error_taxonomy.prefers_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_error_taxonomy_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        error_taxonomy_prefers_tests,
    ),
    ScoreRule::when(
        "error_taxonomy.penalizes_docs_readme_specs",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_error_taxonomy_leaf()]),
        error_taxonomy_penalizes_docs_readme_specs,
    ),
    ScoreRule::when(
        "tool_contracts.prefers_tool_contracts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_tool_contracts_leaf(),
            pred::class_is_tool_contracts_leaf(),
        ]),
        tool_contracts_prefers_tool_contracts,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
