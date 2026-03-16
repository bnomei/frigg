#![allow(dead_code)]

use super::super::dsl::PredicateLeaf;
use super::super::facts::SelectionFacts;

fn wants_entrypoint_build_flow(ctx: &SelectionFacts) -> bool {
    ctx.wants_entrypoint_build_flow
}

fn wants_runtime_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_runtime_witnesses
}

fn wants_class(ctx: &SelectionFacts) -> bool {
    ctx.wants_class
}

fn wants_runtime_config_artifacts(ctx: &SelectionFacts) -> bool {
    ctx.wants_runtime_config_artifacts
}

fn wants_test_witness_recall(ctx: &SelectionFacts) -> bool {
    ctx.wants_test_witness_recall
}

fn wants_examples(ctx: &SelectionFacts) -> bool {
    ctx.wants_examples
}

fn wants_benchmarks(ctx: &SelectionFacts) -> bool {
    ctx.wants_benchmarks
}

fn wants_example_or_bench_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_example_or_bench_witnesses
}

fn wants_python_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_python_witnesses
}

fn wants_mcp_runtime_surface(ctx: &SelectionFacts) -> bool {
    ctx.wants_mcp_runtime_surface
}

fn wants_runtime_companion_tests(ctx: &SelectionFacts) -> bool {
    ctx.wants_runtime_companion_tests
}

fn prefer_runtime_anchor_tests(ctx: &SelectionFacts) -> bool {
    ctx.prefer_runtime_anchor_tests
}

fn wants_rust_workspace_config(ctx: &SelectionFacts) -> bool {
    ctx.wants_rust_workspace_config
}

fn wants_python_workspace_config(ctx: &SelectionFacts) -> bool {
    ctx.wants_python_workspace_config
}

fn penalize_generic_runtime_docs(ctx: &SelectionFacts) -> bool {
    ctx.penalize_generic_runtime_docs
}

fn wants_laravel_ui_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_laravel_ui_witnesses
}

fn wants_blade_component_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_blade_component_witnesses
}

fn wants_laravel_form_action_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_laravel_form_action_witnesses
}

fn wants_livewire_view_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_livewire_view_witnesses
}

fn wants_commands_middleware_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_commands_middleware_witnesses
}

fn wants_jobs_listeners_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_jobs_listeners_witnesses
}

fn wants_laravel_layout_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_laravel_layout_witnesses
}

fn wants_navigation_fallbacks(ctx: &SelectionFacts) -> bool {
    ctx.wants_navigation_fallbacks
}

fn wants_ci_workflow_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_ci_workflow_witnesses
}

fn wants_scripts_ops_witnesses(ctx: &SelectionFacts) -> bool {
    ctx.wants_scripts_ops_witnesses
}

fn query_mentions_cli(ctx: &SelectionFacts) -> bool {
    ctx.query_mentions_cli
}

fn query_has_exact_terms(ctx: &SelectionFacts) -> bool {
    ctx.query_has_exact_terms
}

fn query_has_identifier_anchor(ctx: &SelectionFacts) -> bool {
    ctx.query_has_identifier_anchor
}

fn query_has_specific_blade_anchors(ctx: &SelectionFacts) -> bool {
    ctx.query_has_specific_blade_anchors
}

fn class_is_runtime(ctx: &SelectionFacts) -> bool {
    ctx.class == crate::searcher::surfaces::HybridSourceClass::Runtime
}

fn class_is_support(ctx: &SelectionFacts) -> bool {
    ctx.class == crate::searcher::surfaces::HybridSourceClass::Support
}

fn class_is_tests(ctx: &SelectionFacts) -> bool {
    ctx.class == crate::searcher::surfaces::HybridSourceClass::Tests
}

fn class_is_fixtures(ctx: &SelectionFacts) -> bool {
    ctx.class == crate::searcher::surfaces::HybridSourceClass::Fixtures
}

fn has_exact_query_term_match(ctx: &SelectionFacts) -> bool {
    ctx.has_exact_query_term_match
}

fn excerpt_has_exact_identifier_anchor(ctx: &SelectionFacts) -> bool {
    ctx.excerpt_has_exact_identifier_anchor
}

fn has_path_witness_source(ctx: &SelectionFacts) -> bool {
    ctx.has_path_witness_source
}

fn path_overlap(ctx: &SelectionFacts) -> bool {
    ctx.path_overlap > 0
}

fn specific_witness_path_overlap(ctx: &SelectionFacts) -> bool {
    ctx.specific_witness_path_overlap > 0
}

fn blade_specific_path_overlap(ctx: &SelectionFacts) -> bool {
    ctx.blade_specific_path_overlap > 0
}

fn is_runtime_config_artifact(ctx: &SelectionFacts) -> bool {
    ctx.is_runtime_config_artifact
}

fn is_repo_root_runtime_config_artifact(ctx: &SelectionFacts) -> bool {
    ctx.is_repo_root_runtime_config_artifact
}

fn is_typescript_runtime_module_index(ctx: &SelectionFacts) -> bool {
    ctx.is_typescript_runtime_module_index
}

fn is_entrypoint_runtime(ctx: &SelectionFacts) -> bool {
    ctx.is_entrypoint_runtime
}

