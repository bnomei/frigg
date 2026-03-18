use crate::domain::{ArtifactBias, PlannerStrictness, SearchGoal, SearchIntentRuleId};

use super::{QueryContext, SearchIntentBuilder};

#[derive(Debug, Clone, Copy)]
pub(super) struct SearchIntentRule {
    pub(super) id: SearchIntentRuleId,
    pub(super) apply: fn(&QueryContext, &mut SearchIntentBuilder) -> bool,
}

fn apply_docs_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !context.has_any(&[
        "docs",
        "documented",
        "documentation",
        "public docs",
        "contract",
        "contracts",
        "readme",
        "invalid_params",
        "error_code",
        "typed error",
        "citation",
        "citations",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::Documentation);
    true
}

fn apply_tests_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !context.has_any(&[
        "test",
        "tests",
        "coverage",
        "assert",
        "parity",
        "canary",
        "replay",
        "conformance",
        "inspector",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::Tests);
    true
}

fn apply_examples_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !context.has_any(&[
        "example",
        "examples",
        "quickstart",
        "getting started",
        "getting-started",
        "setup",
        "install",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::Examples);
    true
}

fn apply_onboarding_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !context.has_any(&[
        "quickstart",
        "getting started",
        "getting-started",
        "setup",
        "configure",
        "configuring",
        "install",
        "installation",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::Onboarding);
    true
}

fn apply_readme_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !builder.has_goal(SearchGoal::Onboarding) && !context.has_any(&["readme", "documented"]) {
        return false;
    }

    builder.insert_goal(SearchGoal::Readme);
    true
}

fn apply_contract_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !context.has_any(&[
        "contract",
        "contracts",
        "invalid_params",
        "error_code",
        "typed error",
        "unavailable",
        "strict_failure",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::Contracts);
    true
}

fn apply_error_taxonomy_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !context.has_any(&[
        "invalid_params",
        "-32602",
        "error taxonomy",
        "unavailable",
        "strict_failure",
        "semantic_status",
        "semantic_reason",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::ErrorTaxonomy);
    true
}

fn apply_tool_contract_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !(context.has_any(&[
        "search_hybrid",
        "semantic_status",
        "semantic_reason",
        "tool schema",
        "tool contract",
        "tool contracts",
        "tool surface",
        "tools/list",
        "mcp tool",
        "mcp tools",
        "core versus extended",
        "core vs extended",
        "extended_only",
    ]) || (context.has_any(&["mcp", "tool", "tools"])
        && context.has_any(&["core", "extended", "schema"])))
    {
        return false;
    }

    builder.insert_goal(SearchGoal::ToolContracts);
    true
}

fn apply_mcp_runtime_surface_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !(context.has_any(&[
        "mcp http",
        "http startup",
        "loopback http",
        "workspace_attach",
        "tool surface",
        "tools/list",
        "core versus extended",
        "core vs extended",
        "extended_only",
    ]) || (context.has_any(&["mcp"])
        && context.has_any(&[
            "http", "startup", "runtime", "tool", "tools", "surface", "core", "extended", "attach",
            "loopback",
        ])))
    {
        return false;
    }

    builder.insert_goal(SearchGoal::McpRuntimeSurface);
    true
}

fn apply_fixtures_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !context.has_any(&[
        "fixture",
        "fixtures",
        "playbook",
        "playbooks",
        "replay",
        "trace artifact",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::Fixtures);
    true
}

fn apply_benchmarks_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    if !(context.has_any(&[
        "benchmark",
        "benchmarks",
        "metric",
        "metrics",
        "acceptance metric",
        "acceptance metrics",
        "replayability",
        "deterministic replay",
    ]) || (context.has_any(&["deterministic", "replay", "suite", "fixture", "fixtures"])
        && context.has_any(&["trace artifact", "citation", "citations", "playbook"])))
    {
        return false;
    }

    builder.insert_goal(SearchGoal::Benchmarks);
    true
}

