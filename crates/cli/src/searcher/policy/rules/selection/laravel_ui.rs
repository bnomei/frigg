use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::laravel::LaravelUiSurfaceClass;

fn specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses || ctx.specific_witness_path_overlap == 0 {
        return None;
    }

    let delta = match ctx.specific_witness_path_overlap {
        1 => {
            if ctx.seen_count == 0 {
                0.96
            } else {
                0.44
            }
        }
        2 => {
            if ctx.seen_count == 0 {
                1.68
            } else {
                0.82
            }
        }
        _ => {
            if ctx.seen_count == 0 {
                2.24
            } else {
                1.08
            }
        }
    };

    Some(PolicyEffect::Add(delta))
}

fn missing_specific_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.specific_witness_path_overlap == 0
        && ctx.query_has_specific_blade_anchors
        && (ctx.is_laravel_non_livewire_blade_view || ctx.is_laravel_livewire_view))
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.62
        } else {
            -0.32
        }))
}

fn blade_specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || !ctx.wants_blade_component_witnesses
        || ctx.blade_specific_path_overlap == 0
    {
        return None;
    }

    Some(PolicyEffect::Add(match ctx.blade_specific_path_overlap {
        1 => 0.74,
        2 => 1.62,
        _ => 2.28,
    }))
}

fn generic_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_blade_component_witnesses
        && ctx.blade_specific_path_overlap == 0
        && ctx.query_has_specific_blade_anchors
        && ctx.is_laravel_blade_component
        && !ctx.wants_laravel_layout_witnesses)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.46
        } else {
            -0.22
        }))
}

fn form_action_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_form_action_witnesses
        && ctx.is_laravel_form_action_blade)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.42
        } else {
            0.54
        }))
}

fn form_action_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_form_action_witnesses
        && !ctx.is_laravel_form_action_blade
        && ctx.is_laravel_blade_component)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.24
        } else {
            -0.12
        }))
}

fn non_livewire_view_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_livewire_view_witnesses
        && ctx.is_laravel_non_livewire_blade_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.34
        } else {
            -0.18
        }))
}

fn livewire_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_livewire_view_witnesses
        && ctx.is_laravel_livewire_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.72
        } else {
            0.28
        }))
}

fn view_component_class_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses || !ctx.is_laravel_view_component_class {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.wants_laravel_layout_witnesses {
        if ctx.seen_count == 0 { -1.40 } else { -1.80 }
    } else if ctx.seen_count == 0 {
        -1.00
    } else {
        -1.40
    }))
}

fn livewire_component_blade_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || !ctx.wants_blade_component_witnesses
        || !ctx.is_laravel_livewire_component
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.wants_livewire_view_witnesses {
        if ctx.seen_count == 0 { 0.02 } else { -0.18 }
    } else if ctx.seen_count == 0 {
        -0.54
    } else {
        -0.30
    }))
}

fn non_livewire_blade_view_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_blade_component_witnesses
        && ctx.is_laravel_non_livewire_blade_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.96
        } else {
            0.40
        }))
}

fn livewire_view_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_blade_component_witnesses
        && ctx.is_laravel_livewire_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.12
        } else {
            0.02
        }))
}

fn blade_component_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || !ctx.wants_blade_component_witnesses
        || !ctx.is_laravel_blade_component
    {
        return None;
    }

    Some(PolicyEffect::Add(
        if ctx.is_laravel_nested_blade_component {
            if ctx.seen_count == 0 { 0.24 } else { -0.04 }
        } else if ctx.seen_count == 0 {
            1.48
        } else {
            0.60
        },
    ))
}

fn livewire_component_general_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || ctx.wants_blade_component_witnesses
        || !ctx.is_laravel_livewire_component
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        0.18
    } else {
        -0.18
    }))
}

fn non_livewire_blade_view_general_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_blade_component_witnesses
        && ctx.is_laravel_non_livewire_blade_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.05
        } else {
            0.54
        }))
}

fn livewire_view_general_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_blade_component_witnesses
        && ctx.is_laravel_livewire_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.92
        } else {
            0.44
        }))
}

fn blade_component_general_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || ctx.wants_blade_component_witnesses
        || !ctx.is_laravel_blade_component
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.path_overlap >= 3 {
        if ctx.seen_count == 0 { 0.72 } else { 0.26 }
    } else if ctx.seen_count == 0 {
        0.10
    } else {
        -0.12
    }))
}

fn test_harness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_test_harness).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 0.42 } else { 0.18 },
    ))
}

