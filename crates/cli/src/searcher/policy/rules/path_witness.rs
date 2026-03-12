use super::super::dsl::{GateRule, ScoreRule, any_gate_matches, apply_score_rules};
use super::super::facts::PathWitnessFacts;
use super::super::kernel::PolicyProgram;
use super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn gate_path_overlap(ctx: &PathWitnessFacts) -> bool {
    ctx.path_overlap > 0
}

fn gate_entrypoint(ctx: &PathWitnessFacts) -> bool {
    ctx.is_entrypoint
}

fn gate_entrypoint_build_workflow(ctx: &PathWitnessFacts) -> bool {
    ctx.is_entrypoint_build_workflow
}

fn gate_ci_workflow(ctx: &PathWitnessFacts) -> bool {
    ctx.is_ci_workflow
}

fn gate_entrypoint_config_artifact(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_entrypoint_build_flow && ctx.is_config_artifact
}

fn gate_runtime_config_artifact(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_runtime_config_artifacts && ctx.is_config_artifact
}

fn gate_typescript_runtime_index(ctx: &PathWitnessFacts) -> bool {
    (ctx.wants_entrypoint_build_flow || ctx.wants_runtime_config_artifacts)
        && ctx.is_typescript_runtime_module_index
}

fn gate_python_workspace_config(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_python_workspace_config && ctx.is_python_config
}

fn gate_python_witness(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_python_witnesses && ctx.is_python_test
}

fn gate_test_support(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_test_witness_recall && (ctx.is_test_support || ctx.is_python_test)
}

fn gate_runtime_anchor_test_support(ctx: &PathWitnessFacts) -> bool {
    ctx.is_runtime_anchor_test_support
        && (ctx.wants_entrypoint_build_flow || ctx.wants_runtime_config_artifacts)
}

fn gate_examples(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_examples && ctx.is_example_support
}

fn gate_benchmarks(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_benchmarks && ctx.is_bench_support
}

fn gate_cli_test(ctx: &PathWitnessFacts) -> bool {
    ctx.query_mentions_cli && ctx.is_cli_test
}

fn gate_laravel_ui_harness(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_laravel_ui_witnesses && ctx.is_test_harness
}

fn gate_scripts_ops(ctx: &PathWitnessFacts) -> bool {
    ctx.is_scripts_ops
}

const GATE_RULES: &[GateRule<PathWitnessFacts>] = &[
    GateRule::new("path_overlap", gate_path_overlap),
    GateRule::new("entrypoint", gate_entrypoint),
    GateRule::new("entrypoint_build_workflow", gate_entrypoint_build_workflow),
    GateRule::new("ci_workflow", gate_ci_workflow),
    GateRule::new(
        "entrypoint_config_artifact",
        gate_entrypoint_config_artifact,
    ),
    GateRule::new("runtime_config_artifact", gate_runtime_config_artifact),
    GateRule::new("typescript_runtime_index", gate_typescript_runtime_index),
    GateRule::new("python_workspace_config", gate_python_workspace_config),
    GateRule::new("python_witness", gate_python_witness),
    GateRule::new("test_support", gate_test_support),
    GateRule::new(
        "runtime_anchor_test_support",
        gate_runtime_anchor_test_support,
    ),
    GateRule::new("examples", gate_examples),
    GateRule::new("benchmarks", gate_benchmarks),
    GateRule::new("cli_test", gate_cli_test),
    GateRule::new("laravel_ui_harness", gate_laravel_ui_harness),
    GateRule::new("scripts_ops", gate_scripts_ops),
];

fn path_witness_entrypoint_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_entrypoint.then_some(PolicyEffect::Add(4.0))
}

fn path_witness_build_flow_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.is_entrypoint && ctx.wants_entrypoint_build_flow).then_some(PolicyEffect::Add(3.2))
}

fn path_witness_workflow_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_entrypoint_build_workflow
        .then_some(PolicyEffect::Add(if ctx.path_overlap == 0 {
            10.4
        } else {
            7.2
        }))
}

fn path_witness_ci_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_ci_workflow.then_some(PolicyEffect::Add(6.2))
}

fn laravel_livewire_view_focus_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_livewire_view_witnesses && ctx.is_laravel_livewire_view)
        .then_some(PolicyEffect::Add(2.8))
}

fn laravel_non_livewire_view_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_livewire_view_witnesses && ctx.is_laravel_non_livewire_blade_view)
        .then_some(PolicyEffect::Add(-1.1))
}

