use super::*;

pub(super) fn laravel_livewire_view_focus_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(2.8))
}

pub(super) fn laravel_non_livewire_view_penalty(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-1.1))
}

pub(super) fn laravel_command_middleware_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(4.2))
}

pub(super) fn laravel_job_listener_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(3.4))
}

pub(super) fn entrypoint_laravel_route_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(8.2))
}

pub(super) fn entrypoint_laravel_bootstrap_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(10.5))
}

pub(super) fn entrypoint_laravel_core_provider_bonus(
    _ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(3.0))
}

pub(super) fn entrypoint_laravel_provider_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(1.0))
}

pub(super) fn laravel_ui_harness_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(2.2))
}

pub(super) fn laravel_blade_view_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
        3.6
    } else {
        7.0
    }))
}

pub(super) fn laravel_top_level_blade_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
        4.4
    } else {
        2.6
    }))
}

pub(super) fn laravel_top_level_blade_specific_overlap_bonus(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(1.4 * ctx.specific_path_overlap as f32))
}

pub(super) fn laravel_partial_view_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
        -2.4
    } else {
        -1.2
    }))
}

pub(super) fn laravel_form_action_blade_component_bonus(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
        5.2
    } else {
        3.8
    }))
}

pub(super) fn laravel_blade_component_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
        if ctx.is_laravel_nested_blade_component {
            2.0
        } else {
            7.4
        }
    } else if ctx.path_overlap >= 3 {
        2.8
    } else {
        0.8
    }))
}

pub(super) fn laravel_form_action_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(4.8))
}

pub(super) fn laravel_livewire_component_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
        -0.2
    } else {
        1.8
    }))
}

pub(super) fn laravel_view_component_class_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_laravel_layout_witnesses {
        -4.4
    } else {
        -2.8
    }))
}

pub(super) fn laravel_layout_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
        4.2
    } else {
        6.4
    }))
}

pub(super) fn laravel_missing_specific_anchor_penalty(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.is_laravel_layout_blade_view {
        -1.0
    } else {
        -1.4
    }))
}
