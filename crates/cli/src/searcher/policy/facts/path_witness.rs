use super::super::super::intent::HybridRankingIntent;
use super::super::super::path_witness_projection::StoredPathWitnessProjection;
use super::super::super::surfaces::HybridSourceClass;
use super::{PolicyQueryContext, SharedIntentFacts, SharedPathFacts};

pub(crate) struct PathWitnessFacts {
    pub(crate) path_overlap: usize,
    pub(crate) specific_path_overlap: usize,
    pub(crate) source_class: HybridSourceClass,
    pub(crate) has_exact_query_term_match: bool,
    pub(crate) is_entrypoint: bool,
    pub(crate) is_typescript_runtime_module_index: bool,
    pub(crate) is_entrypoint_build_workflow: bool,
    pub(crate) is_ci_workflow: bool,
    pub(crate) is_config_artifact: bool,
    pub(crate) is_kotlin_android_ui_runtime_surface: bool,
    pub(crate) is_python_config: bool,
    pub(crate) is_rust_workspace_config: bool,
    pub(crate) wants_rust_workspace_config: bool,
    pub(crate) wants_python_workspace_config: bool,
    pub(crate) wants_python_witnesses: bool,
    pub(crate) is_repo_root_runtime_config_artifact: bool,
    pub(crate) is_python_test: bool,
    pub(crate) is_runtime_adjacent_python_test: bool,
    pub(crate) is_example_support: bool,
    pub(crate) is_bench_support: bool,
    pub(crate) wants_example_or_bench_witnesses: bool,
    pub(crate) is_cli_test: bool,
    pub(crate) is_test_harness: bool,
    pub(crate) is_scripts_ops: bool,
    pub(crate) is_test_support: bool,
    pub(crate) is_runtime_anchor_test_support: bool,
    pub(crate) is_examples_rs: bool,
    pub(crate) is_laravel_non_livewire_blade_view: bool,
    pub(crate) is_laravel_livewire_view: bool,
    pub(crate) is_laravel_top_level_blade_view: bool,
    pub(crate) is_laravel_partial_view: bool,
    pub(crate) is_laravel_blade_component: bool,
    pub(crate) is_laravel_nested_blade_component: bool,
    pub(crate) is_laravel_form_action_blade: bool,
    pub(crate) is_laravel_livewire_component: bool,
    pub(crate) is_laravel_view_component_class: bool,
    pub(crate) is_laravel_command_or_middleware: bool,
    pub(crate) is_laravel_job_or_listener: bool,
    pub(crate) is_laravel_layout_blade_view: bool,
    pub(crate) is_laravel_route: bool,
    pub(crate) is_laravel_bootstrap_entrypoint: bool,
    pub(crate) is_laravel_core_provider: bool,
    pub(crate) is_laravel_provider: bool,
    pub(crate) is_frontend_runtime_noise: bool,
    pub(crate) wants_entrypoint_build_flow: bool,
    pub(crate) wants_runtime_config_artifacts: bool,
    pub(crate) wants_ci_workflow_witnesses: bool,
    pub(crate) wants_examples: bool,
    pub(crate) wants_benchmarks: bool,
    pub(crate) wants_laravel_ui_witnesses: bool,
    pub(crate) wants_blade_component_witnesses: bool,
    pub(crate) wants_laravel_form_action_witnesses: bool,
    pub(crate) wants_laravel_layout_witnesses: bool,
    pub(crate) wants_livewire_view_witnesses: bool,
    pub(crate) wants_commands_middleware_witnesses: bool,
    pub(crate) wants_jobs_listeners_witnesses: bool,
    pub(crate) wants_test_witness_recall: bool,
    pub(crate) wants_kotlin_android_ui_witnesses: bool,
    pub(crate) query_mentions_cli: bool,
    pub(crate) has_specific_query_terms: bool,
    pub(crate) path_stem_is_server_or_cli: bool,
    pub(crate) path_stem_is_main: bool,
}