fn apply_runtime_config_artifact_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !(context.is_runtime_config_shorthand()
        || (context.has_any(&["config", "configuration"])
            && (context.has_any_token(&["package"])
                || context.has_any(&["workspace", "workspaces", "build", "builds"])))
        || (context.has_any(&["config", "configuration"])
            && context.has_any(&[
                "workflow",
                "workflows",
                "github workflow",
                "github action",
                "github actions",
                "gh pages",
            ]))
        || context.has_any(&[
            "runtime config",
            "pyproject",
            "setup.py",
            "cargo.toml",
            "package.json",
            "package-lock.json",
            "pnpm-lock.yaml",
            "yarn.lock",
            "composer.json",
            "composer.lock",
            "tsconfig.json",
            "go.mod",
            "go.sum",
            "requirements.txt",
            "pipfile",
            "mix.exs",
        ])
        || (context.has_manifest_hint()
            && context.has_any(&[
                "config",
                "manifest",
                "manifests",
                "dependency",
                "dependencies",
                "lock",
                "workspace",
                "workspaces",
                "build",
                "builds",
            ])))
    {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::RuntimeConfigArtifact);
    true
}

fn apply_laravel_ui_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !context.mentions_laravel_ui() {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::LaravelUi);
    true
}

fn apply_laravel_form_action_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !builder.has_artifact_bias(ArtifactBias::LaravelUi) || !context.has_blade_form_action_terms()
    {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::LaravelFormAction);
    true
}

fn apply_blade_component_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !(context.has_any(&["blade"])
        && ((context.has_any(&["component", "components"])
            && context.has_any(&["layout", "slot", "section", "render", "view", "views"]))
            || context.has_blade_form_action_terms())
        && !context.has_any(&["livewire", "flux"]))
    {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::BladeComponent);
    true
}

fn apply_livewire_view_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !builder.has_artifact_bias(ArtifactBias::LaravelUi)
        || !context.has_any(&[
            "app livewire",
            "wire:model",
            "wire:click",
            "render",
            "class",
        ])
    {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::LivewireView);
    true
}

fn apply_laravel_layout_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !builder.has_artifact_bias(ArtifactBias::LaravelUi)
        || !context.has_any(&["layout", "layouts"])
        || !context.has_any(&["navigation", "page", "pages", "header", "footer", "sidebar"])
    {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::LaravelLayout);
    true
}

fn apply_commands_middleware_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !context.has_any(&["command", "commands", "console", "middleware", "artisan"]) {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::CommandsMiddleware);
    true
}

fn apply_jobs_listeners_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !context.has_any(&[
        "job",
        "jobs",
        "listener",
        "listeners",
        "event",
        "events",
        "queue",
    ]) {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::JobsListeners);
    true
}

fn apply_runtime_witness_terms(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    let explicit_runtime_signal = context.has_any(&[
        "initialize",
        "initialization",
        "server capabilities",
        "initialize result",
        "subscription",
        "subscriptions",
        "completion provider",
        "completion providers",
        "handler",
        "handlers",
        "transport",
        "notification",
        "notifications",
        "resource updated",
        "resource_updated",
        "custom method",
        "auth token",
        "http auth",
        "bearer auth",
        "authorization",
        "entrypoint",
        "client communication",
        "back to client",
        "client aware",
        "client-aware",
        "client gateway",
        "clientgateway",
        "conformance",
        "inspector",
        "resource update",
        "entry point",
        "entry-point",
        "app startup",
        "startup module",
        "cli main",
        "main module",
        "runtime config",
    ]);
    let generic_runtime_signal = context.has_any_token(&["runtime"])
        && (context.has_any(&[
            "self hosted",
            "self-hosted",
            "edge function",
            "edge functions",
            "api",
            "server",
            "service",
            "services",
            "worker",
            "workers",
            "handler",
            "handlers",
            "router",
            "route",
            "routes",
            "transport",
            "http",
            "docker",
            "wasm",
            "cli",
            "bootstrap",
            "startup",
            "entrypoint",
            "entry point",
            "entry-point",
        ]) || (context.has_any_token(&["function", "functions"])
            && context.has_any(&["edge", "self hosted", "self-hosted", "api", "runtime"])));
    let ui_runtime_signal = context.has_ui_runtime_surface_terms()
        && (context.has_any(&[
            "android",
            "compose",
            "playwright",
            "react",
            "svelte",
            "tsx",
            "viewmodel",
            "vue",
        ]) || context.has_any_token(&["runtime"]));
    if !(explicit_runtime_signal
        || generic_runtime_signal
        || ui_runtime_signal
        || context.mentions_model_data_surface()
        || builder.has_artifact_bias(ArtifactBias::LaravelUi)
        || builder.has_goal(SearchGoal::EntryPointBuildFlow)
        || builder.has_goal(SearchGoal::NavigationFallbacks))
    {
        return false;
    }

    builder.insert_goal(SearchGoal::RuntimeWitnesses);
    true
}

