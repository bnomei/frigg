use std::collections::BTreeSet;

use crate::domain::{
    ArtifactBias, FrameworkHint, PlannerStrictness, PlaybookReferencePolicy, SearchGoal,
    SearchIntentRuleId, SourceClass,
};

pub(super) type HybridRankingIntent = SearchIntent;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct SearchIntent {
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
    goals: BTreeSet<SearchGoal>,
    framework_hints: BTreeSet<FrameworkHint>,
    artifact_biases: BTreeSet<ArtifactBias>,
    strictness: PlannerStrictness,
    playbook_reference_policy: PlaybookReferencePolicy,
    applied_rule_ids: Vec<SearchIntentRuleId>,
}

impl Default for SearchIntent {
    fn default() -> Self {
        Self {
            wants_docs: false,
            wants_onboarding: false,
            wants_runtime: false,
            wants_runtime_witnesses: false,
            wants_runtime_config_artifacts: false,
            wants_examples: false,
            wants_laravel_ui_witnesses: false,
            wants_laravel_form_action_witnesses: false,
            wants_livewire_view_witnesses: false,
            wants_laravel_layout_witnesses: false,
            wants_blade_component_witnesses: false,
            wants_commands_middleware_witnesses: false,
            wants_jobs_listeners_witnesses: false,
            wants_entrypoint_build_flow: false,
            wants_ci_workflow_witnesses: false,
            wants_scripts_ops_witnesses: false,
            wants_navigation_fallbacks: false,
            wants_tests: false,
            wants_test_witness_recall: false,
            wants_fixtures: false,
            wants_benchmarks: false,
            wants_readme: false,
            wants_contracts: false,
            wants_error_taxonomy: false,
            wants_tool_contracts: false,
            wants_mcp_runtime_surface: false,
            penalize_playbook_self_reference: true,
            goals: BTreeSet::new(),
            framework_hints: BTreeSet::new(),
            artifact_biases: BTreeSet::new(),
            strictness: PlannerStrictness::Broad,
            playbook_reference_policy: PlaybookReferencePolicy::PenalizeSelfReference,
            applied_rule_ids: Vec::new(),
        }
    }
}

#[allow(dead_code)]
impl SearchIntent {
    pub(super) fn from_query(query_text: &str) -> Self {
        let context = QueryContext::new(query_text);
        let mut builder = SearchIntentBuilder::default();
        builder.insert_goal(SearchGoal::Runtime);
        builder.populate_framework_hints(&context);
        builder.playbook_reference_policy = if context.mentions_playbooks() {
            PlaybookReferencePolicy::AllowSelfReference
        } else {
            PlaybookReferencePolicy::PenalizeSelfReference
        };

        for rule in SEARCH_INTENT_RULES {
            if (rule.apply)(&context, &mut builder) {
                builder.record_rule(rule.id);
            }
        }

        builder.build()
    }

    pub(super) fn goals(&self) -> &BTreeSet<SearchGoal> {
        &self.goals
    }

    pub(super) fn framework_hints(&self) -> &BTreeSet<FrameworkHint> {
        &self.framework_hints
    }

    pub(super) fn artifact_biases(&self) -> &BTreeSet<ArtifactBias> {
        &self.artifact_biases
    }

    pub(super) fn strictness(&self) -> PlannerStrictness {
        self.strictness
    }

    pub(super) fn playbook_reference_policy(&self) -> PlaybookReferencePolicy {
        self.playbook_reference_policy
    }

    pub(super) fn applied_rule_ids(&self) -> &[SearchIntentRuleId] {
        &self.applied_rule_ids
    }

    pub(super) fn has_goal(&self, goal: SearchGoal) -> bool {
        self.goals.contains(&goal)
    }

    pub(super) fn has_framework_hint(&self, hint: FrameworkHint) -> bool {
        self.framework_hints.contains(&hint)
    }

    pub(super) fn has_artifact_bias(&self, bias: ArtifactBias) -> bool {
        self.artifact_biases.contains(&bias)
    }

    pub(super) fn wants_path_witness_recall(&self) -> bool {
        self.has_goal(SearchGoal::RuntimeWitnesses)
            || self.has_artifact_bias(ArtifactBias::LaravelUi)
            || self.has_artifact_bias(ArtifactBias::CommandsMiddleware)
            || self.has_artifact_bias(ArtifactBias::JobsListeners)
            || self.has_goal(SearchGoal::EntryPointBuildFlow)
            || self.has_artifact_bias(ArtifactBias::CiWorkflow)
            || self.has_artifact_bias(ArtifactBias::ScriptsOps)
            || self.has_artifact_bias(ArtifactBias::RuntimeConfigArtifact)
            || self.has_artifact_bias(ArtifactBias::TestWitness)
            || self.has_goal(SearchGoal::Examples)
            || self.has_goal(SearchGoal::Benchmarks)
    }

