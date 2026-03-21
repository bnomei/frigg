#![allow(dead_code)]

use super::super::dsl::PredicateLeaf;
use super::super::facts::PathQualityFacts;
use crate::searcher::surfaces::HybridSourceClass;

fn wants_docs(ctx: &PathQualityFacts) -> bool {
    ctx.wants_docs
}

fn wants_readme(ctx: &PathQualityFacts) -> bool {
    ctx.wants_readme
}

fn wants_onboarding(ctx: &PathQualityFacts) -> bool {
    ctx.wants_onboarding
}

fn wants_contracts(ctx: &PathQualityFacts) -> bool {
    ctx.wants_contracts
}

fn wants_error_taxonomy(ctx: &PathQualityFacts) -> bool {
    ctx.wants_error_taxonomy
}

fn wants_tool_contracts(ctx: &PathQualityFacts) -> bool {
    ctx.wants_tool_contracts
}

fn wants_mcp_runtime_surface(ctx: &PathQualityFacts) -> bool {
    ctx.wants_mcp_runtime_surface
}

fn wants_examples(ctx: &PathQualityFacts) -> bool {
    ctx.wants_examples
}

fn wants_benchmarks(ctx: &PathQualityFacts) -> bool {
    ctx.wants_benchmarks
}

fn wants_tests(ctx: &PathQualityFacts) -> bool {
    ctx.wants_tests
}

fn wants_fixtures(ctx: &PathQualityFacts) -> bool {
    ctx.wants_fixtures
}

fn wants_runtime(ctx: &PathQualityFacts) -> bool {
    ctx.wants_runtime
}

fn wants_runtime_witnesses(ctx: &PathQualityFacts) -> bool {
    ctx.wants_runtime_witnesses
}

fn wants_runtime_config_artifacts(ctx: &PathQualityFacts) -> bool {
    ctx.wants_runtime_config_artifacts
}

fn wants_entrypoint_build_flow(ctx: &PathQualityFacts) -> bool {
    ctx.wants_entrypoint_build_flow
}

fn wants_navigation_fallbacks(ctx: &PathQualityFacts) -> bool {
    ctx.wants_navigation_fallbacks
}

fn wants_laravel_ui_witnesses(ctx: &PathQualityFacts) -> bool {
    ctx.wants_laravel_ui_witnesses
}

fn wants_blade_component_witnesses(ctx: &PathQualityFacts) -> bool {
    ctx.wants_blade_component_witnesses
}

fn wants_laravel_layout_witnesses(ctx: &PathQualityFacts) -> bool {
    ctx.wants_laravel_layout_witnesses
}

fn wants_test_witness_recall(ctx: &PathQualityFacts) -> bool {
    ctx.wants_test_witness_recall
}

fn wants_example_or_bench_witnesses(ctx: &PathQualityFacts) -> bool {
    ctx.wants_example_or_bench_witnesses
}

fn penalize_generic_runtime_docs(ctx: &PathQualityFacts) -> bool {
    ctx.penalize_generic_runtime_docs
}

fn class_is_documentation(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::Documentation
}

fn class_is_error_contracts(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::ErrorContracts
}

fn class_is_tool_contracts(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::ToolContracts
}

fn class_is_benchmark_docs(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::BenchmarkDocs
}

fn class_is_readme(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::Readme
}

fn class_is_specs(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::Specs
}

fn class_is_tests(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::Tests
}

fn class_is_fixtures(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::Fixtures
}

fn class_is_runtime(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::Runtime
}

fn class_is_support(ctx: &PathQualityFacts) -> bool {
    ctx.class == HybridSourceClass::Support
}

fn is_root_readme(ctx: &PathQualityFacts) -> bool {
    ctx.is_root_readme
}

fn is_entrypoint_runtime(ctx: &PathQualityFacts) -> bool {
    ctx.is_entrypoint_runtime
}

fn is_entrypoint_build_workflow(ctx: &PathQualityFacts) -> bool {
    ctx.is_entrypoint_build_workflow
}