fn is_entrypoint_build_workflow(ctx: &SelectionFacts) -> bool {
    ctx.is_entrypoint_build_workflow
}

fn is_python_runtime_config(ctx: &SelectionFacts) -> bool {
    ctx.is_python_runtime_config
}

fn is_python_entrypoint_runtime(ctx: &SelectionFacts) -> bool {
    ctx.is_python_entrypoint_runtime
}

fn is_python_test_witness(ctx: &SelectionFacts) -> bool {
    ctx.is_python_test_witness
}

fn is_loose_python_test_module(ctx: &SelectionFacts) -> bool {
    ctx.is_loose_python_test_module
}

fn is_rust_workspace_config(ctx: &SelectionFacts) -> bool {
    ctx.is_rust_workspace_config
}

fn is_ci_workflow(ctx: &SelectionFacts) -> bool {
    ctx.is_ci_workflow
}

fn is_example_support(ctx: &SelectionFacts) -> bool {
    ctx.is_example_support
}

fn is_bench_support(ctx: &SelectionFacts) -> bool {
    ctx.is_bench_support
}

fn is_test_support(ctx: &SelectionFacts) -> bool {
    ctx.is_test_support
}

fn is_examples_rs(ctx: &SelectionFacts) -> bool {
    ctx.is_examples_rs
}

fn path_stem_is_server_or_cli(ctx: &SelectionFacts) -> bool {
    ctx.path_stem_is_server_or_cli
}

fn path_stem_is_main(ctx: &SelectionFacts) -> bool {
    ctx.path_stem_is_main
}

fn is_cli_test_support(ctx: &SelectionFacts) -> bool {
    ctx.is_cli_test_support
}

fn is_runtime_anchor_test_support(ctx: &SelectionFacts) -> bool {
    ctx.is_runtime_anchor_test_support
}

fn is_test_harness(ctx: &SelectionFacts) -> bool {
    ctx.is_test_harness
}

fn is_non_code_test_doc(ctx: &SelectionFacts) -> bool {
    ctx.is_non_code_test_doc
}

fn is_generic_runtime_witness_doc(ctx: &SelectionFacts) -> bool {
    ctx.is_generic_runtime_witness_doc
}

fn is_laravel_core_provider(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_core_provider
}

fn is_laravel_provider(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_provider
}

fn is_laravel_route(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_route
}

fn is_laravel_bootstrap_entrypoint(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_bootstrap_entrypoint
}

fn is_laravel_non_livewire_blade_view(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_non_livewire_blade_view
}

fn is_laravel_livewire_view(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_livewire_view
}

fn is_laravel_blade_component(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_blade_component
}

fn is_laravel_nested_blade_component(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_nested_blade_component
}

fn is_laravel_form_action_blade(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_form_action_blade
}

fn is_laravel_livewire_component(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_livewire_component
}

fn is_laravel_view_component_class(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_view_component_class
}

fn is_laravel_command_or_middleware(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_command_or_middleware
}

fn is_laravel_job_or_listener(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_job_or_listener
}

fn is_laravel_layout_blade_view(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_layout_blade_view
}

fn laravel_surface_is_blade_view(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(crate::searcher::laravel::LaravelUiSurfaceClass::BladeView)
}

fn laravel_surface_is_livewire_component(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(crate::searcher::laravel::LaravelUiSurfaceClass::LivewireComponent)
}

fn laravel_surface_is_livewire_view(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(crate::searcher::laravel::LaravelUiSurfaceClass::LivewireView)
}

fn laravel_surface_is_blade_component(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(crate::searcher::laravel::LaravelUiSurfaceClass::BladeComponent)
}

fn is_repo_metadata(ctx: &SelectionFacts) -> bool {
    ctx.is_repo_metadata
}

fn has_generic_runtime_anchor_stem(ctx: &SelectionFacts) -> bool {
    ctx.has_generic_runtime_anchor_stem
}

fn is_frontend_runtime_noise(ctx: &SelectionFacts) -> bool {
    ctx.is_frontend_runtime_noise
}

fn seen_count_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.seen_count == 0
}

fn runtime_seen_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.runtime_seen == 0
}

fn has_seen_repo_root_runtime_config(ctx: &SelectionFacts) -> bool {
    ctx.seen_repo_root_runtime_configs > 0
}

fn laravel_surface_seen_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface_seen == 0
}

fn wants_contractish(ctx: &SelectionFacts) -> bool {
    ctx.wants_contracts || ctx.wants_error_taxonomy || ctx.wants_tool_contracts
}

fn wants_runtime_or_entrypoint_build_flow(ctx: &SelectionFacts) -> bool {
    ctx.wants_runtime_witnesses || ctx.wants_entrypoint_build_flow
}

fn wants_runtime_config_or_entrypoint_build_flow(ctx: &SelectionFacts) -> bool {
    ctx.wants_runtime_config_artifacts || ctx.wants_entrypoint_build_flow
}

fn wants_mixed_query_example_or_bench(ctx: &SelectionFacts) -> bool {
    ctx.wants_test_witness_recall && ctx.wants_example_or_bench_witnesses
}

