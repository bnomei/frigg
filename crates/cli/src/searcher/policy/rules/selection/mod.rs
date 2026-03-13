pub(crate) mod base;
pub(crate) mod ci_scripts_ops;
pub(crate) mod companion_tests;
pub(crate) mod contracts;
pub(crate) mod diversification;
pub(crate) mod entrypoint;
pub(crate) mod laravel_ui;
pub(crate) mod navigation;
pub(crate) mod novelty;
pub(crate) mod runtime_config;
pub(crate) mod runtime_witness;
pub(crate) mod support;
pub(crate) mod tail;
pub(crate) mod test_witness;

use super::super::facts::SelectionFacts;
use super::super::kernel::PolicyProgram;
use super::super::trace::PolicyEvaluation;

type SelectionStageApply = fn(&mut PolicyProgram, &SelectionFacts);

const PIPELINE: &[SelectionStageApply] = &[
    base::apply,
    contracts::apply,
    novelty::apply,
    runtime_witness::apply,
    runtime_config::apply,
    companion_tests::apply,
    laravel_ui::apply,
    test_witness::apply,
    navigation::apply,
    ci_scripts_ops::apply,
    entrypoint::apply,
    diversification::apply,
    tail::apply,
];

pub(crate) fn evaluate(ctx: &SelectionFacts, trace: bool) -> PolicyEvaluation {
    let mut program = PolicyProgram::with_optional_trace(ctx.base_score, trace);
    for apply in PIPELINE {
        apply(&mut program, ctx);
    }
    program.finish()
}

