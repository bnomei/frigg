#![allow(dead_code)]

use super::super::dsl::PredicateLeaf;
use super::super::facts::PathWitnessFacts;

fn wants_entrypoint_build_flow(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_entrypoint_build_flow
}

fn wants_runtime_config_artifacts(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_runtime_config_artifacts
}

fn wants_ci_workflow_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_ci_workflow_witnesses
}

fn wants_examples(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_examples
}

fn wants_benchmarks(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_benchmarks
}

fn wants_example_or_bench_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_example_or_bench_witnesses
}

fn wants_test_witness_recall(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_test_witness_recall
}

fn wants_python_workspace_config(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_python_workspace_config
}

fn wants_rust_workspace_config(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_rust_workspace_config
}

fn wants_python_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_python_witnesses
}

fn wants_laravel_ui_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_laravel_ui_witnesses
}

fn wants_blade_component_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_blade_component_witnesses
}

fn wants_laravel_form_action_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_laravel_form_action_witnesses
}

fn wants_laravel_layout_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_laravel_layout_witnesses
}

fn wants_livewire_view_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_livewire_view_witnesses
}

fn wants_commands_middleware_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_commands_middleware_witnesses
}

fn wants_jobs_listeners_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_jobs_listeners_witnesses
}

fn wants_kotlin_android_ui_witnesses(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_kotlin_android_ui_witnesses
}

fn wants_entrypoint_or_runtime_config(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_entrypoint_build_flow || ctx.wants_runtime_config_artifacts
}

fn wants_entrypoint_or_runtime_config_or_test(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_entrypoint_build_flow
        || ctx.wants_runtime_config_artifacts
        || ctx.wants_test_witness_recall
}

fn mixed_example_or_bench_examples_rs(ctx: &PathWitnessFacts) -> bool {
    ctx.wants_example_or_bench_witnesses && ctx.is_examples_rs
}

fn query_mentions_cli(ctx: &PathWitnessFacts) -> bool {
    ctx.query_mentions_cli
}

fn has_specific_query_terms(ctx: &PathWitnessFacts) -> bool {
    ctx.has_specific_query_terms
}

fn path_overlap(ctx: &PathWitnessFacts) -> bool {
    ctx.path_overlap > 0
}

fn specific_path_overlap(ctx: &PathWitnessFacts) -> bool {
    ctx.specific_path_overlap > 0
}

fn path_overlap_at_least_two(ctx: &PathWitnessFacts) -> bool {
    ctx.path_overlap >= 2
}

fn specific_path_overlap_at_least_two(ctx: &PathWitnessFacts) -> bool {
    ctx.specific_path_overlap >= 2
}

fn has_exact_query_term_match(ctx: &PathWitnessFacts) -> bool {
    ctx.has_exact_query_term_match
}

fn is_entrypoint(ctx: &PathWitnessFacts) -> bool {
    ctx.is_entrypoint
}

fn is_entrypoint_build_workflow(ctx: &PathWitnessFacts) -> bool {
    ctx.is_entrypoint_build_workflow
}

fn is_ci_workflow(ctx: &PathWitnessFacts) -> bool {
    ctx.is_ci_workflow
}

fn is_config_artifact(ctx: &PathWitnessFacts) -> bool {
    ctx.is_config_artifact
}

fn is_typescript_runtime_module_index(ctx: &PathWitnessFacts) -> bool {
    ctx.is_typescript_runtime_module_index
}

fn is_python_config(ctx: &PathWitnessFacts) -> bool {
    ctx.is_python_config
}

fn is_rust_workspace_config(ctx: &PathWitnessFacts) -> bool {
    ctx.is_rust_workspace_config
}

fn is_repo_root_runtime_config_artifact(ctx: &PathWitnessFacts) -> bool {
    ctx.is_repo_root_runtime_config_artifact
}

fn is_package_surface(ctx: &PathWitnessFacts) -> bool {
    ctx.is_package_surface
}

fn is_build_config_surface(ctx: &PathWitnessFacts) -> bool {
    ctx.is_build_config_surface
}