fn class_is_documentation(ctx: &SelectionFacts) -> bool {
    ctx.class == crate::searcher::surfaces::HybridSourceClass::Documentation
}

fn class_is_readme(ctx: &SelectionFacts) -> bool {
    ctx.class == crate::searcher::surfaces::HybridSourceClass::Readme
}

fn class_is_specs(ctx: &SelectionFacts) -> bool {
    ctx.class == crate::searcher::surfaces::HybridSourceClass::Specs
}

fn has_laravel_surface(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface.is_some()
}

fn excerpt_has_build_flow_anchor(ctx: &SelectionFacts) -> bool {
    ctx.excerpt_has_build_flow_anchor
}

fn excerpt_has_test_double_anchor(ctx: &SelectionFacts) -> bool {
    ctx.excerpt_has_test_double_anchor
}

fn is_entrypoint_reference_doc(ctx: &SelectionFacts) -> bool {
    ctx.is_entrypoint_reference_doc
}

fn is_navigation_runtime(ctx: &SelectionFacts) -> bool {
    ctx.is_navigation_runtime
}

fn is_navigation_reference_doc(ctx: &SelectionFacts) -> bool {
    ctx.is_navigation_reference_doc
}

fn is_scripts_ops(ctx: &SelectionFacts) -> bool {
    ctx.is_scripts_ops
}

fn is_runtime_adjacent_python_test(ctx: &SelectionFacts) -> bool {
    ctx.is_runtime_adjacent_python_test
}

fn is_non_prefix_python_test_module(ctx: &SelectionFacts) -> bool {
    ctx.is_non_prefix_python_test_module
}

fn runtime_family_prefix_overlap_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.runtime_family_prefix_overlap == 0
}

fn runtime_family_prefix_overlap_at_least_four(ctx: &SelectionFacts) -> bool {
    ctx.runtime_family_prefix_overlap >= 4
}

fn runtime_family_prefix_overlap_one_or_two(ctx: &SelectionFacts) -> bool {
    (1..=2).contains(&ctx.runtime_family_prefix_overlap)
}

fn path_depth_at_least_four(ctx: &SelectionFacts) -> bool {
    ctx.path_depth >= 4
}

fn seen_count_positive(ctx: &SelectionFacts) -> bool {
    ctx.seen_count > 0
}

fn runtime_seen_positive(ctx: &SelectionFacts) -> bool {
    ctx.runtime_seen > 0
}

fn seen_ci_workflows_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.seen_ci_workflows == 0
}

fn seen_ci_workflows_positive(ctx: &SelectionFacts) -> bool {
    ctx.seen_ci_workflows > 0
}

fn seen_example_support_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.seen_example_support == 0
}

fn seen_example_support_positive(ctx: &SelectionFacts) -> bool {
    ctx.seen_example_support > 0
}

fn seen_bench_support_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.seen_bench_support == 0
}

fn seen_bench_support_positive(ctx: &SelectionFacts) -> bool {
    ctx.seen_bench_support > 0
}

fn seen_plain_test_support_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.seen_plain_test_support == 0
}

fn seen_plain_test_support_positive(ctx: &SelectionFacts) -> bool {
    ctx.seen_plain_test_support > 0
}

fn laravel_surface_seen_positive(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface_seen > 0
}

fn seen_typescript_runtime_module_indexes_is_zero(ctx: &SelectionFacts) -> bool {
    ctx.seen_typescript_runtime_module_indexes == 0
}

pub(crate) const fn wants_entrypoint_build_flow_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.entrypoint_build_flow", wants_entrypoint_build_flow)
}

pub(crate) const fn wants_runtime_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.runtime_witnesses", wants_runtime_witnesses)
}

pub(crate) const fn wants_runtime_config_artifacts_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.runtime_config_artifacts",
        wants_runtime_config_artifacts,
    )
}

pub(crate) const fn wants_test_witness_recall_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.test_witness_recall", wants_test_witness_recall)
}

pub(crate) const fn wants_examples_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.examples", wants_examples)
}

pub(crate) const fn wants_benchmarks_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.benchmarks", wants_benchmarks)
}

pub(crate) const fn wants_example_or_bench_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.example_or_bench_witnesses",
        wants_example_or_bench_witnesses,
    )
}

pub(crate) const fn wants_python_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.python_witnesses", wants_python_witnesses)
}

pub(crate) const fn wants_rust_workspace_config_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.rust_workspace_config", wants_rust_workspace_config)
}

pub(crate) const fn wants_python_workspace_config_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.python_workspace_config",
        wants_python_workspace_config,
    )
}

pub(crate) const fn penalize_generic_runtime_docs_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.penalize_generic_runtime_docs",
        penalize_generic_runtime_docs,
    )
}

pub(crate) const fn wants_laravel_ui_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.laravel_ui_witnesses", wants_laravel_ui_witnesses)
}

pub(crate) const fn wants_blade_component_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.blade_component_witnesses",
        wants_blade_component_witnesses,
    )
}

pub(crate) const fn wants_laravel_form_action_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.laravel_form_action_witnesses",
        wants_laravel_form_action_witnesses,
    )
}

pub(crate) const fn wants_livewire_view_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.livewire_view_witnesses",
        wants_livewire_view_witnesses,
    )
}

