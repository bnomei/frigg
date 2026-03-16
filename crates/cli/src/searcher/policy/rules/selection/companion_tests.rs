use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};

fn runtime_anchor_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        if ctx.seen_plain_test_support == 0 {
            2.10
        } else {
            1.22
        }
    } else if ctx.seen_plain_test_support == 0 {
        0.92
    } else {
        0.48
    }))
}

fn runtime_adjacent_python_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        if ctx.seen_plain_test_support == 0 {
            2.64
        } else {
            1.48
        }
    } else if ctx.seen_plain_test_support == 0 {
        1.20
    } else {
        0.68
    }))
}

fn cli_or_harness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
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

fn cli_test_support_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        if ctx.seen_plain_test_support == 0 {
            1.36
        } else {
            0.82
        }
    } else if ctx.seen_plain_test_support == 0 {
        0.62
    } else {
        0.34
    }))
}

fn family_affinity_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
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

fn package_family_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        0.46
    } else if ctx.seen_plain_test_support == 0 {
        1.10
    } else {
        0.62
    }))
}

fn non_prefix_python_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        -0.18
    } else {
        0.12
    }))
}

fn deeper_path_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = match ctx.path_depth {
        0..=3 => 0.0,
        4 => 0.08,
        5 => 0.14,
        _ => 0.20,
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

fn unanchored_plain_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.seen_plain_test_support == 0 {
        -0.84
    } else {
        -0.44
    }))
}

fn cross_family_plain_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        -0.72
    } else {
        -1.40
    }))
}

fn shallow_family_plain_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        -0.28
    } else if ctx.runtime_family_prefix_overlap == 1 {
        -0.82
    } else {
        -0.48
    }))
}

fn scripts_ops_plain_test_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        -0.54
    } else {
        -1.36
    }))
}

fn same_language_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        if ctx.seen_plain_test_support == 0 {
            0.30
        } else {
            0.18
        }
    } else if ctx.seen_plain_test_support == 0 {
        0.18
    } else {
        0.10
    }))
}

fn language_mismatch_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(if ctx.prefer_runtime_anchor_tests {
        if ctx.seen_plain_test_support == 0 {
            -0.42
        } else {
            -0.24
        }
    } else if ctx.seen_plain_test_support == 0 {
        -0.24
    } else {
        -0.14
    }))
}

fn subtree_affinity_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let delta = if ctx.runtime_subtree_affinity >= 2 {
        if ctx.prefer_runtime_anchor_tests {
            0.74
        } else {
            0.38
        }
    } else if ctx.runtime_subtree_affinity > 0 {
        if ctx.prefer_runtime_anchor_tests {
            0.34
        } else {
            0.18
        }
    } else {
        0.0
    };

    (delta > 0.0).then_some(PolicyEffect::Add(delta))
}

