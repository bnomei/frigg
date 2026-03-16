use std::collections::BTreeSet;

use crate::domain::{
    ArtifactBias, FrameworkHint, PlannerStrictness, PlaybookReferencePolicy, SearchGoal,
    SearchIntentRuleId, SourceClass,
};
use crate::languages::SymbolLanguage;
use context::QueryContext;
use rules::SEARCH_INTENT_RULES;

#[path = "intent/context.rs"]
mod context;
#[path = "intent/rules.rs"]
mod rules;
#[cfg(test)]
#[path = "intent/tests.rs"]
mod tests;

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

#[derive(Debug, Clone, Copy)]
pub(super) struct SearchIntentQuerySignals<'a> {
    intent: &'a SearchIntent,
}

impl SearchIntentQuerySignals<'_> {
    pub(super) fn wants_docs(self) -> bool {
        self.intent.wants_docs
    }

    pub(super) fn wants_runtime(self) -> bool {
        self.intent.wants_runtime
    }

    pub(super) fn wants_examples(self) -> bool {
        self.intent.wants_examples
    }

    pub(super) fn wants_entrypoint_build_flow(self) -> bool {
        self.intent.wants_entrypoint_build_flow
    }

    pub(super) fn wants_tests(self) -> bool {
        self.intent.wants_tests
    }

    pub(super) fn wants_fixtures(self) -> bool {
        self.intent.wants_fixtures
    }

    pub(super) fn wants_benchmarks(self) -> bool {
        self.intent.wants_benchmarks
    }

    pub(super) fn wants_readme(self) -> bool {
        self.intent.wants_readme
    }

    pub(super) fn wants_contracts(self) -> bool {
        self.intent.wants_contracts
    }

    pub(super) fn wants_error_taxonomy(self) -> bool {
        self.intent.wants_error_taxonomy
    }

    pub(super) fn wants_tool_contracts(self) -> bool {
        self.intent.wants_tool_contracts
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct SearchIntentWitnessSignals<'a> {
    intent: &'a SearchIntent,
}