pub(crate) const fn wants_commands_middleware_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.commands_middleware_witnesses",
        wants_commands_middleware_witnesses,
    )
}

pub(crate) const fn wants_jobs_listeners_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.jobs_listeners_witnesses",
        wants_jobs_listeners_witnesses,
    )
}

pub(crate) const fn wants_laravel_layout_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.laravel_layout_witnesses",
        wants_laravel_layout_witnesses,
    )
}

pub(crate) const fn query_mentions_cli_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("query.mentions_cli", query_mentions_cli)
}

pub(crate) const fn query_has_specific_blade_anchors_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "query.has_specific_blade_anchors",
        query_has_specific_blade_anchors,
    )
}

pub(crate) const fn class_is_runtime_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.class.runtime", class_is_runtime)
}

pub(crate) const fn class_is_support_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.class.support", class_is_support)
}

pub(crate) const fn class_is_tests_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.class.tests", class_is_tests)
}

pub(crate) const fn class_is_fixtures_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.class.fixtures", class_is_fixtures)
}

pub(crate) const fn has_exact_query_term_match_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.exact_query_term_match",
        has_exact_query_term_match,
    )
}

pub(crate) const fn excerpt_has_exact_identifier_anchor_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.excerpt_exact_identifier_anchor",
        excerpt_has_exact_identifier_anchor,
    )
}

pub(crate) const fn has_path_witness_source_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.path_witness_source", has_path_witness_source)
}

pub(crate) const fn path_overlap_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.path_overlap", path_overlap)
}

pub(crate) const fn specific_witness_path_overlap_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.specific_witness_path_overlap",
        specific_witness_path_overlap,
    )
}

pub(crate) const fn blade_specific_path_overlap_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.blade_specific_path_overlap",
        blade_specific_path_overlap,
    )
}

pub(crate) const fn is_runtime_config_artifact_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.runtime_config_artifact",
        is_runtime_config_artifact,
    )
}

pub(crate) const fn is_repo_root_runtime_config_artifact_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.repo_root_runtime_config_artifact",
        is_repo_root_runtime_config_artifact,
    )
}

pub(crate) const fn is_typescript_runtime_module_index_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.typescript_runtime_module_index",
        is_typescript_runtime_module_index,
    )
}

pub(crate) const fn is_entrypoint_runtime_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.entrypoint_runtime", is_entrypoint_runtime)
}

pub(crate) const fn is_entrypoint_build_workflow_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.entrypoint_build_workflow",
        is_entrypoint_build_workflow,
    )
}

pub(crate) const fn is_python_runtime_config_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.python_runtime_config", is_python_runtime_config)
}

pub(crate) const fn is_python_entrypoint_runtime_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.python_entrypoint_runtime",
        is_python_entrypoint_runtime,
    )
}

pub(crate) const fn is_python_test_witness_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.python_test_witness", is_python_test_witness)
}

pub(crate) const fn is_loose_python_test_module_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.loose_python_test_module",
        is_loose_python_test_module,
    )
}

pub(crate) const fn is_rust_workspace_config_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.rust_workspace_config", is_rust_workspace_config)
}

pub(crate) const fn is_ci_workflow_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.ci_workflow", is_ci_workflow)
}

pub(crate) const fn is_example_support_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.example_support", is_example_support)
}

pub(crate) const fn is_bench_support_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.bench_support", is_bench_support)
}

pub(crate) const fn is_test_support_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.test_support", is_test_support)
}

pub(crate) const fn is_examples_rs_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.examples_rs", is_examples_rs)
}

pub(crate) const fn path_stem_is_server_or_cli_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.path_stem_server_or_cli",
        path_stem_is_server_or_cli,
    )
}

pub(crate) const fn path_stem_is_main_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.path_stem_main", path_stem_is_main)
}

pub(crate) const fn is_cli_test_support_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.cli_test_support", is_cli_test_support)
}

pub(crate) const fn is_runtime_anchor_test_support_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.runtime_anchor_test_support",
        is_runtime_anchor_test_support,
    )
}

pub(crate) const fn is_test_harness_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.test_harness", is_test_harness)
}

pub(crate) const fn is_non_code_test_doc_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.non_code_test_doc", is_non_code_test_doc)
}

pub(crate) const fn is_generic_runtime_witness_doc_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.generic_runtime_witness_doc",
        is_generic_runtime_witness_doc,
    )
}

pub(crate) const fn is_laravel_core_provider_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.laravel_core_provider", is_laravel_core_provider)
}

pub(crate) const fn is_laravel_provider_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.laravel_provider", is_laravel_provider)
}

pub(crate) const fn is_laravel_route_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.laravel_route", is_laravel_route)
}

pub(crate) const fn is_laravel_bootstrap_entrypoint_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_bootstrap_entrypoint",
        is_laravel_bootstrap_entrypoint,
    )
}

pub(crate) const fn is_laravel_non_livewire_blade_view_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_non_livewire_blade_view",
        is_laravel_non_livewire_blade_view,
    )
}

pub(crate) const fn is_laravel_livewire_view_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.laravel_livewire_view", is_laravel_livewire_view)
}

