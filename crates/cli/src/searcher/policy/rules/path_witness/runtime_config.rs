use super::*;

pub(super) fn runtime_config_artifact_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(3.2))
}

pub(super) fn runtime_config_repo_root_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(5.0))
}

pub(super) fn entrypoint_repo_root_runtime_config_bonus(
    _ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(12.0))
}

pub(super) fn workspace_rust_config_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(3.6))
}

pub(super) fn runtime_config_entrypoint_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(6.0))
}

pub(super) fn runtime_config_server_cli_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(4.2))
}

pub(super) fn runtime_config_main_penalty(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-2.2))
}

pub(super) fn runtime_config_typescript_index_bonus_group(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        4.0
    } else {
        4.8
    }))
}

pub(super) fn entrypoint_config_artifact_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        3.6
    } else {
        4.2
    }))
}

pub(super) fn entrypoint_typescript_index_bonus_group(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        4.0
    } else {
        4.6
    }))
}

pub(super) fn workspace_python_config_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_python_workspace_config {
        3.0
    } else {
        0.2
    }))
}

pub(super) fn workspace_python_test_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_python_witnesses {
        3.4
    } else {
        0.4
    }))
}

pub(super) fn runtime_config_package_surface_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        2.8
    } else {
        3.6
    }))
}

pub(super) fn runtime_config_build_surface_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        3.4
    } else {
        4.0
    }))
}

pub(super) fn runtime_config_workspace_surface_bonus(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        4.4
    } else {
        5.2
    }))
}

pub(super) fn entrypoint_package_surface_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        2.4
    } else {
        3.0
    }))
}

pub(super) fn entrypoint_build_surface_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        3.0
    } else {
        3.8
    }))
}

pub(super) fn entrypoint_workspace_surface_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        3.8
    } else {
        4.6
    }))
}

pub(super) fn runtime_adjacent_python_test_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    let delta = if ctx.specific_path_overlap > 0 {
        3.2
    } else if ctx.path_overlap > 0 || ctx.wants_entrypoint_build_flow {
        2.6
    } else {
        2.0
    };

    Some(PolicyEffect::Add(delta))
}
