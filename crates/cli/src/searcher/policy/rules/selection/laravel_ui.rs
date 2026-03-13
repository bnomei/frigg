use super::super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::super::facts::SelectionFacts;
use super::super::super::kernel::PolicyProgram;
use super::super::super::predicates::selection as pred;
use super::super::super::trace::{PolicyEffect, PolicyStage};
fn specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();
    if candidate.specific_witness_path_overlap() == 0 {
        return None;
    }

    let delta = match candidate.specific_witness_path_overlap() {
        1 => {
            if state.seen_count() == 0 {
                0.96
            } else {
                0.44
            }
        }
        2 => {
            if state.seen_count() == 0 {
                1.68
            } else {
                0.82
            }
        }
        _ => {
            if state.seen_count() == 0 {
                2.24
            } else {
                1.08
            }
        }
    };

    Some(PolicyEffect::Add(delta))
}

fn missing_specific_anchor_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let query = ctx.query();
    let state = ctx.state();
    (candidate.specific_witness_path_overlap() == 0
        && query.has_specific_blade_anchors()
        && (candidate.is_laravel_non_livewire_blade_view() || candidate.is_laravel_livewire_view()))
    .then_some(PolicyEffect::Add(if state.seen_count() == 0 {
        -0.62
    } else {
        -0.32
    }))
}

fn blade_specific_overlap_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    if candidate.blade_specific_path_overlap() == 0 {
        return None;
    }

    Some(PolicyEffect::Add(
        match candidate.blade_specific_path_overlap() {
            1 => 0.74,
            2 => 1.62,
            _ => 2.28,
        },
    ))
}

fn generic_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let candidate = ctx.candidate();
    let query = ctx.query();
    let state = ctx.state();
    (candidate.blade_specific_path_overlap() == 0
        && query.has_specific_blade_anchors()
        && candidate.is_laravel_blade_component()
        && !intent.wants_laravel_layout_witnesses())
    .then_some(PolicyEffect::Add(if state.seen_count() == 0 {
        -0.46
    } else {
        -0.22
    }))
}

fn form_action_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        1.42
    } else {
        0.54
    }))
}

fn form_action_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();
    (!candidate.is_laravel_form_action_blade() && candidate.is_laravel_blade_component()).then_some(
        PolicyEffect::Add(if state.seen_count() == 0 {
            -0.24
        } else {
            -0.12
        }),
    )
}

fn non_livewire_view_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        -0.34
    } else {
        -0.18
    }))
}

fn livewire_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.72
    } else {
        0.28
    }))
}

fn view_component_class_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let state = ctx.state();

    Some(PolicyEffect::Add(
        if intent.wants_laravel_layout_witnesses() {
            if state.seen_count() == 0 {
                -1.40
            } else {
                -1.80
            }
        } else if state.seen_count() == 0 {
            -1.00
        } else {
            -1.40
        },
    ))
}

fn livewire_component_blade_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let intent = ctx.intent();
    let state = ctx.state();

    Some(PolicyEffect::Add(
        if intent.wants_livewire_view_witnesses() {
            if state.seen_count() == 0 { 0.02 } else { -0.18 }
        } else if state.seen_count() == 0 {
            -0.54
        } else {
            -0.30
        },
    ))
}

fn non_livewire_blade_view_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.96
    } else {
        0.40
    }))
}

fn livewire_view_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.12
    } else {
        0.02
    }))
}

fn blade_component_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();

    Some(PolicyEffect::Add(
        if candidate.is_laravel_nested_blade_component() {
            if state.seen_count() == 0 { 0.24 } else { -0.04 }
        } else if state.seen_count() == 0 {
            1.48
        } else {
            0.60
        },
    ))
}

fn livewire_component_general_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.18
    } else {
        -0.18
    }))
}

fn non_livewire_blade_view_general_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        1.05
    } else {
        0.54
    }))
}

fn livewire_view_general_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.92
    } else {
        0.44
    }))
}

fn blade_component_general_bias(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();

    Some(PolicyEffect::Add(if candidate.path_overlap() >= 3 {
        if state.seen_count() == 0 { 0.72 } else { 0.26 }
    } else if state.seen_count() == 0 {
        0.10
    } else {
        -0.12
    }))
}

fn test_harness_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.42
    } else {
        0.18
    }))
}

fn command_middleware_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        1.18
    } else {
        0.48
    }))
}

fn job_listener_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.96
    } else {
        0.36
    }))
}

fn layout_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        1.80
    } else {
        0.76
    }))
}

fn layout_blade_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        0.78
    } else {
        0.32
    }))
}

fn blade_page_view_bonus(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        1.80
    } else {
        0.76
    }))
}

