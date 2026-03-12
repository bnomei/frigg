use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn first_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.class == HybridSourceClass::Runtime && ctx.seen_count == 0)
        .then_some(PolicyEffect::Add(0.24))
}

fn first_support_or_test_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && matches!(
            ctx.class,
            HybridSourceClass::Support | HybridSourceClass::Tests
        )
        && ctx.seen_count == 0)
        .then_some(PolicyEffect::Add(0.10))
}

fn identifier_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.excerpt_has_exact_identifier_anchor
        && matches!(
            ctx.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        ))
    .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
        0.30
    } else {
        0.16
    }))
}

fn fixtures_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.class == HybridSourceClass::Fixtures).then_some(
        PolicyEffect::Add(if ctx.seen_count == 0 { -0.42 } else { -0.24 }),
    )
}

fn python_entrypoint_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses || !ctx.is_python_entrypoint_runtime {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.wants_python_witnesses {
        if ctx.seen_count == 0 { 0.26 } else { 0.14 }
    } else if ctx.seen_count == 0 {
        -0.16
    } else {
        -0.08
    }))
}

fn python_config_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses || !ctx.is_python_runtime_config {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.wants_python_workspace_config {
        if ctx.seen_count == 0 { 0.18 } else { 0.10 }
    } else if ctx.seen_count == 0 {
        -0.18
    } else {
        -0.10
    }))
}

fn python_test_adjustment(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses || !ctx.is_python_test_witness {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.wants_python_witnesses {
        if ctx.seen_count == 0 { 0.28 } else { 0.12 }
    } else if ctx.seen_count == 0 {
        -0.22
    } else {
        -0.12
    }))
}

fn loose_python_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.is_loose_python_test_module).then_some(PolicyEffect::Add(
        if ctx.seen_count == 0 { -0.18 } else { -0.10 },
    ))
}

fn path_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses || ctx.path_overlap == 0 {
        return None;
    }

    let delta =
        match ctx.class {
            HybridSourceClass::Runtime => {
                if ctx.path_overlap == 1 {
                    0.10
                } else {
                    0.18
                }
            }
            HybridSourceClass::Support | HybridSourceClass::Tests => {
                if ctx.path_overlap == 1 {
                    0.08
                } else {
                    0.14
                }
            }
            HybridSourceClass::Documentation | HybridSourceClass::Readme => {
                if ctx.path_overlap == 1 { 0.02 } else { 0.06 }
            }
            _ => 0.0,
        };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn generic_doc_repeat_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.penalize_generic_runtime_docs
        && ctx.is_generic_runtime_witness_doc
        && ctx.seen_count > 0)
        .then_some(PolicyEffect::Add(-0.16 * ctx.seen_count as f32))
}

fn generic_doc_first_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.penalize_generic_runtime_docs
        && ctx.is_generic_runtime_witness_doc
        && ctx.runtime_seen == 0)
        .then_some(PolicyEffect::Add(-0.18))
}

fn doc_path_overlap_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses
        || !ctx.penalize_generic_runtime_docs
        || !matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        )
    {
        return None;
    }

    let delta = match ctx.path_overlap {
        0 => -0.18,
        1 => -0.06,
        _ => 0.0,
    };

    (delta != 0.0).then_some(PolicyEffect::Add(delta))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.is_repo_metadata).then_some(PolicyEffect::Add(
        if ctx.runtime_seen == 0 { -0.26 } else { -0.18 },
    ))
}

fn python_config_runtime_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.is_python_runtime_config).then_some(PolicyEffect::Add(
        if ctx.runtime_seen == 0 { 0.16 } else { 0.08 },
    ))
}

fn generic_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.class == HybridSourceClass::Runtime
        && ctx.path_overlap == 0
        && ctx.has_generic_runtime_anchor_stem)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.12
        } else {
            -0.18
        }))
}

fn missing_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses
        || ctx.excerpt_has_exact_identifier_anchor
        || ctx.has_exact_query_term_match
        || !matches!(
            ctx.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        )
    {
        return None;
    }

    let delta = match ctx.path_overlap {
        0 => {
            if ctx.seen_count == 0 {
                -0.24
            } else {
                -0.14
            }
        }
        1 => {
            if ctx.class == HybridSourceClass::Runtime {
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
    (ctx.wants_runtime_witnesses && ctx.is_frontend_runtime_noise).then_some(PolicyEffect::Add(
        if ctx.runtime_seen == 0 { -0.28 } else { -0.18 },
    ))
}

fn example_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses || !ctx.is_example_support {
        return None;
    }

    let corroborated_example_signal = ctx.wants_examples
        || ctx.path_overlap > 0
        || ctx.excerpt_overlap > 0
        || ctx.has_path_witness_source;
    if !corroborated_example_signal {
        return None;
    }

    let overlap = ctx
        .specific_witness_path_overlap
        .max(ctx.path_overlap)
        .max(ctx.excerpt_overlap);
    let delta = if overlap >= 2 {
        if ctx.seen_count == 0 { 0.84 } else { 0.46 }
    } else if overlap == 1 {
        if ctx.seen_count == 0 { 0.66 } else { 0.36 }
    } else if ctx.has_exact_query_term_match {
        if ctx.seen_count == 0 { 0.58 } else { 0.32 }
    } else if ctx.wants_examples {
        if ctx.seen_count == 0 { 0.24 } else { 0.12 }
    } else if ctx.seen_count == 0 {
        0.18
    } else {
        0.10
    };

    Some(PolicyEffect::Add(delta))
}

fn bench_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses || !ctx.wants_benchmarks || !ctx.is_bench_support {
        return None;
    }

    let delta = if ctx.specific_witness_path_overlap >= 2 {
        if ctx.seen_count == 0 { 0.96 } else { 0.52 }
    } else if ctx.specific_witness_path_overlap == 1 {
        if ctx.seen_count == 0 { 0.76 } else { 0.42 }
    } else if ctx.has_exact_query_term_match {
        if ctx.seen_count == 0 { 0.64 } else { 0.36 }
    } else if ctx.seen_count == 0 {
        0.26
    } else {
        0.14
    };

    Some(PolicyEffect::Add(delta))
}