fn apply_entrypoint_build_flow_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !(context.has_any(&[
        "where the app starts",
        "app starts",
        "startup flow",
        "app startup",
        "entry point",
        "entry-point",
        "startup module",
        "cli main",
        "main module",
        "start and build",
        "starts and builds",
        "build pipeline",
        "pipeline runner",
    ]) || (context.has_any(&["entrypoint", "entry point", "entry-point"])
        && context.has_any(&["cli", "command", "commands", "bin"]))
        || (context.has_any(&[
            "start",
            "starts",
            "startup",
            "entrypoint",
            "boot",
            "bootstrap",
        ]) && context.has_any(&[
            "build",
            "builds",
            "builder",
            "construct",
            "constructs",
            "wire",
            "wires",
            "wiring",
            "runner",
            "pipeline",
        ]))
        || (context.has_any(&["route", "routes", "middleware", "provider", "providers"])
            && context.has_any(&["bootstrap", "entrypoint", "entry point", "app"])))
    {
        return false;
    }

    builder.insert_goal(SearchGoal::EntryPointBuildFlow);
    true
}

fn apply_ci_workflow_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    let explicit_ci_workflow_signal = context.has_any(&[
        "github workflow",
        "github action",
        "github actions",
        "autofix",
        "release workflow",
        "publish workflow",
        "deploy workflow",
    ]);
    let generic_workflow_signal = context.has_any(&["workflow", "workflows"])
        && context.has_any(&[
            "github", "ci", "action", "actions", "yaml", "yml", "release", "publish", "deploy",
            "autofix", "build",
        ]);
    if !(explicit_ci_workflow_signal || generic_workflow_signal) {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::CiWorkflow);
    true
}

fn apply_scripts_ops_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !(context.has_any_token(&["script", "scripts"])
        || context.has_any(&["justfile", "makefile", "xtask", "changelog"]))
    {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::ScriptsOps);
    true
}

fn apply_navigation_fallback_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !context.has_any(&[
        "find_implementations",
        "find implementations",
        "go_to_definition",
        "go to definition",
        "find_references",
        "find references",
        "incoming_calls",
        "incoming calls",
        "outgoing_calls",
        "outgoing calls",
        "precise navigation",
        "precise data",
        "precise_absent",
        "precise navigation data",
        "fallback",
        "navigation",
        "scip",
    ]) {
        return false;
    }

    builder.insert_goal(SearchGoal::NavigationFallbacks);
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TestWitnessFocusMode {
    WitnessFocused,
    RuntimeBridge,
}

fn apply_test_witness_focus(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    let Some(mode) = test_witness_focus_mode(context, builder) else {
        return false;
    };

    builder.insert_artifact_bias(ArtifactBias::TestWitness);
    match mode {
        TestWitnessFocusMode::WitnessFocused => {
            builder.strictness = Some(PlannerStrictness::WitnessFocused);
        }
        TestWitnessFocusMode::RuntimeBridge => {
            builder.insert_goal(SearchGoal::RuntimeWitnesses);
            builder.strictness = Some(PlannerStrictness::Broad);
        }
    }

    true
}

fn test_witness_focus_mode(
    context: &QueryContext,
    builder: &SearchIntentBuilder,
) -> Option<TestWitnessFocusMode> {
    if should_skip_test_witness_focus(context, builder) || !builder.has_goal(SearchGoal::Tests) {
        return None;
    }

    if is_runtime_bridge_test_focus(context, builder) {
        return Some(TestWitnessFocusMode::RuntimeBridge);
    }

    let allow_readme = context.has_strong_test_focus_terms();
    if is_laravel_ui_path_hint_test_focus(builder)
        || is_docs_with_strong_test_focus(context, builder, allow_readme)
        || is_plain_test_focus(builder, allow_readme)
    {
        Some(TestWitnessFocusMode::WitnessFocused)
    } else {
        None
    }
}