fn laravel_command_middleware_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_commands_middleware_witnesses && ctx.is_laravel_command_or_middleware)
        .then_some(PolicyEffect::Add(4.2))
}

fn laravel_job_listener_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_jobs_listeners_witnesses && ctx.is_laravel_job_or_listener)
        .then_some(PolicyEffect::Add(3.4))
}

fn entrypoint_laravel_route_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_laravel_route).then_some(PolicyEffect::Add(8.2))
}

fn entrypoint_laravel_bootstrap_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_laravel_bootstrap_entrypoint)
        .then_some(PolicyEffect::Add(10.5))
}

fn entrypoint_laravel_core_provider_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_laravel_core_provider)
        .then_some(PolicyEffect::Add(3.0))
}

fn entrypoint_laravel_provider_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && !ctx.is_laravel_core_provider && ctx.is_laravel_provider)
        .then_some(PolicyEffect::Add(1.0))
}

fn runtime_config_artifact_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_config_artifact).then_some(PolicyEffect::Add(3.2))
}

fn runtime_config_repo_root_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_root_runtime_config_artifact)
        .then_some(PolicyEffect::Add(5.0))
}

fn entrypoint_repo_root_runtime_config_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_repo_root_runtime_config_artifact)
        .then_some(PolicyEffect::Add(12.0))
}

fn workspace_rust_config_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_rust_workspace_config && ctx.is_rust_workspace_config)
        .then_some(PolicyEffect::Add(3.6))
}

fn examples_support_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    if !ctx.wants_examples || !ctx.is_example_support {
        return None;
    }

    let delta = if ctx.specific_path_overlap >= 2 {
        5.8
    } else if ctx.specific_path_overlap == 1 {
        4.2
    } else if ctx.has_exact_query_term_match {
        3.4
    } else {
        1.8
    };

    Some(PolicyEffect::Add(delta))
}

fn benchmarks_support_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    if !ctx.wants_benchmarks || !ctx.is_bench_support {
        return None;
    }

    let delta = if ctx.specific_path_overlap >= 2 {
        6.4
    } else if ctx.specific_path_overlap == 1 {
        4.8
    } else if ctx.has_exact_query_term_match {
        3.8
    } else {
        2.0
    };

    Some(PolicyEffect::Add(delta))
}

fn laravel_ui_harness_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_test_harness).then_some(PolicyEffect::Add(2.2))
}

fn scripts_ops_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_scripts_ops.then_some(PolicyEffect::Add(4.2))
}

fn tests_exact_query_match_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall
        && ctx.has_exact_query_term_match
        && !(ctx.wants_example_or_bench_witnesses && ctx.is_examples_rs))
        .then_some(PolicyEffect::Add(5.6))
}

fn scripts_exact_query_match_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.has_exact_query_term_match && ctx.is_scripts_ops).then_some(PolicyEffect::Add(2.8))
}

fn runtime_config_test_support_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_test_support
        && !ctx.is_config_artifact
        && !ctx.is_runtime_anchor_test_support)
        .then_some(PolicyEffect::Add(-3.2))
}

fn examples_unwanted_example_support_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (!ctx.wants_examples
        && ctx.is_example_support
        && ctx.path_overlap == 0
        && ctx.specific_path_overlap == 0
        && !ctx.has_exact_query_term_match)
        .then_some(PolicyEffect::Add(-3.8))
}

fn cli_test_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.query_mentions_cli && ctx.is_cli_test).then_some(PolicyEffect::Add(3.8))
}

fn source_runtime_support_tests_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    matches!(
        ctx.source_class,
        HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
    )
    .then_some(PolicyEffect::Add(0.4))
}

fn source_frontend_noise_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_frontend_runtime_noise
        .then_some(PolicyEffect::Add(-4.0))
}

fn path_witness_specific_overlap_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    if ctx.specific_path_overlap == 0 {
        return None;
    }

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

fn laravel_blade_view_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_laravel_non_livewire_blade_view).then_some(
        PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
            3.6
        } else {
            7.0
        }),
    )
}

fn laravel_top_level_blade_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_laravel_form_action_witnesses
        && !ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_top_level_blade_view)
        .then_some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
            4.4
        } else {
            2.6
        }))
}

fn laravel_top_level_blade_specific_overlap_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_laravel_form_action_witnesses
        && !ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_top_level_blade_view
        && ctx.specific_path_overlap > 0)
        .then_some(PolicyEffect::Add(1.4 * ctx.specific_path_overlap as f32))
}

