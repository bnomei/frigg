use std::path::Path;

use super::super::super::intent::HybridRankingIntent;
use super::super::super::surfaces::{
    HybridSourceClass, hybrid_source_class, is_entrypoint_runtime_path,
    is_navigation_reference_doc_path, is_navigation_runtime_path, is_repo_metadata_path,
    is_runtime_config_artifact_path, is_test_support_path, is_typescript_runtime_module_index_path,
};

pub(crate) struct PathQualityFacts {
    pub(crate) class: HybridSourceClass,
    pub(crate) base_multiplier: f32,
    pub(crate) wants_docs: bool,
    pub(crate) wants_readme: bool,
    pub(crate) wants_onboarding: bool,
    pub(crate) wants_contracts: bool,
    pub(crate) wants_error_taxonomy: bool,
    pub(crate) wants_tool_contracts: bool,
    pub(crate) wants_mcp_runtime_surface: bool,
    pub(crate) wants_examples: bool,
    pub(crate) wants_benchmarks: bool,
    pub(crate) wants_tests: bool,
    pub(crate) wants_fixtures: bool,
    pub(crate) wants_runtime: bool,
    pub(crate) wants_runtime_witnesses: bool,
    pub(crate) wants_runtime_config_artifacts: bool,
    pub(crate) wants_entrypoint_build_flow: bool,
    pub(crate) wants_navigation_fallbacks: bool,
    pub(crate) wants_laravel_ui_witnesses: bool,
    pub(crate) wants_blade_component_witnesses: bool,
    pub(crate) wants_laravel_layout_witnesses: bool,
    pub(crate) wants_test_witness_recall: bool,
    pub(crate) wants_example_or_bench_witnesses: bool,
    pub(crate) penalize_generic_runtime_docs: bool,
    pub(crate) is_root_readme: bool,
    pub(crate) is_entrypoint_runtime: bool,
    pub(crate) is_entrypoint_build_workflow: bool,
    pub(crate) is_navigation_runtime: bool,
    pub(crate) is_navigation_reference_doc: bool,
    pub(crate) is_ci_workflow: bool,
    pub(crate) is_typescript_runtime_module_index: bool,
    pub(crate) is_runtime_config_artifact: bool,
    pub(crate) is_repo_root_runtime_config_artifact: bool,
    pub(crate) is_example_support: bool,
    pub(crate) is_bench_support: bool,
    pub(crate) is_test_support: bool,
    pub(crate) is_generic_runtime_witness_doc: bool,
    pub(crate) is_python_runtime_config: bool,
    pub(crate) is_entrypoint_reference_doc: bool,
    pub(crate) is_repo_metadata: bool,
    pub(crate) is_laravel_non_livewire_blade_view: bool,
    pub(crate) is_laravel_livewire_view: bool,
    pub(crate) is_laravel_blade_component: bool,
    pub(crate) is_laravel_layout_blade_view: bool,
    pub(crate) is_laravel_view_component_class: bool,
}

