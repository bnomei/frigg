use super::super::dsl::{Predicate, ScoreRule, ScoreRuleSet, apply_score_rule_sets};
use super::super::facts::PathQualityFacts;
use super::super::kernel::PolicyProgram;
use super::super::predicates::path_quality as pred;
use super::super::trace::{PolicyEffect, PolicyStage};
use crate::searcher::surfaces::HybridSourceClass;

fn docs_prefers_documentation_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_docs
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation
                | HybridSourceClass::ErrorContracts
                | HybridSourceClass::ToolContracts
                | HybridSourceClass::BenchmarkDocs
        ))
    .then_some(PolicyEffect::Multiply(1.36))
}

fn readme_prefers_readme_class(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_readme && ctx.class == HybridSourceClass::Readme)
        .then_some(PolicyEffect::Multiply(1.15))
}

fn readme_prefers_root_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_readme && ctx.is_root_readme).then_some(PolicyEffect::Multiply(1.45))
}

fn onboarding_prefers_readme_class(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_onboarding && ctx.class == HybridSourceClass::Readme)
        .then_some(PolicyEffect::Multiply(1.85))
}

fn onboarding_prefers_root_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_onboarding && ctx.is_root_readme).then_some(PolicyEffect::Multiply(1.25))
}

fn onboarding_prefers_documentation_class(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_onboarding && ctx.class == HybridSourceClass::Documentation)
        .then_some(PolicyEffect::Multiply(1.15))
}

fn contracts_prefers_contract_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_contracts
        && matches!(
            ctx.class,
            HybridSourceClass::ErrorContracts | HybridSourceClass::ToolContracts
        ))
    .then_some(PolicyEffect::Multiply(1.55))
}

fn error_taxonomy_prefers_error_contracts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy && ctx.class == HybridSourceClass::ErrorContracts)
        .then_some(PolicyEffect::Multiply(1.95))
}

fn error_taxonomy_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.18))
}

fn error_taxonomy_prefers_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy && ctx.class == HybridSourceClass::Tests)
        .then_some(PolicyEffect::Multiply(1.26))
}

fn error_taxonomy_penalizes_docs_readme_specs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_error_taxonomy
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme | HybridSourceClass::Specs
        ))
    .then_some(PolicyEffect::Multiply(0.78))
}

fn tool_contracts_prefers_tool_contracts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_tool_contracts && ctx.class == HybridSourceClass::ToolContracts)
        .then_some(PolicyEffect::Multiply(2.10))
}

fn mcp_runtime_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.22))
}

fn mcp_runtime_prefers_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Support)
        .then_some(PolicyEffect::Multiply(1.10))
}

fn mcp_runtime_prefers_documentation(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Documentation)
        .then_some(PolicyEffect::Multiply(1.12))
}

fn mcp_runtime_penalizes_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Readme)
        .then_some(PolicyEffect::Multiply(0.92))
}

fn mcp_runtime_penalizes_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_mcp_runtime_surface && ctx.class == HybridSourceClass::Tests)
        .then_some(PolicyEffect::Multiply(0.82))
}

fn benchmarks_prefers_benchmark_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_benchmarks && ctx.class == HybridSourceClass::BenchmarkDocs)
        .then_some(PolicyEffect::Multiply(2.00))
}

fn tests_prefers_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_tests && ctx.class == HybridSourceClass::Tests)
        .then_some(PolicyEffect::Multiply(1.24))
}

fn fixtures_prefers_fixtures(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_fixtures && ctx.class == HybridSourceClass::Fixtures)
        .then_some(PolicyEffect::Multiply(1.14))
}

fn runtime_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.05))
}

fn runtime_witness_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.class == HybridSourceClass::Runtime)
        .then_some(PolicyEffect::Multiply(1.52))
}

fn runtime_witness_prefers_support_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && matches!(
            ctx.class,
            HybridSourceClass::Support | HybridSourceClass::Tests
        ))
    .then_some(PolicyEffect::Multiply(1.24))
}

fn runtime_config_prefers_artifacts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.40))
}

fn runtime_config_prefers_repo_root_artifacts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_root_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.38))
}

fn runtime_config_prefers_entrypoint_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_entrypoint_runtime)
        .then_some(PolicyEffect::Multiply(1.72))
}