fn laravel_partial_view_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && !ctx.wants_laravel_form_action_witnesses
        && !ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_partial_view
        && ctx.is_laravel_non_livewire_blade_view)
        .then_some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
            -2.4
        } else {
            -1.2
        }))
}

fn laravel_form_action_blade_component_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.is_laravel_blade_component
        && ctx.wants_laravel_form_action_witnesses
        && ctx.is_laravel_form_action_blade)
        .then_some(PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
            5.2
        } else {
            3.8
        }))
}

fn laravel_blade_component_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_laravel_blade_component).then_some(PolicyEffect::Add(
        if ctx.wants_blade_component_witnesses {
            if ctx.is_laravel_nested_blade_component {
                2.0
            } else {
                7.4
            }
        } else if ctx.path_overlap >= 3 {
            2.8
        } else {
            0.8
        },
    ))
}

fn laravel_form_action_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (!ctx.is_laravel_blade_component
        && ctx.wants_laravel_form_action_witnesses
        && ctx.is_laravel_form_action_blade)
        .then_some(PolicyEffect::Add(4.8))
}

fn laravel_livewire_component_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_laravel_livewire_component).then_some(
        PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
            -0.2
        } else {
            1.8
        }),
    )
}

fn laravel_view_component_class_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_laravel_view_component_class).then_some(
        PolicyEffect::Add(if ctx.wants_laravel_layout_witnesses {
            -4.4
        } else {
            -2.8
        }),
    )
}

fn laravel_layout_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_layout_witnesses && ctx.is_laravel_layout_blade_view).then_some(
        PolicyEffect::Add(if ctx.wants_blade_component_witnesses {
            4.2
        } else {
            6.4
        }),
    )
}

fn laravel_missing_specific_anchor_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.has_specific_query_terms
        && ctx.specific_path_overlap == 0
        && (ctx.is_laravel_non_livewire_blade_view || ctx.is_laravel_livewire_view))
        .then_some(PolicyEffect::Add(if ctx.is_laravel_layout_blade_view {
            -1.0
        } else {
            -1.4
        }))
}

fn runtime_config_entrypoint_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_entrypoint).then_some(PolicyEffect::Add(6.0))
}

fn runtime_config_server_cli_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_entrypoint && ctx.path_stem_is_server_or_cli)
        .then_some(PolicyEffect::Add(4.2))
}

fn runtime_config_main_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_entrypoint && ctx.path_stem_is_main)
        .then_some(PolicyEffect::Add(-2.2))
}

fn runtime_config_typescript_index_bonus_group(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_typescript_runtime_module_index).then_some(
        PolicyEffect::Add(if ctx.path_overlap == 0 { 4.0 } else { 4.8 }),
    )
}

fn entrypoint_config_artifact_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_config_artifact).then_some(PolicyEffect::Add(
        if ctx.path_overlap == 0 { 3.6 } else { 4.2 },
    ))
}

fn entrypoint_typescript_index_bonus_group(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_typescript_runtime_module_index).then_some(
        PolicyEffect::Add(if ctx.path_overlap == 0 { 4.0 } else { 4.6 }),
    )
}

fn workspace_python_config_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_python_config
        .then_some(PolicyEffect::Add(if ctx.wants_python_workspace_config {
            3.0
        } else {
            0.2
        }))
}

fn workspace_python_test_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    ctx.is_python_test
        .then_some(PolicyEffect::Add(if ctx.wants_python_witnesses {
            3.4
        } else {
            0.4
        }))
}

fn runtime_adjacent_python_test_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    if !ctx.is_runtime_adjacent_python_test
        || !(ctx.wants_entrypoint_build_flow
            || ctx.wants_runtime_config_artifacts
            || ctx.wants_test_witness_recall)
    {
        return None;
    }

    let delta = if ctx.specific_path_overlap > 0 {
        3.2
    } else if ctx.path_overlap > 0 || ctx.wants_entrypoint_build_flow {
        2.6
    } else {
        2.0
    };

    Some(PolicyEffect::Add(delta))
}

fn tests_support_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_test_witness_recall && ctx.is_test_support).then_some(PolicyEffect::Add(2.6))
}

fn runtime_anchor_test_support_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    if !ctx.is_runtime_anchor_test_support {
        return None;
    }

    if ctx.wants_entrypoint_build_flow {
        Some(PolicyEffect::Add(4.4))
    } else if ctx.wants_runtime_config_artifacts {
        Some(PolicyEffect::Add(3.6))
    } else {
        None
    }
}

