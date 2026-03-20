use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn examples_or_bench_prefers_example_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses && ctx.is_example_support).then_some(
        PolicyEffect::Multiply(if ctx.wants_examples { 1.34 } else { 1.18 }),
    )
}

fn examples_or_bench_prefers_bench_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses && ctx.is_bench_support).then_some(
        PolicyEffect::Multiply(if ctx.wants_benchmarks { 1.38 } else { 1.22 }),
    )
}

fn examples_or_bench_penalizes_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Multiply(0.72))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "examples_or_bench.prefers_example_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_example_support_leaf(),
        ]),
        examples_or_bench_prefers_example_support,
    ),
    ScoreRule::when(
        "examples_or_bench.prefers_bench_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_bench_support_leaf(),
        ]),
        examples_or_bench_prefers_bench_support,
    ),
    ScoreRule::when(
        "examples_or_bench.penalizes_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_example_or_bench_witnesses_leaf()]),
        examples_or_bench_penalizes_docs,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
