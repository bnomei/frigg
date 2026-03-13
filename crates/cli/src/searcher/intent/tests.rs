use crate::domain::{
    ArtifactBias, FrameworkHint, PlannerStrictness, PlaybookReferencePolicy, SearchGoal,
    SearchIntentRuleId,
};

use super::SearchIntent;

#[test]
fn docs_and_contract_queries_do_not_activate_test_witness_focus() {
    let intent = SearchIntent::from_query(
        "trace invalid_params typed error from public docs and contracts tests",
    );

    assert!(intent.has_goal(SearchGoal::Documentation));
    assert!(intent.has_goal(SearchGoal::Contracts));
    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(!intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::Broad);
    assert!(
        intent
            .applied_rule_ids()
            .contains(&SearchIntentRuleId::DocumentationTerms)
    );
    assert!(
        intent
            .applied_rule_ids()
            .contains(&SearchIntentRuleId::ContractsTerms)
    );
}

#[test]
fn docs_contract_runtime_helper_queries_can_request_test_witness_recall_without_narrowing() {
    let intent = SearchIntent::from_query(
        "trace invalid_params typed error from public docs to runtime helper and tests",
    );

    assert!(intent.has_goal(SearchGoal::Documentation));
    assert!(intent.has_goal(SearchGoal::Contracts));
    assert!(intent.has_goal(SearchGoal::ErrorTaxonomy));
    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(intent.has_goal(SearchGoal::RuntimeWitnesses));
    assert!(intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::Broad);
    assert!(
        intent
            .applied_rule_ids()
            .contains(&SearchIntentRuleId::TestWitnessFocus)
    );
}

#[test]
fn blade_component_queries_expose_typed_framework_and_artifact_biases() {
    let intent =
        SearchIntent::from_query("blade component layout page header section slot render views");

    assert!(intent.has_framework_hint(FrameworkHint::Php));
    assert!(intent.has_framework_hint(FrameworkHint::Blade));
    assert!(intent.has_framework_hint(FrameworkHint::Laravel));
    assert!(intent.has_artifact_bias(ArtifactBias::LaravelUi));
    assert!(intent.has_artifact_bias(ArtifactBias::BladeComponent));
    assert!(intent.has_artifact_bias(ArtifactBias::LaravelLayout));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
    assert!(
        intent
            .applied_rule_ids()
            .contains(&SearchIntentRuleId::LaravelUiWitnessTerms)
    );
    assert!(
        intent
            .applied_rule_ids()
            .contains(&SearchIntentRuleId::BladeComponentWitnessTerms)
    );
}

#[test]
fn laravel_ui_queries_keep_test_witness_focus_when_docs_are_path_hints() {
    let intent = SearchIntent::from_query(
        "blade component layout slot section view render resources views api docs docs parts tests audit log",
    );

    assert!(intent.has_goal(SearchGoal::Documentation));
    assert!(intent.has_artifact_bias(ArtifactBias::LaravelUi));
    assert!(intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn test_execution_queries_keep_test_witness_focus_when_docs_are_path_hints() {
    let intent = SearchIntent::from_query(
        "tests fixtures integration audit log resources views api docs docs parts",
    );

    assert!(intent.has_goal(SearchGoal::Documentation));
    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn model_data_queries_request_runtime_witness_recall() {
    let intent = SearchIntent::from_query(
        "model migration seeder factory data app models database users table resets table",
    );

    assert!(intent.has_goal(SearchGoal::RuntimeWitnesses));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn playbook_queries_allow_self_reference() {
    let intent = SearchIntent::from_query("playbook replay citations");

    assert_eq!(
        intent.playbook_reference_policy(),
        PlaybookReferencePolicy::AllowSelfReference
    );
    assert!(!intent.penalize_playbook_self_reference);
    assert!(intent.has_goal(SearchGoal::Fixtures));
}

#[test]
fn runtime_config_queries_do_not_overfocus_test_witnesses_for_incidental_test_terms() {
    let intent = SearchIntent::from_query("config package tsconfig github workflow ai tests");

    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(intent.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact));
    assert!(!intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn standalone_config_queries_activate_runtime_config_bias() {
    let intent = SearchIntent::from_query("config");

    assert!(intent.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn config_workflow_queries_activate_runtime_config_bias_without_manifest_terms() {
    let intent = SearchIntent::from_query("config github workflow gh pages test");

    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(intent.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact));
    assert!(!intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn runtime_config_queries_with_mixed_support_test_terms_keep_test_witness_recall() {
    let intent =
        SearchIntent::from_query("config examples benches benchmark pyproject requirements tests");

    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(intent.has_goal(SearchGoal::Examples));
    assert!(intent.has_goal(SearchGoal::Benchmarks));
    assert!(intent.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact));
    assert!(intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn package_library_queries_do_not_infer_runtime_config_bias_from_plural_packages() {
    let intent = SearchIntent::from_query(
        "tests packages internal library integration config manager controller",
    );

    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert!(!intent.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn entrypoint_cli_queries_do_not_activate_test_witness_focus_without_test_terms() {
    let intent = SearchIntent::from_query("entry point bootstrap app startup cli main");

    assert!(intent.has_goal(SearchGoal::EntryPointBuildFlow));
    assert!(!intent.has_goal(SearchGoal::Tests));
    assert!(!intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}

#[test]
fn cli_entrypoint_queries_activate_entrypoint_build_flow_without_build_terms() {
    let intent = SearchIntent::from_query("ruff analyze ruff cli entrypoint");

    assert!(intent.has_goal(SearchGoal::EntryPointBuildFlow));
    assert!(!intent.has_goal(SearchGoal::Tests));
    assert!(!intent.has_artifact_bias(ArtifactBias::TestWitness));
}

#[test]
fn strong_python_test_focus_queries_keep_test_witness_recall_even_with_setup_readme_terms() {
    let intent =
        SearchIntent::from_query("tests fixtures integration helpers e2e config setup pyproject");

    assert!(intent.has_goal(SearchGoal::Tests));
    assert!(intent.has_goal(SearchGoal::Onboarding));
    assert!(intent.has_goal(SearchGoal::Readme));
    assert!(intent.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact));
    assert!(intent.has_artifact_bias(ArtifactBias::TestWitness));
    assert_eq!(intent.strictness(), PlannerStrictness::WitnessFocused);
}
