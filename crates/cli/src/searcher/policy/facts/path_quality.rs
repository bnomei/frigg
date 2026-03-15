use super::super::super::intent::HybridRankingIntent;
use super::super::super::surfaces::HybridSourceClass;
use super::{SharedIntentFacts, SharedPathFacts};

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
        let shared_intent = SharedIntentFacts::from_intent(intent);
        let shared_path = SharedPathFacts::from_path(path);

        Self {
            class: shared_path.class,
            base_multiplier: shared_path.path_quality_base_multiplier(path),
            wants_docs: shared_intent.wants_docs,
            wants_readme: shared_intent.wants_readme,
            wants_onboarding: shared_intent.wants_onboarding,
            wants_contracts: shared_intent.wants_contracts,
            wants_error_taxonomy: shared_intent.wants_error_taxonomy,
            wants_tool_contracts: shared_intent.wants_tool_contracts,
            wants_mcp_runtime_surface: shared_intent.wants_mcp_runtime_surface,
            wants_examples: shared_intent.wants_examples,
            wants_benchmarks: shared_intent.wants_benchmarks,
            wants_tests: shared_intent.wants_tests,
            wants_fixtures: shared_intent.wants_fixtures,
            wants_runtime: shared_intent.wants_runtime,
            wants_runtime_witnesses: shared_intent.wants_runtime_witnesses,
            wants_runtime_config_artifacts: shared_intent.wants_runtime_config_artifacts,
            wants_entrypoint_build_flow: shared_intent.wants_entrypoint_build_flow,
            wants_navigation_fallbacks: shared_intent.wants_navigation_fallbacks,
            wants_laravel_ui_witnesses: shared_intent.wants_laravel_ui_witnesses,
            wants_blade_component_witnesses: shared_intent.wants_blade_component_witnesses,
            wants_laravel_layout_witnesses: shared_intent.wants_laravel_layout_witnesses,
            wants_test_witness_recall: shared_intent.wants_test_witness_recall,
            wants_example_or_bench_witnesses: shared_intent.wants_example_or_bench_witnesses,
            penalize_generic_runtime_docs: shared_intent.penalize_generic_runtime_docs,
            is_root_readme: shared_path.is_root_readme,
            is_entrypoint_runtime: shared_path.is_entrypoint_runtime,
            is_entrypoint_build_workflow: shared_path.is_entrypoint_build_workflow,
            is_navigation_runtime: shared_path.is_navigation_runtime,
            is_navigation_reference_doc: shared_path.is_navigation_reference_doc,
            is_ci_workflow: shared_path.is_ci_workflow,
            is_typescript_runtime_module_index: shared_path.is_typescript_runtime_module_index,
            is_runtime_config_artifact: shared_path.is_runtime_config_artifact,
            is_repo_root_runtime_config_artifact: shared_path.is_repo_root_runtime_config_artifact,
            is_example_support: shared_path.is_example_support,
            is_bench_support: shared_path.is_bench_support,
            is_test_support: shared_path.is_test_support,
            is_generic_runtime_witness_doc: shared_path.is_generic_runtime_witness_doc,
            is_python_runtime_config: shared_path.is_python_runtime_config,
            is_entrypoint_reference_doc: shared_path.is_entrypoint_reference_doc,
            is_repo_metadata: shared_path.is_repo_metadata,
            is_laravel_non_livewire_blade_view: shared_path.is_laravel_non_livewire_blade_view,
            is_laravel_livewire_view: shared_path.is_laravel_livewire_view,
            is_laravel_blade_component: shared_path.is_laravel_blade_component,
            is_laravel_layout_blade_view: shared_path.is_laravel_layout_blade_view,
            is_laravel_view_component_class: shared_path.is_laravel_view_component_class,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_scoped_runtime_configs_receive_repo_root_quality_flag() {
        let intent = HybridRankingIntent::from_query("config");
        let facts =
            PathQualityFacts::from_path("gradle/wrapper/gradle-wrapper.properties", &intent);

        assert!(facts.is_runtime_config_artifact);
        assert!(facts.is_repo_root_runtime_config_artifact);
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
