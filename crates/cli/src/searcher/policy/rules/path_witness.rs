use super::super::dsl::{
    Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets, predicate_matches,
};
use super::super::facts::PathWitnessFacts;
use super::super::kernel::PolicyProgram;
use super::super::predicates::path_witness as pred;
use super::super::trace::{PolicyEffect, PolicyStage};

mod base;
mod examples;
mod laravel;
mod runtime_config;
mod source;

const PATH_WITNESS_ELIGIBILITY_ANY: &[super::super::dsl::PredicateLeaf<PathWitnessFacts>] = &[
    pred::path_overlap_leaf(),
    pred::specific_path_overlap_leaf(),
    pred::is_entrypoint_leaf(),
    pred::is_entrypoint_build_workflow_leaf(),
    pred::is_ci_workflow_leaf(),
    pred::is_config_artifact_leaf(),
    pred::is_package_surface_leaf(),
    pred::is_build_config_surface_leaf(),
    pred::is_workspace_config_surface_leaf(),
    pred::is_typescript_runtime_module_index_leaf(),
    pred::is_python_config_leaf(),
    pred::is_python_test_leaf(),
    pred::is_test_support_leaf(),
    pred::is_runtime_anchor_test_support_leaf(),
    pred::is_example_support_leaf(),
    pred::is_bench_support_leaf(),
    pred::is_cli_test_leaf(),
    pred::is_test_harness_leaf(),
    pred::is_scripts_ops_leaf(),
    pred::is_kotlin_android_ui_runtime_surface_leaf(),
];

const PATH_WITNESS_ELIGIBILITY: Predicate<PathWitnessFacts> =
    Predicate::any(PATH_WITNESS_ELIGIBILITY_ANY);

use base::*;
use examples::*;
use laravel::*;
use runtime_config::*;
use source::*;