fn is_navigation_runtime(ctx: &PathQualityFacts) -> bool {
    ctx.is_navigation_runtime
}

fn is_navigation_reference_doc(ctx: &PathQualityFacts) -> bool {
    ctx.is_navigation_reference_doc
}

fn is_ci_workflow(ctx: &PathQualityFacts) -> bool {
    ctx.is_ci_workflow
}

fn is_typescript_runtime_module_index(ctx: &PathQualityFacts) -> bool {
    ctx.is_typescript_runtime_module_index
}

fn is_runtime_config_artifact(ctx: &PathQualityFacts) -> bool {
    ctx.is_runtime_config_artifact
}

fn is_repo_root_runtime_config_artifact(ctx: &PathQualityFacts) -> bool {
    ctx.is_repo_root_runtime_config_artifact
}

fn is_example_support(ctx: &PathQualityFacts) -> bool {
    ctx.is_example_support
}

fn is_bench_support(ctx: &PathQualityFacts) -> bool {
    ctx.is_bench_support
}

fn is_test_support(ctx: &PathQualityFacts) -> bool {
    ctx.is_test_support
}

fn is_generic_runtime_witness_doc(ctx: &PathQualityFacts) -> bool {
    ctx.is_generic_runtime_witness_doc
}

fn is_python_runtime_config(ctx: &PathQualityFacts) -> bool {
    ctx.is_python_runtime_config
}

fn is_entrypoint_reference_doc(ctx: &PathQualityFacts) -> bool {
    ctx.is_entrypoint_reference_doc
}

fn is_repo_metadata(ctx: &PathQualityFacts) -> bool {
    ctx.is_repo_metadata
}

fn is_laravel_non_livewire_blade_view(ctx: &PathQualityFacts) -> bool {
    ctx.is_laravel_non_livewire_blade_view
}

fn is_laravel_livewire_view(ctx: &PathQualityFacts) -> bool {
    ctx.is_laravel_livewire_view
}

fn is_laravel_blade_component(ctx: &PathQualityFacts) -> bool {
    ctx.is_laravel_blade_component
}

fn is_laravel_layout_blade_view(ctx: &PathQualityFacts) -> bool {
    ctx.is_laravel_layout_blade_view
}

fn is_laravel_view_component_class(ctx: &PathQualityFacts) -> bool {
    ctx.is_laravel_view_component_class
}

pub(crate) const fn wants_docs_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.docs", wants_docs)
}

pub(crate) const fn wants_readme_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.readme", wants_readme)
}

pub(crate) const fn wants_onboarding_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.onboarding", wants_onboarding)
}

pub(crate) const fn wants_contracts_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.contracts", wants_contracts)
}

pub(crate) const fn wants_error_taxonomy_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.error_taxonomy", wants_error_taxonomy)
}

pub(crate) const fn wants_tool_contracts_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.tool_contracts", wants_tool_contracts)
}

pub(crate) const fn wants_mcp_runtime_surface_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.mcp_runtime_surface", wants_mcp_runtime_surface)
}

pub(crate) const fn wants_examples_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.examples", wants_examples)
}

pub(crate) const fn wants_benchmarks_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.benchmarks", wants_benchmarks)
}

pub(crate) const fn wants_tests_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.tests", wants_tests)
}

pub(crate) const fn wants_fixtures_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.fixtures", wants_fixtures)
}

pub(crate) const fn wants_runtime_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.runtime", wants_runtime)
}

pub(crate) const fn wants_runtime_witnesses_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.runtime_witnesses", wants_runtime_witnesses)
}

pub(crate) const fn wants_runtime_config_artifacts_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "intent.runtime_config_artifacts",
        wants_runtime_config_artifacts,
    )
}

pub(crate) const fn wants_entrypoint_build_flow_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.entrypoint_build_flow", wants_entrypoint_build_flow)
}

pub(crate) const fn wants_navigation_fallbacks_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.navigation_fallbacks", wants_navigation_fallbacks)
}

