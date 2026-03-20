use super::super::facts::PathQualityFacts;
use super::super::kernel::PolicyProgram;
use super::super::trace::PolicyEvaluation;

#[path = "path_quality/docs_contracts.rs"]
mod docs_contracts;
#[path = "path_quality/entrypoint.rs"]
mod entrypoint;
#[path = "path_quality/examples_bench.rs"]
mod examples_bench;
#[path = "path_quality/laravel.rs"]
mod laravel;
#[path = "path_quality/navigation.rs"]
mod navigation;
#[path = "path_quality/penalties.rs"]
mod penalties;
#[path = "path_quality/runtime.rs"]
mod runtime;
#[path = "path_quality/runtime_config.rs"]
mod runtime_config;

pub(crate) fn evaluate(ctx: &PathQualityFacts, trace: bool) -> PolicyEvaluation {
    let mut program = PolicyProgram::with_optional_trace(ctx.base_multiplier, trace);
    docs_contracts::apply(&mut program, ctx);
    runtime::apply(&mut program, ctx);
    runtime_config::apply(&mut program, ctx);
    entrypoint::apply(&mut program, ctx);
    navigation::apply(&mut program, ctx);
    examples_bench::apply(&mut program, ctx);
    laravel::apply(&mut program, ctx);
    penalties::apply(&mut program, ctx);
    program.finish()
}

pub(crate) fn score(ctx: &PathQualityFacts) -> f32 {
    evaluate(ctx, false).score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::policy::trace::PolicyEffect;
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
    fn policy_trace_path_quality_runtime_config_entrypoint_typescript_stack() {
        let ctx = PathQualityFacts {
            class: HybridSourceClass::Runtime,
            wants_runtime_config_artifacts: true,
            wants_entrypoint_build_flow: true,
            is_entrypoint_runtime: true,
            is_typescript_runtime_module_index: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"runtime_config.prefers_entrypoint_runtime"));
        assert!(rule_ids.contains(&"runtime_config.prefers_typescript_index"));
        assert!(rule_ids.contains(&"entrypoint.prefers_entrypoint_runtime"));
        assert!(rule_ids.contains(&"entrypoint.prefers_typescript_index"));
        assert_eq!(
            trace_rule(&evaluation, "runtime_config.prefers_entrypoint_runtime").predicate_ids,
            vec![
                "intent.runtime_config_artifacts",
                "candidate.entrypoint_runtime"
            ],
        );
        assert_eq!(
            trace_rule(&evaluation, "entrypoint.prefers_typescript_index").predicate_ids,
            vec![
                "intent.entrypoint_build_flow",
                "candidate.typescript_runtime_module_index",
            ],
        );
    }

    #[test]
    fn policy_trace_path_quality_runtime_witness_generic_doc_penalties_stack() {
        let ctx = PathQualityFacts {
            class: HybridSourceClass::Documentation,
            wants_runtime_witnesses: true,
            penalize_generic_runtime_docs: true,
            is_generic_runtime_witness_doc: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"runtime_witness.penalizes_docs"));
        assert!(rule_ids.contains(&"runtime_witness.penalizes_generic_runtime_docs"));
        assert_eq!(
            trace_rule(
                &evaluation,
                "runtime_witness.penalizes_generic_runtime_docs"
            )
            .predicate_ids,
            vec![
                "intent.runtime_witnesses",
                "intent.penalize_generic_runtime_docs",
                "candidate.generic_runtime_witness_doc",
            ],
        );
    }

    #[test]
    fn policy_trace_path_quality_examples_query_penalizes_non_support_tests() {
        let without_test_focus = PathQualityFacts {
            class: HybridSourceClass::Tests,
            wants_example_or_bench_witnesses: true,
            wants_test_witness_recall: false,
            ..Default::default()
        };
        let without_test_focus_eval = evaluate(&without_test_focus, true);
        let without_ids = trace_rule_ids(&without_test_focus_eval);
        let without_effect = without_test_focus_eval
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .find(|rule| rule.rule_id == "examples_or_bench.penalizes_non_support_tests")
            .expect("examples-or-bench penalty should fire");

        let with_test_focus = PathQualityFacts {
            wants_test_witness_recall: true,
            class: without_test_focus.class,
            wants_example_or_bench_witnesses: without_test_focus.wants_example_or_bench_witnesses,
            ..Default::default()
        };
        let with_test_focus_eval = evaluate(&with_test_focus, true);
        let with_ids = trace_rule_ids(&with_test_focus_eval);
        let with_effect = with_test_focus_eval
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .find(|rule| rule.rule_id == "examples_or_bench.penalizes_non_support_tests")
            .expect("examples-or-bench penalty should fire");

        assert!(without_ids.contains(&"examples_or_bench.penalizes_non_support_tests"));
        assert!(with_ids.contains(&"examples_or_bench.penalizes_non_support_tests"));
        assert_eq!(without_effect.effect, PolicyEffect::Multiply(0.68));
        assert_eq!(with_effect.effect, PolicyEffect::Multiply(0.92));
    }

    #[test]
    fn policy_trace_path_quality_runtime_witness_demotes_root_meta_ci_and_frontend_noise() {
        let ctx = PathQualityFacts {
            class: HybridSourceClass::Support,
            path_depth: 1,
            wants_runtime_witnesses: true,
            is_ci_workflow: true,
            is_repo_metadata: true,
            is_frontend_runtime_noise: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"runtime_witness.penalizes_repo_metadata"));
        assert!(rule_ids.contains(&"runtime_witness.penalizes_root_repo_metadata"));
        assert!(rule_ids.contains(&"runtime_witness.penalizes_ci_workflow_noise"));
        assert!(rule_ids.contains(&"runtime_witness.penalizes_frontend_runtime_noise"));
    }
}
