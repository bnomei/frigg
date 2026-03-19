use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn first_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    (state.seen_count == 0).then_some(PolicyEffect::Add(0.24))
}

fn first_support_or_test_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    (state.seen_count == 0).then_some(PolicyEffect::Add(0.10))
}

fn identifier_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        0.30
    } else {
        0.16
    }))
}

fn fixtures_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.42
    } else {
        -0.24
    }))
}

fn python_entrypoint_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let state = ctx;

    Some(PolicyEffect::Add(if intent.wants_python_witnesses {
        if state.seen_count == 0 { 0.26 } else { 0.14 }
    } else if state.seen_count == 0 {
        -0.16
    } else {
        -0.08
    }))
}

fn python_config_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let state = ctx;

    Some(PolicyEffect::Add(if intent.wants_python_workspace_config {
        if state.seen_count == 0 { 0.18 } else { 0.10 }
    } else if state.seen_count == 0 {
        -0.18
    } else {
        -0.10
    }))
}

fn python_test_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let state = ctx;

    Some(PolicyEffect::Add(if intent.wants_python_witnesses {
        if state.seen_count == 0 { 0.28 } else { 0.12 }
    } else if state.seen_count == 0 {
        -0.22
    } else {
        -0.12
    }))
}

fn loose_python_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.18
    } else {
        -0.10
    }))
}

fn path_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    if candidate.path_overlap == 0 {
        return None;
    }

    let delta = match candidate.class {
        HybridSourceClass::Runtime => {
            if candidate.path_overlap == 1 {
                0.10
            } else {
                0.18
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if candidate.path_overlap == 1 {
                0.08
            } else {
                0.14
            }
        }
        HybridSourceClass::Documentation | HybridSourceClass::Readme => {
            if candidate.path_overlap == 1 {
                0.02
            } else {
                0.06
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn same_language_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.seen_count == 0 {
                0.34
            } else {
                0.18
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.seen_count == 0 {
                0.22
            } else {
                0.12
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn same_language_path_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if ctx.path_overlap == 0 {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.path_overlap >= 2 {
                if ctx.seen_count == 0 { 0.42 } else { 0.24 }
            } else if ctx.seen_count == 0 {
                0.22
            } else {
                0.12
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.path_overlap >= 2 {
                if ctx.seen_count == 0 { 0.28 } else { 0.16 }
            } else if ctx.seen_count == 0 {
                0.16
            } else {
                0.10
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn language_mismatch_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.seen_count == 0 {
                -0.28
            } else {
                -0.16
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.seen_count == 0 {
                -0.20
            } else {
                -0.12
            }
        }
        _ => 0.0,
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn language_mismatch_path_overlap_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if ctx.path_overlap == 0 {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.path_overlap >= 2 {
                if ctx.seen_count == 0 { -0.36 } else { -0.22 }
            } else if ctx.seen_count == 0 {
                -0.20
            } else {
                -0.12
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.path_overlap >= 2 {
                if ctx.seen_count == 0 { -0.24 } else { -0.14 }
            } else if ctx.seen_count == 0 {
                -0.14
            } else {
                -0.08
            }
        }
        _ => 0.0,
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn subtree_affinity_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.runtime_subtree_affinity >= 2 {
                0.26
            } else {
                0.12
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.runtime_subtree_affinity >= 2 {
                0.18
            } else {
                0.10
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn path_witness_subtree_locality_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.has_path_witness_source || ctx.runtime_subtree_affinity < 2 {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.seen_count == 0 {
                0.72
            } else {
                0.38
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.seen_count == 0 {
                0.42
            } else {
                0.22
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn generic_doc_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    (state.seen_count > 0).then_some(PolicyEffect::Add(-0.32 * state.seen_count as f32))
}

fn generic_doc_first_penalty(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-0.68))
}

fn doc_path_overlap_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    if !matches!(
        candidate.class,
        HybridSourceClass::Documentation | HybridSourceClass::Readme
    ) {
        return None;
    }

    let delta = match candidate.path_overlap {
        0 => -0.28,
        1 => -0.18,
        _ => 0.0,
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.runtime_seen == 0 {
        if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
            -0.86
        } else {
            -0.52
        }
    } else {
        if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
            -0.44
        } else {
            -0.24
        }
    }))
}

fn python_config_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.runtime_seen == 0 {
        0.16
    } else {
        0.08
    }))
}

fn generic_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    (candidate.path_overlap == 0).then_some(PolicyEffect::Add(if state.seen_count == 0 {
        -0.12
    } else {
        -0.18
    }))
}

fn missing_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    if candidate.excerpt_has_exact_identifier_anchor
        || candidate.has_exact_query_term_match
        || !matches!(
            candidate.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        )
    {
        return None;
    }

    let delta = match candidate.path_overlap {
        0 => {
            if state.seen_count == 0 {
                -0.24
            } else {
                -0.14
            }
        }
        1 => {
            if candidate.class == HybridSourceClass::Runtime {
                -0.18
            } else {
                -0.10
            }
        }
        _ => 0.0,
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn frontend_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.runtime_seen == 0 {
        if ctx.runtime_subtree_affinity == 0 && !ctx.has_path_witness_source {
            if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
                -0.92
            } else {
                -0.62
            }
        } else if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
            -0.64
        } else {
            -0.46
        }
    } else {
        if ctx.runtime_subtree_affinity == 0 && !ctx.has_path_witness_source {
            if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
                -0.52
            } else {
                -0.34
            }
        } else if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
            -0.38
        } else {
            -0.26
        }
    }))
}