fn is_workspace_config_surface(ctx: &PathWitnessFacts) -> bool {
    ctx.is_workspace_config_surface
}

fn is_python_test(ctx: &PathWitnessFacts) -> bool {
    ctx.is_python_test
}

fn is_test_support(ctx: &PathWitnessFacts) -> bool {
    ctx.is_test_support
}

fn is_runtime_anchor_test_support(ctx: &PathWitnessFacts) -> bool {
    ctx.is_runtime_anchor_test_support
}

fn is_example_support(ctx: &PathWitnessFacts) -> bool {
    ctx.is_example_support
}

fn is_bench_support(ctx: &PathWitnessFacts) -> bool {
    ctx.is_bench_support
}

fn is_cli_test(ctx: &PathWitnessFacts) -> bool {
    ctx.is_cli_test
}

fn is_test_harness(ctx: &PathWitnessFacts) -> bool {
    ctx.is_test_harness
}

fn is_scripts_ops(ctx: &PathWitnessFacts) -> bool {
    ctx.is_scripts_ops
}

fn is_runtime_adjacent_python_test(ctx: &PathWitnessFacts) -> bool {
    ctx.is_runtime_adjacent_python_test
}

fn is_kotlin_android_ui_runtime_surface(ctx: &PathWitnessFacts) -> bool {
    ctx.is_kotlin_android_ui_runtime_surface
}

fn is_examples_rs(ctx: &PathWitnessFacts) -> bool {
    ctx.is_examples_rs
}

fn source_class_is_runtime_support_tests(ctx: &PathWitnessFacts) -> bool {
    matches!(
        ctx.source_class,
        crate::searcher::surfaces::HybridSourceClass::Runtime
            | crate::searcher::surfaces::HybridSourceClass::Support
            | crate::searcher::surfaces::HybridSourceClass::Tests
    )
}

fn is_laravel_non_livewire_blade_view(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_non_livewire_blade_view
}

fn is_laravel_livewire_view(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_livewire_view
}

fn is_laravel_top_level_blade_view(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_top_level_blade_view
}

fn is_laravel_partial_view(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_partial_view
}

fn is_laravel_blade_component(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_blade_component
}

fn is_laravel_nested_blade_component(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_nested_blade_component
}

fn is_laravel_form_action_blade(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_form_action_blade
}

fn is_laravel_livewire_component(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_livewire_component
}

fn is_laravel_view_component_class(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_view_component_class
}

fn is_laravel_command_or_middleware(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_command_or_middleware
}

fn is_laravel_job_or_listener(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_job_or_listener
}

fn is_laravel_layout_blade_view(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_layout_blade_view
}

fn is_laravel_route(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_route
}

fn is_laravel_bootstrap_entrypoint(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_bootstrap_entrypoint
}

fn is_laravel_core_provider(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_core_provider
}

fn is_laravel_provider(ctx: &PathWitnessFacts) -> bool {
    ctx.is_laravel_provider
}

fn is_frontend_runtime_noise(ctx: &PathWitnessFacts) -> bool {
    ctx.is_frontend_runtime_noise
}

fn path_stem_is_server_or_cli(ctx: &PathWitnessFacts) -> bool {
    ctx.path_stem_is_server_or_cli
}

fn path_stem_is_main(ctx: &PathWitnessFacts) -> bool {
    ctx.path_stem_is_main
}

pub(crate) const fn wants_entrypoint_build_flow_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.entrypoint_build_flow", wants_entrypoint_build_flow)
}

pub(crate) const fn wants_runtime_config_artifacts_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.runtime_config_artifacts",
        wants_runtime_config_artifacts,
    )
}

pub(crate) const fn wants_ci_workflow_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.ci_workflow_witnesses", wants_ci_workflow_witnesses)
}

pub(crate) const fn wants_examples_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.examples", wants_examples)
}

pub(crate) const fn wants_benchmarks_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.benchmarks", wants_benchmarks)
}

pub(crate) const fn wants_example_or_bench_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.example_or_bench_witnesses",
        wants_example_or_bench_witnesses,
    )
}