pub(crate) const fn is_laravel_blade_component_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_blade_component",
        is_laravel_blade_component,
    )
}

pub(crate) const fn is_laravel_nested_blade_component_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_nested_blade_component",
        is_laravel_nested_blade_component,
    )
}

pub(crate) const fn is_laravel_form_action_blade_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_form_action_blade",
        is_laravel_form_action_blade,
    )
}

pub(crate) const fn is_laravel_livewire_component_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_livewire_component",
        is_laravel_livewire_component,
    )
}

pub(crate) const fn is_laravel_view_component_class_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_view_component_class",
        is_laravel_view_component_class,
    )
}

pub(crate) const fn is_laravel_command_or_middleware_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_command_or_middleware",
        is_laravel_command_or_middleware,
    )
}

pub(crate) const fn is_laravel_job_or_listener_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_job_or_listener",
        is_laravel_job_or_listener,
    )
}

pub(crate) const fn is_laravel_layout_blade_view_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_layout_blade_view",
        is_laravel_layout_blade_view,
    )
}

pub(crate) const fn laravel_surface_is_blade_view_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_surface.blade_view",
        laravel_surface_is_blade_view,
    )
}

pub(crate) const fn laravel_surface_is_livewire_component_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_surface.livewire_component",
        laravel_surface_is_livewire_component,
    )
}

pub(crate) const fn laravel_surface_is_livewire_view_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_surface.livewire_view",
        laravel_surface_is_livewire_view,
    )
}

pub(crate) const fn laravel_surface_is_blade_component_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.laravel_surface.blade_component",
        laravel_surface_is_blade_component,
    )
}

pub(crate) const fn is_repo_metadata_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.repo_metadata", is_repo_metadata)
}

pub(crate) const fn has_generic_runtime_anchor_stem_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.generic_runtime_anchor_stem",
        has_generic_runtime_anchor_stem,
    )
}

pub(crate) const fn is_frontend_runtime_noise_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.frontend_runtime_noise",
        is_frontend_runtime_noise,
    )
}

pub(crate) const fn seen_count_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("state.seen_count_zero", seen_count_is_zero)
}

pub(crate) const fn runtime_seen_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("state.runtime_seen_zero", runtime_seen_is_zero)
}

pub(crate) const fn has_seen_repo_root_runtime_config_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.has_seen_repo_root_runtime_config",
        has_seen_repo_root_runtime_config,
    )
}

pub(crate) const fn laravel_surface_seen_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.laravel_surface_seen_zero",
        laravel_surface_seen_is_zero,
    )
}

pub(crate) const fn wants_class_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.class", wants_class)
}

pub(crate) const fn wants_mcp_runtime_surface_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.mcp_runtime_surface", wants_mcp_runtime_surface)
}

pub(crate) const fn wants_runtime_companion_tests_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.runtime_companion_tests",
        wants_runtime_companion_tests,
    )
}

pub(crate) const fn prefer_runtime_anchor_tests_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.prefer_runtime_anchor_tests",
        prefer_runtime_anchor_tests,
    )
}

pub(crate) const fn wants_navigation_fallbacks_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.navigation_fallbacks", wants_navigation_fallbacks)
}

pub(crate) const fn wants_ci_workflow_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.ci_workflow_witnesses", wants_ci_workflow_witnesses)
}

pub(crate) const fn wants_scripts_ops_witnesses_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.scripts_ops_witnesses", wants_scripts_ops_witnesses)
}

pub(crate) const fn wants_contractish_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("intent.contractish", wants_contractish)
}

pub(crate) const fn wants_runtime_or_entrypoint_build_flow_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.runtime_or_entrypoint_build_flow",
        wants_runtime_or_entrypoint_build_flow,
    )
}

pub(crate) const fn wants_runtime_config_or_entrypoint_build_flow_leaf()
-> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.runtime_config_or_entrypoint_build_flow",
        wants_runtime_config_or_entrypoint_build_flow,
    )
}

pub(crate) const fn wants_mixed_query_example_or_bench_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "intent.mixed_query_example_or_bench",
        wants_mixed_query_example_or_bench,
    )
}

pub(crate) const fn query_has_exact_terms_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("query.has_exact_terms", query_has_exact_terms)
}

pub(crate) const fn query_has_identifier_anchor_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("query.has_identifier_anchor", query_has_identifier_anchor)
}

pub(crate) const fn class_is_documentation_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.class.documentation", class_is_documentation)
}

pub(crate) const fn class_is_readme_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.class.readme", class_is_readme)
}

pub(crate) const fn class_is_specs_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.class.specs", class_is_specs)
}

pub(crate) const fn has_laravel_surface_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.laravel_surface.present", has_laravel_surface)
}

pub(crate) const fn excerpt_has_build_flow_anchor_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.excerpt_build_flow_anchor",
        excerpt_has_build_flow_anchor,
    )
}

pub(crate) const fn excerpt_has_test_double_anchor_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.excerpt_test_double_anchor",
        excerpt_has_test_double_anchor,
    )
}

pub(crate) const fn is_entrypoint_reference_doc_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.entrypoint_reference_doc",
        is_entrypoint_reference_doc,
    )
}

pub(crate) const fn is_navigation_runtime_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.navigation_runtime", is_navigation_runtime)
}

