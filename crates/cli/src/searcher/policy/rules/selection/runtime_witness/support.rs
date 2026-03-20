use super::super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet};
use super::super::super::super::facts::SelectionFacts;
use super::super::super::super::predicates::selection as pred;
use super::super::super::super::trace::{PolicyEffect, PolicyStage};

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
        if ctx.lexical_only_mode {
            if state.seen_count == 0 { 0.18 } else { 0.10 }
        } else if state.seen_count == 0 {
            -0.18
        } else {
            -0.10
        }
    } else if ctx.lexical_only_mode {
        if state.seen_count == 0 { -0.48 } else { -0.28 }
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

pub(super) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);