pub(crate) const fn wants_test_witness_recall_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.test_witness_recall", wants_test_witness_recall)
}

pub(crate) const fn wants_python_workspace_config_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.python_workspace_config",
        wants_python_workspace_config,
    )
}

pub(crate) const fn wants_rust_workspace_config_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.rust_workspace_config", wants_rust_workspace_config)
}

pub(crate) const fn wants_python_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.python_witnesses", wants_python_witnesses)
}

pub(crate) const fn wants_laravel_ui_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("intent.laravel_ui_witnesses", wants_laravel_ui_witnesses)
}

pub(crate) const fn wants_blade_component_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.blade_component_witnesses",
        wants_blade_component_witnesses,
    )
}

pub(crate) const fn wants_laravel_form_action_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.laravel_form_action_witnesses",
        wants_laravel_form_action_witnesses,
    )
}

pub(crate) const fn wants_laravel_layout_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.laravel_layout_witnesses",
        wants_laravel_layout_witnesses,
    )
}

pub(crate) const fn wants_livewire_view_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.livewire_view_witnesses",
        wants_livewire_view_witnesses,
    )
}

pub(crate) const fn wants_commands_middleware_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.commands_middleware_witnesses",
        wants_commands_middleware_witnesses,
    )
}

pub(crate) const fn wants_jobs_listeners_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.jobs_listeners_witnesses",
        wants_jobs_listeners_witnesses,
    )
}

pub(crate) const fn wants_kotlin_android_ui_witnesses_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.kotlin_android_ui_witnesses",
        wants_kotlin_android_ui_witnesses,
    )
}

pub(crate) const fn wants_entrypoint_or_runtime_config_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.entrypoint_or_runtime_config",
        wants_entrypoint_or_runtime_config,
    )
}

pub(crate) const fn wants_entrypoint_or_runtime_config_or_test_leaf()
-> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "intent.entrypoint_or_runtime_config_or_test",
        wants_entrypoint_or_runtime_config_or_test,
    )
}

pub(crate) const fn mixed_example_or_bench_examples_rs_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.mixed_example_or_bench_examples_rs",
        mixed_example_or_bench_examples_rs,
    )
}

pub(crate) const fn query_mentions_cli_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("query.mentions_cli", query_mentions_cli)
}

pub(crate) const fn has_specific_query_terms_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("query.has_specific_query_terms", has_specific_query_terms)
}

pub(crate) const fn path_overlap_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.path_overlap", path_overlap)
}

pub(crate) const fn path_overlap_at_least_two_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.path_overlap_at_least_two",
        path_overlap_at_least_two,
    )
}

pub(crate) const fn specific_path_overlap_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.specific_path_overlap", specific_path_overlap)
}

pub(crate) const fn specific_path_overlap_at_least_two_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.specific_path_overlap_at_least_two",
        specific_path_overlap_at_least_two,
    )
}

pub(crate) const fn has_exact_query_term_match_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.exact_query_term_match",
        has_exact_query_term_match,
    )
}

pub(crate) const fn is_entrypoint_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.entrypoint", is_entrypoint)
}

pub(crate) const fn is_entrypoint_build_workflow_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.entrypoint_build_workflow",
        is_entrypoint_build_workflow,
    )
}

pub(crate) const fn is_ci_workflow_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.ci_workflow", is_ci_workflow)
}

pub(crate) const fn is_config_artifact_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.config_artifact", is_config_artifact)
}

pub(crate) const fn is_typescript_runtime_module_index_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.typescript_runtime_module_index",
        is_typescript_runtime_module_index,
    )
}

pub(crate) const fn is_python_config_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.python_config", is_python_config)
}

pub(crate) const fn is_rust_workspace_config_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.rust_workspace_config", is_rust_workspace_config)
}

pub(crate) const fn is_repo_root_runtime_config_artifact_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.repo_root_runtime_config_artifact",
        is_repo_root_runtime_config_artifact,
    )
}

pub(crate) const fn is_package_surface_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.package_surface", is_package_surface)
}

pub(crate) const fn is_build_config_surface_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.build_config_surface", is_build_config_surface)
}