fn non_support_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_witnesses
        || !ctx.wants_example_or_bench_witnesses
        || ctx.class != HybridSourceClass::Tests
        || ctx.is_example_support
        || ctx.is_bench_support
        || ctx.specific_witness_path_overlap > 0
    {
        return None;
    }

    Some(PolicyEffect::Add(if ctx.wants_test_witness_recall {
        if ctx.seen_count == 0 { -0.18 } else { -0.10 }
    } else if ctx.seen_count == 0 {
        -0.34
    } else {
        -0.18
    }))
}

fn non_support_runtime_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.wants_example_or_bench_witnesses
        && ctx.class == HybridSourceClass::Runtime
        && !ctx.is_example_support
        && !ctx.is_bench_support)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -0.36
        } else {
            -0.22
        }))
}

fn examples_rs_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.wants_example_or_bench_witnesses
        && ctx.is_test_support
        && ctx.is_examples_rs)
        .then_some(PolicyEffect::Add(if ctx.seen_count == 0 {
            -1.10
        } else {
            -0.58
        }))
}

fn python_test_bridge_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.is_python_test_witness
        && ctx.runtime_seen > 0
        && ctx.seen_count == 0)
        .then_some(PolicyEffect::Add(0.18))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.runtime.first_runtime_bonus",
        PolicyStage::SelectionRuntimeWitness,
        first_runtime_bonus,
    ),
    ScoreRule::new(
        "selection.runtime.first_support_or_test_bonus",
        PolicyStage::SelectionRuntimeWitness,
        first_support_or_test_bonus,
    ),
    ScoreRule::new(
        "selection.runtime.identifier_anchor_bonus",
        PolicyStage::SelectionRuntimeWitness,
        identifier_anchor_bonus,
    ),
    ScoreRule::new(
        "selection.runtime.fixtures_penalty",
        PolicyStage::SelectionRuntimeWitness,
        fixtures_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.python_entrypoint_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        python_entrypoint_adjustment,
    ),
    ScoreRule::new(
        "selection.runtime.python_config_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        python_config_adjustment,
    ),
    ScoreRule::new(
        "selection.runtime.python_test_adjustment",
        PolicyStage::SelectionRuntimeWitness,
        python_test_adjustment,
    ),
    ScoreRule::new(
        "selection.runtime.loose_python_test_penalty",
        PolicyStage::SelectionRuntimeWitness,
        loose_python_test_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.path_overlap_bonus",
        PolicyStage::SelectionRuntimeWitness,
        path_overlap_bonus,
    ),
    ScoreRule::new(
        "selection.runtime.generic_doc_repeat_penalty",
        PolicyStage::SelectionRuntimeWitness,
        generic_doc_repeat_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.generic_doc_first_penalty",
        PolicyStage::SelectionRuntimeWitness,
        generic_doc_first_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.doc_path_overlap_penalty",
        PolicyStage::SelectionRuntimeWitness,
        doc_path_overlap_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.repo_metadata_penalty",
        PolicyStage::SelectionRuntimeWitness,
        repo_metadata_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.python_config_runtime_bonus",
        PolicyStage::SelectionRuntimeWitness,
        python_config_runtime_bonus,
    ),
    ScoreRule::new(
        "selection.runtime.generic_anchor_penalty",
        PolicyStage::SelectionRuntimeWitness,
        generic_anchor_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.missing_anchor_penalty",
        PolicyStage::SelectionRuntimeWitness,
        missing_anchor_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.frontend_noise_penalty",
        PolicyStage::SelectionRuntimeWitness,
        frontend_noise_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.example_support_bonus",
        PolicyStage::SelectionRuntimeWitness,
        example_support_bonus,
    ),
    ScoreRule::new(
        "selection.runtime.bench_support_bonus",
        PolicyStage::SelectionRuntimeWitness,
        bench_support_bonus,
    ),
    ScoreRule::new(
        "selection.runtime.non_support_test_penalty",
        PolicyStage::SelectionRuntimeWitness,
        non_support_test_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.non_support_runtime_penalty",
        PolicyStage::SelectionRuntimeWitness,
        non_support_runtime_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.examples_rs_penalty",
        PolicyStage::SelectionRuntimeWitness,
        examples_rs_penalty,
    ),
    ScoreRule::new(
        "selection.runtime.python_test_bridge_bonus",
        PolicyStage::SelectionRuntimeWitness,
        python_test_bridge_bonus,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.wants_runtime_witnesses {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
