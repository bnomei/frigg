use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn first_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    (state.seen_count() == 0).then_some(PolicyEffect::Add(0.24))
}

fn first_support_or_test_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    (state.seen_count() == 0).then_some(PolicyEffect::Add(0.10))
}

fn identifier_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.30
    } else {
        0.16
    }))
}

fn fixtures_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        -0.42
    } else {
        -0.24
    }))
}

fn python_entrypoint_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let state = ctx.state();

    Some(PolicyEffect::Add(if intent.wants_python_witnesses() {
        if state.seen_count() == 0 { 0.26 } else { 0.14 }
    } else if state.seen_count() == 0 {
        -0.16
    } else {
        -0.08
    }))
}

fn python_config_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let state = ctx.state();

    Some(PolicyEffect::Add(
        if intent.wants_python_workspace_config() {
            if state.seen_count() == 0 { 0.18 } else { 0.10 }
        } else if state.seen_count() == 0 {
            -0.18
        } else {
            -0.10
        },
    ))
}

fn python_test_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let state = ctx.state();

    Some(PolicyEffect::Add(if intent.wants_python_witnesses() {
        if state.seen_count() == 0 { 0.28 } else { 0.12 }
    } else if state.seen_count() == 0 {
        -0.22
    } else {
        -0.12
    }))
}

fn loose_python_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        -0.18
    } else {
        -0.10
    }))
}

fn path_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    if candidate.path_overlap() == 0 {
        return None;
    }

    let delta = match candidate.class() {
        HybridSourceClass::Runtime => {
            if candidate.path_overlap() == 1 {
                0.10
            } else {
                0.18
            }
        }
        HybridSourceClass::Support | HybridSourceClass::Tests => {
            if candidate.path_overlap() == 1 {
                0.08
            } else {
                0.14
            }
        }
        HybridSourceClass::Documentation | HybridSourceClass::Readme => {
            if candidate.path_overlap() == 1 {
                0.02
            } else {
                0.06
            }
        }
        _ => 0.0,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn generic_doc_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    (state.seen_count() > 0).then_some(PolicyEffect::Add(-0.16 * state.seen_count() as f32))
}

fn generic_doc_first_penalty(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(-0.18))
}

fn doc_path_overlap_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    if !matches!(
        candidate.class(),
        HybridSourceClass::Documentation | HybridSourceClass::Readme
    ) {
        return None;
    }

    let delta = match candidate.path_overlap() {
        0 => -0.18,
        1 => -0.06,
        _ => 0.0,
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.runtime_seen() == 0 {
        -0.26
    } else {
        -0.18
    }))
}

fn python_config_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.runtime_seen() == 0 {
        0.16
    } else {
        0.08
    }))
}

fn generic_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();
    (candidate.path_overlap() == 0).then_some(PolicyEffect::Add(if state.seen_count() == 0 {
        -0.12
    } else {
        -0.18
    }))
}

fn missing_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();
    if candidate.excerpt_has_exact_identifier_anchor()
        || candidate.has_exact_query_term_match()
        || !matches!(
            candidate.class(),
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        )
    {
        return None;
    }

    let delta = match candidate.path_overlap() {
        0 => {
            if state.seen_count() == 0 {
                -0.24
            } else {
                -0.14
            }
        }
        1 => {
            if candidate.class() == HybridSourceClass::Runtime {
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
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.runtime_seen() == 0 {
        -0.28
    } else {
        -0.18
    }))
}

fn example_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let candidate = ctx.candidate();
    let state = ctx.state();
    let corroborated_example_signal = intent.wants_examples()
        || candidate.path_overlap() > 0
        || candidate.excerpt_overlap() > 0
        || candidate.has_path_witness_source();
    if !corroborated_example_signal {
        return None;
    }

    let overlap = candidate
        .specific_witness_path_overlap()
        .max(candidate.path_overlap())
        .max(candidate.excerpt_overlap());
    let delta = if overlap >= 2 {
        if state.seen_count() == 0 { 0.84 } else { 0.46 }
    } else if overlap == 1 {
        if state.seen_count() == 0 { 0.66 } else { 0.36 }
    } else if candidate.has_exact_query_term_match() {
        if state.seen_count() == 0 { 0.58 } else { 0.32 }
    } else if intent.wants_examples() {
        if state.seen_count() == 0 { 0.24 } else { 0.12 }
    } else if state.seen_count() == 0 {
        0.18
    } else {
        0.10
    };

    Some(PolicyEffect::Add(delta))
}

fn bench_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();

    let delta = if candidate.specific_witness_path_overlap() >= 2 {
        if state.seen_count() == 0 { 0.96 } else { 0.52 }
    } else if candidate.specific_witness_path_overlap() == 1 {
        if state.seen_count() == 0 { 0.76 } else { 0.42 }
    } else if candidate.has_exact_query_term_match() {
        if state.seen_count() == 0 { 0.64 } else { 0.36 }
    } else if state.seen_count() == 0 {
        0.26
    } else {
        0.14
    };

    Some(PolicyEffect::Add(delta))
}

fn non_support_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let candidate = ctx.candidate();
    let state = ctx.state();
    if candidate.is_example_support()
        || candidate.is_bench_support()
        || candidate.specific_witness_path_overlap() > 0
    {
        return None;
    }

    Some(PolicyEffect::Add(if intent.wants_test_witness_recall() {
        if state.seen_count() == 0 {
            -0.18
        } else {
            -0.10
        }
    } else if state.seen_count() == 0 {
        -0.34
    } else {
        -0.18
    }))
}

fn non_support_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();
    (!candidate.is_example_support() && !candidate.is_bench_support()).then_some(PolicyEffect::Add(
        if state.seen_count() == 0 {
            -0.36
        } else {
            -0.22
        },
    ))
}

fn examples_rs_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        -1.10
    } else {
        -0.58
    }))
}

fn python_test_bridge_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    (state.runtime_seen() > 0 && state.seen_count() == 0).then_some(PolicyEffect::Add(0.18))
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