impl PathWitnessFacts {
    pub(crate) fn from_projection(
        path: &str,
        projection: &StoredPathWitnessProjection,
        intent: &HybridRankingIntent,
        query_context: &PolicyQueryContext,
    ) -> Self {
        let shared_intent = SharedIntentFacts::from_intent(intent);
        let shared_path = SharedPathFacts::from_path(path);
        let path_match = query_context.match_projection_path(path, &projection.path_terms);

        Self {
            path_overlap: path_match.path_overlap,
            specific_path_overlap: path_match.specific_witness_path_overlap,
            source_class: if shared_path.is_test_support {
                HybridSourceClass::Tests
            } else {
                shared_path.class
            },
            has_exact_query_term_match: path_match.has_exact_query_term_match,
            is_entrypoint: shared_path.is_entrypoint_runtime,
            is_typescript_runtime_module_index: shared_path.is_typescript_runtime_module_index,
            is_entrypoint_build_workflow: shared_intent.wants_entrypoint_build_flow
                && shared_path.is_entrypoint_build_workflow,
            is_ci_workflow: shared_path.is_ci_workflow,
            is_config_artifact: shared_path.is_runtime_config_artifact,
            is_kotlin_android_ui_runtime_surface: shared_path.is_kotlin_android_ui_runtime_surface,
            is_python_config: shared_path.is_python_runtime_config,
            is_rust_workspace_config: shared_path.is_rust_workspace_config,
            wants_rust_workspace_config: shared_intent.wants_rust_workspace_config,
            wants_python_workspace_config: shared_intent.wants_python_workspace_config,
            wants_python_witnesses: shared_intent.wants_python_witnesses,
            is_repo_root_runtime_config_artifact: shared_path.is_repo_root_runtime_config_artifact,
            is_python_test: shared_path.is_python_test_witness,
            is_runtime_adjacent_python_test: shared_path.is_runtime_adjacent_python_test,
            is_example_support: shared_path.is_example_support,
            is_bench_support: shared_path.is_bench_support,
            wants_example_or_bench_witnesses: shared_intent.wants_example_or_bench_witnesses,
            is_cli_test: shared_path.is_cli_test_support,
            is_test_harness: shared_path.is_test_harness,
            is_scripts_ops: shared_intent.wants_scripts_ops_witnesses && shared_path.is_scripts_ops,
            is_test_support: shared_path.is_test_support,
            is_runtime_anchor_test_support: shared_path.is_runtime_anchor_test_support,
            is_examples_rs: shared_path.is_examples_rs,
            is_laravel_non_livewire_blade_view: shared_path.is_laravel_non_livewire_blade_view,
            is_laravel_livewire_view: shared_path.is_laravel_livewire_view,
            is_laravel_top_level_blade_view: shared_path.is_laravel_top_level_blade_view,
            is_laravel_partial_view: shared_path.is_laravel_partial_view,
            is_laravel_blade_component: shared_path.is_laravel_blade_component,
            is_laravel_nested_blade_component: shared_path.is_laravel_nested_blade_component,
            is_laravel_form_action_blade: shared_path.is_laravel_form_action_blade,
            is_laravel_livewire_component: shared_path.is_laravel_livewire_component,
            is_laravel_view_component_class: shared_path.is_laravel_view_component_class,
            is_laravel_command_or_middleware: shared_path.is_laravel_command_or_middleware,
            is_laravel_job_or_listener: shared_path.is_laravel_job_or_listener,
            is_laravel_layout_blade_view: shared_path.is_laravel_layout_blade_view,
            is_laravel_route: shared_path.is_laravel_route,
            is_laravel_bootstrap_entrypoint: shared_path.is_laravel_bootstrap_entrypoint,
            is_laravel_core_provider: shared_path.is_laravel_core_provider,
            is_laravel_provider: shared_path.is_laravel_provider,
            is_frontend_runtime_noise: shared_path.effective_frontend_runtime_noise(&shared_intent),
            wants_entrypoint_build_flow: shared_intent.wants_entrypoint_build_flow,
            wants_runtime_config_artifacts: shared_intent.wants_runtime_config_artifacts,
            wants_ci_workflow_witnesses: shared_intent.wants_ci_workflow_witnesses,
            wants_examples: shared_intent.wants_examples,
            wants_benchmarks: shared_intent.wants_benchmarks,
            wants_laravel_ui_witnesses: shared_intent.wants_laravel_ui_witnesses,
            wants_blade_component_witnesses: shared_intent.wants_blade_component_witnesses,
            wants_laravel_form_action_witnesses: shared_intent.wants_laravel_form_action_witnesses,
            wants_laravel_layout_witnesses: shared_intent.wants_laravel_layout_witnesses,
            wants_livewire_view_witnesses: shared_intent.wants_livewire_view_witnesses,
            wants_commands_middleware_witnesses: shared_intent.wants_commands_middleware_witnesses,
            wants_jobs_listeners_witnesses: shared_intent.wants_jobs_listeners_witnesses,
            wants_test_witness_recall: shared_intent.wants_test_witness_recall,
            wants_kotlin_android_ui_witnesses: query_context.wants_kotlin_android_ui_witnesses
                && shared_intent.wants_test_witness_recall,
            query_mentions_cli: query_context.query_mentions_cli,
            has_specific_query_terms: query_context.has_specific_witness_terms(),
            path_stem_is_server_or_cli: shared_path.path_stem_is_server_or_cli,
            path_stem_is_main: shared_path.path_stem_is_main,
        }
    }
}