pub(crate) const fn is_navigation_reference_doc_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.navigation_reference_doc",
        is_navigation_reference_doc,
    )
}

pub(crate) const fn is_scripts_ops_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("candidate.scripts_ops", is_scripts_ops)
}

pub(crate) const fn is_runtime_adjacent_python_test_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.runtime_adjacent_python_test",
        is_runtime_adjacent_python_test,
    )
}

pub(crate) const fn is_non_prefix_python_test_module_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.non_prefix_python_test_module",
        is_non_prefix_python_test_module,
    )
}

pub(crate) const fn runtime_family_prefix_overlap_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.runtime_family_prefix_overlap_zero",
        runtime_family_prefix_overlap_is_zero,
    )
}

pub(crate) const fn runtime_family_prefix_overlap_at_least_four_leaf()
-> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.runtime_family_prefix_overlap_at_least_four",
        runtime_family_prefix_overlap_at_least_four,
    )
}

pub(crate) const fn runtime_family_prefix_overlap_one_or_two_leaf() -> PredicateLeaf<SelectionFacts>
{
    PredicateLeaf::new(
        "candidate.runtime_family_prefix_overlap_one_or_two",
        runtime_family_prefix_overlap_one_or_two,
    )
}

pub(crate) const fn path_depth_at_least_four_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "candidate.path_depth_at_least_four",
        path_depth_at_least_four,
    )
}

pub(crate) const fn seen_count_positive_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("state.seen_count_positive", seen_count_positive)
}

pub(crate) const fn runtime_seen_positive_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("state.runtime_seen_positive", runtime_seen_positive)
}

pub(crate) const fn seen_ci_workflows_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("state.seen_ci_workflows_zero", seen_ci_workflows_is_zero)
}

pub(crate) const fn seen_ci_workflows_positive_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.seen_ci_workflows_positive",
        seen_ci_workflows_positive,
    )
}

pub(crate) const fn seen_example_support_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.seen_example_support_zero",
        seen_example_support_is_zero,
    )
}

pub(crate) const fn seen_example_support_positive_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.seen_example_support_positive",
        seen_example_support_positive,
    )
}

pub(crate) const fn seen_bench_support_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new("state.seen_bench_support_zero", seen_bench_support_is_zero)
}

pub(crate) const fn seen_bench_support_positive_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.seen_bench_support_positive",
        seen_bench_support_positive,
    )
}

pub(crate) const fn seen_plain_test_support_is_zero_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.seen_plain_test_support_zero",
        seen_plain_test_support_is_zero,
    )
}

pub(crate) const fn seen_plain_test_support_positive_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.seen_plain_test_support_positive",
        seen_plain_test_support_positive,
    )
}

pub(crate) const fn laravel_surface_seen_positive_leaf() -> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.laravel_surface_seen_positive",
        laravel_surface_seen_positive,
    )
}

pub(crate) const fn seen_typescript_runtime_module_indexes_is_zero_leaf()
-> PredicateLeaf<SelectionFacts> {
    PredicateLeaf::new(
        "state.seen_typescript_runtime_module_indexes_zero",
        seen_typescript_runtime_module_indexes_is_zero,
    )
}

#[cfg(test)]
mod tests {
    use super::super::super::facts::SelectionFacts;
    use super::*;
    use crate::searcher::laravel::LaravelUiSurfaceClass;

    #[test]
    fn selection_predicates_apply_candidate_and_query_flags() {
        let mut facts = SelectionFacts::default();
        facts.class = crate::searcher::surfaces::HybridSourceClass::Runtime;
        facts.path_overlap = 1;
        facts.blade_specific_path_overlap = 1;
        facts.specific_witness_path_overlap = 1;
        facts.query_has_exact_terms = true;
        facts.query_has_identifier_anchor = true;
        facts.has_exact_query_term_match = true;
        facts.path_stem_is_main = true;
        facts.wants_runtime_witnesses = true;

        assert!((class_is_runtime_leaf().eval)(&facts));
        assert!((path_overlap_leaf().eval)(&facts));
        assert!((blade_specific_path_overlap_leaf().eval)(&facts));
        assert!((specific_witness_path_overlap_leaf().eval)(&facts));
        assert!((has_exact_query_term_match_leaf().eval)(&facts));
        assert!((path_stem_is_main_leaf().eval)(&facts));
    }

    #[test]
    fn selection_predicates_reflect_state_counts() {
        let mut facts = SelectionFacts::default();
        facts.seen_count = 0;
        facts.runtime_seen = 0;
        facts.seen_ci_workflows = 1;
        facts.seen_repo_root_runtime_configs = 2;
        facts.seen_typescript_runtime_module_indexes = 0;

        assert!((seen_count_is_zero_leaf().eval)(&facts));
        assert!((runtime_seen_is_zero_leaf().eval)(&facts));
        assert!((seen_ci_workflows_positive_leaf().eval)(&facts));
        assert!((has_seen_repo_root_runtime_config_leaf().eval)(&facts));
        assert!((seen_typescript_runtime_module_indexes_is_zero_leaf().eval)(&facts));
        assert!(!(runtime_seen_positive_leaf().eval)(&facts));
    }

