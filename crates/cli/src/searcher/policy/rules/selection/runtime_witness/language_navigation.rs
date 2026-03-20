use super::super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet};
use super::super::super::super::facts::SelectionFacts;
use super::super::super::super::predicates::selection as pred;
use super::super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn same_language_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let lexical_only_bonus = if ctx.lexical_only_mode {
        if ctx.seen_count == 0 { 0.10 } else { 0.06 }
    } else {
        0.0
    };
    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.seen_count == 0 {
                0.34 + lexical_only_bonus
            } else {
                0.18 + lexical_only_bonus
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.seen_count == 0 {
                0.22 + lexical_only_bonus
            } else {
                0.12 + lexical_only_bonus
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

fn live_navigation_pivot_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let config_focused = ctx.wants_runtime_config_artifacts
        && !ctx.wants_runtime_witnesses
        && !ctx.wants_navigation_fallbacks
        && !ctx.wants_test_witness_recall;
    if config_focused
        || !ctx.candidate_language_known
        || ctx.is_repo_metadata
        || ctx.is_frontend_runtime_noise
    {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.seen_count == 0 {
                0.18
            } else {
                0.10
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.seen_count == 0 {
                0.12
            } else {
                0.06
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn live_navigation_text_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let config_focused = ctx.wants_runtime_config_artifacts
        && !ctx.wants_runtime_witnesses
        && !ctx.wants_navigation_fallbacks
        && !ctx.wants_test_witness_recall;
    if config_focused {
        return None;
    }

    let lexical_only_penalty = if ctx.lexical_only_mode {
        if ctx.seen_count == 0 { -0.10 } else { -0.06 }
    } else {
        0.0
    };
    let delta = if ctx.is_repo_metadata {
        if ctx.seen_count == 0 { -0.20 } else { -0.10 }
    } else {
        match ctx.class {
            HybridSourceClass::Documentation | HybridSourceClass::Readme => {
                if ctx.has_exact_query_term_match || ctx.path_overlap > 0 {
                    0.0
                } else if ctx.seen_count == 0 {
                    -0.12 + lexical_only_penalty
                } else {
                    -0.06 + lexical_only_penalty
                }
            }
            HybridSourceClass::Project => {
                if ctx.candidate_language_known
                    || ctx.is_runtime_config_artifact
                    || ctx.is_entrypoint_build_workflow
                {
                    0.0
                } else if ctx.seen_count == 0 {
                    -0.08 + lexical_only_penalty
                } else {
                    -0.04 + lexical_only_penalty
                }
            }
            _ => 0.0,
        }
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn lexical_only_runtime_source_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if ctx.wants_test_witness_recall
        || ctx.wants_runtime_config_artifacts
        || ctx.is_repo_metadata
        || ctx.is_frontend_runtime_noise
    {
        return None;
    }

    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.seen_count == 0 {
                0.30
            } else {
                0.16
            }
        }
        HybridSourceClass::Support => {
            if ctx.seen_count == 0 {
                0.18
            } else {
                0.10
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn lexical_only_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if ctx.wants_test_witness_recall
        || !ctx.is_test_support
        || ctx.has_exact_query_term_match
        || ctx.specific_witness_path_overlap > 0
        || ctx.path_overlap > 1
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.seen_count == 0 {
        -0.52
    } else {
        -0.28
    }))
}

fn lexical_only_repo_noise_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if ctx.has_exact_query_term_match || ctx.path_overlap > 0 {
        return None;
    }

    let delta = if ctx.is_repo_metadata || ctx.is_generic_runtime_witness_doc {
        if ctx.seen_count == 0 { -0.34 } else { -0.18 }
    } else {
        0.0
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn language_mismatch_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let lexical_only_penalty = if ctx.lexical_only_mode {
        if ctx.seen_count == 0 { -0.10 } else { -0.06 }
    } else {
        0.0
    };
    let delta = match ctx.class {
        HybridSourceClass::Runtime => {
            if ctx.seen_count == 0 {
                -0.28 + lexical_only_penalty
            } else {
                -0.16 + lexical_only_penalty
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if ctx.seen_count == 0 {
                -0.20 + lexical_only_penalty
            } else {
                -0.12 + lexical_only_penalty
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

const RULES: &[ScoreRule<SelectionFacts>] = &[
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
        "selection.runtime.live_navigation_pivot_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[pred::candidate_language_known_leaf()],
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::wants_navigation_fallbacks_leaf(),
                pred::wants_test_witness_recall_leaf(),
            ],
            &[
                pred::is_repo_metadata_leaf(),
                pred::is_frontend_runtime_noise_leaf(),
            ],
        ),
        live_navigation_pivot_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.live_navigation_text_noise_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::new(
            &[],
            &[
                pred::wants_runtime_witnesses_leaf(),
                pred::wants_navigation_fallbacks_leaf(),
                pred::wants_test_witness_recall_leaf(),
            ],
            &[],
        ),
        live_navigation_text_noise_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.lexical_only_runtime_source_bonus",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::lexical_only_mode_leaf(),
            pred::candidate_language_known_leaf(),
            pred::matches_query_language_leaf(),
        ]),
        lexical_only_runtime_source_bonus,
    ),
    ScoreRule::when(
        "selection.runtime.lexical_only_test_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::lexical_only_mode_leaf(),
            pred::is_test_support_leaf(),
        ]),
        lexical_only_test_penalty,
    ),
    ScoreRule::when(
        "selection.runtime.lexical_only_repo_noise_penalty",
        PolicyStage::SelectionRuntimeWitness,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::lexical_only_mode_leaf(),
        ]),
        lexical_only_repo_noise_penalty,
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
];

pub(super) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);
