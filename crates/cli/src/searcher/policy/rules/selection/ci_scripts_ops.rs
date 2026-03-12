use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn workflow_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_ci_workflow_witnesses && ctx.is_ci_workflow).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 1.44 } else { 0.78 },
    ))
}

fn ci_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_ci_workflow_witnesses
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        )
        && ctx.path_overlap == 0)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.22
        } else {
            -0.12
        }))
}

fn scripts_ops_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_scripts_ops_witnesses && ctx.is_scripts_ops).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 1.24 } else { 0.68 },
    ))
}

fn scripts_exact_query_match_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_scripts_ops_witnesses && ctx.has_exact_query_term_match).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 0.76 } else { 0.40 }),
    )
}

fn scripts_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_scripts_ops_witnesses
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        )
        && ctx.path_overlap == 0)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.18
        } else {
            -0.10
        }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.ci.workflow_bonus",
        PolicyStage::SelectionCiScriptsOps,
        workflow_bonus,
    ),
    ScoreRule::new(
        "selection.ci.doc_penalty",
        PolicyStage::SelectionCiScriptsOps,
        ci_doc_penalty,
    ),
    ScoreRule::new(
        "selection.scripts.ops_bonus",
        PolicyStage::SelectionCiScriptsOps,
        scripts_ops_bonus,
    ),
    ScoreRule::new(
        "selection.scripts.exact_query_match_bonus",
        PolicyStage::SelectionCiScriptsOps,
        scripts_exact_query_match_bonus,
    ),
    ScoreRule::new(
        "selection.scripts.doc_penalty",
        PolicyStage::SelectionCiScriptsOps,
        scripts_doc_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.wants_ci_workflow_witnesses && !ctx.wants_scripts_ops_witnesses {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