    #[test]
    fn selection_predicates_reflect_intent_combinators() {
        let mut facts = SelectionFacts::default();
        facts.wants_examples = true;
        facts.wants_benchmarks = true;
        facts.wants_runtime_witnesses = false;
        facts.wants_entrypoint_build_flow = true;
        facts.wants_runtime_config_artifacts = false;
        facts.wants_contracts = true;
        facts.wants_test_witness_recall = false;
        facts.wants_example_or_bench_witnesses = true;
        facts.runtime_family_prefix_overlap = 2;

        assert!((wants_example_or_bench_witnesses_leaf().eval)(&facts));
        assert!((wants_runtime_or_entrypoint_build_flow_leaf().eval)(&facts));
        assert!((wants_runtime_config_or_entrypoint_build_flow_leaf().eval)(
            &facts
        ));
        assert!((wants_contractish_leaf().eval)(&facts));
        assert!((runtime_family_prefix_overlap_one_or_two_leaf().eval)(
            &facts
        ));
        assert!(!(runtime_family_prefix_overlap_at_least_four_leaf().eval)(
            &facts
        ));

        facts.wants_entrypoint_build_flow = false;
        assert!(!(wants_runtime_config_or_entrypoint_build_flow_leaf().eval)(&facts));
        facts.wants_runtime_config_artifacts = true;
        assert!((wants_runtime_config_or_entrypoint_build_flow_leaf().eval)(
            &facts
        ));
    }

    #[test]
    fn selection_predicates_handle_ambiguous_runtime_anchor_logic() {
        let mut facts = SelectionFacts::default();
        facts.class = crate::searcher::surfaces::HybridSourceClass::Tests;
        facts.runtime_family_prefix_overlap = 0;
        facts.is_runtime_anchor_test_support = true;
        facts.wants_runtime_companion_tests = true;
        facts.prefer_runtime_anchor_tests = false;

        assert!((class_is_tests_leaf().eval)(&facts));
        assert!((runtime_family_prefix_overlap_is_zero_leaf().eval)(&facts));
        assert!((is_runtime_anchor_test_support_leaf().eval)(&facts));
        assert!((wants_runtime_companion_tests_leaf().eval)(&facts));
        assert!(!(prefer_runtime_anchor_tests_leaf().eval)(&facts));
    }

    #[test]
    fn selection_predicates_cover_overlap_thresholds_and_count_states() {
        let mut facts = SelectionFacts::default();

        assert!(!(path_overlap_leaf().eval)(&facts));
        assert!(!(specific_witness_path_overlap_leaf().eval)(&facts));
        assert!(!(blade_specific_path_overlap_leaf().eval)(&facts));
        assert!(!(path_depth_at_least_four_leaf().eval)(&facts));
        assert!((runtime_family_prefix_overlap_is_zero_leaf().eval)(&facts));
        assert!(!(runtime_family_prefix_overlap_at_least_four_leaf().eval)(
            &facts
        ));
        assert!(!(runtime_family_prefix_overlap_one_or_two_leaf().eval)(
            &facts
        ));
        assert!(!(seen_ci_workflows_positive_leaf().eval)(&facts));
        assert!(!(seen_example_support_positive_leaf().eval)(&facts));
        assert!(!(seen_bench_support_positive_leaf().eval)(&facts));
        assert!(!(seen_plain_test_support_positive_leaf().eval)(&facts));
        assert!(!(laravel_surface_seen_positive_leaf().eval)(&facts));

        facts.path_overlap = 1;
        facts.specific_witness_path_overlap = 1;
        facts.blade_specific_path_overlap = 1;
        facts.path_depth = 4;
        facts.runtime_family_prefix_overlap = 2;
        facts.seen_count = 1;
        facts.runtime_seen = 1;
        facts.seen_ci_workflows = 1;
        facts.seen_example_support = 1;
        facts.seen_bench_support = 1;
        facts.seen_plain_test_support = 1;
        facts.laravel_surface_seen = 1;

        assert!((path_overlap_leaf().eval)(&facts));
        assert!((specific_witness_path_overlap_leaf().eval)(&facts));
        assert!((blade_specific_path_overlap_leaf().eval)(&facts));
        assert!((path_depth_at_least_four_leaf().eval)(&facts));
        assert!(!(runtime_family_prefix_overlap_is_zero_leaf().eval)(&facts));
        assert!(!(runtime_family_prefix_overlap_at_least_four_leaf().eval)(
            &facts
        ));
        assert!((runtime_family_prefix_overlap_one_or_two_leaf().eval)(
            &facts
        ));
        assert!((seen_ci_workflows_positive_leaf().eval)(&facts));
        assert!((seen_example_support_positive_leaf().eval)(&facts));
        assert!((seen_bench_support_positive_leaf().eval)(&facts));
        assert!((seen_plain_test_support_positive_leaf().eval)(&facts));
        assert!((laravel_surface_seen_positive_leaf().eval)(&facts));

        facts.runtime_family_prefix_overlap = 4;
        assert!((runtime_family_prefix_overlap_at_least_four_leaf().eval)(
            &facts
        ));
        assert!(!(runtime_family_prefix_overlap_one_or_two_leaf().eval)(
            &facts
        ));
        facts.runtime_family_prefix_overlap = 0;
        assert!((runtime_family_prefix_overlap_is_zero_leaf().eval)(&facts));
    }

