use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
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
            5.40
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

fn same_language_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.is_entrypoint_runtime {
        if ctx.runtime_seen == 0 {
            0.48
        } else if ctx.seen_count == 0 {
            0.28
        } else {
            0.16
        }
    } else if matches!(
        ctx.class,
        HybridSourceClass::Runtime | HybridSourceClass::Tests
    ) {
        if ctx.seen_count == 0 { 0.18 } else { 0.10 }
    } else {
        0.0
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn language_mismatch_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.is_entrypoint_runtime {
        if ctx.runtime_seen == 0 {
            -0.44
        } else if ctx.seen_count == 0 {
            -0.24
        } else {
            -0.14
        }
    } else if matches!(
        ctx.class,
        HybridSourceClass::Runtime | HybridSourceClass::Tests
    ) {
        if ctx.seen_count == 0 { -0.18 } else { -0.10 }
    } else {
        0.0
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn subtree_affinity_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.runtime_subtree_affinity >= 2 {
        if ctx.is_entrypoint_runtime {
            0.52
        } else if ctx.is_runtime_config_artifact {
            0.32
        } else {
            0.16
        }
    } else if ctx.runtime_subtree_affinity > 0 {
        if ctx.is_entrypoint_runtime {
            0.26
        } else if ctx.is_runtime_config_artifact {
            0.18
        } else {
            0.08
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
        if ctx.seen_count == 0 { 0.68 } else { 0.36 }
    } else if matches!(
        ctx.class,
        HybridSourceClass::Runtime | HybridSourceClass::Tests
    ) {
        if ctx.seen_count == 0 { 0.30 } else { 0.16 }
    } else {
        0.0
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn lexical_only_runtime_focus_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.is_entrypoint_runtime {
        if ctx.runtime_seen == 0 {
            0.42
        } else if ctx.seen_count == 0 {
            0.20
        } else {
            0.10
        }
    } else if ctx.is_runtime_config_artifact {
        if ctx.seen_count == 0 { 0.18 } else { 0.10 }
    } else {
        0.0
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn lexical_only_non_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if ctx.is_entrypoint_runtime
        || ctx.is_runtime_config_artifact
        || ctx.is_entrypoint_build_workflow
        || ctx.has_exact_query_term_match
        || ctx.path_overlap > 0
    {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime | HybridSourceClass::Tests => {
            if ctx.seen_count == 0 {
                -0.18
            } else {
                -0.10
            }
        }
        HybridSourceClass::Documentation
        | HybridSourceClass::Readme
        | HybridSourceClass::Project => {
            if ctx.seen_count == 0 {
                -0.26
            } else {
                -0.14
            }
        }
        _ => 0.0,
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn repo_root_runtime_config_without_locality_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_repo_root_runtime_config_artifact
        && ctx.runtime_subtree_affinity == 0
        && ctx.specific_witness_path_overlap == 0
        && !ctx.has_path_witness_source
        && !ctx.excerpt_has_build_flow_anchor)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -3.20
        } else {
            -1.40
        }))
}

fn cli_test_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.query_mentions_cli && ctx.is_cli_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            0.78
        } else {
            0.42
        }))
}

fn cli_specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let best_overlap = ctx
        .specific_witness_path_overlap
        .max(ctx.path_overlap)
        .max(ctx.excerpt_overlap);
    let delta = match best_overlap {
        0 => {
            if ctx.has_exact_query_term_match {
                1.20
            } else {
                0.0
            }
        }
        1 => 2.60,
        _ => 4.20,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn cli_generic_support_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.query_mentions_cli
        && ctx.is_cli_test_support
        && ctx.specific_witness_path_overlap == 0
        && ctx.path_overlap == 0
        && ctx.excerpt_overlap == 0
        && !ctx.has_exact_query_term_match
        && !ctx.is_runtime_anchor_test_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -1.20
        } else {
            -0.68
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

fn ci_workflow_without_build_signal_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_ci_workflow
        && !ctx.excerpt_has_build_flow_anchor
        && ctx.specific_witness_path_overlap == 0)
        .then_some(PolicyEffect::Add(if ctx.runtime_seen == 0 {
            -2.40
        } else {
            -0.88
        }))
}

fn generic_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && ctx.is_generic_runtime_witness_doc
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(if ctx.runtime_seen == 0 {
            -0.56
        } else {
            -0.30
        }))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_repo_metadata && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Add(if ctx.runtime_seen == 0 {
            -0.52
        } else {
            -0.26
        }))
}

fn frontend_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_frontend_runtime_noise).then_some(PolicyEffect::Add(
        if ctx.runtime_seen == 0 {
            if ctx.path_overlap == 0 && !ctx.excerpt_has_build_flow_anchor {
                -0.52
            } else {
                -0.22
            }
        } else if ctx.path_overlap == 0 && !ctx.excerpt_has_build_flow_anchor {
            -0.28
        } else {
            -0.14
        },
    ))
}