pub(crate) const fn is_workspace_config_surface_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.workspace_config_surface",
        is_workspace_config_surface,
    )
}

pub(crate) const fn is_python_test_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.python_test", is_python_test)
}

pub(crate) const fn is_test_support_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.test_support", is_test_support)
}

pub(crate) const fn is_runtime_anchor_test_support_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.runtime_anchor_test_support",
        is_runtime_anchor_test_support,
    )
}

pub(crate) const fn is_example_support_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.example_support", is_example_support)
}

pub(crate) const fn is_bench_support_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.bench_support", is_bench_support)
}

pub(crate) const fn is_cli_test_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.cli_test", is_cli_test)
}

pub(crate) const fn is_test_harness_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.test_harness", is_test_harness)
}

pub(crate) const fn is_scripts_ops_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.scripts_ops", is_scripts_ops)
}

pub(crate) const fn is_runtime_adjacent_python_test_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.runtime_adjacent_python_test",
        is_runtime_adjacent_python_test,
    )
}

pub(crate) const fn is_kotlin_android_ui_runtime_surface_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.kotlin_android_ui_runtime_surface",
        is_kotlin_android_ui_runtime_surface,
    )
}

pub(crate) const fn is_examples_rs_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.examples_rs", is_examples_rs)
}

pub(crate) const fn source_class_is_runtime_support_tests_leaf() -> PredicateLeaf<PathWitnessFacts>
{
    PredicateLeaf::new(
        "candidate.source_class.runtime_support_tests",
        source_class_is_runtime_support_tests,
    )
}

pub(crate) const fn is_laravel_non_livewire_blade_view_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_non_livewire_blade_view",
        is_laravel_non_livewire_blade_view,
    )
}

pub(crate) const fn is_laravel_livewire_view_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.laravel_livewire_view", is_laravel_livewire_view)
}

pub(crate) const fn is_laravel_top_level_blade_view_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_top_level_blade_view",
        is_laravel_top_level_blade_view,
    )
}

pub(crate) const fn is_laravel_partial_view_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.laravel_partial_view", is_laravel_partial_view)
}

pub(crate) const fn is_laravel_blade_component_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_blade_component",
        is_laravel_blade_component,
    )
}

pub(crate) const fn is_laravel_nested_blade_component_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_nested_blade_component",
        is_laravel_nested_blade_component,
    )
}

pub(crate) const fn is_laravel_form_action_blade_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_form_action_blade",
        is_laravel_form_action_blade,
    )
}

pub(crate) const fn is_laravel_livewire_component_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_livewire_component",
        is_laravel_livewire_component,
    )
}

pub(crate) const fn is_laravel_view_component_class_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_view_component_class",
        is_laravel_view_component_class,
    )
}

pub(crate) const fn is_laravel_command_or_middleware_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_command_or_middleware",
        is_laravel_command_or_middleware,
    )
}

pub(crate) const fn is_laravel_job_or_listener_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_job_or_listener",
        is_laravel_job_or_listener,
    )
}

pub(crate) const fn is_laravel_layout_blade_view_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_layout_blade_view",
        is_laravel_layout_blade_view,
    )
}

pub(crate) const fn is_laravel_route_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.laravel_route", is_laravel_route)
}

pub(crate) const fn is_laravel_bootstrap_entrypoint_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.laravel_bootstrap_entrypoint",
        is_laravel_bootstrap_entrypoint,
    )
}

pub(crate) const fn is_laravel_core_provider_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.laravel_core_provider", is_laravel_core_provider)
}

pub(crate) const fn is_laravel_provider_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.laravel_provider", is_laravel_provider)
}

pub(crate) const fn is_frontend_runtime_noise_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.frontend_runtime_noise",
        is_frontend_runtime_noise,
    )
}

pub(crate) const fn path_stem_is_server_or_cli_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new(
        "candidate.path_stem_server_or_cli",
        path_stem_is_server_or_cli,
    )
}

pub(crate) const fn path_stem_is_main_leaf() -> PredicateLeaf<PathWitnessFacts> {
    PredicateLeaf::new("candidate.path_stem_main", path_stem_is_main)
}