const CLI_OR_HARNESS_ANY: &[super::super::super::dsl::PredicateLeaf<SelectionFacts>] = &[
    pred::is_cli_test_support_leaf(),
    pred::is_test_harness_leaf(),
];

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.companion.runtime_anchor_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::is_runtime_anchor_test_support_leaf(),
        ]),
        runtime_anchor_bonus,
    ),
    ScoreRule::when(
        "selection.companion.runtime_adjacent_python_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::is_runtime_adjacent_python_test_leaf(),
        ]),
        runtime_adjacent_python_bonus,
    ),
    ScoreRule::when(
        "selection.companion.cli_or_harness_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::new(
            &[pred::wants_runtime_companion_tests_leaf()],
            CLI_OR_HARNESS_ANY,
            &[],
        ),
        cli_or_harness_bonus,
    ),
    ScoreRule::when(
        "selection.companion.cli_test_support_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::is_cli_test_support_leaf(),
        ]),
        cli_test_support_bonus,
    ),
    ScoreRule::when(
        "selection.companion.same_language_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::wants_language_locality_bias_leaf(),
            pred::candidate_language_known_leaf(),
            pred::matches_query_language_leaf(),
            pred::is_test_support_leaf(),
        ]),
        same_language_bonus,
    ),
    ScoreRule::when(
        "selection.companion.language_mismatch_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::new(
            &[
                pred::wants_runtime_companion_tests_leaf(),
                pred::wants_language_locality_bias_leaf(),
                pred::candidate_language_known_leaf(),
                pred::is_test_support_leaf(),
            ],
            &[],
            &[pred::matches_query_language_leaf()],
        ),
        language_mismatch_penalty,
    ),
    ScoreRule::when(
        "selection.companion.subtree_affinity_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::is_test_support_leaf(),
            pred::runtime_subtree_affinity_positive_leaf(),
        ]),
        subtree_affinity_bonus,
    ),
    ScoreRule::when(
        "selection.companion.family_affinity_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::is_test_support_leaf(),
        ]),
        family_affinity_bonus,
    ),
    ScoreRule::when(
        "selection.companion.package_family_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::new(
            &[
                pred::wants_runtime_companion_tests_leaf(),
                pred::is_test_support_leaf(),
                pred::runtime_seen_positive_leaf(),
                pred::runtime_family_prefix_overlap_at_least_four_leaf(),
            ],
            &[],
            &[
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
            ],
        ),
        package_family_bonus,
    ),
    ScoreRule::when(
        "selection.companion.non_prefix_python_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::is_runtime_adjacent_python_test_leaf(),
            pred::is_non_prefix_python_test_module_leaf(),
        ]),
        non_prefix_python_bonus,
    ),
    ScoreRule::when(
        "selection.companion.deeper_path_bonus",
        PolicyStage::SelectionTestWitness,
        Predicate::all(&[
            pred::wants_runtime_companion_tests_leaf(),
            pred::is_test_support_leaf(),
            pred::path_depth_at_least_four_leaf(),
        ]),
        deeper_path_bonus,
    ),
    ScoreRule::when(
        "selection.companion.unanchored_plain_test_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::new(
            &[
                pred::prefer_runtime_anchor_tests_leaf(),
                pred::is_test_support_leaf(),
                pred::runtime_family_prefix_overlap_is_zero_leaf(),
            ],
            &[],
            &[
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
                pred::is_runtime_anchor_test_support_leaf(),
                pred::is_runtime_adjacent_python_test_leaf(),
                pred::is_cli_test_support_leaf(),
                pred::is_test_harness_leaf(),
            ],
        ),
        unanchored_plain_test_penalty,
    ),
    ScoreRule::when(
        "selection.companion.cross_family_plain_test_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::new(
            &[
                pred::wants_runtime_companion_tests_leaf(),
                pred::is_test_support_leaf(),
                pred::runtime_family_prefix_overlap_is_zero_leaf(),
            ],
            &[],
            &[
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
                pred::is_cli_test_support_leaf(),
                pred::is_test_harness_leaf(),
                pred::is_runtime_adjacent_python_test_leaf(),
            ],
        ),
        cross_family_plain_test_penalty,
    ),
    ScoreRule::when(
        "selection.companion.shallow_family_plain_test_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::new(
            &[
                pred::wants_runtime_companion_tests_leaf(),
                pred::is_test_support_leaf(),
                pred::runtime_seen_positive_leaf(),
                pred::runtime_family_prefix_overlap_one_or_two_leaf(),
            ],
            &[],
            &[
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
                pred::is_cli_test_support_leaf(),
                pred::is_test_harness_leaf(),
                pred::is_runtime_adjacent_python_test_leaf(),
            ],
        ),
        shallow_family_plain_test_penalty,
    ),
    ScoreRule::when(
        "selection.companion.scripts_ops_plain_test_penalty",
        PolicyStage::SelectionTestWitness,
        Predicate::new(
            &[
                pred::wants_runtime_companion_tests_leaf(),
                pred::is_test_support_leaf(),
                pred::is_scripts_ops_leaf(),
            ],
            &[],
            &[pred::wants_scripts_ops_witnesses_leaf()],
        ),
        scripts_ops_plain_test_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