#[cfg(test)]
impl Default for PathWitnessFacts {
    fn default() -> Self {
        Self {
            path_overlap: 0,
            specific_path_overlap: 0,
            source_class: HybridSourceClass::Other,
            has_exact_query_term_match: false,
            is_entrypoint: false,
            is_typescript_runtime_module_index: false,
            is_entrypoint_build_workflow: false,
            is_ci_workflow: false,
            is_config_artifact: false,
            is_kotlin_android_ui_runtime_surface: false,
            is_python_config: false,
            is_rust_workspace_config: false,
            wants_rust_workspace_config: false,
            wants_python_workspace_config: false,
            wants_python_witnesses: false,
            is_repo_root_runtime_config_artifact: false,
            is_python_test: false,
            is_runtime_adjacent_python_test: false,
            is_example_support: false,
            is_bench_support: false,
            wants_example_or_bench_witnesses: false,
            is_cli_test: false,
            is_test_harness: false,
            is_scripts_ops: false,
            is_test_support: false,
            is_runtime_anchor_test_support: false,
            is_examples_rs: false,
            is_laravel_non_livewire_blade_view: false,
            is_laravel_livewire_view: false,
            is_laravel_top_level_blade_view: false,
            is_laravel_partial_view: false,
            is_laravel_blade_component: false,
            is_laravel_nested_blade_component: false,
            is_laravel_form_action_blade: false,
            is_laravel_livewire_component: false,
            is_laravel_view_component_class: false,
            is_laravel_command_or_middleware: false,
            is_laravel_job_or_listener: false,
            is_laravel_layout_blade_view: false,
            is_laravel_route: false,
            is_laravel_bootstrap_entrypoint: false,
            is_laravel_core_provider: false,
            is_laravel_provider: false,
            is_frontend_runtime_noise: false,
            wants_entrypoint_build_flow: false,
            wants_runtime_config_artifacts: false,
            wants_ci_workflow_witnesses: false,
            wants_examples: false,
            wants_benchmarks: false,
            wants_laravel_ui_witnesses: false,
            wants_blade_component_witnesses: false,
            wants_laravel_form_action_witnesses: false,
            wants_laravel_layout_witnesses: false,
            wants_livewire_view_witnesses: false,
            wants_commands_middleware_witnesses: false,
            wants_jobs_listeners_witnesses: false,
            wants_test_witness_recall: false,
            wants_kotlin_android_ui_witnesses: false,
            query_mentions_cli: false,
            has_specific_query_terms: false,
            path_stem_is_server_or_cli: false,
            path_stem_is_main: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::intent::HybridRankingIntent;
    use crate::searcher::path_witness_projection::StoredPathWitnessProjection;

    #[test]
    fn path_witness_facts_use_live_example_support_for_stale_projections() {
        let query = "tests fixtures integration entry point main app package platform runtime bytes stdin command line examples benches benchmark";
        let intent = HybridRankingIntent::from_query(query);
        let query_context = PolicyQueryContext::new(&intent, query);
        let mut stale_example =
            StoredPathWitnessProjection::from_path("examples/command-line-args.roc");
        stale_example.flags.is_example_support = false;

        let facts = PathWitnessFacts::from_projection(
            "examples/command-line-args.roc",
            &stale_example,
            &intent,
            &query_context,
        );

        assert!(
            facts.is_example_support,
            "live path detection should recover stale example-support projections"
        );
        assert!(facts.wants_examples);
        assert!(facts.wants_test_witness_recall);
    }

    #[test]
    fn path_witness_facts_treat_root_scoped_tool_configs_as_repo_root_artifacts() {
        let query = "config";
        let intent = HybridRankingIntent::from_query(query);
        let query_context = PolicyQueryContext::new(&intent, query);
        let projection =
            StoredPathWitnessProjection::from_path("gradle/wrapper/gradle-wrapper.properties");

        let facts = PathWitnessFacts::from_projection(
            "gradle/wrapper/gradle-wrapper.properties",
            &projection,
            &intent,
            &query_context,
        );

        assert!(facts.is_config_artifact);
        assert!(facts.is_repo_root_runtime_config_artifact);
    }
}