impl PathQualityFacts {
    pub(crate) fn from_path(path: &str, intent: &HybridRankingIntent) -> Self {
        let normalized_path = path.trim_start_matches("./");
        let class = hybrid_source_class(path);
        let is_runtime_config_artifact = is_runtime_config_artifact_path(path);
        let base_multiplier = if class == HybridSourceClass::Other {
            match Path::new(path).extension().and_then(|ext| ext.to_str()) {
                Some(
                    "rs" | "php" | "go" | "py" | "ts" | "tsx" | "js" | "jsx" | "java" | "kt"
                    | "kts",
                ) => 1.0,
                _ => 0.9,
            }
        } else {
            match class {
                HybridSourceClass::ErrorContracts => 1.0,
                HybridSourceClass::ToolContracts => 1.0,
                HybridSourceClass::BenchmarkDocs => 0.98,
                HybridSourceClass::Documentation => 0.88,
                HybridSourceClass::Readme => 0.78,
                HybridSourceClass::Specs => 0.82,
                HybridSourceClass::Fixtures => 0.92,
                HybridSourceClass::Project => 0.94,
                HybridSourceClass::Support => 0.78,
                HybridSourceClass::Tests => 0.97,
                HybridSourceClass::Runtime => 1.0,
                _ => 0.94,
            }
        };

        Self {
            class,
            base_multiplier,
            wants_docs: intent.wants_docs,
            wants_readme: intent.wants_readme,
            wants_onboarding: intent.wants_onboarding,
            wants_contracts: intent.wants_contracts,
            wants_error_taxonomy: intent.wants_error_taxonomy,
            wants_tool_contracts: intent.wants_tool_contracts,
            wants_mcp_runtime_surface: intent.wants_mcp_runtime_surface,
            wants_examples: intent.wants_examples,
            wants_benchmarks: intent.wants_benchmarks,
            wants_tests: intent.wants_tests,
            wants_fixtures: intent.wants_fixtures,
            wants_runtime: intent.wants_runtime,
            wants_runtime_witnesses: intent.wants_runtime_witnesses,
            wants_runtime_config_artifacts: intent.wants_runtime_config_artifacts,
            wants_entrypoint_build_flow: intent.wants_entrypoint_build_flow,
            wants_navigation_fallbacks: intent.wants_navigation_fallbacks,
            wants_laravel_ui_witnesses: intent.wants_laravel_ui_witnesses,
            wants_blade_component_witnesses: intent.wants_blade_component_witnesses,
            wants_laravel_layout_witnesses: intent.wants_laravel_layout_witnesses,
            wants_test_witness_recall: intent.wants_test_witness_recall,
            wants_example_or_bench_witnesses: intent.wants_examples || intent.wants_benchmarks,
            penalize_generic_runtime_docs: !intent.wants_docs
                && !intent.wants_onboarding
                && !intent.wants_readme,
            is_root_readme: normalized_path.eq_ignore_ascii_case("README.md"),
            is_entrypoint_runtime: is_entrypoint_runtime_path(path),
            is_entrypoint_build_workflow:
                super::super::super::surfaces::is_entrypoint_build_workflow_path(path),
            is_navigation_runtime: is_navigation_runtime_path(path),
            is_navigation_reference_doc: is_navigation_reference_doc_path(path),
            is_ci_workflow: super::super::super::surfaces::is_ci_workflow_path(path),
            is_typescript_runtime_module_index: is_typescript_runtime_module_index_path(path),
            is_runtime_config_artifact,
            is_repo_root_runtime_config_artifact: is_runtime_config_artifact
                && !normalized_path.contains('/'),
            is_example_support: super::super::super::surfaces::is_example_support_path(path),
            is_bench_support: super::super::super::surfaces::is_bench_support_path(path),
            is_test_support: is_test_support_path(path),
            is_generic_runtime_witness_doc:
                super::super::super::surfaces::is_generic_runtime_witness_doc_path(path),
            is_python_runtime_config: super::super::super::surfaces::is_python_runtime_config_path(
                path,
            ),
            is_entrypoint_reference_doc:
                super::super::super::surfaces::is_entrypoint_reference_doc_path(path),
            is_repo_metadata: is_repo_metadata_path(path),
            is_laravel_non_livewire_blade_view:
                super::super::super::is_laravel_non_livewire_blade_view_path(path),
            is_laravel_livewire_view: super::super::super::is_laravel_livewire_view_path(path),
            is_laravel_blade_component: super::super::super::is_laravel_blade_component_path(path),
            is_laravel_layout_blade_view: super::super::super::is_laravel_layout_blade_view_path(
                path,
            ),
            is_laravel_view_component_class:
                super::super::super::is_laravel_view_component_class_path(path),
        }
    }
}

#[cfg(test)]
impl Default for PathQualityFacts {
    fn default() -> Self {
        Self {
            class: HybridSourceClass::Other,
            base_multiplier: 1.0,
            wants_docs: false,
            wants_readme: false,
            wants_onboarding: false,
            wants_contracts: false,
            wants_error_taxonomy: false,
            wants_tool_contracts: false,
            wants_mcp_runtime_surface: false,
            wants_examples: false,
            wants_benchmarks: false,
            wants_tests: false,
            wants_fixtures: false,
            wants_runtime: false,
            wants_runtime_witnesses: false,
            wants_runtime_config_artifacts: false,
            wants_entrypoint_build_flow: false,
            wants_navigation_fallbacks: false,
            wants_laravel_ui_witnesses: false,
            wants_blade_component_witnesses: false,
            wants_laravel_layout_witnesses: false,
            wants_test_witness_recall: false,
            wants_example_or_bench_witnesses: false,
            penalize_generic_runtime_docs: false,
            is_root_readme: false,
            is_entrypoint_runtime: false,
            is_entrypoint_build_workflow: false,
            is_navigation_runtime: false,
            is_navigation_reference_doc: false,
            is_ci_workflow: false,
            is_typescript_runtime_module_index: false,
            is_runtime_config_artifact: false,
            is_repo_root_runtime_config_artifact: false,
            is_example_support: false,
            is_bench_support: false,
            is_test_support: false,
            is_generic_runtime_witness_doc: false,
            is_python_runtime_config: false,
            is_entrypoint_reference_doc: false,
            is_repo_metadata: false,
            is_laravel_non_livewire_blade_view: false,
            is_laravel_livewire_view: false,
            is_laravel_blade_component: false,
            is_laravel_layout_blade_view: false,
            is_laravel_view_component_class: false,
        }
    }
}