fn locality_with_path_witness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.has_path_witness_source || ctx.runtime_subtree_affinity == 0 {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.runtime_subtree_affinity >= 2 {
                if ctx.seen_count == 0 { 0.72 } else { 0.38 }
            } else if ctx.seen_count == 0 {
                0.28
            } else {
                0.14
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.runtime_subtree_affinity >= 2 {
                if ctx.seen_count == 0 { 0.48 } else { 0.26 }
            } else if ctx.seen_count == 0 {
                0.18
            } else {
                0.10
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn specific_witness_locality_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if ctx.specific_witness_path_overlap == 0 {
        return None;
    }

    let anchored_locality = ctx.has_path_witness_source || ctx.runtime_subtree_affinity > 0;
    let language_locality = ctx.wants_language_locality_bias && ctx.matches_query_language;
    if !(anchored_locality || language_locality) {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if anchored_locality {
                if ctx.seen_count == 0 { 0.52 } else { 0.28 }
            } else if ctx.seen_count == 0 {
                0.32
            } else {
                0.18
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if anchored_locality {
                if ctx.seen_count == 0 { 0.38 } else { 0.20 }
            } else if ctx.seen_count == 0 {
                0.24
            } else {
                0.14
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn ci_workflow_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && !ctx.wants_ci_workflow_witnesses && ctx.is_ci_workflow)
        .then_some(PolicyEffect::Add(
            if ctx.specific_witness_path_overlap == 0
                && ctx.runtime_subtree_affinity == 0
                && !ctx.has_path_witness_source
            {
                if ctx.seen_count == 0 { -3.40 } else { -2.04 }
            } else if ctx.path_overlap == 0 {
                if ctx.seen_count == 0 { -2.20 } else { -1.34 }
            } else if ctx.seen_count == 0 {
                -1.54
            } else {
                -0.92
            },
        ))
}

fn example_support_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_example_support)
        .then_some(PolicyEffect::Add(
            if ctx.specific_witness_path_overlap > 0
                || ctx.has_exact_query_term_match
                || ctx.runtime_subtree_affinity > 0
                || ctx.has_path_witness_source
            {
                -0.70
            } else if ctx.path_overlap > 0 {
                -1.18
            } else if ctx.seen_count == 0 {
                -1.64
            } else {
                -0.96
            },
        ))
}

fn repo_metadata_locality_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.is_repo_metadata
        && ctx.runtime_subtree_affinity == 0
        && !ctx.has_path_witness_source)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -1.08
        } else {
            -0.64
        }))
}

