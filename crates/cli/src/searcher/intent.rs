use super::surfaces::HybridSourceClass;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct HybridRankingIntent {
    pub(super) wants_docs: bool,
    pub(super) wants_onboarding: bool,
    pub(super) wants_runtime: bool,
    pub(super) wants_runtime_witnesses: bool,
    pub(super) wants_runtime_config_artifacts: bool,
    pub(super) wants_examples: bool,
    pub(super) wants_laravel_ui_witnesses: bool,
    pub(super) wants_laravel_form_action_witnesses: bool,
    pub(super) wants_livewire_view_witnesses: bool,
    pub(super) wants_laravel_layout_witnesses: bool,
    pub(super) wants_blade_component_witnesses: bool,
    pub(super) wants_commands_middleware_witnesses: bool,
    pub(super) wants_jobs_listeners_witnesses: bool,
    pub(super) wants_entrypoint_build_flow: bool,
    pub(super) wants_ci_workflow_witnesses: bool,
    pub(super) wants_scripts_ops_witnesses: bool,
    pub(super) wants_navigation_fallbacks: bool,
    pub(super) wants_tests: bool,
    pub(super) wants_test_witness_recall: bool,
    pub(super) wants_fixtures: bool,
    pub(super) wants_benchmarks: bool,
    pub(super) wants_readme: bool,
    pub(super) wants_contracts: bool,
    pub(super) wants_error_taxonomy: bool,
    pub(super) wants_tool_contracts: bool,
    pub(super) wants_mcp_runtime_surface: bool,
    pub(super) penalize_playbook_self_reference: bool,
}

