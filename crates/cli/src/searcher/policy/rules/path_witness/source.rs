use super::*;

pub(super) fn path_witness_specific_overlap_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_entrypoint_build_flow {
        3.0 * ctx.specific_path_overlap as f32
    } else if ctx.wants_test_witness_recall && ctx.is_test_support {
        7.2 * ctx.specific_path_overlap as f32
    } else if ctx.wants_laravel_ui_witnesses {
        2.2 * ctx.specific_path_overlap as f32
    } else {
        1.2 * ctx.specific_path_overlap as f32
    }))
}

pub(super) fn tests_exact_query_match_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(5.6))
}

pub(super) fn scripts_exact_query_match_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(2.8))
}

pub(super) fn runtime_config_test_support_penalty(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-3.2))
}

pub(super) fn runtime_config_test_tree_harness_bonus(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.is_cli_test { 4.8 } else { 3.6 }))
}

pub(super) fn cli_test_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(3.8))
}

pub(super) fn source_runtime_support_tests_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(0.4))
}

pub(super) fn source_frontend_noise_penalty(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-4.0))
}

pub(super) fn scripts_ops_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(4.2))
}

pub(super) fn kotlin_android_ui_runtime_surface_bonus(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    let delta = if ctx.specific_path_overlap >= 2 {
        6.0
    } else if ctx.specific_path_overlap == 1 {
        4.2
    } else if ctx.path_overlap >= 2 {
        2.8
    } else {
        1.4
    };

    Some(PolicyEffect::Add(delta))
}

pub(super) fn runtime_focus_ci_workflow_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    let wants_runtime_focus = ctx.wants_entrypoint_build_flow
        || ctx.wants_runtime_config_artifacts
        || ctx.wants_test_witness_recall;
    (wants_runtime_focus && ctx.is_ci_workflow && !ctx.wants_ci_workflow_witnesses).then_some(
        PolicyEffect::Add(
            if ctx.specific_path_overlap > 0 || ctx.has_exact_query_term_match {
                -3.8
            } else if ctx.path_overlap > 0 {
                -4.6
            } else {
                -5.8
            },
        ),
    )
}

pub(super) fn runtime_focus_example_support_penalty(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    let wants_runtime_focus = ctx.wants_entrypoint_build_flow
        || ctx.wants_runtime_config_artifacts
        || ctx.wants_test_witness_recall;
    (wants_runtime_focus && ctx.is_example_support && !ctx.wants_examples).then_some(
        PolicyEffect::Add(
            if ctx.specific_path_overlap > 0 || ctx.has_exact_query_term_match {
                -1.8
            } else if ctx.path_overlap > 0 {
                -2.8
            } else {
                -3.6
            },
        ),
    )
}

pub(super) fn runtime_anchor_test_support_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    if ctx.wants_entrypoint_build_flow {
        Some(PolicyEffect::Add(4.4))
    } else if ctx.wants_runtime_config_artifacts {
        Some(PolicyEffect::Add(3.6))
    } else {
        None
    }
}

pub(super) fn tests_support_bonus(_ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(2.6))
}

pub(super) fn tests_support_path_overlap_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    let delta = match ctx.path_overlap {
        0 | 1 => 0.0,
        2 => 1.2,
        _ => 5.4,
    };
    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

pub(super) fn examples_or_bench_non_support_test_penalty(
    ctx: &PathWitnessFacts,
) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_test_witness_recall {
        -1.4
    } else {
        -3.0
    }))
}