fn layout_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();
    Some(PolicyEffect::Add(
        if candidate.blade_specific_path_overlap() == 0
            && candidate.specific_witness_path_overlap() == 0
        {
            if state.seen_count() == 0 {
                -0.92
            } else {
                -0.44
            }
        } else if state.seen_count() == 0 {
            0.14
        } else {
            0.06
        },
    ))
}

fn repo_metadata_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();
    Some(PolicyEffect::Add(if state.seen_count() == 0 {
        -0.34
    } else {
        -0.20
    }))
}

fn surface_blade_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.laravel_surface_seen() == 0 {
        0.44
    } else {
        -0.18 * state.laravel_surface_seen() as f32
    }))
}

fn surface_livewire_component(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.laravel_surface_seen() == 0 {
        0.08
    } else {
        -0.12 * state.laravel_surface_seen() as f32
    }))
}

fn surface_livewire_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.laravel_surface_seen() == 0 {
        0.10
    } else {
        -0.12 * state.laravel_surface_seen() as f32
    }))
}

fn surface_blade_component(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let candidate = ctx.candidate();
    let state = ctx.state();

    Some(PolicyEffect::Add(
        if candidate.is_laravel_nested_blade_component() {
            if state.laravel_surface_seen() == 0 {
                0.10
            } else {
                -0.12 * state.laravel_surface_seen() as f32
            }
        } else if state.laravel_surface_seen() == 0 {
            0.96
        } else {
            0.34 - (0.08 * state.laravel_surface_seen() as f32)
        },
    ))
}

fn surface_blade_component_first_bonus(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(0.28))
}

fn surface_general_blade_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.laravel_surface_seen() == 0 {
        0.72
    } else {
        0.14
    }))
}

fn surface_first_blade_view_bonus(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(0.34))
}

fn surface_general_livewire_component(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.laravel_surface_seen() == 0 {
        0.18
    } else {
        -0.14 * state.laravel_surface_seen() as f32
    }))
}

fn surface_general_livewire_view(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.laravel_surface_seen() == 0 {
        0.84
    } else {
        0.18
    }))
}

fn surface_first_livewire_view_bonus(_ctx: &SelectionFacts) -> Option<PolicyEffect> {
    Some(PolicyEffect::Add(0.28))
}

fn surface_general_blade_component_penalty(ctx: &SelectionFacts) -> Option<PolicyEffect> {
    let state = ctx.state();

    Some(PolicyEffect::Add(if state.laravel_surface_seen() == 0 {
        -0.04
    } else {
        -(0.72 * state.laravel_surface_seen() as f32)
    }))
}