#[cfg(test)]
mod tests {
    use super::super::super::facts::PathWitnessFacts;
    use super::*;

    #[test]
    fn path_witness_predicates_apply_query_and_candidate_flags() {
        let mut facts = PathWitnessFacts::default();
        facts.path_overlap = 3;
        facts.specific_path_overlap = 2;
        facts.has_specific_query_terms = true;
        facts.query_mentions_cli = true;
        facts.has_exact_query_term_match = true;
        facts.is_entrypoint = true;
        facts.is_ci_workflow = true;
        facts.is_config_artifact = true;
        facts.source_class = crate::searcher::surfaces::HybridSourceClass::Runtime;
        facts.path_stem_is_main = true;

        assert!((path_overlap_leaf().eval)(&facts));
        assert!((specific_path_overlap_leaf().eval)(&facts));
        assert!((path_overlap_at_least_two_leaf().eval)(&facts));
        assert!((specific_path_overlap_at_least_two_leaf().eval)(&facts));
        assert!((has_specific_query_terms_leaf().eval)(&facts));
        assert!((query_mentions_cli_leaf().eval)(&facts));
        assert!((has_exact_query_term_match_leaf().eval)(&facts));
        assert!((is_entrypoint_leaf().eval)(&facts));
        assert!((is_ci_workflow_leaf().eval)(&facts));
        assert!((is_config_artifact_leaf().eval)(&facts));
        assert!((path_stem_is_main_leaf().eval)(&facts));
    }

    #[test]
    fn path_witness_predicates_capture_intent_and_runtime_combinations() {
        let mut facts = PathWitnessFacts::default();
        facts.wants_entrypoint_build_flow = true;
        facts.wants_example_or_bench_witnesses = true;
        facts.wants_examples = true;
        facts.wants_benchmarks = true;
        facts.wants_test_witness_recall = false;
        facts.wants_python_workspace_config = true;
        facts.wants_rust_workspace_config = true;
        facts.wants_python_witnesses = true;
        facts.is_examples_rs = true;

        assert!((wants_entrypoint_or_runtime_config_leaf().eval)(&facts));
        assert!((wants_entrypoint_or_runtime_config_or_test_leaf().eval)(
            &facts
        ));
        assert!((mixed_example_or_bench_examples_rs_leaf().eval)(&facts));

        facts.wants_entrypoint_build_flow = false;
        assert!(!(wants_entrypoint_or_runtime_config_or_test_leaf().eval)(
            &facts
        ));
        facts.wants_test_witness_recall = true;
        assert!((wants_entrypoint_or_runtime_config_or_test_leaf().eval)(
            &facts
        ));
    }

    #[test]
    fn path_witness_predicates_capture_stateful_class_checks() {
        let mut facts = PathWitnessFacts::default();
        facts.source_class = crate::searcher::surfaces::HybridSourceClass::Tests;
        facts.is_test_support = true;
        facts.is_examples_rs = true;
        facts.is_laravel_non_livewire_blade_view = true;
        facts.is_frontend_runtime_noise = true;
        facts.is_laravel_layout_blade_view = true;
        facts.is_laravel_bootstrap_entrypoint = true;
        facts.path_stem_is_server_or_cli = true;

        assert!((source_class_is_runtime_support_tests_leaf().eval)(&facts));
        assert!((is_test_support_leaf().eval)(&facts));
        assert!((is_examples_rs_leaf().eval)(&facts));
        assert!((is_laravel_non_livewire_blade_view_leaf().eval)(&facts));
        assert!((is_frontend_runtime_noise_leaf().eval)(&facts));
        assert!((is_laravel_layout_blade_view_leaf().eval)(&facts));
        assert!((is_laravel_bootstrap_entrypoint_leaf().eval)(&facts));
        assert!((path_stem_is_server_or_cli_leaf().eval)(&facts));
    }