pub(crate) const fn wants_laravel_ui_witnesses_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.laravel_ui_witnesses", wants_laravel_ui_witnesses)
}

pub(crate) const fn wants_blade_component_witnesses_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "intent.blade_component_witnesses",
        wants_blade_component_witnesses,
    )
}

pub(crate) const fn wants_laravel_layout_witnesses_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "intent.laravel_layout_witnesses",
        wants_laravel_layout_witnesses,
    )
}

pub(crate) const fn wants_test_witness_recall_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("intent.test_witness_recall", wants_test_witness_recall)
}

pub(crate) const fn wants_example_or_bench_witnesses_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "intent.example_or_bench_witnesses",
        wants_example_or_bench_witnesses,
    )
}

pub(crate) const fn penalize_generic_runtime_docs_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "intent.penalize_generic_runtime_docs",
        penalize_generic_runtime_docs,
    )
}

pub(crate) const fn class_is_documentation_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.documentation", class_is_documentation)
}

pub(crate) const fn class_is_error_contracts_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.error_contracts", class_is_error_contracts)
}

pub(crate) const fn class_is_tool_contracts_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.tool_contracts", class_is_tool_contracts)
}

pub(crate) const fn class_is_benchmark_docs_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.benchmark_docs", class_is_benchmark_docs)
}

pub(crate) const fn class_is_readme_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.readme", class_is_readme)
}

pub(crate) const fn class_is_specs_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.specs", class_is_specs)
}

pub(crate) const fn class_is_tests_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.tests", class_is_tests)
}

pub(crate) const fn class_is_fixtures_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.fixtures", class_is_fixtures)
}

pub(crate) const fn class_is_runtime_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.runtime", class_is_runtime)
}

pub(crate) const fn class_is_support_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.class.support", class_is_support)
}

pub(crate) const fn is_root_readme_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.root_readme", is_root_readme)
}

pub(crate) const fn is_entrypoint_runtime_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.entrypoint_runtime", is_entrypoint_runtime)
}

pub(crate) const fn is_entrypoint_build_workflow_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.entrypoint_build_workflow",
        is_entrypoint_build_workflow,
    )
}

pub(crate) const fn is_navigation_runtime_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.navigation_runtime", is_navigation_runtime)
}

pub(crate) const fn is_navigation_reference_doc_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.navigation_reference_doc",
        is_navigation_reference_doc,
    )
}

pub(crate) const fn is_ci_workflow_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.ci_workflow", is_ci_workflow)
}

pub(crate) const fn is_typescript_runtime_module_index_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.typescript_runtime_module_index",
        is_typescript_runtime_module_index,
    )
}

pub(crate) const fn is_runtime_config_artifact_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.runtime_config_artifact",
        is_runtime_config_artifact,
    )
}

pub(crate) const fn is_repo_root_runtime_config_artifact_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.repo_root_runtime_config_artifact",
        is_repo_root_runtime_config_artifact,
    )
}

pub(crate) const fn is_example_support_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.example_support", is_example_support)
}

pub(crate) const fn is_bench_support_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.bench_support", is_bench_support)
}

pub(crate) const fn is_test_support_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.test_support", is_test_support)
}

pub(crate) const fn is_generic_runtime_witness_doc_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.generic_runtime_witness_doc",
        is_generic_runtime_witness_doc,
    )
}

pub(crate) const fn is_python_runtime_config_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.python_runtime_config", is_python_runtime_config)
}

pub(crate) const fn is_entrypoint_reference_doc_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.entrypoint_reference_doc",
        is_entrypoint_reference_doc,
    )
}

pub(crate) const fn is_repo_metadata_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.repo_metadata", is_repo_metadata)
}

pub(crate) const fn is_laravel_non_livewire_blade_view_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.laravel_non_livewire_blade_view",
        is_laravel_non_livewire_blade_view,
    )
}

pub(crate) const fn is_laravel_livewire_view_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new("candidate.laravel_livewire_view", is_laravel_livewire_view)
}