impl SearchIntentWitnessSignals<'_> {
    pub(super) fn wants_runtime_witnesses(self) -> bool {
        self.intent.wants_runtime_witnesses
    }

    pub(super) fn wants_runtime_config_artifacts(self) -> bool {
        self.intent.wants_runtime_config_artifacts
    }

    pub(super) fn wants_laravel_ui_witnesses(self) -> bool {
        self.intent.wants_laravel_ui_witnesses
    }

    pub(super) fn wants_commands_middleware_witnesses(self) -> bool {
        self.intent.wants_commands_middleware_witnesses
    }

    pub(super) fn wants_jobs_listeners_witnesses(self) -> bool {
        self.intent.wants_jobs_listeners_witnesses
    }

    pub(super) fn wants_ci_workflow_witnesses(self) -> bool {
        self.intent.wants_ci_workflow_witnesses
    }

    pub(super) fn wants_scripts_ops_witnesses(self) -> bool {
        self.intent.wants_scripts_ops_witnesses
    }

    pub(super) fn wants_test_witness_recall(self) -> bool {
        self.intent.wants_test_witness_recall
    }
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

    pub(super) fn query_signals(&self) -> SearchIntentQuerySignals<'_> {
        SearchIntentQuerySignals { intent: self }
    }

    pub(super) fn witness_signals(&self) -> SearchIntentWitnessSignals<'_> {
        SearchIntentWitnessSignals { intent: self }
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

    pub(super) fn penalizes_playbook_self_reference(&self) -> bool {
        self.penalize_playbook_self_reference
    }

    pub(super) fn penalizes_generic_runtime_docs(&self) -> bool {
        !self.wants_docs && !self.wants_onboarding && !self.wants_readme
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

    pub(super) fn has_language_hint(&self) -> bool {
        self.framework_hints.iter().any(|hint| {
            matches!(
                hint,
                FrameworkHint::Rust
                    | FrameworkHint::Php
                    | FrameworkHint::TypeScript
                    | FrameworkHint::Python
                    | FrameworkHint::Go
                    | FrameworkHint::Kotlin
                    | FrameworkHint::Lua
                    | FrameworkHint::Roc
                    | FrameworkHint::Nim
                    | FrameworkHint::Blade
            )
        })
    }

    pub(super) fn wants_language_locality_bias(&self) -> bool {
        self.has_language_hint()
            && (self.wants_runtime_witnesses
                || self.wants_runtime_config_artifacts
                || self.wants_entrypoint_build_flow
                || self.wants_test_witness_recall)
    }

    pub(super) fn prefers_symbol_language(&self, language: SymbolLanguage) -> bool {
        match language {
            SymbolLanguage::Rust => self.has_framework_hint(FrameworkHint::Rust),
            SymbolLanguage::Php => {
                self.has_framework_hint(FrameworkHint::Php)
                    || self.has_framework_hint(FrameworkHint::Blade)
                    || self.has_framework_hint(FrameworkHint::Laravel)
            }
            SymbolLanguage::Blade => {
                self.has_framework_hint(FrameworkHint::Blade)
                    || self.has_framework_hint(FrameworkHint::Php)
                    || self.has_framework_hint(FrameworkHint::Laravel)
            }
            SymbolLanguage::TypeScript => self.has_framework_hint(FrameworkHint::TypeScript),
            SymbolLanguage::Python => self.has_framework_hint(FrameworkHint::Python),
            SymbolLanguage::Go => self.has_framework_hint(FrameworkHint::Go),
            SymbolLanguage::Kotlin => self.has_framework_hint(FrameworkHint::Kotlin),
            SymbolLanguage::Lua => self.has_framework_hint(FrameworkHint::Lua),
            SymbolLanguage::Roc => self.has_framework_hint(FrameworkHint::Roc),
            SymbolLanguage::Nim => self.has_framework_hint(FrameworkHint::Nim),
        }
    }

    pub(super) fn wants_path_witness_recall(&self) -> bool {
        let query = self.query_signals();
        let witness = self.witness_signals();

        witness.wants_runtime_witnesses()
            || witness.wants_laravel_ui_witnesses()
            || witness.wants_commands_middleware_witnesses()
            || witness.wants_jobs_listeners_witnesses()
            || query.wants_entrypoint_build_flow()
            || witness.wants_ci_workflow_witnesses()
            || witness.wants_scripts_ops_witnesses()
            || witness.wants_runtime_config_artifacts()
            || witness.wants_test_witness_recall()
            || self.wants_example_or_bench_witnesses()
    }

    pub(super) fn wants_example_or_bench_witnesses(&self) -> bool {
        self.wants_examples || self.wants_benchmarks
    }

    pub(super) fn wants_class(&self, class: SourceClass) -> bool {
        let query = self.query_signals();
        let witness = self.witness_signals();

        match class {
            SourceClass::ErrorContracts => query.wants_error_taxonomy() || query.wants_contracts(),
            SourceClass::ToolContracts => query.wants_tool_contracts() || query.wants_contracts(),
            SourceClass::BenchmarkDocs => query.wants_benchmarks(),
            SourceClass::Documentation => query.wants_docs(),
            SourceClass::Readme => query.wants_readme(),
            SourceClass::Runtime => query.wants_runtime(),
            SourceClass::Project => false,
            SourceClass::Support => {
                witness.wants_runtime_witnesses()
                    || query.wants_examples()
                    || query.wants_benchmarks()
                    || witness.wants_ci_workflow_witnesses()
                    || witness.wants_scripts_ops_witnesses()
            }
            SourceClass::Tests => {
                query.wants_tests() || witness.wants_runtime_witnesses() || query.wants_examples()
            }
            SourceClass::Fixtures => query.wants_fixtures(),
            _ => false,
        }
    }
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
        if context.mentions_rust_family() {
            self.insert_framework_hint(FrameworkHint::Rust);
        }
        if context.mentions_php_family() {
            self.insert_framework_hint(FrameworkHint::Php);
        }
        if context.mentions_typescript_family() {
            self.insert_framework_hint(FrameworkHint::TypeScript);
        }
        if context.mentions_python_family() {
            self.insert_framework_hint(FrameworkHint::Python);
        }
        if context.mentions_go_family() {
            self.insert_framework_hint(FrameworkHint::Go);
        }
        if context.mentions_kotlin_family() {
            self.insert_framework_hint(FrameworkHint::Kotlin);
        }
        if context.mentions_lua_family() {
            self.insert_framework_hint(FrameworkHint::Lua);
        }
        if context.mentions_roc_family() {
            self.insert_framework_hint(FrameworkHint::Roc);
        }
        if context.mentions_nim_family() {
            self.insert_framework_hint(FrameworkHint::Nim);
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
