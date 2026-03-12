use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn exact_identifier_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.query_has_exact_terms
        && (ctx.wants_contracts || ctx.wants_error_taxonomy || ctx.wants_tool_contracts)
        && ctx.excerpt_has_exact_identifier_anchor)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.78
        } else {
            0.38
        }))
}

fn runtime_support_tests_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.query_has_exact_terms
        && (ctx.wants_contracts || ctx.wants_error_taxonomy || ctx.wants_tool_contracts)
        && ctx.excerpt_has_exact_identifier_anchor
        && matches!(
            ctx.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        ))
    .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
        0.18
    } else {
        0.08
    }))
}

fn missing_identifier_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.query_has_exact_terms
        && (ctx.wants_contracts || ctx.wants_error_taxonomy || ctx.wants_tool_contracts)
        && !ctx.excerpt_has_exact_identifier_anchor
        && matches!(
            ctx.class,
            HybridSourceClass::Runtime
                | HybridSourceClass::Support
                | HybridSourceClass::Tests
                | HybridSourceClass::Documentation
                | HybridSourceClass::Readme
                | HybridSourceClass::Fixtures
        ))
    .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
        -0.46
    } else {
        -0.24
    }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.contracts.exact_identifier_bonus",
        PolicyStage::SelectionContracts,
        exact_identifier_bonus,
    ),
    ScoreRule::new(
        "selection.contracts.runtime_support_tests_bonus",
        PolicyStage::SelectionContracts,
        runtime_support_tests_bonus,
    ),
    ScoreRule::new(
        "selection.contracts.missing_identifier_penalty",
        PolicyStage::SelectionContracts,
        missing_identifier_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rules(program, ctx, RULES);
}
