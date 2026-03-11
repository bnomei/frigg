use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PathClass {
    Runtime,
    Project,
    Support,
}

impl PathClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Project => "project",
            Self::Support => "support",
        }
    }

    pub const fn rank(self) -> u8 {
        match self {
            Self::Runtime => 0,
            Self::Project => 1,
            Self::Support => 2,
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "runtime" => Some(Self::Runtime),
            "project" => Some(Self::Project),
            "support" => Some(Self::Support),
            _ => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SourceClass {
    ErrorContracts,
    ToolContracts,
    BenchmarkDocs,
    Documentation,
    Readme,
    Runtime,
    Project,
    Support,
    Tests,
    Fixtures,
    Playbooks,
    Specs,
    Other,
}

impl SourceClass {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ErrorContracts => "error_contracts",
            Self::ToolContracts => "tool_contracts",
            Self::BenchmarkDocs => "benchmark_docs",
            Self::Documentation => "documentation",
            Self::Readme => "readme",
            Self::Runtime => "runtime",
            Self::Project => "project",
            Self::Support => "support",
            Self::Tests => "tests",
            Self::Fixtures => "fixtures",
            Self::Playbooks => "playbooks",
            Self::Specs => "specs",
            Self::Other => "other",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value {
            "error_contracts" => Some(Self::ErrorContracts),
            "tool_contracts" => Some(Self::ToolContracts),
            "benchmark_docs" => Some(Self::BenchmarkDocs),
            "documentation" => Some(Self::Documentation),
            "readme" => Some(Self::Readme),
            "runtime" => Some(Self::Runtime),
            "project" => Some(Self::Project),
            "support" => Some(Self::Support),
            "tests" => Some(Self::Tests),
            "fixtures" => Some(Self::Fixtures),
            "playbooks" => Some(Self::Playbooks),
            "specs" => Some(Self::Specs),
            "other" => Some(Self::Other),
            _ => None,
        }
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SearchGoal {
    Documentation,
    Onboarding,
    Runtime,
    RuntimeWitnesses,
    Examples,
    Tests,
    Fixtures,
    Benchmarks,
    Readme,
    Contracts,
    ErrorTaxonomy,
    ToolContracts,
    McpRuntimeSurface,
    NavigationFallbacks,
    EntryPointBuildFlow,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum FrameworkHint {
    Rust,
    Php,
    Python,
    Blade,
    Laravel,
    Livewire,
    Flux,
    Mcp,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactBias {
    RuntimeConfigArtifact,
    TestWitness,
    LaravelUi,
    LaravelFormAction,
    LivewireView,
    LaravelLayout,
    BladeComponent,
    CommandsMiddleware,
    JobsListeners,
    CiWorkflow,
    ScriptsOps,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PlannerStrictness {
    Broad,
    WitnessFocused,
    ExactAnchorBiased,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PlaybookReferencePolicy {
    PenalizeSelfReference,
    AllowSelfReference,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum SearchIntentRuleId {
    DocumentationTerms,
    TestsTerms,
    ExamplesTerms,
    OnboardingTerms,
    ReadmeTerms,
    ContractsTerms,
    ErrorTaxonomyTerms,
    ToolContractTerms,
    McpRuntimeSurfaceTerms,
    FixturesTerms,
    BenchmarksTerms,
    RuntimeConfigArtifactTerms,
    LaravelUiWitnessTerms,
    LaravelFormActionWitnessTerms,
    BladeComponentWitnessTerms,
    LivewireViewWitnessTerms,
    LaravelLayoutWitnessTerms,
    CommandsMiddlewareWitnessTerms,
    JobsListenersWitnessTerms,
    RuntimeWitnessTerms,
    EntryPointBuildFlowTerms,
    CiWorkflowWitnessTerms,
    ScriptsOpsWitnessTerms,
    NavigationFallbackTerms,
    TestWitnessFocus,
}

impl SearchIntentRuleId {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DocumentationTerms => "documentation_terms",
            Self::TestsTerms => "tests_terms",
            Self::ExamplesTerms => "examples_terms",
            Self::OnboardingTerms => "onboarding_terms",
            Self::ReadmeTerms => "readme_terms",
            Self::ContractsTerms => "contracts_terms",
            Self::ErrorTaxonomyTerms => "error_taxonomy_terms",
            Self::ToolContractTerms => "tool_contract_terms",
            Self::McpRuntimeSurfaceTerms => "mcp_runtime_surface_terms",
            Self::FixturesTerms => "fixtures_terms",
            Self::BenchmarksTerms => "benchmarks_terms",
            Self::RuntimeConfigArtifactTerms => "runtime_config_artifact_terms",
            Self::LaravelUiWitnessTerms => "laravel_ui_witness_terms",
            Self::LaravelFormActionWitnessTerms => "laravel_form_action_witness_terms",
            Self::BladeComponentWitnessTerms => "blade_component_witness_terms",
            Self::LivewireViewWitnessTerms => "livewire_view_witness_terms",
            Self::LaravelLayoutWitnessTerms => "laravel_layout_witness_terms",
            Self::CommandsMiddlewareWitnessTerms => "commands_middleware_witness_terms",
            Self::JobsListenersWitnessTerms => "jobs_listeners_witness_terms",
            Self::RuntimeWitnessTerms => "runtime_witness_terms",
            Self::EntryPointBuildFlowTerms => "entrypoint_build_flow_terms",
            Self::CiWorkflowWitnessTerms => "ci_workflow_witness_terms",
            Self::ScriptsOpsWitnessTerms => "scripts_ops_witness_terms",
            Self::NavigationFallbackTerms => "navigation_fallback_terms",
            Self::TestWitnessFocus => "test_witness_focus",
        }
    }
}