fn command_middleware_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_commands_middleware_witnesses
        && ctx.is_laravel_command_or_middleware)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.18
        } else {
            0.48
        }))
}

fn job_listener_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_jobs_listeners_witnesses
        && ctx.is_laravel_job_or_listener)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.96
        } else {
            0.36
        }))
}

fn layout_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_layout_blade_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.80
        } else {
            0.76
        }))
}

fn layout_blade_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_layout_witnesses
        && ctx.specific_witness_path_overlap > 0
        && ctx.is_laravel_non_livewire_blade_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.78
        } else {
            0.32
        }))
}

fn blade_page_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_laravel_form_action_witnesses
        && !ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_non_livewire_blade_view
        && !ctx.is_laravel_layout_blade_view)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.80
        } else {
            0.76
        }))
}

fn layout_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_blade_component)
        .then_some(PolicyEffect::Add(
            if ctx.blade_specific_path_overlap == 0 && ctx.specific_witness_path_overlap == 0 {
                if ctx.seen_count == 0 { -0.92 } else { -0.44 }
            } else if ctx.seen_count == 0 {
                0.14
            } else {
                0.06
            },
        ))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_repo_metadata).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { -0.34 } else { -0.20 },
    ))
}

fn surface_blade_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || !ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::BladeView)
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.laravel_surface_seen == 0 {
        0.44
    } else {
        -0.18 * ctx.laravel_surface_seen as f32
    }))
}

fn surface_livewire_component(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || !ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::LivewireComponent)
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.laravel_surface_seen == 0 {
        0.08
    } else {
        -0.12 * ctx.laravel_surface_seen as f32
    }))
}

fn surface_livewire_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || !ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::LivewireView)
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.laravel_surface_seen == 0 {
        0.10
    } else {
        -0.12 * ctx.laravel_surface_seen as f32
    }))
}

fn surface_blade_component(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || !ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::BladeComponent)
    {
        return None;
    }

    Some(PolicyEffect::Add(
        if ctx.is_laravel_nested_blade_component {
            if ctx.laravel_surface_seen == 0 {
                0.10
            } else {
                -0.12 * ctx.laravel_surface_seen as f32
            }
        } else if ctx.laravel_surface_seen == 0 {
            0.96
        } else {
            0.34 - (0.08 * ctx.laravel_surface_seen as f32)
        },
    ))
}

fn surface_blade_component_first_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_blade_component_witnesses
        && !ctx.is_laravel_nested_blade_component
        && ctx.laravel_surface == Some(LaravelUiSurfaceClass::BladeComponent)
        && ctx.laravel_surface_seen == 0)
        .then_some(PolicyEffect::Add(0.28))
}

fn surface_general_blade_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::BladeView)
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.laravel_surface_seen == 0 {
        0.72
    } else {
        0.14
    }))
}

fn surface_first_blade_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_blade_component_witnesses
        && ctx.laravel_surface == Some(LaravelUiSurfaceClass::BladeView)
        && ctx.laravel_surface_seen == 0)
        .then_some(PolicyEffect::Add(0.34))
}

fn surface_general_livewire_component(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::LivewireComponent)
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.laravel_surface_seen == 0 {
        0.18
    } else {
        -0.14 * ctx.laravel_surface_seen as f32
    }))
}

fn surface_general_livewire_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::LivewireView)
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.laravel_surface_seen == 0 {
        0.84
    } else {
        0.18
    }))
}

fn surface_first_livewire_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_blade_component_witnesses
        && ctx.laravel_surface == Some(LaravelUiSurfaceClass::LivewireView)
        && ctx.laravel_surface_seen == 0)
        .then_some(PolicyEffect::Add(0.28))
}