pub(crate) fn score(ctx: &SelectionFacts) -> f32 {
    evaluate(ctx, false).score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::laravel::LaravelUiSurfaceClass;
    use crate::searcher::surfaces::HybridSourceClass;

    fn trace_rule_ids(evaluation: &PolicyEvaluation) -> Vec<&'static str> {
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
        evaluation: &'a PolicyEvaluation,
        rule_id: &'static str,
    ) -> &'a crate::searcher::policy::trace::PolicyRuleTrace {
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
    fn policy_trace_selection_base_and_contracts_exact_identifier_stack() {
        let ctx = SelectionFacts {
            class: HybridSourceClass::Runtime,
            canonical_match_multiplier: 1.24,
            query_has_exact_terms: true,
            wants_contracts: true,
            excerpt_has_exact_identifier_anchor: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"selection.base.canonical_match"));
        assert!(rule_ids.contains(&"selection.contracts.exact_identifier_bonus"));
        assert!(rule_ids.contains(&"selection.contracts.runtime_support_tests_bonus"));
        assert!(!rule_ids.contains(&"selection.contracts.missing_identifier_penalty"));
        assert_eq!(
            trace_rule(&evaluation, "selection.contracts.exact_identifier_bonus").predicate_ids,
            vec![
                "query.has_exact_terms",
                "intent.contractish",
                "candidate.excerpt_exact_identifier_anchor",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_novelty_first_laravel_surface_then_repeat_penalty() {
        let first_ctx = SelectionFacts {
            class: HybridSourceClass::Runtime,
            wants_class: true,
            wants_laravel_ui_witnesses: true,
            laravel_surface: Some(LaravelUiSurfaceClass::BladeView),
            ..Default::default()
        };
        let first_eval = evaluate(&first_ctx, true);
        let first_ids = trace_rule_ids(&first_eval);

        let repeat_ctx = SelectionFacts {
            seen_count: 1,
            laravel_surface_seen: 1,
            ..first_ctx
        };
        let repeat_eval = evaluate(&repeat_ctx, true);
        let repeat_ids = trace_rule_ids(&repeat_eval);

        assert!(first_ids.contains(&"selection.novelty.class_bonus"));
        assert!(first_ids.contains(&"selection.novelty.laravel_surface_bonus"));
        assert!(repeat_ids.contains(&"selection.novelty.class_repeat_penalty"));
        assert!(repeat_ids.contains(&"selection.novelty.laravel_surface_repeat_penalty"));
        assert_eq!(
            trace_rule(&first_eval, "selection.novelty.laravel_surface_bonus").predicate_ids,
            vec![
                "intent.laravel_ui_witnesses",
                "candidate.laravel_surface.present",
                "state.laravel_surface_seen_zero",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_entrypoint_first_workflow_bonus_and_without_runtime_penalty() {
        let ctx = SelectionFacts {
            wants_entrypoint_build_flow: true,
            is_entrypoint_build_workflow: true,
            is_ci_workflow: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"selection.entrypoint.workflow_bonus"));
        assert!(rule_ids.contains(&"selection.entrypoint.workflow_without_runtime_penalty"));
        assert!(rule_ids.contains(&"selection.diversification.first_build_workflow_bonus"));
        assert!(rule_ids.contains(&"selection.diversification.first_ci_workflow_bonus"));
    }

    #[test]
    fn policy_trace_selection_runtime_config_ci_repeat_penalty_after_repo_root_config() {
        let ctx = SelectionFacts {
            class: HybridSourceClass::Tests,
            wants_runtime_config_artifacts: true,
            is_ci_workflow: true,
            seen_ci_workflows: 1,
            seen_repo_root_runtime_configs: 1,
            runtime_seen: 0,
            seen_count: 1,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"selection.runtime_config.ci_penalty_without_runtime"));
        assert!(rule_ids.contains(&"selection.diversification.ci_repeat_penalty"));
        assert!(rule_ids.contains(&"selection.diversification.ci_repo_root_penalty"));
        assert_eq!(
            trace_rule(
                &evaluation,
                "selection.diversification.ci_repo_root_penalty"
            )
            .predicate_ids,
            vec![
                "intent.runtime_config_or_entrypoint_build_flow",
                "candidate.ci_workflow",
                "state.seen_ci_workflows_positive",
                "state.has_seen_repo_root_runtime_config",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_cli_test_support_beats_cli_runtime_noise() {
        let ctx = SelectionFacts {
            class: HybridSourceClass::Tests,
            wants_test_witness_recall: true,
            query_mentions_cli: true,
            is_cli_test_support: true,
            is_test_support: true,
            has_exact_query_term_match: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"selection.tests.exact_query_match_bonus"));
        assert!(rule_ids.contains(&"selection.tests.support_bonus"));
        assert!(rule_ids.contains(&"selection.tests.cli_support_bonus"));
        assert!(!rule_ids.contains(&"selection.entrypoint.cli_runtime_penalty"));
        assert_eq!(
            trace_rule(&evaluation, "selection.tests.cli_support_bonus").predicate_ids,
            vec![
                "intent.test_witness_recall",
                "query.mentions_cli",
                "candidate.cli_test_support",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_runtime_generic_doc_penalties_without_anchor() {
        let ctx = SelectionFacts {
            class: HybridSourceClass::Documentation,
            wants_runtime_witnesses: true,
            penalize_generic_runtime_docs: true,
            is_generic_runtime_witness_doc: true,
            runtime_seen: 0,
            seen_count: 1,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"selection.runtime.generic_doc_repeat_penalty"));
        assert!(rule_ids.contains(&"selection.runtime.generic_doc_first_penalty"));
        assert!(rule_ids.contains(&"selection.runtime.doc_path_overlap_penalty"));
        assert_eq!(
            trace_rule(&evaluation, "selection.runtime.generic_doc_first_penalty").predicate_ids,
            vec![
                "intent.runtime_witnesses",
                "intent.penalize_generic_runtime_docs",
                "candidate.generic_runtime_witness_doc",
                "state.runtime_seen_zero",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_laravel_surface_general_blade_view_records_predicates() {
        let ctx = SelectionFacts {
            wants_laravel_ui_witnesses: true,
            laravel_surface: Some(LaravelUiSurfaceClass::BladeView),
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule = trace_rule(&evaluation, "selection.laravel.surface.general_blade_view");

        assert_eq!(
            rule.predicate_ids,
            vec![
                "intent.laravel_ui_witnesses",
                "candidate.laravel_surface.blade_view",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_mixed_query_diversification_rules_fire_for_first_plain_test_and_repeated_benches()
     {
        let plain_test_ctx = SelectionFacts {
            wants_test_witness_recall: true,
            wants_example_or_bench_witnesses: true,
            is_test_support: true,
            has_exact_query_term_match: true,
            ..Default::default()
        };
        let plain_test_eval = evaluate(&plain_test_ctx, true);
        let plain_test_ids = trace_rule_ids(&plain_test_eval);

        let bench_ctx = SelectionFacts {
            wants_test_witness_recall: true,
            wants_example_or_bench_witnesses: true,
            is_bench_support: true,
            seen_bench_support: 1,
            specific_witness_path_overlap: 1,
            ..Default::default()
        };
        let bench_eval = evaluate(&bench_ctx, true);
        let bench_ids = trace_rule_ids(&bench_eval);

        assert!(
            plain_test_ids
                .contains(&"selection.diversification.mixed_query_first_plain_test_bonus")
        );
        assert!(bench_ids.contains(&"selection.diversification.mixed_query_bench_repeat_penalty"));
    }

    #[test]
    fn policy_trace_selection_mixed_query_first_example_bonus_fires() {
        let ctx = SelectionFacts {
            wants_test_witness_recall: true,
            wants_example_or_bench_witnesses: true,
            is_example_support: true,
            path_overlap: 1,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"selection.diversification.mixed_query_first_example_bonus"));
        assert_eq!(
            trace_rule(
                &evaluation,
                "selection.diversification.mixed_query_first_example_bonus",
            )
            .predicate_ids,
            vec![
                "intent.mixed_query_example_or_bench",
                "candidate.example_support",
                "state.seen_example_support_zero",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_runtime_companion_rules_prefer_runtime_adjacent_python_tests() {
        let ctx = SelectionFacts {
            wants_entrypoint_build_flow: true,
            wants_runtime_companion_tests: true,
            prefer_runtime_anchor_tests: true,
            is_test_support: true,
            is_runtime_anchor_test_support: true,
            is_runtime_adjacent_python_test: true,
            is_non_prefix_python_test_module: true,
            runtime_family_prefix_overlap: 3,
            path_depth: 6,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"selection.companion.runtime_anchor_bonus"));
        assert!(rule_ids.contains(&"selection.companion.runtime_adjacent_python_bonus"));
        assert!(rule_ids.contains(&"selection.companion.non_prefix_python_bonus"));
        assert!(rule_ids.contains(&"selection.companion.family_affinity_bonus"));
        assert!(rule_ids.contains(&"selection.companion.deeper_path_bonus"));
        assert_eq!(
            trace_rule(&evaluation, "selection.companion.deeper_path_bonus").predicate_ids,
            vec![
                "intent.runtime_companion_tests",
                "candidate.test_support",
                "candidate.path_depth_at_least_four",
            ],
        );
    }

    #[test]
    fn policy_trace_selection_navigation_scripts_and_tail_rules_record_predicates() {
        let navigation_ctx = SelectionFacts {
            wants_navigation_fallbacks: true,
            wants_mcp_runtime_surface: true,
            is_navigation_runtime: true,
            seen_count: 0,
            ..Default::default()
        };
        let navigation_eval = evaluate(&navigation_ctx, true);

        let scripts_ctx = SelectionFacts {
            wants_scripts_ops_witnesses: true,
            is_scripts_ops: true,
            seen_count: 0,
            ..Default::default()
        };
        let scripts_eval = evaluate(&scripts_ctx, true);

        let tail_ctx = SelectionFacts {
            class: HybridSourceClass::Runtime,
            query_has_identifier_anchor: true,
            wants_runtime_witnesses: true,
            path_overlap: 0,
            excerpt_overlap: 0,
            ..Default::default()
        };
        let tail_eval = evaluate(&tail_ctx, true);

        assert_eq!(
            trace_rule(&navigation_eval, "selection.navigation.mcp_runtime_bonus").predicate_ids,
            vec![
                "intent.navigation_fallbacks",
                "intent.mcp_runtime_surface",
                "candidate.navigation_runtime",
            ],
        );
        assert_eq!(
            trace_rule(&scripts_eval, "selection.scripts.ops_bonus").predicate_ids,
            vec!["intent.scripts_ops_witnesses", "candidate.scripts_ops"],
        );
        assert_eq!(
            trace_rule(
                &tail_eval,
                "selection.tail.missing_identifier_anchor_penalty"
            )
            .predicate_ids,
            vec![
                "query.has_identifier_anchor",
                "intent.runtime_or_entrypoint_build_flow",
                "candidate.class.runtime",
            ],
        );
    }
}