fn root_repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.is_repo_metadata
        && !ctx.is_runtime_config_artifact
        && ctx.path_depth <= 1)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -1.28
        } else {
            -0.76
        }))
}

fn root_generic_doc_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.is_generic_runtime_witness_doc && ctx.path_depth <= 1)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.72
        } else {
            -0.44
        }))
}

fn example_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let candidate = ctx;
    let state = ctx;
    let corroborated_example_signal = intent.wants_examples
        || candidate.path_overlap > 0
        || candidate.excerpt_overlap > 0
        || candidate.has_path_witness_source;
    if !corroborated_example_signal {
        return None;
    }

    let overlap = candidate
        .specific_witness_path_overlap
        .max(candidate.path_overlap)
        .max(candidate.excerpt_overlap);
    let delta = if overlap >= 2 {
        if state.seen_count == 0 { 0.84 } else { 0.46 }
    } else if overlap == 1 {
        if state.seen_count == 0 { 0.66 } else { 0.36 }
    } else if candidate.has_exact_query_term_match {
        if state.seen_count == 0 { 0.58 } else { 0.32 }
    } else if intent.wants_examples {
        if state.seen_count == 0 { 0.24 } else { 0.12 }
    } else if state.seen_count == 0 {
        0.18
    } else {
        0.10
    };

    Some(PolicyEffect::Add(delta))
}

fn bench_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;

    let delta = if candidate.specific_witness_path_overlap >= 2 {
        if state.seen_count == 0 { 0.96 } else { 0.52 }
    } else if candidate.specific_witness_path_overlap == 1 {
        if state.seen_count == 0 { 0.76 } else { 0.42 }
    } else if candidate.has_exact_query_term_match {
        if state.seen_count == 0 { 0.64 } else { 0.36 }
    } else if state.seen_count == 0 {
        0.26
    } else {
        0.14
    };

    Some(PolicyEffect::Add(delta))
}

fn non_support_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx;
    let candidate = ctx;
    let state = ctx;
    if candidate.is_example_support
        || candidate.is_bench_support
        || candidate.specific_witness_path_overlap > 0
    {
        return None;
    }

    Some(PolicyEffect::Add(if intent.wants_test_witness_recall {
        if state.seen_count == 0 { -0.18 } else { -0.10 }
    } else if state.seen_count == 0 {
        -0.34
    } else {
        -0.18
    }))
}

fn non_support_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx;
    let state = ctx;
    (!candidate.is_example_support && !candidate.is_bench_support).then_some(PolicyEffect::Add(
        if state.seen_count == 0 { -0.36 } else { -0.22 },
    ))
}

fn examples_rs_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    Some(PolicyEffect::Add(if state.seen_count == 0 {
        -1.10
    } else {
        -0.58
    }))
}