    #[test]
    fn path_witness_predicates_cover_overlap_thresholds_and_negatives() {
        let mut facts = PathWitnessFacts::default();

        assert!(!(path_overlap_leaf().eval)(&facts));
        assert!(!(path_overlap_at_least_two_leaf().eval)(&facts));
        assert!(!(specific_path_overlap_leaf().eval)(&facts));
        assert!(!(specific_path_overlap_at_least_two_leaf().eval)(&facts));
        assert!(!(has_exact_query_term_match_leaf().eval)(&facts));
        assert!(!(is_python_config_leaf().eval)(&facts));
        assert!(!(is_rust_workspace_config_leaf().eval)(&facts));
        assert!(!(is_runtime_anchor_test_support_leaf().eval)(&facts));
        assert!(!(is_python_test_leaf().eval)(&facts));

        facts.path_overlap = 1;
        facts.specific_path_overlap = 1;
        assert!((path_overlap_leaf().eval)(&facts));
        assert!(!(path_overlap_at_least_two_leaf().eval)(&facts));
        assert!((specific_path_overlap_leaf().eval)(&facts));
        assert!(!(specific_path_overlap_at_least_two_leaf().eval)(&facts));

        facts.path_overlap = 2;
        facts.specific_path_overlap = 2;
        assert!((path_overlap_at_least_two_leaf().eval)(&facts));
        assert!((specific_path_overlap_at_least_two_leaf().eval)(&facts));
        facts.has_exact_query_term_match = true;
        facts.query_mentions_cli = true;
        facts.has_specific_query_terms = true;
        assert!((has_exact_query_term_match_leaf().eval)(&facts));
        assert!((query_mentions_cli_leaf().eval)(&facts));
        assert!((has_specific_query_terms_leaf().eval)(&facts));
    }

    #[test]
    fn path_witness_predicates_cover_entrypoint_and_witness_or_logic() {
        let mut facts = PathWitnessFacts::default();
        facts.wants_entrypoint_build_flow = true;
        facts.wants_runtime_config_artifacts = false;
        facts.wants_test_witness_recall = false;
        facts.wants_example_or_bench_witnesses = false;
        facts.is_examples_rs = false;

        assert!((wants_entrypoint_or_runtime_config_leaf().eval)(&facts));
        assert!((wants_entrypoint_or_runtime_config_or_test_leaf().eval)(
            &facts
        ));
        assert!(!(mixed_example_or_bench_examples_rs_leaf().eval)(&facts));

        facts.wants_entrypoint_build_flow = false;
        facts.wants_runtime_config_artifacts = false;
        facts.wants_test_witness_recall = false;
        assert!(!(wants_entrypoint_or_runtime_config_leaf().eval)(&facts));
        assert!(!(wants_entrypoint_or_runtime_config_or_test_leaf().eval)(
            &facts
        ));

        facts.wants_runtime_config_artifacts = true;
        assert!((wants_entrypoint_or_runtime_config_leaf().eval)(&facts));
        assert!((wants_entrypoint_or_runtime_config_or_test_leaf().eval)(
            &facts
        ));

        facts.wants_runtime_config_artifacts = false;
        facts.wants_test_witness_recall = true;
        assert!((wants_entrypoint_or_runtime_config_or_test_leaf().eval)(
            &facts
        ));
    }

    #[test]
    fn path_witness_predicates_cover_source_class_paths() {
        let mut facts = PathWitnessFacts::default();
        facts.source_class = crate::searcher::surfaces::HybridSourceClass::Other;
        assert!(!(source_class_is_runtime_support_tests_leaf().eval)(&facts));

        facts.source_class = crate::searcher::surfaces::HybridSourceClass::Runtime;
        assert!((source_class_is_runtime_support_tests_leaf().eval)(&facts));

        facts.source_class = crate::searcher::surfaces::HybridSourceClass::Support;
        assert!((source_class_is_runtime_support_tests_leaf().eval)(&facts));

        facts.source_class = crate::searcher::surfaces::HybridSourceClass::Tests;
        assert!((source_class_is_runtime_support_tests_leaf().eval)(&facts));

        facts.source_class = crate::searcher::surfaces::HybridSourceClass::Fixtures;
        assert!(!(source_class_is_runtime_support_tests_leaf().eval)(&facts));
    }
}
