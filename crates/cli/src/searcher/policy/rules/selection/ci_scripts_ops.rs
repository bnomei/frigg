use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn workflow_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        1.44
    } else {
        0.78
    }))
}

fn ci_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        -0.22
    } else {
        -0.12
    }))
}

fn scripts_ops_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        1.24
    } else {
        0.68
    }))
}

fn scripts_exact_query_match_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        0.76
    } else {
        0.40
    }))
}

fn scripts_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        -0.18
    } else {
        -0.10
    }))
}

const DOCISH_CLASSES: &[super::super::super::dsl::PredicateLeaf<SelectionFacts>] = &[
    pred::class_is_documentation_leaf(),
    pred::class_is_readme_leaf(),
];

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.ci.workflow_bonus",
        PolicyStage::SelectionCiScriptsOps,
        Predicate::all(&[
            pred::wants_ci_workflow_witnesses_leaf(),
            pred::is_ci_workflow_leaf(),
        ]),
        workflow_bonus,
    ),
    ScoreRule::when(
        "selection.ci.doc_penalty",
        PolicyStage::SelectionCiScriptsOps,
        Predicate::new(
            &[pred::wants_ci_workflow_witnesses_leaf()],
            DOCISH_CLASSES,
            &[pred::path_overlap_leaf()],
        ),
        ci_doc_penalty,
    ),
    ScoreRule::when(
        "selection.scripts.ops_bonus",
        PolicyStage::SelectionCiScriptsOps,
        Predicate::all(&[
            pred::wants_scripts_ops_witnesses_leaf(),
            pred::is_scripts_ops_leaf(),
        ]),
        scripts_ops_bonus,
    ),
    ScoreRule::when(
        "selection.scripts.exact_query_match_bonus",
        PolicyStage::SelectionCiScriptsOps,
        Predicate::all(&[
            pred::wants_scripts_ops_witnesses_leaf(),
            pred::has_exact_query_term_match_leaf(),
        ]),
        scripts_exact_query_match_bonus,
    ),
    ScoreRule::when(
        "selection.scripts.doc_penalty",
        PolicyStage::SelectionCiScriptsOps,
        Predicate::new(
            &[pred::wants_scripts_ops_witnesses_leaf()],
            DOCISH_CLASSES,
            &[pred::path_overlap_leaf()],
        ),
        scripts_doc_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
