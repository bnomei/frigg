use super::{PredicateLeaf, SelectionFacts};

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

macro_rules! leaf {
    ($name:ident, $id:literal, $pred:ident) => {
        pub(crate) const fn $name() -> PredicateLeaf<SelectionFacts> {
            PredicateLeaf::new($id, $pred)
        }
    };
}

leaf!(
    seen_count_is_zero_leaf,
    "state.seen_count_zero",
    seen_count_is_zero
);
leaf!(
    runtime_seen_is_zero_leaf,
    "state.runtime_seen_zero",
    runtime_seen_is_zero
);
leaf!(
    has_seen_repo_root_runtime_config_leaf,
    "state.has_seen_repo_root_runtime_config",
    has_seen_repo_root_runtime_config
);
leaf!(
    laravel_surface_seen_is_zero_leaf,
    "state.laravel_surface_seen_zero",
    laravel_surface_seen_is_zero
);
leaf!(
    seen_count_positive_leaf,
    "state.seen_count_positive",
    seen_count_positive
);
leaf!(
    runtime_seen_positive_leaf,
    "state.runtime_seen_positive",
    runtime_seen_positive
);
leaf!(
    seen_ci_workflows_is_zero_leaf,
    "state.seen_ci_workflows_zero",
    seen_ci_workflows_is_zero
);
leaf!(
    seen_ci_workflows_positive_leaf,
    "state.seen_ci_workflows_positive",
    seen_ci_workflows_positive
);
leaf!(
    seen_example_support_is_zero_leaf,
    "state.seen_example_support_zero",
    seen_example_support_is_zero
);
leaf!(
    seen_example_support_positive_leaf,
    "state.seen_example_support_positive",
    seen_example_support_positive
);
leaf!(
    seen_bench_support_is_zero_leaf,
    "state.seen_bench_support_zero",
    seen_bench_support_is_zero
);
leaf!(
    seen_bench_support_positive_leaf,
    "state.seen_bench_support_positive",
    seen_bench_support_positive
);
leaf!(
    seen_plain_test_support_is_zero_leaf,
    "state.seen_plain_test_support_zero",
    seen_plain_test_support_is_zero
);
leaf!(
    seen_plain_test_support_positive_leaf,
    "state.seen_plain_test_support_positive",
    seen_plain_test_support_positive
);
leaf!(
    laravel_surface_seen_positive_leaf,
    "state.laravel_surface_seen_positive",
    laravel_surface_seen_positive
);
leaf!(
    seen_typescript_runtime_module_indexes_is_zero_leaf,
    "state.seen_typescript_runtime_module_indexes_zero",
    seen_typescript_runtime_module_indexes_is_zero
);
