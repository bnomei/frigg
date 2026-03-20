use super::{PredicateLeaf, SelectionFacts};

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

fn lexical_only_mode(ctx: &SelectionFacts) -> bool {
    ctx.lexical_only_mode
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

fn wants_language_locality_bias(ctx: &SelectionFacts) -> bool {
    ctx.wants_language_locality_bias
}

macro_rules! leaf {
    ($name:ident, $id:literal, $pred:ident) => {
        pub(crate) const fn $name() -> PredicateLeaf<SelectionFacts> {
            PredicateLeaf::new($id, $pred)
        }
    };
}

leaf!(
    wants_entrypoint_build_flow_leaf,
    "intent.entrypoint_build_flow",
    wants_entrypoint_build_flow
);
leaf!(
    wants_runtime_witnesses_leaf,
    "intent.runtime_witnesses",
    wants_runtime_witnesses
);
leaf!(wants_class_leaf, "intent.class", wants_class);
leaf!(
    wants_runtime_config_artifacts_leaf,
    "intent.runtime_config_artifacts",
    wants_runtime_config_artifacts
);
leaf!(
    wants_test_witness_recall_leaf,
    "intent.test_witness_recall",
    wants_test_witness_recall
);
leaf!(wants_examples_leaf, "intent.examples", wants_examples);
leaf!(wants_benchmarks_leaf, "intent.benchmarks", wants_benchmarks);
leaf!(
    lexical_only_mode_leaf,
    "execution.lexical_only_mode",
    lexical_only_mode
);
leaf!(
    wants_example_or_bench_witnesses_leaf,
    "intent.example_or_bench_witnesses",
    wants_example_or_bench_witnesses
);
leaf!(
    wants_language_locality_bias_leaf,
    "intent.language_locality_bias",
    wants_language_locality_bias
);
leaf!(
    wants_python_witnesses_leaf,
    "intent.python_witnesses",
    wants_python_witnesses
);
leaf!(
    wants_rust_workspace_config_leaf,
    "intent.rust_workspace_config",
    wants_rust_workspace_config
);
leaf!(
    wants_python_workspace_config_leaf,
    "intent.python_workspace_config",
    wants_python_workspace_config
);
leaf!(
    penalize_generic_runtime_docs_leaf,
    "intent.penalize_generic_runtime_docs",
    penalize_generic_runtime_docs
);
leaf!(
    wants_laravel_ui_witnesses_leaf,
    "intent.laravel_ui_witnesses",
    wants_laravel_ui_witnesses
);
leaf!(
    wants_blade_component_witnesses_leaf,
    "intent.blade_component_witnesses",
    wants_blade_component_witnesses
);
leaf!(
    wants_laravel_form_action_witnesses_leaf,
    "intent.laravel_form_action_witnesses",
    wants_laravel_form_action_witnesses
);
leaf!(
    wants_livewire_view_witnesses_leaf,
    "intent.livewire_view_witnesses",
    wants_livewire_view_witnesses
);
leaf!(
    wants_commands_middleware_witnesses_leaf,
    "intent.commands_middleware_witnesses",
    wants_commands_middleware_witnesses
);
leaf!(
    wants_jobs_listeners_witnesses_leaf,
    "intent.jobs_listeners_witnesses",
    wants_jobs_listeners_witnesses
);
leaf!(
    wants_laravel_layout_witnesses_leaf,
    "intent.laravel_layout_witnesses",
    wants_laravel_layout_witnesses
);
leaf!(
    wants_mcp_runtime_surface_leaf,
    "intent.mcp_runtime_surface",
    wants_mcp_runtime_surface
);
leaf!(
    wants_runtime_companion_tests_leaf,
    "intent.runtime_companion_tests",
    wants_runtime_companion_tests
);
leaf!(
    prefer_runtime_anchor_tests_leaf,
    "intent.prefer_runtime_anchor_tests",
    prefer_runtime_anchor_tests
);
leaf!(
    wants_navigation_fallbacks_leaf,
    "intent.navigation_fallbacks",
    wants_navigation_fallbacks
);
leaf!(
    wants_ci_workflow_witnesses_leaf,
    "intent.ci_workflow_witnesses",
    wants_ci_workflow_witnesses
);
leaf!(
    wants_scripts_ops_witnesses_leaf,
    "intent.scripts_ops_witnesses",
    wants_scripts_ops_witnesses
);
leaf!(
    wants_contractish_leaf,
    "intent.contractish",
    wants_contractish
);
leaf!(
    wants_runtime_or_entrypoint_build_flow_leaf,
    "intent.runtime_or_entrypoint_build_flow",
    wants_runtime_or_entrypoint_build_flow
);
leaf!(
    wants_runtime_config_or_entrypoint_build_flow_leaf,
    "intent.runtime_config_or_entrypoint_build_flow",
    wants_runtime_config_or_entrypoint_build_flow
);
leaf!(
    wants_mixed_query_example_or_bench_leaf,
    "intent.mixed_query_example_or_bench",
    wants_mixed_query_example_or_bench
);