pub(crate) const fn is_laravel_blade_component_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.laravel_blade_component",
        is_laravel_blade_component,
    )
}

pub(crate) const fn is_laravel_layout_blade_view_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.laravel_layout_blade_view",
        is_laravel_layout_blade_view,
    )
}

pub(crate) const fn is_laravel_view_component_class_leaf() -> PredicateLeaf<PathQualityFacts> {
    PredicateLeaf::new(
        "candidate.laravel_view_component_class",
        is_laravel_view_component_class,
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::bool_comparison, clippy::field_reassign_with_default)]

    use super::*;
    use crate::searcher::HybridRankingIntent;

    #[test]
    fn path_quality_predicates_reflect_intent_flags() {
        let mut facts = PathQualityFacts::default();
        facts.wants_docs = true;
        facts.wants_readme = true;
        facts.wants_onboarding = true;
        facts.wants_contracts = true;
        facts.wants_error_taxonomy = true;
        facts.wants_tool_contracts = true;
        facts.wants_mcp_runtime_surface = true;
        facts.wants_examples = true;
        facts.wants_benchmarks = true;
        facts.wants_tests = true;
        facts.wants_fixtures = true;
        facts.wants_runtime = true;
        facts.wants_runtime_witnesses = true;
        facts.wants_runtime_config_artifacts = true;
        facts.wants_entrypoint_build_flow = true;
        facts.wants_navigation_fallbacks = true;
        facts.wants_laravel_ui_witnesses = true;
        facts.wants_blade_component_witnesses = true;
        facts.wants_laravel_layout_witnesses = true;
        facts.wants_test_witness_recall = true;
        facts.wants_example_or_bench_witnesses = true;
        facts.penalize_generic_runtime_docs = true;

        assert!((wants_docs_leaf().eval)(&facts));
        assert!((wants_readme_leaf().eval)(&facts));
        assert!((wants_onboarding_leaf().eval)(&facts));
        assert!((wants_contracts_leaf().eval)(&facts));
        assert!((wants_error_taxonomy_leaf().eval)(&facts));
        assert!((wants_tool_contracts_leaf().eval)(&facts));
        assert!((wants_mcp_runtime_surface_leaf().eval)(&facts));
        assert!((wants_examples_leaf().eval)(&facts));
        assert!((wants_benchmarks_leaf().eval)(&facts));
        assert!((wants_tests_leaf().eval)(&facts));
        assert!((wants_fixtures_leaf().eval)(&facts));
        assert!((wants_runtime_leaf().eval)(&facts));
        assert!((wants_runtime_witnesses_leaf().eval)(&facts));
        assert!((wants_runtime_config_artifacts_leaf().eval)(&facts));
        assert!((wants_entrypoint_build_flow_leaf().eval)(&facts));
        assert!((wants_navigation_fallbacks_leaf().eval)(&facts));
        assert!((wants_laravel_ui_witnesses_leaf().eval)(&facts));
        assert!((wants_blade_component_witnesses_leaf().eval)(&facts));
        assert!((wants_laravel_layout_witnesses_leaf().eval)(&facts));
        assert!((wants_test_witness_recall_leaf().eval)(&facts));
        assert!((wants_example_or_bench_witnesses_leaf().eval)(&facts));
        assert!((penalize_generic_runtime_docs_leaf().eval)(&facts));
    }

    #[test]
    fn path_quality_predicates_reflect_candidate_class_and_path_flags() {
        let mut facts = PathQualityFacts::default();
        facts.class = HybridSourceClass::Documentation;
        facts.is_root_readme = true;
        facts.is_entrypoint_runtime = true;
        facts.is_entrypoint_build_workflow = true;
        facts.is_navigation_runtime = true;
        facts.is_navigation_reference_doc = true;
        facts.is_ci_workflow = true;
        facts.is_typescript_runtime_module_index = true;
        facts.is_runtime_config_artifact = true;
        facts.is_repo_root_runtime_config_artifact = true;
        facts.is_example_support = true;
        facts.is_bench_support = true;
        facts.is_test_support = true;
        facts.is_generic_runtime_witness_doc = true;
        facts.is_python_runtime_config = true;
        facts.is_entrypoint_reference_doc = true;
        facts.is_repo_metadata = true;
        facts.is_laravel_non_livewire_blade_view = true;
        facts.is_laravel_livewire_view = true;
        facts.is_laravel_blade_component = true;
        facts.is_laravel_layout_blade_view = true;
        facts.is_laravel_view_component_class = true;

        assert!((class_is_documentation_leaf().eval)(&facts));
        assert!(!(class_is_readme_leaf().eval)(&facts));
        assert!((is_root_readme_leaf().eval)(&facts));
        assert!((is_entrypoint_runtime_leaf().eval)(&facts));
        assert!((is_entrypoint_build_workflow_leaf().eval)(&facts));
        assert!((is_navigation_runtime_leaf().eval)(&facts));
        assert!((is_navigation_reference_doc_leaf().eval)(&facts));
        assert!((is_ci_workflow_leaf().eval)(&facts));
        assert!((is_typescript_runtime_module_index_leaf().eval)(&facts));
        assert!((is_runtime_config_artifact_leaf().eval)(&facts));
        assert!((is_repo_root_runtime_config_artifact_leaf().eval)(&facts));
        assert!((is_example_support_leaf().eval)(&facts));
        assert!((is_bench_support_leaf().eval)(&facts));
        assert!((is_test_support_leaf().eval)(&facts));
        assert!((is_generic_runtime_witness_doc_leaf().eval)(&facts));
        assert!((is_python_runtime_config_leaf().eval)(&facts));
        assert!((is_entrypoint_reference_doc_leaf().eval)(&facts));
        assert!((is_repo_metadata_leaf().eval)(&facts));
        assert!((is_laravel_non_livewire_blade_view_leaf().eval)(&facts));
        assert!((is_laravel_livewire_view_leaf().eval)(&facts));
        assert!((is_laravel_blade_component_leaf().eval)(&facts));
        assert!((is_laravel_layout_blade_view_leaf().eval)(&facts));
        assert!((is_laravel_view_component_class_leaf().eval)(&facts));
        assert!((class_is_error_contracts_leaf().eval)(&facts) == false);
        assert!((class_is_tool_contracts_leaf().eval)(&facts) == false);
        assert!((class_is_benchmark_docs_leaf().eval)(&facts) == false);
    }

    #[test]
    fn path_quality_predicates_capture_defaults_as_false() {
        let facts = PathQualityFacts::default();

        assert!(!(wants_docs_leaf().eval)(&facts));
        assert!(!(wants_readme_leaf().eval)(&facts));
        assert!(!(wants_benchmarks_leaf().eval)(&facts));
        assert!(!(class_is_readme_leaf().eval)(&facts));
        assert!(!(class_is_specs_leaf().eval)(&facts));
        assert!(!(class_is_tests_leaf().eval)(&facts));
        assert!(!(class_is_fixtures_leaf().eval)(&facts));
        assert!(!(class_is_runtime_leaf().eval)(&facts));
        assert!(!(class_is_support_leaf().eval)(&facts));
        assert!(!(is_ci_workflow_leaf().eval)(&facts));
        assert!(!(is_repo_metadata_leaf().eval)(&facts));
        assert!(!(is_laravel_non_livewire_blade_view_leaf().eval)(&facts));
    }

    #[test]
    fn path_quality_predicates_derive_from_path_and_intent() {
        let intent = HybridRankingIntent::from_query("docs runtime-config");
        let facts = PathQualityFacts::from_path("README.md", &intent);

        assert!((class_is_readme_leaf().eval)(&facts));
        assert!((wants_docs_leaf().eval)(&facts));
        assert!(!(wants_runtime_config_artifacts_leaf().eval)(&facts));
        assert!((is_root_readme_leaf().eval)(&facts));
    }
}