const SCORE_RULES: &[ScoreRule<PathWitnessFacts>] = &[
    ScoreRule::when(
        "path_witness.specific_overlap_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::specific_path_overlap_leaf()]),
        path_witness_specific_overlap_bonus,
    ),
    ScoreRule::when(
        "path_witness.entrypoint_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::is_entrypoint_leaf()]),
        path_witness_entrypoint_bonus,
    ),
    ScoreRule::when(
        "path_witness.build_flow_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_leaf(),
        ]),
        path_witness_build_flow_bonus,
    ),
    ScoreRule::when(
        "path_witness.workflow_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::is_entrypoint_build_workflow_leaf()]),
        path_witness_workflow_bonus,
    ),
    ScoreRule::when(
        "path_witness.ci_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::is_ci_workflow_leaf()]),
        path_witness_ci_bonus,
    ),
    ScoreRule::when(
        "laravel.livewire_view_focus_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_livewire_view_witnesses_leaf(),
            pred::is_laravel_livewire_view_leaf(),
        ]),
        laravel_livewire_view_focus_bonus,
    ),
    ScoreRule::when(
        "laravel.non_livewire_view_penalty",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_livewire_view_witnesses_leaf(),
            pred::is_laravel_non_livewire_blade_view_leaf(),
        ]),
        laravel_non_livewire_view_penalty,
    ),
    ScoreRule::when(
        "laravel.command_middleware_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_commands_middleware_witnesses_leaf(),
            pred::is_laravel_command_or_middleware_leaf(),
        ]),
        laravel_command_middleware_bonus,
    ),
    ScoreRule::when(
        "laravel.job_listener_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_jobs_listeners_witnesses_leaf(),
            pred::is_laravel_job_or_listener_leaf(),
        ]),
        laravel_job_listener_bonus,
    ),
    ScoreRule::when(
        "entrypoint.laravel_route_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_laravel_route_leaf(),
        ]),
        entrypoint_laravel_route_bonus,
    ),
    ScoreRule::when(
        "entrypoint.laravel_bootstrap_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_laravel_bootstrap_entrypoint_leaf(),
        ]),
        entrypoint_laravel_bootstrap_bonus,
    ),
    ScoreRule::when(
        "entrypoint.laravel_core_provider_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_laravel_core_provider_leaf(),
        ]),
        entrypoint_laravel_core_provider_bonus,
    ),
    ScoreRule::when(
        "entrypoint.laravel_provider_bonus",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_entrypoint_build_flow_leaf(),
                pred::is_laravel_provider_leaf(),
            ],
            &[],
            &[pred::is_laravel_core_provider_leaf()],
        ),
        entrypoint_laravel_provider_bonus,
    ),
    ScoreRule::when(
        "runtime_config.artifact_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_config_artifact_leaf(),
        ]),
        runtime_config_artifact_bonus,
    ),
    ScoreRule::when(
        "runtime_config.repo_root_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        runtime_config_repo_root_bonus,
    ),
    ScoreRule::when(
        "entrypoint.repo_root_runtime_config_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        entrypoint_repo_root_runtime_config_bonus,
    ),
    ScoreRule::when(
        "workspace.rust_config_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_rust_workspace_config_leaf(),
            pred::is_rust_workspace_config_leaf(),
        ]),
        workspace_rust_config_bonus,
    ),
    ScoreRule::when(
        "examples.support_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::wants_examples_leaf(), pred::is_example_support_leaf()]),
        examples_support_bonus,
    ),
    ScoreRule::when(
        "benchmarks.support_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::wants_benchmarks_leaf(), pred::is_bench_support_leaf()]),
        benchmarks_support_bonus,
    ),
    ScoreRule::when(
        "laravel.ui_harness_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_test_harness_leaf(),
        ]),
        laravel_ui_harness_bonus,
    ),
    ScoreRule::when(
        "scripts.ops_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::is_scripts_ops_leaf()]),
        scripts_ops_bonus,
    ),
    ScoreRule::when(
        "tests.exact_query_match_bonus",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_test_witness_recall_leaf(),
                pred::has_exact_query_term_match_leaf(),
            ],
            &[],
            &[pred::mixed_example_or_bench_examples_rs_leaf()],
        ),
        tests_exact_query_match_bonus,
    ),
    ScoreRule::when(
        "kotlin.ui_runtime_surface_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_kotlin_android_ui_witnesses_leaf(),
            pred::is_kotlin_android_ui_runtime_surface_leaf(),
        ]),
        kotlin_android_ui_runtime_surface_bonus,
    ),
    ScoreRule::when(
        "scripts.exact_query_match_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::is_scripts_ops_leaf(),
            pred::has_exact_query_term_match_leaf(),
        ]),
        scripts_exact_query_match_bonus,
    ),
    ScoreRule::when(
        "runtime_config.test_support_penalty",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_runtime_config_artifacts_leaf(),
                pred::is_test_support_leaf(),
            ],
            &[],
            &[
                pred::is_config_artifact_leaf(),
                pred::is_runtime_anchor_test_support_leaf(),
            ],
        ),
        runtime_config_test_support_penalty,
    ),
    ScoreRule::when(
        "runtime_config.test_tree_harness_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_test_harness_leaf(),
            pred::path_overlap_leaf(),
        ]),
        runtime_config_test_tree_harness_bonus,
    ),
    ScoreRule::when(
        "runtime_focus.ci_workflow_penalty",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::is_ci_workflow_leaf(),
                pred::wants_entrypoint_or_runtime_config_or_test_leaf(),
            ],
            &[],
            &[pred::wants_ci_workflow_witnesses_leaf()],
        ),
        runtime_focus_ci_workflow_penalty,
    ),
    ScoreRule::when(
        "runtime_focus.example_support_penalty",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::is_example_support_leaf(),
                pred::wants_entrypoint_or_runtime_config_or_test_leaf(),
            ],
            &[],
            &[pred::wants_examples_leaf()],
        ),
        runtime_focus_example_support_penalty,
    ),
    ScoreRule::when(
        "examples.unwanted_example_support_penalty",
        PolicyStage::PathWitness,
        Predicate::new(
            &[pred::is_example_support_leaf()],
            &[],
            &[
                pred::wants_examples_leaf(),
                pred::path_overlap_leaf(),
                pred::specific_path_overlap_leaf(),
                pred::has_exact_query_term_match_leaf(),
            ],
        ),
        examples_unwanted_example_support_penalty,
    ),
    ScoreRule::when(
        "cli.test_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::query_mentions_cli_leaf(), pred::is_cli_test_leaf()]),
        cli_test_bonus,
    ),
    ScoreRule::when(
        "source.runtime_support_tests_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::source_class_is_runtime_support_tests_leaf()]),
        source_runtime_support_tests_bonus,
    ),
    ScoreRule::when(
        "source.frontend_noise_penalty",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::is_frontend_runtime_noise_leaf()]),
        source_frontend_noise_penalty,
    ),
    ScoreRule::when(
        "laravel.blade_view_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_laravel_non_livewire_blade_view_leaf(),
        ]),
        laravel_blade_view_bonus,
    ),
    ScoreRule::when(
        "laravel.top_level_blade_bonus",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_top_level_blade_view_leaf(),
            ],
            &[],
            &[
                pred::wants_laravel_form_action_witnesses_leaf(),
                pred::wants_laravel_layout_witnesses_leaf(),
            ],
        ),
        laravel_top_level_blade_bonus,
    ),
    ScoreRule::when(
        "laravel.top_level_blade_specific_overlap_bonus",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_top_level_blade_view_leaf(),
                pred::specific_path_overlap_leaf(),
            ],
            &[],
            &[
                pred::wants_laravel_form_action_witnesses_leaf(),
                pred::wants_laravel_layout_witnesses_leaf(),
            ],
        ),
        laravel_top_level_blade_specific_overlap_bonus,
    ),
    ScoreRule::when(
        "laravel.partial_view_penalty",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::is_laravel_partial_view_leaf(),
                pred::is_laravel_non_livewire_blade_view_leaf(),
            ],
            &[],
            &[
                pred::wants_laravel_form_action_witnesses_leaf(),
                pred::wants_laravel_layout_witnesses_leaf(),
            ],
        ),
        laravel_partial_view_penalty,
    ),
    ScoreRule::when(
        "laravel.form_action_blade_component_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_form_action_witnesses_leaf(),
            pred::is_laravel_blade_component_leaf(),
            pred::is_laravel_form_action_blade_leaf(),
        ]),
        laravel_form_action_blade_component_bonus,
    ),
    ScoreRule::when(
        "laravel.blade_component_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_laravel_blade_component_leaf(),
        ]),
        laravel_blade_component_bonus,
    ),
    ScoreRule::when(
        "laravel.form_action_bonus",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_laravel_form_action_witnesses_leaf(),
                pred::is_laravel_form_action_blade_leaf(),
            ],
            &[],
            &[pred::is_laravel_blade_component_leaf()],
        ),
        laravel_form_action_bonus,
    ),
    ScoreRule::when(
        "laravel.livewire_component_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_laravel_livewire_component_leaf(),
        ]),
        laravel_livewire_component_bonus,
    ),
    ScoreRule::when(
        "laravel.view_component_class_penalty",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_laravel_view_component_class_leaf(),
        ]),
        laravel_view_component_class_penalty,
    ),
    ScoreRule::when(
        "laravel.layout_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::is_laravel_layout_blade_view_leaf(),
        ]),
        laravel_layout_bonus,
    ),
    ScoreRule::when(
        "laravel.missing_specific_anchor_penalty",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_laravel_ui_witnesses_leaf(),
                pred::has_specific_query_terms_leaf(),
            ],
            &[
                pred::is_laravel_non_livewire_blade_view_leaf(),
                pred::is_laravel_livewire_view_leaf(),
            ],
            &[pred::specific_path_overlap_leaf()],
        ),
        laravel_missing_specific_anchor_penalty,
    ),
    ScoreRule::when(
        "runtime_config.entrypoint_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_leaf(),
        ]),
        runtime_config_entrypoint_bonus,
    ),
    ScoreRule::when(
        "runtime_config.server_cli_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_leaf(),
            pred::path_stem_is_server_or_cli_leaf(),
        ]),
        runtime_config_server_cli_bonus,
    ),
    ScoreRule::when(
        "runtime_config.main_penalty",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_leaf(),
            pred::path_stem_is_main_leaf(),
        ]),
        runtime_config_main_penalty,
    ),
    ScoreRule::when(
        "runtime_config.typescript_index_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        runtime_config_typescript_index_bonus_group,
    ),
    ScoreRule::when(
        "runtime_config.package_surface_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_package_surface_leaf(),
        ]),
        runtime_config_package_surface_bonus,
    ),
    ScoreRule::when(
        "runtime_config.build_surface_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_build_config_surface_leaf(),
        ]),
        runtime_config_build_surface_bonus,
    ),
    ScoreRule::when(
        "runtime_config.workspace_surface_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_workspace_config_surface_leaf(),
        ]),
        runtime_config_workspace_surface_bonus,
    ),
    ScoreRule::when(
        "entrypoint.config_artifact_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_config_artifact_leaf(),
        ]),
        entrypoint_config_artifact_bonus,
    ),
    ScoreRule::when(
        "entrypoint.package_surface_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_package_surface_leaf(),
        ]),
        entrypoint_package_surface_bonus,
    ),
    ScoreRule::when(
        "entrypoint.build_surface_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_build_config_surface_leaf(),
        ]),
        entrypoint_build_surface_bonus,
    ),
    ScoreRule::when(
        "entrypoint.workspace_surface_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_workspace_config_surface_leaf(),
        ]),
        entrypoint_workspace_surface_bonus,
    ),
    ScoreRule::when(
        "entrypoint.typescript_index_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        entrypoint_typescript_index_bonus_group,
    ),
    ScoreRule::when(
        "workspace.python_config_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::is_python_config_leaf()]),
        workspace_python_config_bonus,
    ),
    ScoreRule::when(
        "workspace.python_test_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[pred::is_python_test_leaf()]),
        workspace_python_test_bonus,
    ),
    ScoreRule::when(
        "tests.runtime_adjacent_python_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_or_runtime_config_or_test_leaf(),
            pred::is_runtime_adjacent_python_test_leaf(),
        ]),
        runtime_adjacent_python_test_bonus,
    ),
    ScoreRule::when(
        "tests.support_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_test_witness_recall_leaf(),
            pred::is_test_support_leaf(),
        ]),
        tests_support_bonus,
    ),
    ScoreRule::when(
        "tests.runtime_anchor_support_bonus",
        PolicyStage::PathWitness,
        Predicate::all(&[
            pred::wants_entrypoint_or_runtime_config_leaf(),
            pred::is_runtime_anchor_test_support_leaf(),
        ]),
        runtime_anchor_test_support_bonus,
    ),
    ScoreRule::when(
        "tests.support_path_overlap_bonus",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_test_witness_recall_leaf(),
                pred::is_test_support_leaf(),
            ],
            &[],
            &[
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
            ],
        ),
        tests_support_path_overlap_bonus,
    ),
    ScoreRule::when(
        "examples_or_bench.non_support_test_penalty",
        PolicyStage::PathWitness,
        Predicate::new(
            &[
                pred::wants_example_or_bench_witnesses_leaf(),
                pred::is_test_support_leaf(),
            ],
            &[],
            &[
                pred::is_python_test_leaf(),
                pred::is_example_support_leaf(),
                pred::is_bench_support_leaf(),
            ],
        ),
        examples_or_bench_non_support_test_penalty,
    ),
];