fn tests_support_path_overlap_bonus(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    if !ctx.wants_test_witness_recall
        || !ctx.is_test_support
        || ctx.is_example_support
        || ctx.is_bench_support
    {
        return None;
    }

    let delta = match ctx.path_overlap {
        0 | 1 => 0.0,
        2 => 1.2,
        _ => 5.4,
    };
    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn examples_or_bench_non_support_test_penalty(ctx: &PathWitnessFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses
        && ctx.is_test_support
        && !ctx.is_python_test
        && !ctx.is_example_support
        && !ctx.is_bench_support)
        .then_some(PolicyEffect::Add(if ctx.wants_test_witness_recall {
            -1.4
        } else {
            -3.0
        }))
}

const SCORE_RULES: &[ScoreRule<PathWitnessFacts>] = &[
    ScoreRule::new(
        "path_witness.specific_overlap_bonus",
        PolicyStage::PathWitness,
        path_witness_specific_overlap_bonus,
    ),
    ScoreRule::new(
        "path_witness.entrypoint_bonus",
        PolicyStage::PathWitness,
        path_witness_entrypoint_bonus,
    ),
    ScoreRule::new(
        "path_witness.build_flow_bonus",
        PolicyStage::PathWitness,
        path_witness_build_flow_bonus,
    ),
    ScoreRule::new(
        "path_witness.workflow_bonus",
        PolicyStage::PathWitness,
        path_witness_workflow_bonus,
    ),
    ScoreRule::new(
        "path_witness.ci_bonus",
        PolicyStage::PathWitness,
        path_witness_ci_bonus,
    ),
    ScoreRule::new(
        "laravel.livewire_view_focus_bonus",
        PolicyStage::PathWitness,
        laravel_livewire_view_focus_bonus,
    ),
    ScoreRule::new(
        "laravel.non_livewire_view_penalty",
        PolicyStage::PathWitness,
        laravel_non_livewire_view_penalty,
    ),
    ScoreRule::new(
        "laravel.command_middleware_bonus",
        PolicyStage::PathWitness,
        laravel_command_middleware_bonus,
    ),
    ScoreRule::new(
        "laravel.job_listener_bonus",
        PolicyStage::PathWitness,
        laravel_job_listener_bonus,
    ),
    ScoreRule::new(
        "entrypoint.laravel_route_bonus",
        PolicyStage::PathWitness,
        entrypoint_laravel_route_bonus,
    ),
    ScoreRule::new(
        "entrypoint.laravel_bootstrap_bonus",
        PolicyStage::PathWitness,
        entrypoint_laravel_bootstrap_bonus,
    ),
    ScoreRule::new(
        "entrypoint.laravel_core_provider_bonus",
        PolicyStage::PathWitness,
        entrypoint_laravel_core_provider_bonus,
    ),
    ScoreRule::new(
        "entrypoint.laravel_provider_bonus",
        PolicyStage::PathWitness,
        entrypoint_laravel_provider_bonus,
    ),
    ScoreRule::new(
        "runtime_config.artifact_bonus",
        PolicyStage::PathWitness,
        runtime_config_artifact_bonus,
    ),
    ScoreRule::new(
        "runtime_config.repo_root_bonus",
        PolicyStage::PathWitness,
        runtime_config_repo_root_bonus,
    ),
    ScoreRule::new(
        "entrypoint.repo_root_runtime_config_bonus",
        PolicyStage::PathWitness,
        entrypoint_repo_root_runtime_config_bonus,
    ),
    ScoreRule::new(
        "workspace.rust_config_bonus",
        PolicyStage::PathWitness,
        workspace_rust_config_bonus,
    ),
    ScoreRule::new(
        "examples.support_bonus",
        PolicyStage::PathWitness,
        examples_support_bonus,
    ),
    ScoreRule::new(
        "benchmarks.support_bonus",
        PolicyStage::PathWitness,
        benchmarks_support_bonus,
    ),
    ScoreRule::new(
        "laravel.ui_harness_bonus",
        PolicyStage::PathWitness,
        laravel_ui_harness_bonus,
    ),
    ScoreRule::new(
        "scripts.ops_bonus",
        PolicyStage::PathWitness,
        scripts_ops_bonus,
    ),
    ScoreRule::new(
        "tests.exact_query_match_bonus",
        PolicyStage::PathWitness,
        tests_exact_query_match_bonus,
    ),
    ScoreRule::new(
        "scripts.exact_query_match_bonus",
        PolicyStage::PathWitness,
        scripts_exact_query_match_bonus,
    ),
    ScoreRule::new(
        "runtime_config.test_support_penalty",
        PolicyStage::PathWitness,
        runtime_config_test_support_penalty,
    ),
    ScoreRule::new(
        "examples.unwanted_example_support_penalty",
        PolicyStage::PathWitness,
        examples_unwanted_example_support_penalty,
    ),
    ScoreRule::new("cli.test_bonus", PolicyStage::PathWitness, cli_test_bonus),
    ScoreRule::new(
        "source.runtime_support_tests_bonus",
        PolicyStage::PathWitness,
        source_runtime_support_tests_bonus,
    ),
    ScoreRule::new(
        "source.frontend_noise_penalty",
        PolicyStage::PathWitness,
        source_frontend_noise_penalty,
    ),
    ScoreRule::new(
        "laravel.blade_view_bonus",
        PolicyStage::PathWitness,
        laravel_blade_view_bonus,
    ),
    ScoreRule::new(
        "laravel.top_level_blade_bonus",
        PolicyStage::PathWitness,
        laravel_top_level_blade_bonus,
    ),
    ScoreRule::new(
        "laravel.top_level_blade_specific_overlap_bonus",
        PolicyStage::PathWitness,
        laravel_top_level_blade_specific_overlap_bonus,
    ),
    ScoreRule::new(
        "laravel.partial_view_penalty",
        PolicyStage::PathWitness,
        laravel_partial_view_penalty,
    ),
    ScoreRule::new(
        "laravel.form_action_blade_component_bonus",
        PolicyStage::PathWitness,
        laravel_form_action_blade_component_bonus,
    ),
    ScoreRule::new(
        "laravel.blade_component_bonus",
        PolicyStage::PathWitness,
        laravel_blade_component_bonus,
    ),
    ScoreRule::new(
        "laravel.form_action_bonus",
        PolicyStage::PathWitness,
        laravel_form_action_bonus,
    ),
    ScoreRule::new(
        "laravel.livewire_component_bonus",
        PolicyStage::PathWitness,
        laravel_livewire_component_bonus,
    ),
    ScoreRule::new(
        "laravel.view_component_class_penalty",
        PolicyStage::PathWitness,
        laravel_view_component_class_penalty,
    ),
    ScoreRule::new(
        "laravel.layout_bonus",
        PolicyStage::PathWitness,
        laravel_layout_bonus,
    ),
    ScoreRule::new(
        "laravel.missing_specific_anchor_penalty",
        PolicyStage::PathWitness,
        laravel_missing_specific_anchor_penalty,
    ),
    ScoreRule::new(
        "runtime_config.entrypoint_bonus",
        PolicyStage::PathWitness,
        runtime_config_entrypoint_bonus,
    ),
    ScoreRule::new(
        "runtime_config.server_cli_bonus",
        PolicyStage::PathWitness,
        runtime_config_server_cli_bonus,
    ),
    ScoreRule::new(
        "runtime_config.main_penalty",
        PolicyStage::PathWitness,
        runtime_config_main_penalty,
    ),
    ScoreRule::new(
        "runtime_config.typescript_index_bonus",
        PolicyStage::PathWitness,
        runtime_config_typescript_index_bonus_group,
    ),
    ScoreRule::new(
        "entrypoint.config_artifact_bonus",
        PolicyStage::PathWitness,
        entrypoint_config_artifact_bonus,
    ),
    ScoreRule::new(
        "entrypoint.typescript_index_bonus",
        PolicyStage::PathWitness,
        entrypoint_typescript_index_bonus_group,
    ),
    ScoreRule::new(
        "workspace.python_config_bonus",
        PolicyStage::PathWitness,
        workspace_python_config_bonus,
    ),
    ScoreRule::new(
        "workspace.python_test_bonus",
        PolicyStage::PathWitness,
        workspace_python_test_bonus,
    ),
    ScoreRule::new(
        "tests.runtime_adjacent_python_bonus",
        PolicyStage::PathWitness,
        runtime_adjacent_python_test_bonus,
    ),
    ScoreRule::new(
        "tests.support_bonus",
        PolicyStage::PathWitness,
        tests_support_bonus,
    ),
    ScoreRule::new(
        "tests.runtime_anchor_support_bonus",
        PolicyStage::PathWitness,
        runtime_anchor_test_support_bonus,
    ),
    ScoreRule::new(
        "tests.support_path_overlap_bonus",
        PolicyStage::PathWitness,
        tests_support_path_overlap_bonus,
    ),
    ScoreRule::new(
        "examples_or_bench.non_support_test_penalty",
        PolicyStage::PathWitness,
        examples_or_bench_non_support_test_penalty,
    ),
];
pub(crate) fn evaluate(
    ctx: &PathWitnessFacts,
    trace: bool,
) -> Option<super::super::trace::PolicyEvaluation> {
    if !any_gate_matches(ctx, GATE_RULES) {
        return None;
    }

    let mut program = PolicyProgram::with_optional_trace(ctx.path_overlap as f32, trace);
    apply_score_rules(&mut program, ctx, SCORE_RULES);
    let evaluation = program.finish();
    (evaluation.score > 0.0).then_some(evaluation)
}