const RULES: &[ScoreRule<SelectionFacts>] = &[
    ScoreRule::when(
        "selection.laravel.specific_overlap_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::specific_witness_path_overlap_leaf(),
        ]),
        specific_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.missing_specific_anchor_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::query_has_specific_blade_anchors_leaf(),
            ],
            &[
                pred::is_laravel_non_livewire_blade_view_leaf(),
                pred::is_laravel_livewire_view_leaf(),
            ],
            &[],
        ),
        missing_specific_anchor_penalty,
    ),
    ScoreRule::when(
        "selection.laravel.blade_specific_overlap_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::blade_specific_path_overlap_leaf(),
        ]),
        blade_specific_overlap_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.generic_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::query_has_specific_blade_anchors_leaf(),
            pred::is_laravel_blade_component_leaf(),
        ]),
        generic_blade_component_penalty,
    ),
    ScoreRule::when(
        "selection.laravel.form_action_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_form_action_witnesses_leaf(),
            pred::is_laravel_form_action_blade_leaf(),
        ]),
        form_action_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.form_action_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_form_action_witnesses_leaf(),
            pred::is_laravel_blade_component_leaf(),
        ]),
        form_action_blade_component_penalty,
    ),
    ScoreRule::when(
        "selection.laravel.non_livewire_view_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_livewire_view_witnesses_leaf(),
            pred::is_laravel_non_livewire_blade_view_leaf(),
        ]),
        non_livewire_view_penalty,
    ),
    ScoreRule::when(
        "selection.laravel.livewire_view_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_livewire_view_witnesses_leaf(),
            pred::is_laravel_livewire_view_leaf(),
        ]),
        livewire_view_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.view_component_class_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_laravel_view_component_class_leaf(),
        ]),
        view_component_class_penalty,
    ),
    ScoreRule::when(
        "selection.laravel.livewire_component_bias",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::is_laravel_livewire_component_leaf(),
        ]),
        livewire_component_blade_bias,
    ),
    ScoreRule::when(
        "selection.laravel.non_livewire_blade_view_bias",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::is_laravel_non_livewire_blade_view_leaf(),
        ]),
        non_livewire_blade_view_bias,
    ),
    ScoreRule::when(
        "selection.laravel.livewire_view_bias",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::is_laravel_livewire_view_leaf(),
        ]),
        livewire_view_bias,
    ),
    ScoreRule::when(
        "selection.laravel.blade_component_bias",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::is_laravel_blade_component_leaf(),
        ]),
        blade_component_bias,
    ),
    ScoreRule::when(
        "selection.laravel.livewire_component_general_bias",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_livewire_component_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        livewire_component_general_bias,
    ),
    ScoreRule::when(
        "selection.laravel.non_livewire_blade_view_general_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_non_livewire_blade_view_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        non_livewire_blade_view_general_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.livewire_view_general_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_livewire_view_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        livewire_view_general_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.blade_component_general_bias",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_blade_component_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        blade_component_general_bias,
    ),
    ScoreRule::when(
        "selection.laravel.test_harness_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_test_harness_leaf(),
        ]),
        test_harness_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.command_middleware_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_commands_middleware_witnesses_leaf(),
            pred::is_laravel_command_or_middleware_leaf(),
        ]),
        command_middleware_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.job_listener_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_jobs_listeners_witnesses_leaf(),
            pred::is_laravel_job_or_listener_leaf(),
        ]),
        job_listener_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.layout_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::is_laravel_layout_blade_view_leaf(),
        ]),
        layout_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.layout_blade_view_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::specific_witness_path_overlap_leaf(),
            pred::is_laravel_non_livewire_blade_view_leaf(),
        ]),
        layout_blade_view_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.blade_page_view_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_non_livewire_blade_view_leaf(),
            ],
            &[],
            &[
                pred::wants_laravel_form_action_witnesses_leaf(),
                pred::wants_laravel_layout_witnesses_leaf(),
            ],
        ),
        blade_page_view_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.layout_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::is_laravel_blade_component_leaf(),
        ]),
        layout_blade_component_penalty,
    ),
    ScoreRule::when(
        "selection.laravel.repo_metadata_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        repo_metadata_penalty,
    ),
    ScoreRule::when(
        "selection.laravel.surface.blade_view",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::laravel_surface_is_blade_view_leaf(),
        ]),
        surface_blade_view,
    ),
    ScoreRule::when(
        "selection.laravel.surface.livewire_component",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::laravel_surface_is_livewire_component_leaf(),
        ]),
        surface_livewire_component,
    ),
    ScoreRule::when(
        "selection.laravel.surface.livewire_view",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::laravel_surface_is_livewire_view_leaf(),
        ]),
        surface_livewire_view,
    ),
    ScoreRule::when(
        "selection.laravel.surface.blade_component",
        PolicyStage::SelectionLaravelUi,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::laravel_surface_is_blade_component_leaf(),
        ]),
        surface_blade_component,
    ),
    ScoreRule::when(
        "selection.laravel.surface.blade_component_first_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::wants_blade_component_witnesses_leaf(),
                pred::laravel_surface_is_blade_component_leaf(),
                pred::laravel_surface_seen_is_zero_leaf(),
            ],
            &[],
            &[pred::is_laravel_nested_blade_component_leaf()],
        ),
        surface_blade_component_first_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.surface.general_blade_view",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::laravel_surface_is_blade_view_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        surface_general_blade_view,
    ),
    ScoreRule::when(
        "selection.laravel.surface.first_blade_view_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::laravel_surface_is_blade_view_leaf(),
                pred::laravel_surface_seen_is_zero_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        surface_first_blade_view_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.surface.general_livewire_component",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::laravel_surface_is_livewire_component_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        surface_general_livewire_component,
    ),
    ScoreRule::when(
        "selection.laravel.surface.general_livewire_view",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::laravel_surface_is_livewire_view_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        surface_general_livewire_view,
    ),
    ScoreRule::when(
        "selection.laravel.surface.first_livewire_view_bonus",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::laravel_surface_is_livewire_view_leaf(),
                pred::laravel_surface_seen_is_zero_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        surface_first_livewire_view_bonus,
    ),
    ScoreRule::when(
        "selection.laravel.surface.general_blade_component_penalty",
        PolicyStage::SelectionLaravelUi,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::laravel_surface_is_blade_component_leaf(),
            ],
            &[],
            &[pred::wants_blade_component_witnesses_leaf()],
        ),
        surface_general_blade_component_penalty,
    ),
];

const RULE_SET: ScoreRuleSet<SelectionFacts> = ScoreRuleSet::new(RULES);

pub(crate) fn apply(program: &mut PolicyProgram, ctx: &SelectionFacts) {
    apply_score_rule_sets(program, ctx, &[RULE_SET]);
}