fn should_skip_test_witness_focus(context: &QueryContext, builder: &SearchIntentBuilder) -> bool {
    builder.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact)
        && !context.has_strong_test_focus_terms()
        && !builder.has_goal(SearchGoal::Examples)
        && !builder.has_goal(SearchGoal::Benchmarks)
}

fn is_runtime_bridge_test_focus(context: &QueryContext, builder: &SearchIntentBuilder) -> bool {
    has_contract_or_error_focus(builder)
        && context.has_any(&["runtime helper", "runtime helpers", "helper", "call site"])
}

fn is_laravel_ui_path_hint_test_focus(builder: &SearchIntentBuilder) -> bool {
    builder.has_artifact_bias(ArtifactBias::LaravelUi)
        && builder.has_goal(SearchGoal::Documentation)
        && !builder.has_goal(SearchGoal::Readme)
        && !has_contract_or_error_focus(builder)
}

fn is_docs_with_strong_test_focus(
    context: &QueryContext,
    builder: &SearchIntentBuilder,
    allow_readme: bool,
) -> bool {
    builder.has_goal(SearchGoal::Documentation)
        && context.has_strong_test_focus_terms()
        && test_witness_focus_readme_allowed(builder, allow_readme)
        && !has_contract_or_error_focus(builder)
}

fn is_plain_test_focus(builder: &SearchIntentBuilder, allow_readme: bool) -> bool {
    !builder.has_goal(SearchGoal::Documentation)
        && test_witness_focus_readme_allowed(builder, allow_readme)
        && !has_contract_or_error_focus(builder)
}

fn test_witness_focus_readme_allowed(builder: &SearchIntentBuilder, allow_readme: bool) -> bool {
    !builder.has_goal(SearchGoal::Readme) || allow_readme
}

fn has_contract_or_error_focus(builder: &SearchIntentBuilder) -> bool {
    builder.has_goal(SearchGoal::Contracts)
        || builder.has_goal(SearchGoal::ErrorTaxonomy)
        || builder.has_goal(SearchGoal::ToolContracts)
}

pub(super) static SEARCH_INTENT_RULES: &[SearchIntentRule] = &[
    SearchIntentRule {
        id: SearchIntentRuleId::DocumentationTerms,
        apply: apply_docs_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::TestsTerms,
        apply: apply_tests_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::ExamplesTerms,
        apply: apply_examples_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::OnboardingTerms,
        apply: apply_onboarding_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::ContractsTerms,
        apply: apply_contract_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::ErrorTaxonomyTerms,
        apply: apply_error_taxonomy_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::ToolContractTerms,
        apply: apply_tool_contract_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::McpRuntimeSurfaceTerms,
        apply: apply_mcp_runtime_surface_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::FixturesTerms,
        apply: apply_fixtures_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::BenchmarksTerms,
        apply: apply_benchmarks_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::RuntimeConfigArtifactTerms,
        apply: apply_runtime_config_artifact_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::LaravelUiWitnessTerms,
        apply: apply_laravel_ui_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::LaravelFormActionWitnessTerms,
        apply: apply_laravel_form_action_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::BladeComponentWitnessTerms,
        apply: apply_blade_component_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::LivewireViewWitnessTerms,
        apply: apply_livewire_view_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::LaravelLayoutWitnessTerms,
        apply: apply_laravel_layout_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::CommandsMiddlewareWitnessTerms,
        apply: apply_commands_middleware_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::JobsListenersWitnessTerms,
        apply: apply_jobs_listeners_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::EntryPointBuildFlowTerms,
        apply: apply_entrypoint_build_flow_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::CiWorkflowWitnessTerms,
        apply: apply_ci_workflow_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::ScriptsOpsWitnessTerms,
        apply: apply_scripts_ops_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::NavigationFallbackTerms,
        apply: apply_navigation_fallback_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::RuntimeWitnessTerms,
        apply: apply_runtime_witness_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::ReadmeTerms,
        apply: apply_readme_terms,
    },
    SearchIntentRule {
        id: SearchIntentRuleId::TestWitnessFocus,
        apply: apply_test_witness_focus,
    },
];
