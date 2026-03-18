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

fn same_language_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.is_entrypoint_runtime {
        if ctx.seen_count == 0 { 0.36 } else { 0.18 }
    } else if matches!(
        ctx.class,
        HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
    ) {
        if ctx.seen_count == 0 { 0.22 } else { 0.12 }
    } else {
        0.0
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn language_mismatch_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.is_entrypoint_runtime {
        if ctx.seen_count == 0 { -0.34 } else { -0.18 }
    } else if matches!(
        ctx.class,
        HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
    ) {
        if ctx.seen_count == 0 { -0.20 } else { -0.10 }
    } else {
        0.0
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn subtree_affinity_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.runtime_subtree_affinity >= 2 {
        if ctx.is_entrypoint_runtime || ctx.is_runtime_config_artifact {
            0.54
        } else {
            0.28
        }
    } else if ctx.runtime_subtree_affinity > 0 {
        if ctx.is_entrypoint_runtime || ctx.is_runtime_config_artifact {
            0.30
        } else {
            0.16
        }
    } else {
        0.0
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn path_witness_subtree_locality_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.has_path_witness_source || ctx.runtime_subtree_affinity < 2 {
        return None;
    }

    let delta = if ctx.is_entrypoint_runtime || ctx.is_runtime_config_artifact {
        if ctx.seen_count == 0 { 0.86 } else { 0.44 }
    } else if matches!(
        ctx.class,
        HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
    ) {
        if ctx.seen_count == 0 { 0.42 } else { 0.24 }
    } else {
        0.0
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
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

fn generic_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_generic_runtime_witness_doc
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(if ctx.path_overlap == 0 {
            if ctx.runtime_seen == 0 { -0.74 } else { -0.42 }
        } else if ctx.runtime_seen == 0 {
            -0.38
        } else {
            -0.22
        }))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_metadata && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(
            if ctx.runtime_subtree_affinity == 0 && !ctx.has_path_witness_source {
                if ctx.seen_count == 0 {
                    if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
                        -0.92
                    } else {
                        -0.54
                    }
                } else if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
                    -0.46
                } else {
                    -0.28
                }
            } else if ctx.seen_count == 0 {
                if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
                    -0.56
                } else {
                    -0.32
                }
            } else if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
                -0.28
            } else {
                -0.18
            },
        ))
}

fn frontend_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_frontend_runtime_noise
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(
            if ctx.runtime_subtree_affinity == 0 && !ctx.has_path_witness_source {
                if ctx.path_overlap == 0 {
                    if ctx.runtime_seen == 0 { -0.96 } else { -0.56 }
                } else if ctx.runtime_seen == 0 {
                    -0.44
                } else {
                    -0.26
                }
            } else if ctx.path_overlap == 0 {
                if ctx.runtime_seen == 0 { -0.62 } else { -0.36 }
            } else if ctx.runtime_seen == 0 {
                -0.30
            } else {
                -0.18
            },
        ))
}

fn example_support_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && !ctx.wants_examples && ctx.is_example_support).then_some(
        PolicyEffect::Add(
            if ctx.specific_witness_path_overlap > 0
                || ctx.has_exact_query_term_match
                || ctx.runtime_subtree_affinity > 0
                || ctx.has_path_witness_source
            {
                -0.44
            } else if ctx.path_overlap > 0 {
                -0.86
            } else if ctx.runtime_seen == 0 {
                -1.22
            } else {
                -0.64
            },
        ),
    )
}

fn ci_workflow_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && !ctx.wants_ci_workflow_witnesses && ctx.is_ci_workflow)
        .then_some(PolicyEffect::Add(
            if ctx.specific_witness_path_overlap == 0
                && ctx.runtime_subtree_affinity == 0
                && !ctx.has_path_witness_source
            {
                if ctx.seen_count == 0 { -2.24 } else { -1.32 }
            } else if ctx.path_overlap == 0 {
                if ctx.seen_count == 0 { -1.22 } else { -0.72 }
            } else if ctx.seen_count == 0 {
                -0.68
            } else {
                -0.40
            },
        ))
}

fn root_repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_repo_metadata
        && !ctx.is_runtime_config_artifact
        && ctx.path_depth <= 1)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -1.08
        } else {
            -0.64
        }))
}

fn root_generic_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_generic_runtime_witness_doc
        && ctx.path_depth <= 1
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.78
        } else {
            -0.46
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
        "selection.runtime_config.same_language_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::wants_language_locality_bias_leaf(),
            pred::candidate_language_known_leaf(),
            pred::matches_query_language_leaf(),
        ]),
        same_language_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.language_mismatch_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::new(
            &[
                pred::wants_runtime_config_artifacts_leaf(),
                pred::wants_language_locality_bias_leaf(),
                pred::candidate_language_known_leaf(),
            ],
            &[],
            &[pred::matches_query_language_leaf()],
        ),
        language_mismatch_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.subtree_affinity_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::runtime_subtree_affinity_positive_leaf(),
        ]),
        subtree_affinity_bonus,
    ),
    ScoreRule::when(
        "selection.runtime_config.path_witness_subtree_locality_bonus",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::has_path_witness_source_leaf(),
            pred::runtime_subtree_affinity_at_least_two_leaf(),
        ]),
        path_witness_subtree_locality_bonus,
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
        "selection.runtime_config.generic_doc_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        generic_doc_penalty,
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
        "selection.runtime_config.root_repo_metadata_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        root_repo_metadata_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.root_generic_doc_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        root_generic_doc_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.frontend_noise_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_frontend_runtime_noise_leaf(),
        ]),
        frontend_noise_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.example_support_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_example_support_leaf(),
        ]),
        example_support_penalty,
    ),
    ScoreRule::when(
        "selection.runtime_config.ci_workflow_penalty",
        PolicyStage::SelectionRuntimeConfig,
        Predicate::new(
            &[
                pred::wants_runtime_config_artifacts_leaf(),
                pred::is_ci_workflow_leaf(),
            ],
            &[],
            &[pred::wants_ci_workflow_witnesses_leaf()],
        ),
        ci_workflow_penalty,
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