fn loose_python_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_loose_python_test_module).then_some(
        PolicyEffect::Add(if ctx.runtime_seen == 0 { -0.18 } else { -0.10 }),
    )
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.entrypoint.runtime_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_runtime_leaf(),
        ]),
        runtime_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.workflow_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::is_entrypoint_build_workflow_leaf(),
            ],
            &[
                pred::excerpt_has_build_flow_anchor_leaf(),
                pred::specific_witness_path_overlap_leaf(),
            ],
            &[],
        ),
        workflow_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.workflow_without_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_build_workflow_leaf(),
            pred::runtime_seen_is_zero_leaf(),
        ]),
        workflow_without_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.laravel_core_provider_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_laravel_core_provider_leaf(),
        ]),
        laravel_core_provider_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.laravel_provider_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::is_laravel_provider_leaf(),
            ],
            &[],
            &[pred::is_laravel_core_provider_leaf()],
        ),
        laravel_provider_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.laravel_route_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_laravel_route_leaf(),
        ]),
        laravel_route_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.laravel_bootstrap_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_laravel_bootstrap_entrypoint_leaf(),
        ]),
        laravel_bootstrap_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.laravel_bootstrap_specific_overlap_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_laravel_bootstrap_entrypoint_leaf(),
            pred::specific_witness_path_overlap_leaf(),
        ]),
        laravel_bootstrap_specific_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_runtime_config_artifact_leaf(),
        ]),
        runtime_config_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.typescript_index_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        typescript_index_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.repo_root_runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        repo_root_runtime_config_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.repo_root_runtime_config_after_runtime_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::is_repo_root_runtime_config_artifact_leaf(),
                pred::runtime_seen_positive_leaf(),
            ],
            &[],
            &[pred::has_seen_repo_root_runtime_config_leaf()],
        ),
        repo_root_runtime_config_after_runtime_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.path_witness_runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_runtime_config_artifact_leaf(),
            pred::has_path_witness_source_leaf(),
        ]),
        path_witness_runtime_config_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.path_witness_repo_root_runtime_config_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
            pred::has_path_witness_source_leaf(),
        ]),
        path_witness_repo_root_runtime_config_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.path_witness_typescript_index_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
            pred::has_path_witness_source_leaf(),
        ]),
        path_witness_typescript_index_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.python_config_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_python_runtime_config_leaf(),
        ]),
        python_config_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.same_language_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::wants_language_locality_bias_leaf(),
            pred::candidate_language_known_leaf(),
            pred::matches_query_language_leaf(),
        ]),
        same_language_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.lexical_only_runtime_focus_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::lexical_only_mode_leaf(),
        ]),
        lexical_only_runtime_focus_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.lexical_only_non_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::lexical_only_mode_leaf(),
        ]),
        lexical_only_non_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.language_mismatch_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::wants_language_locality_bias_leaf(),
                pred::candidate_language_known_leaf(),
            ],
            &[],
            &[pred::matches_query_language_leaf()],
        ),
        language_mismatch_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.subtree_affinity_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::runtime_subtree_affinity_positive_leaf(),
        ]),
        subtree_affinity_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.path_witness_subtree_locality_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::has_path_witness_source_leaf(),
            pred::runtime_subtree_affinity_at_least_two_leaf(),
        ]),
        path_witness_subtree_locality_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.repo_root_runtime_config_without_locality_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        repo_root_runtime_config_without_locality_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.cli_test_support_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::query_mentions_cli_leaf(),
            pred::is_cli_test_support_leaf(),
        ]),
        cli_test_support_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.cli_specific_overlap_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::query_mentions_cli_leaf(),
            pred::is_cli_test_support_leaf(),
        ]),
        cli_specific_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.cli_generic_support_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::query_mentions_cli_leaf(),
                pred::is_cli_test_support_leaf(),
            ],
            &[],
            &[pred::is_runtime_anchor_test_support_leaf()],
        ),
        cli_generic_support_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.build_flow_anchor_bonus",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::excerpt_has_build_flow_anchor_leaf(),
        ]),
        build_flow_anchor_bonus,
    ),
    ScoreRule::when(
        "selection.entrypoint.test_double_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::excerpt_has_test_double_anchor_leaf(),
        ]),
        test_double_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.cli_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::query_mentions_cli_leaf(),
                pred::class_is_runtime_leaf(),
            ],
            &[],
            &[pred::is_cli_test_support_leaf()],
        ),
        cli_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.non_entry_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::class_is_runtime_leaf(),
            ],
            &[],
            &[
                pred::is_entrypoint_runtime_leaf(),
                pred::is_runtime_config_artifact_leaf(),
            ],
        ),
        non_entry_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.tests_specs_without_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::runtime_seen_is_zero_leaf(),
            ],
            &[pred::class_is_tests_leaf(), pred::class_is_specs_leaf()],
            &[],
        ),
        tests_specs_without_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.reference_doc_without_runtime_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_reference_doc_leaf(),
            pred::runtime_seen_is_zero_leaf(),
        ]),
        reference_doc_without_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.ci_workflow_without_build_signal_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::is_ci_workflow_leaf(),
            ],
            &[],
            &[
                pred::excerpt_has_build_flow_anchor_leaf(),
                pred::specific_witness_path_overlap_leaf(),
            ],
        ),
        ci_workflow_without_build_signal_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.generic_doc_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        generic_doc_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.repo_metadata_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        repo_metadata_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.frontend_noise_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_frontend_runtime_noise_leaf(),
        ]),
        frontend_noise_penalty,
    ),
    ScoreRule::when(
        "selection.entrypoint.loose_python_test_penalty",
        PolicyStage::SelectionEntrypoint,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_loose_python_test_module_leaf(),
        ]),
        loose_python_test_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
