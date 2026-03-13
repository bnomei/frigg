use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use super::support::{selection_class_novelty_bonus, selection_class_repeat_penalty};
use crate::searcher::laravel::{
    laravel_ui_surface_novelty_bonus, laravel_ui_surface_repeat_penalty,
};

fn class_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(selection_class_novelty_bonus(ctx.class)))
}

fn class_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(
        -selection_class_repeat_penalty(ctx.class) * ctx.seen_count as f32,
    ))
}

fn laravel_surface_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let surface = ctx.laravel_surface?;
    Some(PolicyEffect::Add(laravel_ui_surface_novelty_bonus(
        surface,
        ctx.wants_blade_component_witnesses,
    )))
}

fn laravel_surface_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let surface = ctx.laravel_surface?;
    Some(PolicyEffect::Add(
        -laravel_ui_surface_repeat_penalty(surface, ctx.wants_blade_component_witnesses)
            * ctx.laravel_surface_seen as f32,
    ))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.novelty.class_bonus",
        PolicyStage::SelectionNovelty,
        Predicate::all(&[pred::wants_class_leaf(), pred::seen_count_is_zero_leaf()]),
        class_bonus,
    ),
    ScoreRule::when(
        "selection.novelty.class_repeat_penalty",
        PolicyStage::SelectionNovelty,
        Predicate::all(&[pred::seen_count_positive_leaf()]),
        class_repeat_penalty,
    ),
    ScoreRule::when(
        "selection.novelty.laravel_surface_bonus",
        PolicyStage::SelectionNovelty,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::has_laravel_surface_leaf(),
            pred::laravel_surface_seen_is_zero_leaf(),
        ]),
        laravel_surface_bonus,
    ),
    ScoreRule::when(
        "selection.novelty.laravel_surface_repeat_penalty",
        PolicyStage::SelectionNovelty,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::has_laravel_surface_leaf(),
            pred::laravel_surface_seen_positive_leaf(),
        ]),
        laravel_surface_repeat_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