pub(crate) const RULE_SET: ScoreRuleSet<PathWitnessFacts> = ScoreRuleSet::new(SCORE_RULES);

pub(crate) fn evaluate(
    ctx: &PathWitnessFacts,
    trace: bool,
) -> Option<super::super::trace::PolicyEvaluation> {
    if !predicate_matches(ctx, PATH_WITNESS_ELIGIBILITY) {
        return None;
    }

    let mut program = PolicyProgram::with_optional_trace(ctx.path_overlap as f32, trace);
    apply_score_rule_sets(&mut program, ctx, &[RULE_SET]);
    let evaluation = program.finish();
    (evaluation.score > 0.0).then_some(evaluation)
}

pub(crate) fn score(ctx: &PathWitnessFacts) -> Option<f32> {
    evaluate(ctx, false).map(|evaluation| evaluation.score)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::surfaces::HybridSourceClass;

    fn trace_rule_ids(
        evaluation: &super::super::super::trace::PolicyEvaluation,
    ) -> Vec<&'static str> {
        evaluation
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .map(|rule| rule.rule_id)
            .collect()
    }

    fn trace_rule<'a>(
        evaluation: &'a super::super::super::trace::PolicyEvaluation,
        rule_id: &'static str,
    ) -> &'a super::super::super::trace::PolicyRuleTrace {
        evaluation
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .find(|rule| rule.rule_id == rule_id)
            .expect("rule trace should exist")
    }

    #[test]
    fn policy_trace_path_witness_entrypoint_typescript_runtime_config_stack() {
        let ctx = PathWitnessFacts {
            path_overlap: 1,
            specific_path_overlap: 1,
            is_entrypoint: true,
            is_typescript_runtime_module_index: true,
            wants_entrypoint_build_flow: true,
            wants_runtime_config_artifacts: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"path_witness.specific_overlap_bonus"));
        assert!(rule_ids.contains(&"path_witness.entrypoint_bonus"));
        assert!(rule_ids.contains(&"path_witness.build_flow_bonus"));
        assert!(rule_ids.contains(&"runtime_config.typescript_index_bonus"));
        assert!(rule_ids.contains(&"entrypoint.typescript_index_bonus"));
        assert_eq!(
            trace_rule(&evaluation, "path_witness.specific_overlap_bonus").predicate_ids,
            vec!["candidate.specific_path_overlap"],
        );
    }

    #[test]
    fn policy_trace_path_witness_laravel_blade_component_focus_and_missing_anchor_penalty() {
        let ctx = PathWitnessFacts {
            path_overlap: 1,
            wants_laravel_ui_witnesses: true,
            wants_blade_component_witnesses: true,
            has_specific_query_terms: true,
            is_laravel_non_livewire_blade_view: true,
            is_laravel_blade_component: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"laravel.blade_view_bonus"));
        assert!(rule_ids.contains(&"laravel.blade_component_bonus"));
        assert!(rule_ids.contains(&"laravel.missing_specific_anchor_penalty"));
        assert_eq!(
            trace_rule(&evaluation, "laravel.missing_specific_anchor_penalty").predicate_ids,
            vec![
                "intent.laravel_ui_witnesses",
                "query.has_specific_query_terms",
                "candidate.laravel_non_livewire_blade_view",
            ],
        );
    }

    #[test]
    fn policy_trace_path_witness_test_support_bonus_and_example_crowding_penalty() {
        let ctx = PathWitnessFacts {
            path_overlap: 3,
            source_class: HybridSourceClass::Tests,
            wants_test_witness_recall: true,
            wants_example_or_bench_witnesses: true,
            has_exact_query_term_match: true,
            is_test_support: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"tests.exact_query_match_bonus"));
        assert!(rule_ids.contains(&"tests.support_bonus"));
        assert!(rule_ids.contains(&"tests.support_path_overlap_bonus"));
        assert!(rule_ids.contains(&"examples_or_bench.non_support_test_penalty"));
        assert_eq!(
            trace_rule(&evaluation, "examples_or_bench.non_support_test_penalty").predicate_ids,
            vec![
                "intent.example_or_bench_witnesses",
                "candidate.test_support",
            ],
        );
    }

    #[test]
    fn policy_trace_path_witness_mixed_examples_query_skips_examples_rs_exact_bonus() {
        let ctx = PathWitnessFacts {
            path_overlap: 1,
            wants_test_witness_recall: true,
            wants_example_or_bench_witnesses: true,
            has_exact_query_term_match: true,
            is_test_support: true,
            is_examples_rs: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(!rule_ids.contains(&"tests.exact_query_match_bonus"));
    }

    #[test]
    fn policy_trace_path_witness_entrypoint_runtime_anchor_test_bonus() {
        let ctx = PathWitnessFacts {
            wants_entrypoint_build_flow: true,
            is_test_support: true,
            is_runtime_anchor_test_support: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"tests.runtime_anchor_support_bonus"));
        assert!(!rule_ids.contains(&"runtime_config.test_support_penalty"));
        assert_eq!(
            trace_rule(&evaluation, "tests.runtime_anchor_support_bonus").predicate_ids,
            vec![
                "intent.entrypoint_or_runtime_config",
                "candidate.runtime_anchor_test_support",
            ],
        );
    }

    #[test]
    fn policy_trace_path_witness_runtime_config_runtime_anchor_test_skips_generic_penalty() {
        let ctx = PathWitnessFacts {
            wants_runtime_config_artifacts: true,
            is_test_support: true,
            is_runtime_anchor_test_support: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"tests.runtime_anchor_support_bonus"));
        assert!(!rule_ids.contains(&"runtime_config.test_support_penalty"));
    }

    #[test]
    fn policy_trace_path_witness_runtime_config_test_tree_harness_bonus() {
        let ctx = PathWitnessFacts {
            path_overlap: 1,
            wants_runtime_config_artifacts: true,
            is_test_support: true,
            is_python_test: true,
            is_test_harness: true,
            is_cli_test: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"runtime_config.test_tree_harness_bonus"));
        assert!(rule_ids.contains(&"workspace.python_test_bonus"));
        assert_eq!(
            trace_rule(&evaluation, "runtime_config.test_tree_harness_bonus").predicate_ids,
            vec![
                "intent.runtime_config_artifacts",
                "candidate.test_harness",
                "candidate.path_overlap",
            ],
        );
    }

    #[test]
    fn policy_trace_path_witness_kotlin_ui_surface_focus_opens_gate_without_path_overlap() {
        let ctx = PathWitnessFacts {
            wants_test_witness_recall: true,
            wants_kotlin_android_ui_witnesses: true,
            is_kotlin_android_ui_runtime_surface: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true).expect("evaluation");
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"kotlin.ui_runtime_surface_bonus"));
        assert_eq!(
            trace_rule(&evaluation, "kotlin.ui_runtime_surface_bonus").predicate_ids,
            vec![
                "intent.kotlin_android_ui_witnesses",
                "candidate.kotlin_android_ui_runtime_surface",
            ],
        );
    }
}
