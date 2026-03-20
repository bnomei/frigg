use crate::searcher::policy::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use crate::searcher::policy::facts::PathQualityFacts;
use crate::searcher::policy::kernel::PolicyProgram;
use crate::searcher::policy::predicates::path_quality as pred;
use crate::searcher::policy::trace::{PolicyEffect, PolicyStage};

fn laravel_ui_prefers_blade_views(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && (ctx.is_laravel_non_livewire_blade_view || ctx.is_laravel_livewire_view))
        .then_some(PolicyEffect::Multiply(1.44))
}

fn laravel_ui_prefers_blade_components(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_blade_component_witnesses
        && ctx.is_laravel_blade_component)
        .then_some(PolicyEffect::Multiply(1.34))
}

fn laravel_layout_prefers_layout_views(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_layout_blade_view)
        .then_some(PolicyEffect::Multiply(1.48))
}

fn laravel_layout_penalizes_component_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_view_component_class)
        .then_some(PolicyEffect::Multiply(0.64))
}

fn laravel_ui_penalizes_view_component_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_laravel_view_component_class)
        .then_some(PolicyEffect::Multiply(0.72))
}

const RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "laravel_ui.prefers_blade_views",
        PolicyStage::PathQuality,
        Predicate::new(
            &[pred::wants_laravel_ui_witnesses_leaf()],
            &[
                pred::is_laravel_non_livewire_blade_view_leaf(),
                pred::is_laravel_livewire_view_leaf(),
            ],
            &[],
        ),
        laravel_ui_prefers_blade_views,
    ),
    ScoreRule::when(
        "laravel_ui.prefers_blade_components",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::is_laravel_blade_component_leaf(),
        ]),
        laravel_ui_prefers_blade_components,
    ),
    ScoreRule::when(
        "laravel_layout.prefers_layout_views",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::is_laravel_layout_blade_view_leaf(),
        ]),
        laravel_layout_prefers_layout_views,
    ),
    ScoreRule::when(
        "laravel_layout.penalizes_component_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::is_laravel_view_component_class_leaf(),
        ]),
        laravel_layout_penalizes_component_classes,
    ),
    ScoreRule::when(
        "laravel_ui.penalizes_view_component_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_laravel_view_component_class_leaf(),
        ]),
        laravel_ui_penalizes_view_component_classes,
    ),
];

const RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(RULES);

pub(super) fn apply(program: &mut PolicyProgram, ctx: &PathQualityFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
