use super::{PredicateLeaf, SelectionFacts};
use crate::searcher::surfaces::HybridSourceClass;

fn class_is_runtime(ctx: &SelectionFacts) -> bool {
    ctx.class == HybridSourceClass::Runtime
}

fn class_is_support(ctx: &SelectionFacts) -> bool {
    ctx.class == HybridSourceClass::Support
}

fn class_is_tests(ctx: &SelectionFacts) -> bool {
    ctx.class == HybridSourceClass::Tests
}

fn class_is_fixtures(ctx: &SelectionFacts) -> bool {
    ctx.class == HybridSourceClass::Fixtures
}

fn class_is_documentation(ctx: &SelectionFacts) -> bool {
    ctx.class == HybridSourceClass::Documentation
}

fn class_is_readme(ctx: &SelectionFacts) -> bool {
    ctx.class == HybridSourceClass::Readme
}

fn class_is_specs(ctx: &SelectionFacts) -> bool {
    ctx.class == HybridSourceClass::Specs
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

fn candidate_language_known(ctx: &SelectionFacts) -> bool {
    ctx.candidate_language_known
}

fn matches_query_language(ctx: &SelectionFacts) -> bool {
    ctx.matches_query_language
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

fn path_depth_is_one_or_less(ctx: &SelectionFacts) -> bool {
    ctx.path_depth <= 1
}

fn runtime_subtree_affinity_positive(ctx: &SelectionFacts) -> bool {
    ctx.runtime_subtree_affinity > 0
}

fn runtime_subtree_affinity_at_least_two(ctx: &SelectionFacts) -> bool {
    ctx.runtime_subtree_affinity >= 2
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

macro_rules! leaf {
    ($name:ident, $id:literal, $pred:ident) => {
        pub(crate) const fn $name() -> PredicateLeaf<SelectionFacts> {
            PredicateLeaf::new($id, $pred)
        }
    };
}

leaf!(
    class_is_runtime_leaf,
    "candidate.class.runtime",
    class_is_runtime
);
leaf!(
    class_is_support_leaf,
    "candidate.class.support",
    class_is_support
);
leaf!(class_is_tests_leaf, "candidate.class.tests", class_is_tests);
leaf!(
    class_is_fixtures_leaf,
    "candidate.class.fixtures",
    class_is_fixtures
);
leaf!(
    class_is_documentation_leaf,
    "candidate.class.documentation",
    class_is_documentation
);
leaf!(
    class_is_readme_leaf,
    "candidate.class.readme",
    class_is_readme
);
leaf!(class_is_specs_leaf, "candidate.class.specs", class_is_specs);
leaf!(
    has_exact_query_term_match_leaf,
    "candidate.exact_query_term_match",
    has_exact_query_term_match
);
leaf!(
    excerpt_has_exact_identifier_anchor_leaf,
    "candidate.excerpt_exact_identifier_anchor",
    excerpt_has_exact_identifier_anchor
);
leaf!(
    has_path_witness_source_leaf,
    "candidate.path_witness_source",
    has_path_witness_source
);
leaf!(path_overlap_leaf, "candidate.path_overlap", path_overlap);
leaf!(
    specific_witness_path_overlap_leaf,
    "candidate.specific_witness_path_overlap",
    specific_witness_path_overlap
);
leaf!(
    blade_specific_path_overlap_leaf,
    "candidate.blade_specific_path_overlap",
    blade_specific_path_overlap
);
leaf!(
    is_runtime_config_artifact_leaf,
    "candidate.runtime_config_artifact",
    is_runtime_config_artifact
);
leaf!(
    is_repo_root_runtime_config_artifact_leaf,
    "candidate.repo_root_runtime_config_artifact",
    is_repo_root_runtime_config_artifact
);
leaf!(
    is_typescript_runtime_module_index_leaf,
    "candidate.typescript_runtime_module_index",
    is_typescript_runtime_module_index
);
leaf!(
    is_entrypoint_runtime_leaf,
    "candidate.entrypoint_runtime",
    is_entrypoint_runtime
);
leaf!(
    is_entrypoint_build_workflow_leaf,
    "candidate.entrypoint_build_workflow",
    is_entrypoint_build_workflow
);
leaf!(
    is_python_runtime_config_leaf,
    "candidate.python_runtime_config",
    is_python_runtime_config
);
leaf!(
    is_python_entrypoint_runtime_leaf,
    "candidate.python_entrypoint_runtime",
    is_python_entrypoint_runtime
);
leaf!(
    is_python_test_witness_leaf,
    "candidate.python_test_witness",
    is_python_test_witness
);
leaf!(
    is_loose_python_test_module_leaf,
    "candidate.loose_python_test_module",
    is_loose_python_test_module
);
leaf!(
    is_rust_workspace_config_leaf,
    "candidate.rust_workspace_config",
    is_rust_workspace_config
);
leaf!(is_ci_workflow_leaf, "candidate.ci_workflow", is_ci_workflow);
leaf!(
    is_example_support_leaf,
    "candidate.example_support",
    is_example_support
);
leaf!(
    is_bench_support_leaf,
    "candidate.bench_support",
    is_bench_support
);
leaf!(
    is_test_support_leaf,
    "candidate.test_support",
    is_test_support
);
leaf!(
    candidate_language_known_leaf,
    "candidate.language_known",
    candidate_language_known
);
leaf!(
    matches_query_language_leaf,
    "candidate.language_matches_query",
    matches_query_language
);
leaf!(is_examples_rs_leaf, "candidate.examples_rs", is_examples_rs);
leaf!(
    path_stem_is_server_or_cli_leaf,
    "candidate.path_stem_server_or_cli",
    path_stem_is_server_or_cli
);
leaf!(
    path_stem_is_main_leaf,
    "candidate.path_stem_main",
    path_stem_is_main
);
leaf!(
    is_cli_test_support_leaf,
    "candidate.cli_test_support",
    is_cli_test_support
);
leaf!(
    is_runtime_anchor_test_support_leaf,
    "candidate.runtime_anchor_test_support",
    is_runtime_anchor_test_support
);
leaf!(
    is_test_harness_leaf,
    "candidate.test_harness",
    is_test_harness
);
leaf!(
    is_non_code_test_doc_leaf,
    "candidate.non_code_test_doc",
    is_non_code_test_doc
);
leaf!(
    is_generic_runtime_witness_doc_leaf,
    "candidate.generic_runtime_witness_doc",
    is_generic_runtime_witness_doc
);
leaf!(
    has_laravel_surface_leaf,
    "candidate.laravel_surface.present",
    has_laravel_surface
);
leaf!(
    excerpt_has_build_flow_anchor_leaf,
    "candidate.excerpt_build_flow_anchor",
    excerpt_has_build_flow_anchor
);
leaf!(
    excerpt_has_test_double_anchor_leaf,
    "candidate.excerpt_test_double_anchor",
    excerpt_has_test_double_anchor
);
leaf!(
    is_entrypoint_reference_doc_leaf,
    "candidate.entrypoint_reference_doc",
    is_entrypoint_reference_doc
);
leaf!(
    is_navigation_runtime_leaf,
    "candidate.navigation_runtime",
    is_navigation_runtime
);
leaf!(
    is_navigation_reference_doc_leaf,
    "candidate.navigation_reference_doc",
    is_navigation_reference_doc
);
leaf!(is_scripts_ops_leaf, "candidate.scripts_ops", is_scripts_ops);
leaf!(
    is_runtime_adjacent_python_test_leaf,
    "candidate.runtime_adjacent_python_test",
    is_runtime_adjacent_python_test
);
leaf!(
    is_non_prefix_python_test_module_leaf,
    "candidate.non_prefix_python_test_module",
    is_non_prefix_python_test_module
);
leaf!(
    runtime_family_prefix_overlap_is_zero_leaf,
    "candidate.runtime_family_prefix_overlap_zero",
    runtime_family_prefix_overlap_is_zero
);
leaf!(
    runtime_family_prefix_overlap_at_least_four_leaf,
    "candidate.runtime_family_prefix_overlap_at_least_four",
    runtime_family_prefix_overlap_at_least_four
);
leaf!(
    runtime_family_prefix_overlap_one_or_two_leaf,
    "candidate.runtime_family_prefix_overlap_one_or_two",
    runtime_family_prefix_overlap_one_or_two
);
leaf!(
    path_depth_at_least_four_leaf,
    "candidate.path_depth_at_least_four",
    path_depth_at_least_four
);
leaf!(
    path_depth_is_one_or_less_leaf,
    "candidate.path_depth_is_one_or_less",
    path_depth_is_one_or_less
);
leaf!(
    runtime_subtree_affinity_positive_leaf,
    "candidate.runtime_subtree_affinity_positive",
    runtime_subtree_affinity_positive
);
leaf!(
    runtime_subtree_affinity_at_least_two_leaf,
    "candidate.runtime_subtree_affinity_at_least_two",
    runtime_subtree_affinity_at_least_two
);
leaf!(
    is_repo_metadata_leaf,
    "candidate.repo_metadata",
    is_repo_metadata
);
leaf!(
    has_generic_runtime_anchor_stem_leaf,
    "candidate.generic_runtime_anchor_stem",
    has_generic_runtime_anchor_stem
);
leaf!(
    is_frontend_runtime_noise_leaf,
    "candidate.frontend_runtime_noise",
    is_frontend_runtime_noise
);
