use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_runtime).then_some(PolicyEffect::Add(
        if ctx.runtime_seen == 0 {
            1.92
        } else if ctx.seen_count == 0 {
            0.42
        } else {
            0.22
        },
    ))
}

fn workflow_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_build_workflow).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 2.20 } else { 1.20 }),
    )
}

fn workflow_without_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_build_workflow && ctx.runtime_seen == 0)
        .then_some(PolicyEffect::Add(-1.26))
}

fn laravel_core_provider_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_laravel_core_provider).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 0.96 } else { 0.54 },
    ))
}

fn laravel_provider_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && !ctx.is_laravel_core_provider && ctx.is_laravel_provider)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.18
        } else {
            0.10
        }))
}

fn laravel_route_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_laravel_route).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 1.90 } else { 1.10 },
    ))
}

fn laravel_bootstrap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_laravel_bootstrap_entrypoint).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 1.56 } else { 0.88 }),
    )
}

fn laravel_bootstrap_specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_laravel_bootstrap_entrypoint
        && ctx.specific_witness_path_overlap > 0)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.92
        } else {
            0.44
        }))
}

fn runtime_config_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_runtime_config_artifact).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 0.34 } else { 0.18 }),
    )
}

fn typescript_index_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_typescript_runtime_module_index).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 0.48 } else { 0.24 }),
    )
}

fn repo_root_runtime_config_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_repo_root_runtime_config_artifact).then_some(
        PolicyEffect::Add(if ctx.seen_repo_root_runtime_configs == 0 {
            7.40
        } else {
            1.20
        }),
    )
}

fn repo_root_runtime_config_after_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_repo_root_runtime_config_artifact
        && ctx.seen_repo_root_runtime_configs == 0
        && ctx.runtime_seen > 0)
        .then_some(PolicyEffect::Add(2.80))
}

fn path_witness_runtime_config_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_runtime_config_artifact
        && ctx.has_path_witness_source)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            2.00
        } else {
            1.00
        }))
}

fn path_witness_repo_root_runtime_config_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_repo_root_runtime_config_artifact
        && ctx.has_path_witness_source)
        .then_some(PolicyEffect::Add(
            if ctx.seen_repo_root_runtime_configs == 0 {
                3.20
            } else {
                1.20
            },
        ))
}

fn path_witness_typescript_index_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_typescript_runtime_module_index
        && ctx.has_path_witness_source)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.24
        } else {
            0.62
        }))
}

fn python_config_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_python_runtime_config).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { 0.32 } else { 0.16 },
    ))
}

fn cli_test_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.query_mentions_cli && ctx.is_cli_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.78
        } else {
            0.42
        }))
}

fn build_flow_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.excerpt_has_build_flow_anchor)
        .then_some(PolicyEffect::Add(0.16))
}

fn test_double_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.excerpt_has_test_double_anchor)
        .then_some(PolicyEffect::Add(-0.24))
}

fn cli_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.query_mentions_cli
        && ctx.class == HybridSourceClass::Runtime
        && !ctx.is_cli_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.28
        } else {
            -0.16
        }))
}

fn non_entry_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.class == HybridSourceClass::Runtime
        && ctx.path_overlap == 0
        && !ctx.is_entrypoint_runtime
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.38
        } else {
            -0.22
        }))
}

fn tests_specs_without_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && matches!(
            ctx.class,
            HybridSourceClass::Tests | HybridSourceClass::Specs
        )
        && ctx.runtime_seen == 0
        && !(ctx.query_mentions_cli && ctx.is_cli_test_support))
        .then_some(PolicyEffect::Add(-0.18))
}

fn reference_doc_without_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_reference_doc && ctx.runtime_seen == 0)
        .then_some(PolicyEffect::Add(-0.14))
}

fn frontend_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_frontend_runtime_noise).then_some(PolicyEffect::Add(
        if ctx.runtime_seen == 0 { -0.22 } else { -0.14 },
    ))
}

fn loose_python_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_loose_python_test_module).then_some(
        PolicyEffect::Add(if ctx.runtime_seen == 0 { -0.18 } else { -0.10 }),
    )
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.entrypoint.runtime_bonus",
        PolicyStage::SelectionEntrypoint,
        runtime_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.workflow_bonus",
        PolicyStage::SelectionEntrypoint,
        workflow_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.workflow_without_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        workflow_without_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.entrypoint.laravel_core_provider_bonus",
        PolicyStage::SelectionEntrypoint,
        laravel_core_provider_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.laravel_provider_bonus",
        PolicyStage::SelectionEntrypoint,
        laravel_provider_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.laravel_route_bonus",
        PolicyStage::SelectionEntrypoint,
        laravel_route_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.laravel_bootstrap_bonus",
        PolicyStage::SelectionEntrypoint,
        laravel_bootstrap_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.laravel_bootstrap_specific_overlap_bonus",
        PolicyStage::SelectionEntrypoint,
        laravel_bootstrap_specific_overlap_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        runtime_config_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.typescript_index_bonus",
        PolicyStage::SelectionEntrypoint,
        typescript_index_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.repo_root_runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        repo_root_runtime_config_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.repo_root_runtime_config_after_runtime_bonus",
        PolicyStage::SelectionEntrypoint,
        repo_root_runtime_config_after_runtime_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.path_witness_runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        path_witness_runtime_config_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.path_witness_repo_root_runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        path_witness_repo_root_runtime_config_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.path_witness_typescript_index_bonus",
        PolicyStage::SelectionEntrypoint,
        path_witness_typescript_index_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.python_config_bonus",
        PolicyStage::SelectionEntrypoint,
        python_config_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.cli_test_support_bonus",
        PolicyStage::SelectionEntrypoint,
        cli_test_support_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.build_flow_anchor_bonus",
        PolicyStage::SelectionEntrypoint,
        build_flow_anchor_bonus,
    ),
    ScoreRule::new(
        "selection.entrypoint.test_double_penalty",
        PolicyStage::SelectionEntrypoint,
        test_double_penalty,
    ),
    ScoreRule::new(
        "selection.entrypoint.cli_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        cli_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.entrypoint.non_entry_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        non_entry_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.entrypoint.tests_specs_without_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        tests_specs_without_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.entrypoint.reference_doc_without_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        reference_doc_without_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.entrypoint.frontend_noise_penalty",
        PolicyStage::SelectionEntrypoint,
        frontend_noise_penalty,
    ),
    ScoreRule::new(
        "selection.entrypoint.loose_python_test_penalty",
        PolicyStage::SelectionEntrypoint,
        loose_python_test_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.wants_entrypoint_build_flow {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
