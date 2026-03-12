use super::super::super::dsl::{ScoreRule, apply_score_rules};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn runtime_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_companion_tests && ctx.is_runtime_anchor_test_support).then_some(
        PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
            if ctx.seen_plain_test_support == 0 {
                2.10
            } else {
                1.22
            }
        } else if ctx.seen_plain_test_support == 0 {
            0.92
        } else {
            0.48
        }),
    )
}

fn runtime_adjacent_python_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_companion_tests && ctx.is_runtime_adjacent_python_test).then_some(
        PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
            if ctx.seen_plain_test_support == 0 {
                2.64
            } else {
                1.48
            }
        } else if ctx.seen_plain_test_support == 0 {
            1.20
        } else {
            0.68
        }),
    )
}

fn cli_or_harness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_companion_tests && (ctx.is_cli_test_support || ctx.is_test_harness))
        .then_some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
            if ctx.seen_plain_test_support == 0 {
                0.92
            } else {
                0.54
            }
        } else if ctx.seen_plain_test_support == 0 {
            0.44
        } else {
            0.24
        }))
}

fn family_affinity_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_companion_tests || !ctx.is_test_support {
        return None;
    }

    let delta = match ctx.runtime_family_prefix_overlap {
        0 => 0.0,
        1 => 0.08,
        2 => {
            if ctx.prefer_runtime_anchor_tests {
                0.16
            } else {
                0.52
            }
        }
        3 => {
            if ctx.prefer_runtime_anchor_tests {
                0.34
            } else {
                1.20
            }
        }
        4 => {
            if ctx.prefer_runtime_anchor_tests {
                0.58
            } else {
                1.80
            }
        }
        _ => {
            if ctx.prefer_runtime_anchor_tests {
                0.82
            } else {
                2.20
            }
        }
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn non_prefix_python_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_companion_tests
        && ctx.is_runtime_adjacent_python_test
        && ctx.is_non_prefix_python_test_module)
        .then_some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
            -0.18
        } else {
            0.12
        }))
}

fn deeper_path_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    if !ctx.wants_runtime_companion_tests || !ctx.is_test_support || ctx.path_depth < 3 {
        return None;
    }

    let delta = match ctx.path_depth {
        0..=3 => 0.0,
        4 => 0.08,
        5 => 0.14,
        _ => 0.20,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn unanchored_plain_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.prefer_runtime_anchor_tests
        && ctx.is_test_support
        && !ctx.is_example_support
        && !ctx.is_bench_support
        && !ctx.is_runtime_anchor_test_support
        && !ctx.is_runtime_adjacent_python_test
        && !ctx.is_cli_test_support
        && !ctx.is_test_harness
        && ctx.runtime_family_prefix_overlap == 0)
        .then_some(PolicyEffect::Add(if ctx.seen_plain_test_support == 0 {
            -0.84
        } else {
            -0.44
        }))
}

fn cross_family_plain_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_companion_tests
        && ctx.is_test_support
        && !ctx.is_example_support
        && !ctx.is_bench_support
        && !ctx.is_cli_test_support
        && !ctx.is_test_harness
        && !ctx.is_runtime_adjacent_python_test
        && ctx.runtime_family_prefix_overlap == 0)
        .then_some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
            -0.72
        } else {
            -1.40
        }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::new(
        "selection.companion.runtime_anchor_bonus",
        PolicyStage::SelectionTestWitness,
        runtime_anchor_bonus,
    ),
    ScoreRule::new(
        "selection.companion.runtime_adjacent_python_bonus",
        PolicyStage::SelectionTestWitness,
        runtime_adjacent_python_bonus,
    ),
    ScoreRule::new(
        "selection.companion.cli_or_harness_bonus",
        PolicyStage::SelectionTestWitness,
        cli_or_harness_bonus,
    ),
    ScoreRule::new(
        "selection.companion.family_affinity_bonus",
        PolicyStage::SelectionTestWitness,
        family_affinity_bonus,
    ),
    ScoreRule::new(
        "selection.companion.non_prefix_python_bonus",
        PolicyStage::SelectionTestWitness,
        non_prefix_python_bonus,
    ),
    ScoreRule::new(
        "selection.companion.deeper_path_bonus",
        PolicyStage::SelectionTestWitness,
        deeper_path_bonus,
    ),
    ScoreRule::new(
        "selection.companion.unanchored_plain_test_penalty",
        PolicyStage::SelectionTestWitness,
        unanchored_plain_test_penalty,
    ),
    ScoreRule::new(
        "selection.companion.cross_family_plain_test_penalty",
        PolicyStage::SelectionTestWitness,
        cross_family_plain_test_penalty,
    ),
];

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    if !ctx.wants_runtime_companion_tests {
        return;
    }

    apply_score_rules(program, ctx, RULES);
}