fn surface_general_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_laravel_ui_witnesses
        || ctx.wants_blade_component_witnesses
        || ctx.laravel_surface != Some(LaravelUiSurfaceClass::BladeComponent)
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.laravel_surface_seen == 0 {
        -0.04
    } else {
        -(0.72 * ctx.laravel_surface_seen as f32)
    }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.laravel.specific_overlap_bonus",
        PolicyStage::SelectionLaravelUi,
        specific_overlap_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.missing_specific_anchor_penalty",
        PolicyStage::SelectionLaravelUi,
        missing_specific_anchor_penalty,
    ),
    ScoreRule::new(
        "selection.laravel.blade_specific_overlap_bonus",
        PolicyStage::SelectionLaravelUi,
        blade_specific_overlap_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.generic_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        generic_blade_component_penalty,
    ),
    ScoreRule::new(
        "selection.laravel.form_action_bonus",
        PolicyStage::SelectionLaravelUi,
        form_action_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.form_action_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        form_action_blade_component_penalty,
    ),
    ScoreRule::new(
        "selection.laravel.non_livewire_view_penalty",
        PolicyStage::SelectionLaravelUi,
        non_livewire_view_penalty,
    ),
    ScoreRule::new(
        "selection.laravel.livewire_view_bonus",
        PolicyStage::SelectionLaravelUi,
        livewire_view_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.view_component_class_penalty",
        PolicyStage::SelectionLaravelUi,
        view_component_class_penalty,
    ),
    ScoreRule::new(
        "selection.laravel.livewire_component_bias",
        PolicyStage::SelectionLaravelUi,
        livewire_component_blade_bias,
    ),
    ScoreRule::new(
        "selection.laravel.non_livewire_blade_view_bias",
        PolicyStage::SelectionLaravelUi,
        non_livewire_blade_view_bias,
    ),
    ScoreRule::new(
        "selection.laravel.livewire_view_bias",
        PolicyStage::SelectionLaravelUi,
        livewire_view_bias,
    ),
    ScoreRule::new(
        "selection.laravel.blade_component_bias",
        PolicyStage::SelectionLaravelUi,
        blade_component_bias,
    ),
    ScoreRule::new(
        "selection.laravel.livewire_component_general_bias",
        PolicyStage::SelectionLaravelUi,
        livewire_component_general_bias,
    ),
    ScoreRule::new(
        "selection.laravel.non_livewire_blade_view_general_bonus",
        PolicyStage::SelectionLaravelUi,
        non_livewire_blade_view_general_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.livewire_view_general_bonus",
        PolicyStage::SelectionLaravelUi,
        livewire_view_general_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.blade_component_general_bias",
        PolicyStage::SelectionLaravelUi,
        blade_component_general_bias,
    ),
    ScoreRule::new(
        "selection.laravel.test_harness_bonus",
        PolicyStage::SelectionLaravelUi,
        test_harness_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.command_middleware_bonus",
        PolicyStage::SelectionLaravelUi,
        command_middleware_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.job_listener_bonus",
        PolicyStage::SelectionLaravelUi,
        job_listener_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.layout_bonus",
        PolicyStage::SelectionLaravelUi,
        layout_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.layout_blade_view_bonus",
        PolicyStage::SelectionLaravelUi,
        layout_blade_view_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.blade_page_view_bonus",
        PolicyStage::SelectionLaravelUi,
        blade_page_view_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.layout_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        layout_blade_component_penalty,
    ),
    ScoreRule::new(
        "selection.laravel.repo_metadata_penalty",
        PolicyStage::SelectionLaravelUi,
        repo_metadata_penalty,
    ),
    ScoreRule::new(
        "selection.laravel.surface.blade_view",
        PolicyStage::SelectionLaravelUi,
        surface_blade_view,
    ),
    ScoreRule::new(
        "selection.laravel.surface.livewire_component",
        PolicyStage::SelectionLaravelUi,
        surface_livewire_component,
    ),
    ScoreRule::new(
        "selection.laravel.surface.livewire_view",
        PolicyStage::SelectionLaravelUi,
        surface_livewire_view,
    ),
    ScoreRule::new(
        "selection.laravel.surface.blade_component",
        PolicyStage::SelectionLaravelUi,
        surface_blade_component,
    ),
    ScoreRule::new(
        "selection.laravel.surface.blade_component_first_bonus",
        PolicyStage::SelectionLaravelUi,
        surface_blade_component_first_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.surface.general_blade_view",
        PolicyStage::SelectionLaravelUi,
        surface_general_blade_view,
    ),
    ScoreRule::new(
        "selection.laravel.surface.first_blade_view_bonus",
        PolicyStage::SelectionLaravelUi,
        surface_first_blade_view_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.surface.general_livewire_component",
        PolicyStage::SelectionLaravelUi,
        surface_general_livewire_component,
    ),
    ScoreRule::new(
        "selection.laravel.surface.general_livewire_view",
        PolicyStage::SelectionLaravelUi,
        surface_general_livewire_view,
    ),
    ScoreRule::new(
        "selection.laravel.surface.first_livewire_view_bonus",
        PolicyStage::SelectionLaravelUi,
        surface_first_livewire_view_bonus,
    ),
    ScoreRule::new(
        "selection.laravel.surface.general_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        surface_general_blade_component_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.wants_laravel_ui_witnesses {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