    #[test]
    fn selection_predicates_cover_boolean_queriable_and_query_mix_states() {
        let mut facts = SelectionFacts::default();

        assert!(!(query_has_exact_terms_leaf().eval)(&facts));
        assert!(!(query_has_identifier_anchor_leaf().eval)(&facts));
        assert!(!(query_has_specific_blade_anchors_leaf().eval)(&facts));
        assert!(!(excerpt_has_build_flow_anchor_leaf().eval)(&facts));
        assert!(!(excerpt_has_test_double_anchor_leaf().eval)(&facts));
        assert!(!(query_mentions_cli_leaf().eval)(&facts));
        assert!(!(wants_mcp_runtime_surface_leaf().eval)(&facts));
        assert!(!(wants_runtime_companion_tests_leaf().eval)(&facts));
        assert!(!(prefer_runtime_anchor_tests_leaf().eval)(&facts));

        facts.query_has_exact_terms = true;
        facts.query_has_identifier_anchor = true;
        facts.query_has_specific_blade_anchors = true;
        facts.excerpt_has_build_flow_anchor = true;
        facts.excerpt_has_test_double_anchor = true;
        facts.query_mentions_cli = true;
        facts.wants_mcp_runtime_surface = true;
        facts.wants_runtime_witnesses = true;
        facts.wants_entrypoint_build_flow = true;
        facts.wants_test_witness_recall = false;
        facts.runtime_seen = 1;
        facts.wants_contracts = true;
        facts.wants_error_taxonomy = true;

        assert!((query_has_exact_terms_leaf().eval)(&facts));
        assert!((query_has_identifier_anchor_leaf().eval)(&facts));
        assert!((query_has_specific_blade_anchors_leaf().eval)(&facts));
        assert!((excerpt_has_build_flow_anchor_leaf().eval)(&facts));
        assert!((excerpt_has_test_double_anchor_leaf().eval)(&facts));
        assert!((query_mentions_cli_leaf().eval)(&facts));
        assert!((wants_mcp_runtime_surface_leaf().eval)(&facts));
        assert!((excerpt_has_test_double_anchor_leaf().eval)(&facts));
        assert!(!(wants_runtime_companion_tests_leaf().eval)(&facts));
        assert!(!(prefer_runtime_anchor_tests_leaf().eval)(&facts));

        facts.wants_runtime_companion_tests = true;
        facts.prefer_runtime_anchor_tests = true;
        assert!((wants_runtime_companion_tests_leaf().eval)(&facts));
        assert!((prefer_runtime_anchor_tests_leaf().eval)(&facts));
        assert!((wants_contractish_leaf().eval)(&facts));

        facts.wants_runtime_companion_tests = true;
        facts.prefer_runtime_anchor_tests = false;
        facts.wants_test_witness_recall = true;
        assert!((wants_runtime_companion_tests_leaf().eval)(&facts));
        assert!(!(prefer_runtime_anchor_tests_leaf().eval)(&facts));
        assert!((wants_contractish_leaf().eval)(&facts));
    }

    #[test]
    fn selection_predicates_cover_class_and_surface_variants() {
        let mut facts = SelectionFacts::default();

        facts.class = crate::searcher::surfaces::HybridSourceClass::Documentation;
        assert!((class_is_documentation_leaf().eval)(&facts));
        assert!(!(class_is_readme_leaf().eval)(&facts));
        assert!(!(class_is_specs_leaf().eval)(&facts));

        facts.class = crate::searcher::surfaces::HybridSourceClass::Readme;
        assert!((class_is_readme_leaf().eval)(&facts));
        assert!(!(class_is_documentation_leaf().eval)(&facts));

        facts.class = crate::searcher::surfaces::HybridSourceClass::Specs;
        assert!((class_is_specs_leaf().eval)(&facts));
        assert!(!(class_is_runtime_leaf().eval)(&facts));
        assert!(!(class_is_support_leaf().eval)(&facts));
        assert!(!(class_is_tests_leaf().eval)(&facts));
        assert!(!(class_is_fixtures_leaf().eval)(&facts));

        facts.laravel_surface = Some(LaravelUiSurfaceClass::BladeView);
        facts.path_stem_is_main = true;
        facts.is_laravel_non_livewire_blade_view = true;
        facts.is_laravel_layout_blade_view = true;
        facts.is_laravel_bootstrap_entrypoint = true;
        facts.is_navigation_reference_doc = true;

        assert!((laravel_surface_is_blade_view_leaf().eval)(&facts));
        assert!((has_laravel_surface_leaf().eval)(&facts));
        assert!((path_stem_is_main_leaf().eval)(&facts));
        assert!((is_laravel_non_livewire_blade_view_leaf().eval)(&facts));
        assert!((is_laravel_layout_blade_view_leaf().eval)(&facts));
        assert!((is_laravel_bootstrap_entrypoint_leaf().eval)(&facts));
        assert!((is_navigation_reference_doc_leaf().eval)(&facts));
    }
}
