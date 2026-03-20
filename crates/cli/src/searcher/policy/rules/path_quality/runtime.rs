use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn mcp_runtime_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.22))
}

fn mcp_runtime_prefers_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Support)
        .then_some(PolicyEffect::Multiply(1.10))
}

fn mcp_runtime_prefers_documentation(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Documentation)
        .then_some(PolicyEffect::Multiply(1.12))
}

fn mcp_runtime_penalizes_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Readme)
        .then_some(PolicyEffect::Multiply(0.92))
}

fn mcp_runtime_penalizes_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Tests)
        .then_some(PolicyEffect::Multiply(0.82))
}

fn benchmarks_prefers_benchmark_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_benchmarks && ctx.class == HybridSourceClass::BenchmarkDocs)
        .then_some(PolicyEffect::Multiply(2.00))
}

fn tests_prefers_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_tests && ctx.class == HybridSourceClass::Tests)
        .then_some(PolicyEffect::Multiply(1.24))
}

fn fixtures_prefers_fixtures(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_fixtures && ctx.class == HybridSourceClass::Fixtures)
        .then_some(PolicyEffect::Multiply(1.14))
}

fn runtime_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.05))
}

fn runtime_witness_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.52))
}

fn runtime_witness_prefers_support_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && matches!(
            ctx.class,
            HybridSourceClass::Support | HybridSourceClass::Tests
        ))
    .then_some(PolicyEffect::Multiply(1.24))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "mcp_runtime.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        mcp_runtime_prefers_runtime,
    ),
    ScoreRule::when(
        "mcp_runtime.prefers_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_support_leaf(),
        ]),
        mcp_runtime_prefers_support,
    ),
    ScoreRule::when(
        "mcp_runtime.prefers_documentation",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_documentation_leaf(),
        ]),
        mcp_runtime_prefers_documentation,
    ),
    ScoreRule::when(
        "mcp_runtime.penalizes_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_readme_leaf(),
        ]),
        mcp_runtime_penalizes_readme,
    ),
    ScoreRule::when(
        "mcp_runtime.penalizes_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        mcp_runtime_penalizes_tests,
    ),
    ScoreRule::when(
        "benchmarks.prefers_benchmark_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_benchmarks_leaf(),
            pred::class_is_benchmark_docs_leaf(),
        ]),
        benchmarks_prefers_benchmark_docs,
    ),
    ScoreRule::when(
        "tests.prefers_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_tests_leaf(), pred::class_is_tests_leaf()]),
        tests_prefers_tests,
    ),
    ScoreRule::when(
        "fixtures.prefers_fixtures",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_fixtures_leaf(), pred::class_is_fixtures_leaf()]),
        fixtures_prefers_fixtures,
    ),
    ScoreRule::when(
        "runtime.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_runtime_leaf(), pred::class_is_runtime_leaf()]),
        runtime_prefers_runtime,
    ),
    ScoreRule::when(
        "runtime_witness.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        runtime_witness_prefers_runtime,
    ),
    ScoreRule::when(
        "runtime_witness.prefers_support_tests",
        PolicyStage::PathQuality,
        Predicate::new(
            &[pred::wants_runtime_witnesses_leaf()],
            &[pred::class_is_support_leaf(), pred::class_is_tests_leaf()],
            &[],
        ),
        runtime_witness_prefers_support_tests,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