    pub(super) fn wants_class(&self, class: SourceClass) -> bool {
        match class {
            SourceClass::ErrorContracts => {
                self.has_goal(SearchGoal::ErrorTaxonomy) || self.has_goal(SearchGoal::Contracts)
            }
            SourceClass::ToolContracts => {
                self.has_goal(SearchGoal::ToolContracts) || self.has_goal(SearchGoal::Contracts)
            }
            SourceClass::BenchmarkDocs => self.has_goal(SearchGoal::Benchmarks),
            SourceClass::Documentation => self.has_goal(SearchGoal::Documentation),
            SourceClass::Readme => self.has_goal(SearchGoal::Readme),
            SourceClass::Runtime => self.has_goal(SearchGoal::Runtime),
            SourceClass::Project => false,
            SourceClass::Support => {
                self.has_goal(SearchGoal::RuntimeWitnesses)
                    || self.has_goal(SearchGoal::Examples)
                    || self.has_goal(SearchGoal::Benchmarks)
                    || self.has_artifact_bias(ArtifactBias::CiWorkflow)
                    || self.has_artifact_bias(ArtifactBias::ScriptsOps)
            }
            SourceClass::Tests => {
                self.has_goal(SearchGoal::Tests)
                    || self.has_goal(SearchGoal::RuntimeWitnesses)
                    || self.has_goal(SearchGoal::Examples)
            }
            SourceClass::Fixtures => self.has_goal(SearchGoal::Fixtures),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct SearchIntentRule {
    id: SearchIntentRuleId,
    apply: fn(&QueryContext, &mut SearchIntentBuilder) -> bool,
}

#[derive(Debug)]
struct SearchIntentBuilder {
    goals: BTreeSet<SearchGoal>,
    framework_hints: BTreeSet<FrameworkHint>,
    artifact_biases: BTreeSet<ArtifactBias>,
    applied_rule_ids: Vec<SearchIntentRuleId>,
    strictness: Option<PlannerStrictness>,
    playbook_reference_policy: PlaybookReferencePolicy,
}

impl SearchIntentBuilder {
    fn insert_goal(&mut self, goal: SearchGoal) {
        self.goals.insert(goal);
    }

    fn insert_framework_hint(&mut self, hint: FrameworkHint) {
        self.framework_hints.insert(hint);
    }

    fn insert_artifact_bias(&mut self, bias: ArtifactBias) {
        self.artifact_biases.insert(bias);
    }

    fn has_goal(&self, goal: SearchGoal) -> bool {
        self.goals.contains(&goal)
    }

    fn has_artifact_bias(&self, bias: ArtifactBias) -> bool {
        self.artifact_biases.contains(&bias)
    }

    fn record_rule(&mut self, rule_id: SearchIntentRuleId) {
        if !self.applied_rule_ids.contains(&rule_id) {
            self.applied_rule_ids.push(rule_id);
        }
    }

    fn populate_framework_hints(&mut self, context: &QueryContext) {
        if context.has_any(&[
            "cargo",
            "cargo.toml",
            "cargo.lock",
            "rust",
            "crate",
            "crates",
        ]) {
            self.insert_framework_hint(FrameworkHint::Rust);
        }
        if context.has_any(&[
            "php", "composer", "artisan", "laravel", "blade", "livewire", "flux",
        ]) {
            self.insert_framework_hint(FrameworkHint::Php);
        }
        if context.has_any(&[
            "python",
            "pyproject",
            "pipfile",
            "requirements.txt",
            "pytest",
        ]) {
            self.insert_framework_hint(FrameworkHint::Python);
        }
        if context.has_any(&["blade"]) {
            self.insert_framework_hint(FrameworkHint::Blade);
        }
        if context.has_any(&["laravel", "blade", "livewire", "flux", "artisan"]) {
            self.insert_framework_hint(FrameworkHint::Laravel);
        }
        if context.has_any(&["livewire"]) {
            self.insert_framework_hint(FrameworkHint::Livewire);
        }
        if context.has_any(&["flux"]) {
            self.insert_framework_hint(FrameworkHint::Flux);
        }
        if context.has_any(&["mcp"]) {
            self.insert_framework_hint(FrameworkHint::Mcp);
        }
    }

    fn build(self) -> SearchIntent {
        let strictness = self.strictness.unwrap_or_else(|| {
            if self.goals.contains(&SearchGoal::RuntimeWitnesses)
                || self.artifact_biases.contains(&ArtifactBias::LaravelUi)
                || self
                    .artifact_biases
                    .contains(&ArtifactBias::RuntimeConfigArtifact)
                || self.artifact_biases.contains(&ArtifactBias::TestWitness)
            {
                PlannerStrictness::WitnessFocused
            } else {
                PlannerStrictness::Broad
            }
        });

        let wants_docs = self.goals.contains(&SearchGoal::Documentation);
        let wants_onboarding = self.goals.contains(&SearchGoal::Onboarding);
        let wants_runtime = self.goals.contains(&SearchGoal::Runtime);
        let wants_runtime_witnesses = self.goals.contains(&SearchGoal::RuntimeWitnesses);
        let wants_runtime_config_artifacts = self
            .artifact_biases
            .contains(&ArtifactBias::RuntimeConfigArtifact);
        let wants_examples = self.goals.contains(&SearchGoal::Examples);
        let wants_laravel_ui_witnesses = self.artifact_biases.contains(&ArtifactBias::LaravelUi);
        let wants_laravel_form_action_witnesses = self
            .artifact_biases
            .contains(&ArtifactBias::LaravelFormAction);
        let wants_livewire_view_witnesses =
            self.artifact_biases.contains(&ArtifactBias::LivewireView);
        let wants_laravel_layout_witnesses =
            self.artifact_biases.contains(&ArtifactBias::LaravelLayout);
        let wants_blade_component_witnesses =
            self.artifact_biases.contains(&ArtifactBias::BladeComponent);
        let wants_commands_middleware_witnesses = self
            .artifact_biases
            .contains(&ArtifactBias::CommandsMiddleware);
        let wants_jobs_listeners_witnesses =
            self.artifact_biases.contains(&ArtifactBias::JobsListeners);
        let wants_entrypoint_build_flow = self.goals.contains(&SearchGoal::EntryPointBuildFlow);
        let wants_ci_workflow_witnesses = self.artifact_biases.contains(&ArtifactBias::CiWorkflow);
        let wants_scripts_ops_witnesses = self.artifact_biases.contains(&ArtifactBias::ScriptsOps);
        let wants_navigation_fallbacks = self.goals.contains(&SearchGoal::NavigationFallbacks);
        let wants_tests = self.goals.contains(&SearchGoal::Tests);
        let wants_test_witness_recall = self.artifact_biases.contains(&ArtifactBias::TestWitness);
        let wants_fixtures = self.goals.contains(&SearchGoal::Fixtures);
        let wants_benchmarks = self.goals.contains(&SearchGoal::Benchmarks);
        let wants_readme = self.goals.contains(&SearchGoal::Readme);
        let wants_contracts = self.goals.contains(&SearchGoal::Contracts);
        let wants_error_taxonomy = self.goals.contains(&SearchGoal::ErrorTaxonomy);
        let wants_tool_contracts = self.goals.contains(&SearchGoal::ToolContracts);
        let wants_mcp_runtime_surface = self.goals.contains(&SearchGoal::McpRuntimeSurface);
        let penalize_playbook_self_reference =
            self.playbook_reference_policy == PlaybookReferencePolicy::PenalizeSelfReference;

        SearchIntent {
            wants_docs,
            wants_onboarding,
            wants_runtime,
            wants_runtime_witnesses,
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
            penalize_playbook_self_reference,
            goals: self.goals,
            framework_hints: self.framework_hints,
            artifact_biases: self.artifact_biases,
            strictness,
            playbook_reference_policy: self.playbook_reference_policy,
            applied_rule_ids: self.applied_rule_ids,
        }
    }
}

impl Default for SearchIntentBuilder {
    fn default() -> Self {
        Self {
            goals: BTreeSet::new(),
            framework_hints: BTreeSet::new(),
            artifact_biases: BTreeSet::new(),
            applied_rule_ids: Vec::new(),
            strictness: None,
            playbook_reference_policy: PlaybookReferencePolicy::PenalizeSelfReference,
        }
    }
}

#[derive(Debug, Clone)]
struct QueryContext {
    query: String,
}

impl QueryContext {
    fn new(query_text: &str) -> Self {
        Self {
            query: query_text.trim().to_ascii_lowercase(),
        }
    }

    fn has_any(&self, needles: &[&str]) -> bool {
        needles.iter().any(|needle| self.query.contains(needle))
    }

    fn has_blade_ui_surface_terms(&self) -> bool {
        self.has_any(&[
            "component",
            "components",
            "view",
            "views",
            "slot",
            "section",
        ])
    }

    fn has_blade_form_action_terms(&self) -> bool {
        self.has_any(&[
            "form", "forms", "modal", "modals", "partial", "partials", "table", "tables",
        ])
    }

    fn has_manifest_hint(&self) -> bool {
        self.has_any(&[
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
        ])
    }

    fn mentions_cli_context(&self) -> bool {
        self.has_any(&["cli", "command-line", "command line"])
    }

    fn mentions_laravel_ui(&self) -> bool {
        self.has_any(&["blade", "livewire", "flux"])
            && (self.has_blade_ui_surface_terms() || self.has_blade_form_action_terms())
    }

    fn has_strong_test_focus_terms(&self) -> bool {
        self.has_any(&[
            "fixture",
            "fixtures",
            "integration",
            "scenario",
            "assert",
            "coverage",
            "parity",
            "replay",
            "conformance",
            "inspector",
        ])
    }

    fn mentions_model_data_surface(&self) -> bool {
        self.has_any(&[
            "model",
            "models",
            "migration",
            "migrations",
            "seeder",
            "seeders",
            "factory",
            "factories",
            "policy",
            "policies",
            "validation",
            "database",
            "schema",
            "table",
            "tables",
        ])
    }

    fn mentions_playbooks(&self) -> bool {
        self.has_any(&["playbook", "playbooks"])
    }
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
    if !(context.has_any(&[
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
    ]) || (context.has_manifest_hint()
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
    if !(context.has_any(&[
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
    ]) || context.mentions_model_data_surface()
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
    ]) || (context.has_any(&[
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
    ])) || (context.has_any(&["route", "routes", "middleware", "provider", "providers"])
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
    if !context.has_any(&[
        "workflow",
        "workflows",
        "github action",
        "github actions",
        "autofix",
        "release workflow",
        "publish workflow",
        "deploy workflow",
    ]) {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::CiWorkflow);
    true
}

fn apply_scripts_ops_witness_terms(
    context: &QueryContext,
    builder: &mut SearchIntentBuilder,
) -> bool {
    if !context.has_any(&[
        "script",
        "scripts",
        "justfile",
        "makefile",
        "xtask",
        "changelog",
    ]) {
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

fn apply_test_witness_focus(context: &QueryContext, builder: &mut SearchIntentBuilder) -> bool {
    let has_test_or_cli_signal = builder.has_goal(SearchGoal::Tests)
        || (context.mentions_cli_context()
            && (builder.has_goal(SearchGoal::EntryPointBuildFlow)
                || builder.has_goal(SearchGoal::RuntimeWitnesses)));
    let bridge_docs_contract_runtime_tests = builder.has_goal(SearchGoal::Tests)
        && (builder.has_goal(SearchGoal::Contracts)
            || builder.has_goal(SearchGoal::ErrorTaxonomy)
            || builder.has_goal(SearchGoal::ToolContracts))
        && context.has_any(&["runtime helper", "runtime helpers", "helper", "call site"]);
    let laravel_ui_with_path_hints = builder.has_goal(SearchGoal::Tests)
        && builder.has_artifact_bias(ArtifactBias::LaravelUi)
        && builder.has_goal(SearchGoal::Documentation)
        && !builder.has_goal(SearchGoal::Readme)
        && !builder.has_goal(SearchGoal::Contracts)
        && !builder.has_goal(SearchGoal::ErrorTaxonomy)
        && !builder.has_goal(SearchGoal::ToolContracts);
    let docs_with_strong_test_focus = builder.has_goal(SearchGoal::Tests)
        && builder.has_goal(SearchGoal::Documentation)
        && context.has_strong_test_focus_terms()
        && !builder.has_goal(SearchGoal::Readme)
        && !builder.has_goal(SearchGoal::Contracts)
        && !builder.has_goal(SearchGoal::ErrorTaxonomy)
        && !builder.has_goal(SearchGoal::ToolContracts);

    if !(has_test_or_cli_signal
        && (bridge_docs_contract_runtime_tests
            || laravel_ui_with_path_hints
            || docs_with_strong_test_focus
            || (!builder.has_goal(SearchGoal::Documentation)
                && !builder.has_goal(SearchGoal::Readme)
                && !builder.has_goal(SearchGoal::Contracts)
                && !builder.has_goal(SearchGoal::ErrorTaxonomy)
                && !builder.has_goal(SearchGoal::ToolContracts))))
    {
        return false;
    }

    builder.insert_artifact_bias(ArtifactBias::TestWitness);
    if !bridge_docs_contract_runtime_tests {
        builder.strictness = Some(PlannerStrictness::WitnessFocused);
    } else {
        builder.insert_goal(SearchGoal::RuntimeWitnesses);
        builder.strictness = Some(PlannerStrictness::Broad);
    }
    true
}

static SEARCH_INTENT_RULES: &[SearchIntentRule] = &[
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

#[cfg(test)]
mod tests {
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
        let intent = SearchIntent::from_query(
            "blade component layout page header section slot render views",
        );

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
}
