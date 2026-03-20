use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};
fn navigation_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.is_navigation_runtime)
        .then_some(PolicyEffect::Multiply(1.24))
}

fn navigation_penalizes_reference_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.is_navigation_reference_doc)
        .then_some(PolicyEffect::Multiply(0.72))
}

fn navigation_mcp_runtime_bonus(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.wants_mcp_runtime_surface && ctx.is_navigation_runtime)
        .then_some(PolicyEffect::Multiply(1.28))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "navigation.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::is_navigation_runtime_leaf(),
        ]),
        navigation_prefers_runtime,
    ),
    ScoreRule::when(
        "navigation.penalizes_reference_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::is_navigation_reference_doc_leaf(),
        ]),
        navigation_penalizes_reference_docs,
    ),
    ScoreRule::when(
        "navigation.mcp_runtime_bonus",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::wants_mcp_runtime_surface_leaf(),
            pred::is_navigation_runtime_leaf(),
        ]),
        navigation_mcp_runtime_bonus,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