fn runtime_config_prefers_typescript_index(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_typescript_runtime_module_index)
        .then_some(PolicyEffect::Multiply(1.22))
}

fn runtime_config_prefers_python_config(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_python_runtime_config)
        .then_some(PolicyEffect::Multiply(1.36))
}

fn runtime_config_penalizes_docs_readme(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Multiply(if ctx.is_root_readme {
        0.62
    } else {
        0.74
    }))
}

fn entrypoint_prefers_entrypoint_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_runtime)
        .then_some(PolicyEffect::Multiply(1.92))
}

fn entrypoint_prefers_build_workflows(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_build_workflow)
        .then_some(PolicyEffect::Multiply(2.20))
}

fn entrypoint_prefers_ci_workflows(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && !ctx.is_entrypoint_build_workflow && ctx.is_ci_workflow)
        .then_some(PolicyEffect::Multiply(1.36))
}

fn entrypoint_prefers_typescript_index(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_typescript_runtime_module_index)
        .then_some(PolicyEffect::Multiply(1.16))
}

fn entrypoint_prefers_runtime_config_artifacts(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.18))
}

fn entrypoint_prefers_repo_root_runtime_config_artifacts(
    ctx: &PathQualityFacts,
) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_repo_root_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(1.28))
}

fn entrypoint_penalizes_example_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_example_support)
        .then_some(PolicyEffect::Multiply(0.62))
}

fn entrypoint_penalizes_bench_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow
        && !ctx.wants_examples
        && !ctx.wants_benchmarks
        && ctx.is_bench_support)
        .then_some(PolicyEffect::Multiply(0.72))
}

fn navigation_prefers_runtime(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.is_navigation_runtime)
        .then_some(PolicyEffect::Multiply(1.24))
}

fn navigation_penalizes_reference_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.is_navigation_reference_doc)
        .then_some(PolicyEffect::Multiply(0.72))
}

fn navigation_mcp_runtime_bonus(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_navigation_fallbacks && ctx.wants_mcp_runtime_surface && ctx.is_navigation_runtime)
        .then_some(PolicyEffect::Multiply(1.28))
}

fn examples_or_bench_prefers_example_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses && ctx.is_example_support).then_some(
        PolicyEffect::Multiply(if ctx.wants_examples { 1.34 } else { 1.18 }),
    )
}

fn examples_or_bench_prefers_bench_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses && ctx.is_bench_support).then_some(
        PolicyEffect::Multiply(if ctx.wants_benchmarks { 1.38 } else { 1.22 }),
    )
}

fn examples_or_bench_penalizes_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Multiply(0.72))
}

fn laravel_ui_prefers_blade_views(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && (ctx.is_laravel_non_livewire_blade_view || ctx.is_laravel_livewire_view))
        .then_some(PolicyEffect::Multiply(1.44))
}

fn laravel_ui_prefers_blade_components(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_blade_component_witnesses
        && ctx.is_laravel_blade_component)
        .then_some(PolicyEffect::Multiply(1.34))
}

fn laravel_layout_prefers_layout_views(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_layout_blade_view)
        .then_some(PolicyEffect::Multiply(1.48))
}

fn laravel_layout_penalizes_component_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses
        && ctx.wants_laravel_layout_witnesses
        && ctx.is_laravel_view_component_class)
        .then_some(PolicyEffect::Multiply(0.64))
}

fn laravel_ui_penalizes_view_component_classes(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_laravel_ui_witnesses && ctx.is_laravel_view_component_class)
        .then_some(PolicyEffect::Multiply(0.72))
}

fn examples_or_bench_penalizes_non_support_tests(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_example_or_bench_witnesses
        && ctx.class == HybridSourceClass::Tests
        && !ctx.is_example_support
        && !ctx.is_bench_support)
        .then_some(PolicyEffect::Multiply(if ctx.wants_test_witness_recall {
            0.92
        } else {
            0.68
        }))
}

fn runtime_witness_penalizes_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.penalize_generic_runtime_docs
        && matches!(
            ctx.class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ))
    .then_some(PolicyEffect::Multiply(0.48))
}

