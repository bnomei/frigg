use super::super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet};
use super::super::super::super::facts::SelectionFacts;
use super::super::super::super::predicates::selection as pred;
use super::super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

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
    let lexical_only_penalty = if ctx.lexical_only_mode {
        if ctx.seen_count == 0 { -0.18 } else { -0.10 }
    } else {
        0.0
    };
    let state = ctx;
    Some(PolicyEffect::Add(if state.runtime_seen == 0 {
        if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
            -0.86 + lexical_only_penalty
        } else {
            -0.52 + lexical_only_penalty
        }
    } else if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
        -0.44 + lexical_only_penalty
    } else {
        -0.24 + lexical_only_penalty
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
    } else if ctx.runtime_subtree_affinity == 0 && !ctx.has_path_witness_source {
        if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
            -0.52
        } else {
            -0.34
        }
    } else if ctx.path_overlap == 0 && !ctx.has_exact_query_term_match {
        -0.38
    } else {
        -0.26
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

const RULES: &[ScoreRule<SelectionFacts>] = &[
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
];

pub(super) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);