fn python_test_bridge_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx;
    (state.runtime_seen > 0 && state.seen_count == 0).then_some(PolicyEffect::Add(0.18))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.runtime.first_runtime_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::class_is_runtime_leaf(),
            pred::seen_count_is_zero_leaf(),
        ]),
        first_runtime_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.first_support_or_test_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::seen_count_is_zero_leaf(),
            ],
            &[pred::class_is_support_leaf(), pred::class_is_tests_leaf()],
            &[],
        ),
        first_support_or_test_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.identifier_anchor_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::excerpt_has_exact_identifier_anchor_leaf(),
            ],
            &[
                pred::class_is_runtime_leaf(),
                pred::class_is_support_leaf(),
                pred::class_is_tests_leaf(),
            ],
            &[],
        ),
        identifier_anchor_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.fixtures_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::class_is_fixtures_leaf(),
        ]),
        fixtures_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.python_entrypoint_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_entrypoint_runtime_leaf(),
        ]),
        python_entrypoint_adjustment,
    ),
    ScoreRule::when(
        "selection.runtime.python_config_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_runtime_config_leaf(),
        ]),
        python_config_adjustment,
    ),
    ScoreRule::when(
        "selection.runtime.python_test_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_test_witness_leaf(),
        ]),
        python_test_adjustment,
    ),
    ScoreRule::when(
        "selection.runtime.loose_python_test_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_loose_python_test_module_leaf(),
        ]),
        loose_python_test_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.path_overlap_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::path_overlap_leaf(),
        ]),
        path_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.same_language_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::wants_language_locality_bias_leaf(),
            pred::candidate_language_known_leaf(),
            pred::matches_query_language_leaf(),
        ]),
        same_language_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.same_language_path_overlap_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::wants_language_locality_bias_leaf(),
            pred::candidate_language_known_leaf(),
            pred::matches_query_language_leaf(),
            pred::path_overlap_leaf(),
        ]),
        same_language_path_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.language_mismatch_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::wants_language_locality_bias_leaf(),
                pred::candidate_language_known_leaf(),
            ],
            &[],
            &[pred::matches_query_language_leaf()],
        ),
        language_mismatch_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.language_mismatch_path_overlap_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::wants_language_locality_bias_leaf(),
                pred::candidate_language_known_leaf(),
                pred::path_overlap_leaf(),
            ],
            &[],
            &[pred::matches_query_language_leaf()],
        ),
        language_mismatch_path_overlap_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.subtree_affinity_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::runtime_subtree_affinity_positive_leaf(),
        ]),
        subtree_affinity_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.path_witness_subtree_locality_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::has_path_witness_source_leaf(),
            pred::runtime_subtree_affinity_at_least_two_leaf(),
        ]),
        path_witness_subtree_locality_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.generic_doc_repeat_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::penalize_generic_runtime_docs_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        generic_doc_repeat_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.generic_doc_first_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::penalize_generic_runtime_docs_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
            pred::runtime_seen_is_zero_leaf(),
        ]),
        generic_doc_first_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.doc_path_overlap_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::penalize_generic_runtime_docs_leaf(),
        ]),
        doc_path_overlap_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.repo_metadata_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        repo_metadata_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.python_config_runtime_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_runtime_config_leaf(),
        ]),
        python_config_runtime_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.generic_anchor_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::class_is_runtime_leaf(),
            pred::has_generic_runtime_anchor_stem_leaf(),
        ]),
        generic_anchor_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.missing_anchor_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[pred::wants_runtime_witnesses_leaf()],
            &[
                pred::class_is_runtime_leaf(),
                pred::class_is_support_leaf(),
                pred::class_is_tests_leaf(),
            ],
            &[],
        ),
        missing_anchor_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.frontend_noise_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_frontend_runtime_noise_leaf(),
        ]),
        frontend_noise_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.locality_with_path_witness_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::has_path_witness_source_leaf(),
            pred::runtime_subtree_affinity_positive_leaf(),
        ]),
        locality_with_path_witness_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.specific_witness_locality_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::specific_witness_path_overlap_leaf(),
            ],
            &[
                pred::class_is_runtime_leaf(),
                pred::class_is_support_leaf(),
                pred::class_is_tests_leaf(),
            ],
            &[],
        ),
        specific_witness_locality_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.ci_workflow_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::is_ci_workflow_leaf(),
            ],
            &[],
            &[pred::wants_ci_workflow_witnesses_leaf()],
        ),
        ci_workflow_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.example_support_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_example_support_leaf(),
        ]),
        example_support_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.repo_metadata_locality_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        repo_metadata_locality_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.root_repo_metadata_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        root_repo_metadata_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.root_generic_doc_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        root_generic_doc_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.example_support_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_example_support_leaf(),
        ]),
        example_support_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.bench_support_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::wants_benchmarks_leaf(),
            pred::is_bench_support_leaf(),
        ]),
        bench_support_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.non_support_test_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        non_support_test_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.non_support_runtime_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        non_support_runtime_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.examples_rs_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_test_support_leaf(),
            pred::is_examples_rs_leaf(),
        ]),
        examples_rs_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.python_test_bridge_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_python_test_witness_leaf(),
            pred::seen_count_is_zero_leaf(),
        ]),
        python_test_bridge_bonus,
    ),
];

const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
