use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn artifact_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_runtime_config_artifact).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 0.42 } else { 0.22 }),
    )
}

fn server_or_cli_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_entrypoint_runtime
        && ctx.path_stem_is_server_or_cli)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            1.60
        } else {
            0.80
        }))
}

fn main_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_entrypoint_runtime && ctx.path_stem_is_main)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.84
        } else {
            -0.42
        }))
}

fn main_without_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_entrypoint_runtime
        && ctx.path_stem_is_main
        && ctx.seen_repo_root_runtime_configs > 0
        && ctx.runtime_seen == 0)
        .then_some(PolicyEffect::Add(-1.48))
}

fn repo_root_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_entrypoint_runtime
        && ctx.seen_repo_root_runtime_configs > 0
        && ctx.runtime_seen == 0)
        .then_some(PolicyEffect::Add(if ctx.path_stem_is_server_or_cli {
            0.96
        } else {
            0.44
        }))
}

fn path_witness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(
        if ctx.seen_repo_root_runtime_configs > 0 && ctx.runtime_seen == 0 {
            if ctx.path_stem_is_server_or_cli {
                2.20
            } else {
                1.10
            }
        } else if ctx.seen_count == 0 {
            0.44
        } else {
            0.22
        },
    ))
}

fn typescript_index_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_typescript_runtime_module_index).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { 1.10 } else { 0.54 }),
    )
}

fn typescript_index_repo_root_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_typescript_runtime_module_index
        && ctx.seen_repo_root_runtime_configs > 0
        && ctx.runtime_seen == 0)
        .then_some(PolicyEffect::Add(0.36))
}

fn typescript_index_path_witness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(
        if ctx.seen_repo_root_runtime_configs > 0 && ctx.runtime_seen == 0 {
            1.34
        } else if ctx.seen_count == 0 {
            0.30
        } else {
            0.14
        },
    ))
}

fn repo_root_artifact_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_root_runtime_config_artifact).then_some(
        PolicyEffect::Add(if ctx.seen_repo_root_runtime_configs == 0 {
            2.40
        } else {
            0.18
        }),
    )
}

fn repo_root_artifact_path_witness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_repo_root_runtime_config_artifact
        && ctx.has_path_witness_source)
        .then_some(PolicyEffect::Add(
            if ctx.seen_repo_root_runtime_configs == 0 {
                2.60
            } else {
                0.80
            },
        ))
}

fn rust_workspace_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.wants_rust_workspace_config
        && ctx.is_rust_workspace_config)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.72
        } else {
            0.34
        }))
}

fn python_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.wants_python_workspace_config {
        if ctx.seen_count == 0 { 0.24 } else { 0.12 }
    } else if ctx.seen_count == 0 {
        -0.34
    } else {
        -0.18
    }))
}

fn exact_runtime_config_match_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_runtime_config_artifact
        && ctx.has_exact_query_term_match)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.68
        } else {
            0.34
        }))
}

fn doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Add(if ctx.path_overlap == 0 {
        -0.40
    } else {
        -0.18
    }))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_metadata && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.32
        } else {
            -0.18
        }))
}

fn ci_penalty_without_runtime(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.seen_repo_root_runtime_configs > 0
        && ctx.runtime_seen == 0
        && ctx.is_ci_workflow)
        .then_some(PolicyEffect::Add(-1.24))
}

fn tests_specs_penalty_without_runtime(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.seen_repo_root_runtime_configs > 0
        && ctx.runtime_seen == 0
        && matches!(
            ctx.class,
            HybridSourceClass::Tests | HybridSourceClass::Specs
        ))
    .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
        -1.34
    } else {
        -(0.88 + (0.28 * ctx.seen_count as f32))
    }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.runtime_config.artifact_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_runtime_config_artifact_leaf(),
        ]),
        artifact_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.server_or_cli_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_runtime_leaf(),
            pred::path_stem_is_server_or_cli_leaf(),
        ]),
        server_or_cli_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.main_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_runtime_leaf(),
            pred::path_stem_is_main_leaf(),
        ]),
        main_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.main_without_runtime_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_runtime_leaf(),
            pred::path_stem_is_main_leaf(),
            pred::has_seen_repo_root_runtime_config_leaf(),
            pred::runtime_seen_is_zero_leaf(),
        ]),
        main_without_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.repo_root_runtime_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_runtime_leaf(),
            pred::has_seen_repo_root_runtime_config_leaf(),
            pred::runtime_seen_is_zero_leaf(),
        ]),
        repo_root_runtime_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.path_witness_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_runtime_leaf(),
            pred::has_path_witness_source_leaf(),
        ]),
        path_witness_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.typescript_index_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        typescript_index_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.typescript_index_repo_root_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
            pred::has_seen_repo_root_runtime_config_leaf(),
            pred::runtime_seen_is_zero_leaf(),
        ]),
        typescript_index_repo_root_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.typescript_index_path_witness_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
            pred::has_path_witness_source_leaf(),
        ]),
        typescript_index_path_witness_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.repo_root_artifact_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        repo_root_artifact_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.repo_root_artifact_path_witness_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
            pred::has_path_witness_source_leaf(),
        ]),
        repo_root_artifact_path_witness_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.rust_workspace_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::wants_rust_workspace_config_leaf(),
            pred::is_rust_workspace_config_leaf(),
        ]),
        rust_workspace_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.python_adjustment",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_python_runtime_config_leaf(),
        ]),
        python_adjustment,
    ),
    ScoreRule::when(
        "selection.runtime_config.exact_runtime_config_match_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_runtime_config_artifact_leaf(),
            pred::has_exact_query_term_match_leaf(),
        ]),
        exact_runtime_config_match_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.doc_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::new(
            &[pred::wants_runtime_config_artifacts_leaf()],
            &[
                pred::class_is_documentation_leaf(),
                pred::class_is_readme_leaf(),
            ],
            &[],
        ),
        doc_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.repo_metadata_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        repo_metadata_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.ci_penalty_without_runtime",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::has_seen_repo_root_runtime_config_leaf(),
            pred::runtime_seen_is_zero_leaf(),
            pred::is_ci_workflow_leaf(),
        ]),
        ci_penalty_without_runtime,
    ),
    ScoreRule::when(
        "selection.runtime_config.tests_specs_penalty_without_runtime",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::new(
            &[
                pred::wants_runtime_config_artifacts_leaf(),
                pred::has_seen_repo_root_runtime_config_leaf(),
                pred::runtime_seen_is_zero_leaf(),
            ],
            &[pred::class_is_tests_leaf(), pred::class_is_specs_leaf()],
            &[],
        ),
        tests_specs_penalty_without_runtime,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