fn runtime_witness_penalizes_generic_runtime_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses
        && ctx.penalize_generic_runtime_docs
        && ctx.is_generic_runtime_witness_doc)
        .then_some(PolicyEffect::Multiply(0.34))
}

fn runtime_witness_penalizes_repo_metadata(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_witnesses && ctx.is_repo_metadata && !ctx.is_python_runtime_config)
        .then_some(PolicyEffect::Multiply(0.14))
}

fn runtime_config_penalizes_test_support(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_test_support && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.58))
}

fn runtime_config_penalizes_generic_runtime_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts
        && ctx.is_generic_runtime_witness_doc
        && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.28))
}

fn runtime_config_penalizes_repo_metadata(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_runtime_config_artifacts && ctx.is_repo_metadata && !ctx.is_runtime_config_artifact)
        .then_some(PolicyEffect::Multiply(0.16))
}

fn entrypoint_penalizes_reference_docs(ctx: &PathQualityFacts) -> Option<PolicyEffect> {
    (ctx.wants_entrypoint_build_flow && ctx.is_entrypoint_reference_doc)
        .then_some(PolicyEffect::Multiply(0.72))
}

const PATH_QUALITY_RULES: &[ScoreRule<PathQualityFacts>] = &[
    ScoreRule::when(
        "docs.prefers_documentation_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_docs_leaf()]),
        docs_prefers_documentation_classes,
    ),
    ScoreRule::when(
        "readme.prefers_readme_class",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_readme_leaf(), pred::class_is_readme_leaf()]),
        readme_prefers_readme_class,
    ),
    ScoreRule::when(
        "readme.prefers_root_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_readme_leaf(), pred::is_root_readme_leaf()]),
        readme_prefers_root_readme,
    ),
    ScoreRule::when(
        "onboarding.prefers_readme_class",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_onboarding_leaf(), pred::class_is_readme_leaf()]),
        onboarding_prefers_readme_class,
    ),
    ScoreRule::when(
        "onboarding.prefers_root_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_onboarding_leaf(), pred::is_root_readme_leaf()]),
        onboarding_prefers_root_readme,
    ),
    ScoreRule::when(
        "onboarding.prefers_documentation_class",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_onboarding_leaf(),
            pred::class_is_documentation_leaf(),
        ]),
        onboarding_prefers_documentation_class,
    ),
    ScoreRule::when(
        "contracts.prefers_contract_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_contracts_leaf()]),
        contracts_prefers_contract_classes,
    ),
    ScoreRule::when(
        "error_taxonomy.prefers_error_contracts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_error_taxonomy_leaf(),
            pred::class_is_error_contracts_leaf(),
        ]),
        error_taxonomy_prefers_error_contracts,
    ),
    ScoreRule::when(
        "error_taxonomy.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_error_taxonomy_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        error_taxonomy_prefers_runtime,
    ),
    ScoreRule::when(
        "error_taxonomy.prefers_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_error_taxonomy_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        error_taxonomy_prefers_tests,
    ),
    ScoreRule::when(
        "error_taxonomy.penalizes_docs_readme_specs",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_error_taxonomy_leaf()]),
        error_taxonomy_penalizes_docs_readme_specs,
    ),
    ScoreRule::when(
        "tool_contracts.prefers_tool_contracts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_tool_contracts_leaf(),
            pred::class_is_tool_contracts_leaf(),
        ]),
        tool_contracts_prefers_tool_contracts,
    ),
    ScoreRule::when(
        "mcp_runtime.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        mcp_runtime_prefers_runtime,
    ),
    ScoreRule::when(
        "mcp_runtime.prefers_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_support_leaf(),
        ]),
        mcp_runtime_prefers_support,
    ),
    ScoreRule::when(
        "mcp_runtime.prefers_documentation",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_documentation_leaf(),
        ]),
        mcp_runtime_prefers_documentation,
    ),
    ScoreRule::when(
        "mcp_runtime.penalizes_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_readme_leaf(),
        ]),
        mcp_runtime_penalizes_readme,
    ),
    ScoreRule::when(
        "mcp_runtime.penalizes_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_mcp_runtime_surface_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        mcp_runtime_penalizes_tests,
    ),
    ScoreRule::when(
        "benchmarks.prefers_benchmark_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_benchmarks_leaf(),
            pred::class_is_benchmark_docs_leaf(),
        ]),
        benchmarks_prefers_benchmark_docs,
    ),
    ScoreRule::when(
        "tests.prefers_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_tests_leaf(), pred::class_is_tests_leaf()]),
        tests_prefers_tests,
    ),
    ScoreRule::when(
        "fixtures.prefers_fixtures",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_fixtures_leaf(), pred::class_is_fixtures_leaf()]),
        fixtures_prefers_fixtures,
    ),
    ScoreRule::when(
        "runtime.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_runtime_leaf(), pred::class_is_runtime_leaf()]),
        runtime_prefers_runtime,
    ),
    ScoreRule::when(
        "runtime_witness.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::class_is_runtime_leaf(),
        ]),
        runtime_witness_prefers_runtime,
    ),
    ScoreRule::when(
        "runtime_witness.prefers_support_tests",
        PolicyStage::PathQuality,
        Predicate::new(
            &[pred::wants_runtime_witnesses_leaf()],
            &[pred::class_is_support_leaf(), pred::class_is_tests_leaf()],
            &[],
        ),
        runtime_witness_prefers_support_tests,
    ),
    ScoreRule::when(
        "runtime_config.prefers_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_runtime_config_artifact_leaf(),
        ]),
        runtime_config_prefers_artifacts,
    ),
    ScoreRule::when(
        "runtime_config.prefers_repo_root_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        runtime_config_prefers_repo_root_artifacts,
    ),
    ScoreRule::when(
        "runtime_config.prefers_entrypoint_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_entrypoint_runtime_leaf(),
        ]),
        runtime_config_prefers_entrypoint_runtime,
    ),
    ScoreRule::when(
        "runtime_config.prefers_typescript_index",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        runtime_config_prefers_typescript_index,
    ),
    ScoreRule::when(
        "runtime_config.prefers_python_config",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_python_runtime_config_leaf(),
        ]),
        runtime_config_prefers_python_config,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_docs_readme",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_runtime_config_artifacts_leaf()]),
        runtime_config_penalizes_docs_readme,
    ),
    ScoreRule::when(
        "entrypoint.prefers_entrypoint_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_runtime_leaf(),
        ]),
        entrypoint_prefers_entrypoint_runtime,
    ),
    ScoreRule::when(
        "entrypoint.prefers_build_workflows",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_build_workflow_leaf(),
        ]),
        entrypoint_prefers_build_workflows,
    ),
    ScoreRule::when(
        "entrypoint.prefers_ci_workflows",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_ci_workflow_leaf(),
        ]),
        entrypoint_prefers_ci_workflows,
    ),
    ScoreRule::when(
        "entrypoint.prefers_typescript_index",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_typescript_runtime_module_index_leaf(),
        ]),
        entrypoint_prefers_typescript_index,
    ),
    ScoreRule::when(
        "entrypoint.prefers_runtime_config_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_runtime_config_artifact_leaf(),
        ]),
        entrypoint_prefers_runtime_config_artifacts,
    ),
    ScoreRule::when(
        "entrypoint.prefers_repo_root_runtime_config_artifacts",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_repo_root_runtime_config_artifact_leaf(),
        ]),
        entrypoint_prefers_repo_root_runtime_config_artifacts,
    ),
    ScoreRule::when(
        "entrypoint.penalizes_example_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_example_support_leaf(),
        ]),
        entrypoint_penalizes_example_support,
    ),
    ScoreRule::when(
        "entrypoint.penalizes_bench_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_bench_support_leaf(),
        ]),
        entrypoint_penalizes_bench_support,
    ),
    ScoreRule::when(
        "navigation.prefers_runtime",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::is_navigation_runtime_leaf(),
        ]),
        navigation_prefers_runtime,
    ),
    ScoreRule::when(
        "navigation.penalizes_reference_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::is_navigation_reference_doc_leaf(),
        ]),
        navigation_penalizes_reference_docs,
    ),
    ScoreRule::when(
        "navigation.mcp_runtime_bonus",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_navigation_fallbacks_leaf(),
            pred::wants_mcp_runtime_surface_leaf(),
            pred::is_navigation_runtime_leaf(),
        ]),
        navigation_mcp_runtime_bonus,
    ),
    ScoreRule::when(
        "examples_or_bench.prefers_example_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_example_support_leaf(),
        ]),
        examples_or_bench_prefers_example_support,
    ),
    ScoreRule::when(
        "examples_or_bench.prefers_bench_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::is_bench_support_leaf(),
        ]),
        examples_or_bench_prefers_bench_support,
    ),
    ScoreRule::when(
        "examples_or_bench.penalizes_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[pred::wants_example_or_bench_witnesses_leaf()]),
        examples_or_bench_penalizes_docs,
    ),
    ScoreRule::when(
        "laravel_ui.prefers_blade_views",
        PolicyStage::PathQuality,
        Predicate::new(
            &[pred::wants_laravel_ui_witnesses_leaf()],
            &[
                pred::is_laravel_non_livewire_blade_view_leaf(),
                pred::is_laravel_livewire_view_leaf(),
            ],
            &[],
        ),
        laravel_ui_prefers_blade_views,
    ),
    ScoreRule::when(
        "laravel_ui.prefers_blade_components",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_blade_component_witnesses_leaf(),
            pred::is_laravel_blade_component_leaf(),
        ]),
        laravel_ui_prefers_blade_components,
    ),
    ScoreRule::when(
        "laravel_layout.prefers_layout_views",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::is_laravel_layout_blade_view_leaf(),
        ]),
        laravel_layout_prefers_layout_views,
    ),
    ScoreRule::when(
        "laravel_layout.penalizes_component_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::wants_laravel_layout_witnesses_leaf(),
            pred::is_laravel_view_component_class_leaf(),
        ]),
        laravel_layout_penalizes_component_classes,
    ),
    ScoreRule::when(
        "laravel_ui.penalizes_view_component_classes",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_laravel_ui_witnesses_leaf(),
            pred::is_laravel_view_component_class_leaf(),
        ]),
        laravel_ui_penalizes_view_component_classes,
    ),
    ScoreRule::when(
        "examples_or_bench.penalizes_non_support_tests",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_example_or_bench_witnesses_leaf(),
            pred::class_is_tests_leaf(),
        ]),
        examples_or_bench_penalizes_non_support_tests,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::penalize_generic_runtime_docs_leaf(),
        ]),
        runtime_witness_penalizes_docs,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_generic_runtime_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::penalize_generic_runtime_docs_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        runtime_witness_penalizes_generic_runtime_docs,
    ),
    ScoreRule::when(
        "runtime_witness.penalizes_repo_metadata",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_witnesses_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        runtime_witness_penalizes_repo_metadata,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_test_support",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_test_support_leaf(),
        ]),
        runtime_config_penalizes_test_support,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_generic_runtime_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_generic_runtime_witness_doc_leaf(),
        ]),
        runtime_config_penalizes_generic_runtime_docs,
    ),
    ScoreRule::when(
        "runtime_config.penalizes_repo_metadata",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_runtime_config_artifacts_leaf(),
            pred::is_repo_metadata_leaf(),
        ]),
        runtime_config_penalizes_repo_metadata,
    ),
    ScoreRule::when(
        "entrypoint.penalizes_reference_docs",
        PolicyStage::PathQuality,
        Predicate::all(&[
            pred::wants_entrypoint_build_flow_leaf(),
            pred::is_entrypoint_reference_doc_leaf(),
        ]),
        entrypoint_penalizes_reference_docs,
    ),
];

