use std::path::Path;

use crate::domain::FrameworkHint;

use super::super::super::intent::HybridRankingIntent;
use super::super::super::path_witness_projection::StoredPathWitnessProjection;
use super::super::super::query_terms::{
    hybrid_overlap_count, hybrid_query_exact_terms, hybrid_query_overlap_terms,
    hybrid_specific_witness_query_terms, path_has_exact_query_term_match,
};
use super::super::super::surfaces::{
    HybridSourceClass, is_bench_support_path, is_entrypoint_runtime_path, is_example_support_path,
    is_python_test_witness_path, is_runtime_adjacent_python_test_path,
    is_runtime_anchor_test_support_path, is_rust_workspace_config_path, is_test_support_path,
    is_typescript_runtime_module_index_path,
};

pub(crate) struct PathWitnessQueryContext {
    pub(crate) exact_terms: Vec<String>,
    pub(crate) query_overlap_terms: Vec<String>,
    pub(crate) specific_query_terms: Vec<String>,
    pub(crate) query_mentions_cli: bool,
}

impl PathWitnessQueryContext {
    pub(crate) fn new(query_text: &str) -> Self {
        Self {
            exact_terms: hybrid_query_exact_terms(query_text),
            query_overlap_terms: hybrid_query_overlap_terms(query_text),
            specific_query_terms: hybrid_specific_witness_query_terms(query_text),
            query_mentions_cli: query_text.to_ascii_lowercase().contains("cli"),
        }
    }
}

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
        query_context: &PathWitnessQueryContext,
    ) -> Self {
        let path_overlap =
            hybrid_overlap_count(&projection.path_terms, &query_context.query_overlap_terms);
        let specific_path_overlap =
            hybrid_overlap_count(&projection.path_terms, &query_context.specific_query_terms);
        let is_config_artifact = projection.flags.is_runtime_config_artifact;
        let is_test_support = projection.flags.is_test_support || is_test_support_path(path);
        let is_runtime_anchor_test_support = is_runtime_anchor_test_support_path(path);
        let path_stem = Path::new(path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let is_repo_root_runtime_config_artifact =
            is_config_artifact && !path.trim_start_matches("./").contains('/');
        let is_laravel_partial_view = path.contains("/parts/") || path.contains("/partials/");
        let is_laravel_top_level_blade_view = projection.flags.is_laravel_non_livewire_blade_view
            && !projection.flags.is_laravel_layout_blade_view
            && !is_laravel_partial_view;
        let is_examples_rs = is_test_support
            && Path::new(path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("examples.rs"));
        let is_frontend_runtime_noise = projection.flags.is_frontend_runtime_noise
            && !(is_repo_root_runtime_config_artifact
                && (intent.wants_entrypoint_build_flow || intent.wants_runtime_config_artifacts));

        Self {
            path_overlap,
            specific_path_overlap,
            source_class: if is_test_support {
                HybridSourceClass::Tests
            } else {
                projection.source_class
            },
            has_exact_query_term_match: path_has_exact_query_term_match(
                path,
                &query_context.exact_terms,
            ),
            is_entrypoint: projection.flags.is_entrypoint_runtime
                || is_entrypoint_runtime_path(path),
            is_typescript_runtime_module_index: is_typescript_runtime_module_index_path(path),
            is_entrypoint_build_workflow: intent.wants_entrypoint_build_flow
                && projection.flags.is_entrypoint_build_workflow,
            is_ci_workflow: intent.wants_ci_workflow_witnesses && projection.flags.is_ci_workflow,
            is_config_artifact,
            is_python_config: projection.flags.is_python_runtime_config,
            is_rust_workspace_config: is_rust_workspace_config_path(path),
            wants_rust_workspace_config: intent.has_framework_hint(FrameworkHint::Rust),
            wants_python_workspace_config: intent.has_framework_hint(FrameworkHint::Python),
            wants_python_witnesses: intent.has_framework_hint(FrameworkHint::Python),
            is_repo_root_runtime_config_artifact,
            is_python_test: projection.flags.is_python_test_witness
                || is_python_test_witness_path(path),
            is_runtime_adjacent_python_test: is_runtime_adjacent_python_test_path(path),
            is_example_support: projection.flags.is_example_support
                || is_example_support_path(path),
            is_bench_support: projection.flags.is_bench_support || is_bench_support_path(path),
            wants_example_or_bench_witnesses: intent.wants_examples || intent.wants_benchmarks,
            is_cli_test: projection.flags.is_cli_test_support,
            is_test_harness: projection.flags.is_test_harness,
            is_scripts_ops: intent.wants_scripts_ops_witnesses && projection.flags.is_scripts_ops,
            is_test_support,
            is_runtime_anchor_test_support,
            is_examples_rs,
            is_laravel_non_livewire_blade_view: projection.flags.is_laravel_non_livewire_blade_view,
            is_laravel_livewire_view: projection.flags.is_laravel_livewire_view,
            is_laravel_top_level_blade_view,
            is_laravel_partial_view,
            is_laravel_blade_component: projection.flags.is_laravel_blade_component,
            is_laravel_nested_blade_component: projection.flags.is_laravel_nested_blade_component,
            is_laravel_form_action_blade: projection.flags.is_laravel_form_action_blade,
            is_laravel_livewire_component: projection.flags.is_laravel_livewire_component,
            is_laravel_view_component_class: projection.flags.is_laravel_view_component_class,
            is_laravel_command_or_middleware: projection.flags.is_laravel_command_or_middleware,
            is_laravel_job_or_listener: projection.flags.is_laravel_job_or_listener,
            is_laravel_layout_blade_view: projection.flags.is_laravel_layout_blade_view,
            is_laravel_route: projection.flags.is_laravel_route,
            is_laravel_bootstrap_entrypoint: projection.flags.is_laravel_bootstrap_entrypoint,
            is_laravel_core_provider: projection.flags.is_laravel_core_provider,
            is_laravel_provider: projection.flags.is_laravel_provider,
            is_frontend_runtime_noise,
            wants_entrypoint_build_flow: intent.wants_entrypoint_build_flow,
            wants_runtime_config_artifacts: intent.wants_runtime_config_artifacts,
            wants_examples: intent.wants_examples,
            wants_benchmarks: intent.wants_benchmarks,
            wants_laravel_ui_witnesses: intent.wants_laravel_ui_witnesses,
            wants_blade_component_witnesses: intent.wants_blade_component_witnesses,
            wants_laravel_form_action_witnesses: intent.wants_laravel_form_action_witnesses,
            wants_laravel_layout_witnesses: intent.wants_laravel_layout_witnesses,
            wants_livewire_view_witnesses: intent.wants_livewire_view_witnesses,
            wants_commands_middleware_witnesses: intent.wants_commands_middleware_witnesses,
            wants_jobs_listeners_witnesses: intent.wants_jobs_listeners_witnesses,
            wants_test_witness_recall: intent.wants_test_witness_recall,
            query_mentions_cli: query_context.query_mentions_cli,
            has_specific_query_terms: !query_context.specific_query_terms.is_empty(),
            path_stem_is_server_or_cli: matches!(path_stem.as_str(), "server" | "cli"),
            path_stem_is_main: path_stem == "main",
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
        let query_context = PathWitnessQueryContext::new(query);
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
}
