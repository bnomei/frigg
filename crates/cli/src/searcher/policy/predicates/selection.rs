#![allow(dead_code)]

pub(super) use super::super::dsl::PredicateLeaf;
pub(super) use super::super::facts::SelectionFacts;

mod candidate;
mod intent;
mod laravel;
mod query;
mod state;

pub(crate) use candidate::*;
pub(crate) use intent::*;
pub(crate) use laravel::*;
pub(crate) use query::*;
pub(crate) use state::*;

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
        assert!((path_depth_is_one_or_less_leaf().eval)(&facts));
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
        assert!(!(path_depth_is_one_or_less_leaf().eval)(&facts));
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
        assert!(!(wants_language_locality_bias_leaf().eval)(&facts));
        assert!(!(candidate_language_known_leaf().eval)(&facts));
        assert!(!(matches_query_language_leaf().eval)(&facts));
        assert!(!(runtime_subtree_affinity_positive_leaf().eval)(&facts));
        assert!(!(runtime_subtree_affinity_at_least_two_leaf().eval)(&facts));

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
        facts.wants_language_locality_bias = true;
        facts.candidate_language_known = true;
        facts.matches_query_language = true;
        facts.runtime_subtree_affinity = 2;

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
        assert!((wants_language_locality_bias_leaf().eval)(&facts));
        assert!((candidate_language_known_leaf().eval)(&facts));
        assert!((matches_query_language_leaf().eval)(&facts));
        assert!((runtime_subtree_affinity_positive_leaf().eval)(&facts));
        assert!((runtime_subtree_affinity_at_least_two_leaf().eval)(&facts));

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