const PATH_QUALITY_RULE_SET: ScoreRuleSet<PathQualityFacts> = ScoreRuleSet::new(PATH_QUALITY_RULES);

pub(crate) fn evaluate(
    ctx: &PathQualityFacts,
    trace: bool,
) -> super::super::trace::PolicyEvaluation {
    let mut program = PolicyProgram::with_optional_trace(ctx.base_multiplier, trace);
    apply_score_rule_sets(&mut program, ctx, &[PATH_QUALITY_RULE_SET]);
    program.finish()
}

pub(crate) fn score(ctx: &PathQualityFacts) -> f32 {
    evaluate(ctx, false).score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::surfaces::HybridSourceClass;

    fn trace_rule_ids(
        evaluation: &super::super::super::trace::PolicyEvaluation,
    ) -> Vec<&'static str> {
        evaluation
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .map(|rule| rule.rule_id)
            .collect()
    }

    fn trace_rule<'a>(
        evaluation: &'a super::super::super::trace::PolicyEvaluation,
        rule_id: &'static str,
    ) -> &'a crate::searcher::policy::trace::PolicyRuleTrace {
        evaluation
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .find(|rule| rule.rule_id == rule_id)
            .expect("rule trace should exist")
    }

    #[test]
    fn policy_trace_path_quality_runtime_config_entrypoint_typescript_stack() {
        let ctx = PathQualityFacts {
            class: HybridSourceClass::Runtime,
            wants_runtime_config_artifacts: true,
            wants_entrypoint_build_flow: true,
            is_entrypoint_runtime: true,
            is_typescript_runtime_module_index: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"runtime_config.prefers_entrypoint_runtime"));
        assert!(rule_ids.contains(&"runtime_config.prefers_typescript_index"));
        assert!(rule_ids.contains(&"entrypoint.prefers_entrypoint_runtime"));
        assert!(rule_ids.contains(&"entrypoint.prefers_typescript_index"));
        assert_eq!(
            trace_rule(&evaluation, "runtime_config.prefers_entrypoint_runtime").predicate_ids,
            vec![
                "intent.runtime_config_artifacts",
                "candidate.entrypoint_runtime"
            ],
        );
        assert_eq!(
            trace_rule(&evaluation, "entrypoint.prefers_typescript_index").predicate_ids,
            vec![
                "intent.entrypoint_build_flow",
                "candidate.typescript_runtime_module_index",
            ],
        );
    }

    #[test]
    fn policy_trace_path_quality_runtime_witness_generic_doc_penalties_stack() {
        let ctx = PathQualityFacts {
            class: HybridSourceClass::Documentation,
            wants_runtime_witnesses: true,
            penalize_generic_runtime_docs: true,
            is_generic_runtime_witness_doc: true,
            ..Default::default()
        };

        let evaluation = evaluate(&ctx, true);
        let rule_ids = trace_rule_ids(&evaluation);

        assert!(rule_ids.contains(&"runtime_witness.penalizes_docs"));
        assert!(rule_ids.contains(&"runtime_witness.penalizes_generic_runtime_docs"));
        assert_eq!(
            trace_rule(
                &evaluation,
                "runtime_witness.penalizes_generic_runtime_docs"
            )
            .predicate_ids,
            vec![
                "intent.runtime_witnesses",
                "intent.penalize_generic_runtime_docs",
                "candidate.generic_runtime_witness_doc",
            ],
        );
    }

    #[test]
    fn policy_trace_path_quality_examples_query_penalizes_non_support_tests() {
        let without_test_focus = PathQualityFacts {
            class: HybridSourceClass::Tests,
            wants_example_or_bench_witnesses: true,
            wants_test_witness_recall: false,
            ..Default::default()
        };
        let without_test_focus_eval = evaluate(&without_test_focus, true);
        let without_ids = trace_rule_ids(&without_test_focus_eval);
        let without_effect = without_test_focus_eval
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .find(|rule| rule.rule_id == "examples_or_bench.penalizes_non_support_tests")
            .expect("examples-or-bench penalty should fire");

        let with_test_focus = PathQualityFacts {
            wants_test_witness_recall: true,
            class: without_test_focus.class,
            wants_example_or_bench_witnesses: without_test_focus.wants_example_or_bench_witnesses,
            ..Default::default()
        };
        let with_test_focus_eval = evaluate(&with_test_focus, true);
        let with_ids = trace_rule_ids(&with_test_focus_eval);
        let with_effect = with_test_focus_eval
            .trace
            .as_ref()
            .expect("trace")
            .rules
            .iter()
            .find(|rule| rule.rule_id == "examples_or_bench.penalizes_non_support_tests")
            .expect("examples-or-bench penalty should fire");

        assert!(without_ids.contains(&"examples_or_bench.penalizes_non_support_tests"));
        assert!(with_ids.contains(&"examples_or_bench.penalizes_non_support_tests"));
        assert_eq!(without_effect.effect, PolicyEffect::Multiply(0.68));
        assert_eq!(with_effect.effect, PolicyEffect::Multiply(0.92));
    }
}