pub(crate) fn score(ctx: &PathWitnessFacts) -> Option<f32> {
    evaluate(ctx, false).map(|evaluation| evaluation.score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::surfaces::HybridSourceClass;

    fn trace_rule_ids(
        evaluation: &super::super::super::trace::PolicyEvaluation,
    ) -> Vec<&'static str> {
        evaluation
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .map(|rule| rule.rule_id)
            .collect()
    }

    #[test]
    fn policy_trace_path_witness_entrypoint_typescript_runtime_config_stack() {
        let ctx = PathWitnessFacts {
            path_overlap: 1,
            specific_path_overlap: 1,
            is_entrypoint: true,
            is_typescript_runtime_module_index: true,
            wants_entrypoint_build_flow: true,
            wants_runtime_config_artifacts: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"path_witness.specific_overlap_bonus"));
        assert!(rule_ids.contains(&"path_witness.entrypoint_bonus"));
        assert!(rule_ids.contains(&"path_witness.build_flow_bonus"));
        assert!(rule_ids.contains(&"runtime_config.typescript_index_bonus"));
        assert!(rule_ids.contains(&"entrypoint.typescript_index_bonus"));
    }

    #[test]
    fn policy_trace_path_witness_laravel_blade_component_focus_and_missing_anchor_penalty() {
        let ctx = PathWitnessFacts {
            path_overlap: 1,
            wants_laravel_ui_witnesses: true,
            wants_blade_component_witnesses: true,
            has_specific_query_terms: true,
            is_laravel_non_livewire_blade_view: true,
            is_laravel_blade_component: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"laravel.blade_view_bonus"));
        assert!(rule_ids.contains(&"laravel.blade_component_bonus"));
        assert!(rule_ids.contains(&"laravel.missing_specific_anchor_penalty"));
    }

    #[test]
    fn policy_trace_path_witness_test_support_bonus_and_example_crowding_penalty() {
        let ctx = PathWitnessFacts {
            path_overlap: 3,
            source_class: HybridSourceClass::Tests,
            wants_test_witness_recall: true,
            wants_example_or_bench_witnesses: true,
            has_exact_query_term_match: true,
            is_test_support: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"tests.exact_query_match_bonus"));
        assert!(rule_ids.contains(&"tests.support_bonus"));
        assert!(rule_ids.contains(&"tests.support_path_overlap_bonus"));
        assert!(rule_ids.contains(&"examples_or_bench.non_support_test_penalty"));
    }

    #[test]
    fn policy_trace_path_witness_mixed_examples_query_skips_examples_rs_exact_bonus() {
        let ctx = PathWitnessFacts {
            path_overlap: 1,
            wants_test_witness_recall: true,
            wants_example_or_bench_witnesses: true,
            has_exact_query_term_match: true,
            is_test_support: true,
            is_examples_rs: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(!rule_ids.contains(&"tests.exact_query_match_bonus"));
    }

    #[test]
    fn policy_trace_path_witness_entrypoint_runtime_anchor_test_bonus() {
        let ctx = PathWitnessFacts {
            wants_entrypoint_build_flow: true,
            is_test_support: true,
            is_runtime_anchor_test_support: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"tests.runtime_anchor_support_bonus"));
        assert!(!rule_ids.contains(&"runtime_config.test_support_penalty"));
    }

    #[test]
    fn policy_trace_path_witness_runtime_config_runtime_anchor_test_skips_generic_penalty() {
        let ctx = PathWitnessFacts {
            wants_runtime_config_artifacts: true,
            is_test_support: true,
            is_runtime_anchor_test_support: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"tests.runtime_anchor_support_bonus"));
        assert!(!rule_ids.contains(&"runtime_config.test_support_penalty"));
    }
}
