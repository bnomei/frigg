use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use super::support::{selection_class_novelty_bonus, selection_class_repeat_penalty};
use crate::searcher::laravel::{
    laravel_ui_surface_novelty_bonus, laravel_ui_surface_repeat_penalty,
};

fn class_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_class && ctx.seen_count == 0)
        .then_some(PolicyEffect::Add(selection_class_novelty_bonus(ctx.class)))
}

fn class_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.seen_count > 0).then_some(PolicyEffect::Add(
        -selection_class_repeat_penalty(ctx.class) * ctx.seen_count as f32,
    ))
}

fn laravel_surface_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let surface = ctx.laravel_surface?;
    (ctx.wants_laravel_ui_witnesses && ctx.laravel_surface_seen == 0).then_some(PolicyEffect::Add(
        laravel_ui_surface_novelty_bonus(surface, ctx.wants_blade_component_witnesses),
    ))
}

fn laravel_surface_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let surface = ctx.laravel_surface?;
    (ctx.wants_laravel_ui_witnesses && ctx.laravel_surface_seen > 0).then_some(PolicyEffect::Add(
        -laravel_ui_surface_repeat_penalty(surface, ctx.wants_blade_component_witnesses)
            * ctx.laravel_surface_seen as f32,
    ))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.novelty.class_bonus",
        PolicyStage::SelectionNovelty,
        class_bonus,
    ),
    ScoreRule::new(
        "selection.novelty.class_repeat_penalty",
        PolicyStage::SelectionNovelty,
        class_repeat_penalty,
    ),
    ScoreRule::new(
        "selection.novelty.laravel_surface_bonus",
        PolicyStage::SelectionNovelty,
        laravel_surface_bonus,
    ),
    ScoreRule::new(
        "selection.novelty.laravel_surface_repeat_penalty",
        PolicyStage::SelectionNovelty,
        laravel_surface_repeat_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rules(program, ctx, RULES);
}