impl HybridRankingIntent {
    pub(super) fn from_query(query_text: &str) -> Self {
        let query = query_text.trim().to_ascii_lowercase();
        let has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));
        let has_blade_ui_surface_terms = has_any(&[
            "component",
            "components",
            "view",
            "views",
            "slot",
            "section",
        ]);
        let has_blade_form_action_terms = has_any(&[
            "form", "forms", "modal", "modals", "partial", "partials", "table", "tables",
        ]);
        let has_manifest_hint = has_any(&[
            "cargo",
            "pyproject",
            "package",
            "composer",
            "requirements",
            "pipfile",
            "tsconfig",
            "go.mod",
            "go.sum",
            "mix.exs",
        ]);

        let wants_docs = has_any(&[
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
        ]);
        let wants_tests = has_any(&[
            "test",
            "tests",
            "coverage",
            "assert",
            "parity",
            "canary",
            "replay",
            "conformance",
            "inspector",
        ]);
        let wants_examples = has_any(&[
            "example",
            "examples",
            "quickstart",
            "getting started",
            "getting-started",
            "setup",
            "install",
        ]);
        let wants_blade_component_witnesses = has_any(&["blade"])
            && ((has_any(&["component", "components"])
                && has_any(&["layout", "slot", "section", "render", "view", "views"]))
                || has_blade_form_action_terms)
            && !has_any(&["livewire", "flux"]);
        let wants_laravel_ui_witnesses = has_any(&["blade", "livewire", "flux"])
            && (has_blade_ui_surface_terms || has_blade_form_action_terms);
        let wants_laravel_form_action_witnesses =
            wants_laravel_ui_witnesses && has_blade_form_action_terms;
        let wants_livewire_view_witnesses = wants_laravel_ui_witnesses
            && has_any(&[
                "app livewire",
                "wire:model",
                "wire:click",
                "render",
                "class",
            ]);
        let wants_laravel_layout_witnesses = wants_laravel_ui_witnesses
            && has_any(&["layout", "layouts"])
            && has_any(&["navigation", "page", "pages", "header", "footer", "sidebar"]);
        let wants_commands_middleware_witnesses =
            has_any(&["command", "commands", "console", "middleware", "artisan"]);
        let wants_jobs_listeners_witnesses = has_any(&[
            "job",
            "jobs",
            "listener",
            "listeners",
            "event",
            "events",
            "queue",
        ]);
        let wants_runtime_witnesses = has_any(&[
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
        ]) || wants_laravel_ui_witnesses;
        let wants_entrypoint_build_flow =
            has_any(&[
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
            ]) || (has_any(&[
                "start",
                "starts",
                "startup",
                "entrypoint",
                "boot",
                "bootstrap",
            ]) && has_any(&[
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
            ])) || (has_any(&["route", "routes", "middleware", "provider", "providers"])
                && has_any(&["bootstrap", "entrypoint", "entry point", "app"]));
        let wants_ci_workflow_witnesses = has_any(&[
            "workflow",
            "workflows",
            "github action",
            "github actions",
            "autofix",
            "release workflow",
            "publish workflow",
            "deploy workflow",
        ]);
        let wants_scripts_ops_witnesses = has_any(&[
            "script",
            "scripts",
            "justfile",
            "makefile",
            "xtask",
            "changelog",
        ]);
        let wants_navigation_fallbacks = has_any(&[
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
        ]);
        let wants_onboarding = has_any(&[
            "quickstart",
            "getting started",
            "getting-started",
            "setup",
            "configure",
            "configuring",
            "install",
            "installation",
        ]);
        let wants_fixtures = has_any(&[
            "fixture",
            "fixtures",
            "playbook",
            "playbooks",
            "replay",
            "trace artifact",
        ]);
        let wants_benchmarks =
            has_any(&[
                "benchmark",
                "benchmarks",
                "metric",
                "metrics",
                "acceptance metric",
                "acceptance metrics",
                "replayability",
                "deterministic replay",
            ]) || (has_any(&["deterministic", "replay", "suite", "fixture", "fixtures"])
                && has_any(&["trace artifact", "citation", "citations", "playbook"]));
        let wants_error_taxonomy = has_any(&[
            "invalid_params",
            "-32602",
            "error taxonomy",
            "unavailable",
            "strict_failure",
            "semantic_status",
            "semantic_reason",
        ]);
        let wants_tool_contracts = has_any(&[
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
        ]) || (has_any(&["mcp", "tool", "tools"])
            && has_any(&["core", "extended", "schema"]));
        let wants_mcp_runtime_surface = has_any(&[
            "mcp http",
            "http startup",
            "loopback http",
            "workspace_attach",
            "tool surface",
            "tools/list",
            "core versus extended",
            "core vs extended",
            "extended_only",
        ]) || (has_any(&["mcp"])
            && has_any(&[
                "http", "startup", "runtime", "tool", "tools", "surface", "core", "extended",
                "attach", "loopback",
            ]));
        let wants_runtime_config_artifacts = has_any(&[
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
        ]) || (has_manifest_hint
            && has_any(&[
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
            ]));
        let wants_readme = wants_onboarding || has_any(&["readme", "documented"]);
        let wants_contracts = has_any(&[
            "contract",
            "contracts",
            "invalid_params",
            "error_code",
            "typed error",
            "unavailable",
            "strict_failure",
        ]);
        let mentions_cli_context = has_any(&["cli", "command-line", "command line"]);
        let wants_test_witness_recall = (wants_tests
            || (mentions_cli_context && (wants_entrypoint_build_flow || wants_runtime_witnesses)))
            && !wants_docs
            && !wants_readme
            && !wants_contracts
            && !wants_error_taxonomy
            && !wants_tool_contracts;

        Self {
            wants_docs,
            wants_onboarding,
            wants_runtime: true,
            wants_runtime_witnesses: wants_runtime_witnesses
                || wants_entrypoint_build_flow
                || wants_navigation_fallbacks,
            wants_runtime_config_artifacts,
            wants_examples,
            wants_laravel_ui_witnesses,
            wants_laravel_form_action_witnesses,
            wants_livewire_view_witnesses,
            wants_laravel_layout_witnesses,
            wants_blade_component_witnesses,
            wants_commands_middleware_witnesses,
            wants_jobs_listeners_witnesses,
            wants_entrypoint_build_flow,
            wants_ci_workflow_witnesses,
            wants_scripts_ops_witnesses,
            wants_navigation_fallbacks,
            wants_tests,
            wants_test_witness_recall,
            wants_fixtures,
            wants_benchmarks,
            wants_readme,
            wants_contracts,
            wants_error_taxonomy,
            wants_tool_contracts,
            wants_mcp_runtime_surface,
            penalize_playbook_self_reference: !has_any(&["playbook", "playbooks"]),
        }
    }

    pub(super) fn wants_path_witness_recall(self) -> bool {
        self.wants_runtime_witnesses
            || self.wants_laravel_ui_witnesses
            || self.wants_commands_middleware_witnesses
            || self.wants_jobs_listeners_witnesses
            || self.wants_entrypoint_build_flow
            || self.wants_ci_workflow_witnesses
            || self.wants_scripts_ops_witnesses
            || self.wants_runtime_config_artifacts
            || self.wants_test_witness_recall
            || self.wants_examples
            || self.wants_benchmarks
    }

    pub(super) fn wants_class(self, class: HybridSourceClass) -> bool {
        match class {
            HybridSourceClass::ErrorContracts => self.wants_error_taxonomy || self.wants_contracts,
            HybridSourceClass::ToolContracts => self.wants_tool_contracts || self.wants_contracts,
            HybridSourceClass::BenchmarkDocs => self.wants_benchmarks,
            HybridSourceClass::Documentation => self.wants_docs,
            HybridSourceClass::Readme => self.wants_readme,
            HybridSourceClass::Runtime => self.wants_runtime,
            HybridSourceClass::Project => false,
            HybridSourceClass::Support => {
                self.wants_runtime_witnesses
                    || self.wants_examples
                    || self.wants_benchmarks
                    || self.wants_ci_workflow_witnesses
                    || self.wants_scripts_ops_witnesses
            }
            HybridSourceClass::Tests => {
                self.wants_tests || self.wants_runtime_witnesses || self.wants_examples
            }
            HybridSourceClass::Fixtures => self.wants_fixtures,
            HybridSourceClass::Playbooks => !self.penalize_playbook_self_reference,
            HybridSourceClass::Specs | HybridSourceClass::Other => false,
        }
    }
}
