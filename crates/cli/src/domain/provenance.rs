//! Normalized workload metadata emitted by MCP tool invocations.
//!
//! Bounds repository ids, query text, and timing fields into compact workload records attached to
//! MCP tool responses for observability without leaking full request payloads.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const WORKLOAD_REPOSITORY_ID_LIMIT: usize = 8;
const WORKLOAD_TEXT_LIMIT: usize = 256;

fn bounded_to_u64(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn bounded_text(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        value.to_owned()
    } else {
        value.chars().take(max_chars).collect()
    }
}

fn bounded_repository_ids(repository_ids: Vec<String>, limit: usize) -> Vec<String> {
    repository_ids
        .into_iter()
        .take(limit)
        .map(|id| bounded_text(&id, WORKLOAD_TEXT_LIMIT))
        .collect()
}

/// Canonical workload families observed by the MCP layer.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadToolFamily {
    Content,
    Search,
    Navigation,
    Playbook,
    Workspace,
    Unknown,
}

impl WorkloadToolFamily {
    /// Snake-case wire label for workload-family grouping in provenance.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Search => "search",
            Self::Navigation => "navigation",
            Self::Playbook => "playbook",
            Self::Workspace => "workspace",
            Self::Unknown => "unknown",
        }
    }
}

/// High-level normalized tool class within a workload family.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadToolClass {
    BoundedFileExploration,
    CallHierarchy,
    DefinitionNavigation,
    DocumentLookup,
    HybridDiscovery,
    LiteralLookup,
    PlaybookComposeCitations,
    PlaybookReplay,
    PlaybookRun,
    ReferenceNavigation,
    StructuralSearch,
    SymbolLookup,
    SymbolNavigation,
    WorkspaceMetadata,
    Unknown,
}

impl WorkloadToolClass {
    /// Maps an MCP tool name to its normalized workload class; unknown tools map to `Unknown`.
    pub fn from_tool_name(tool_name: &str) -> Self {
        match tool_name {
            "read_file" => Self::BoundedFileExploration,
            "explore" => Self::BoundedFileExploration,
            "search_text" => Self::LiteralLookup,
            "search_hybrid" => Self::HybridDiscovery,
            "search_symbol" => Self::SymbolLookup,
            "find_references" => Self::ReferenceNavigation,
            "go_to_definition" => Self::DefinitionNavigation,
            "find_declarations" => Self::SymbolNavigation,
            "find_implementations" => Self::SymbolNavigation,
            "incoming_calls" => Self::CallHierarchy,
            "outgoing_calls" => Self::CallHierarchy,
            "document_symbols" => Self::DocumentLookup,
            "search_structural" => Self::StructuralSearch,
            "playbook_run" => Self::PlaybookRun,
            "playbook_replay" => Self::PlaybookReplay,
            "playbook_compose_citations" => Self::PlaybookComposeCitations,
            "workspace" => Self::WorkspaceMetadata,
            "list_repositories" => Self::WorkspaceMetadata,
            "workspace_attach" => Self::WorkspaceMetadata,
            "workspace_detach" => Self::WorkspaceMetadata,
            "workspace_prepare" => Self::WorkspaceMetadata,
            "workspace_index" => Self::WorkspaceMetadata,
            "workspace_current" => Self::WorkspaceMetadata,
            _ => Self::Unknown,
        }
    }

    /// Parent workload family used for coarse provenance grouping.
    pub fn family(self) -> WorkloadToolFamily {
        match self {
            Self::BoundedFileExploration => WorkloadToolFamily::Content,
            Self::DefinitionNavigation => WorkloadToolFamily::Navigation,
            Self::SymbolNavigation => WorkloadToolFamily::Navigation,
            Self::CallHierarchy => WorkloadToolFamily::Navigation,
            Self::ReferenceNavigation => WorkloadToolFamily::Navigation,
            Self::DocumentLookup => WorkloadToolFamily::Content,
            Self::PlaybookComposeCitations | Self::PlaybookReplay | Self::PlaybookRun => {
                WorkloadToolFamily::Playbook
            }
            Self::LiteralLookup
            | Self::HybridDiscovery
            | Self::SymbolLookup
            | Self::StructuralSearch => WorkloadToolFamily::Search,
            Self::WorkspaceMetadata => WorkloadToolFamily::Workspace,
            Self::Unknown => WorkloadToolFamily::Unknown,
        }
    }

    /// Snake-case wire label for tool-class provenance fields.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::BoundedFileExploration => "bounded_file_exploration",
            Self::DefinitionNavigation => "definition_navigation",
            Self::DocumentLookup => "document_lookup",
            Self::CallHierarchy => "call_hierarchy",
            Self::HybridDiscovery => "hybrid_discovery",
            Self::LiteralLookup => "literal_lookup",
            Self::PlaybookComposeCitations => "playbook_compose_citations",
            Self::PlaybookReplay => "playbook_replay",
            Self::PlaybookRun => "playbook_run",
            Self::ReferenceNavigation => "reference_navigation",
            Self::StructuralSearch => "structural_search",
            Self::SymbolLookup => "symbol_lookup",
            Self::SymbolNavigation => "symbol_navigation",
            Self::WorkspaceMetadata => "workspace_metadata",
            Self::Unknown => "unknown",
        }
    }
}

/// Precision tier reported for a workload after graph and semantic fallbacks.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadPrecisionMode {
    Exact,
    Precise,
    Heuristic,
    Fallback,
    Unknown,
}

impl WorkloadPrecisionMode {
    /// Snake-case wire label for precision tier in workload metadata.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Precise => "precise",
            Self::Heuristic => "heuristic",
            Self::Fallback => "fallback",
            Self::Unknown => "unknown",
        }
    }

    /// Parses a precision label; unrecognized values map to `Unknown`.
    pub fn from_label(value: &str) -> Self {
        match value {
            "exact" => Self::Exact,
            "precise" => Self::Precise,
            "heuristic" => Self::Heuristic,
            "fallback" => Self::Fallback,
            _ => Self::Unknown,
        }
    }
}

/// Reason a workload degraded from precise to heuristic or fallback precision.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadFallbackReason {
    None,
    PreciseAbsent,
    ResourceBudget,
    StageFiltered,
    SemanticUnavailable,
    Timeout,
    UnsupportedFeature,
    Unknown,
}

impl WorkloadFallbackReason {
    /// Snake-case wire label for recovery/fallback attribution.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::PreciseAbsent => "precise_absent",
            Self::ResourceBudget => "resource_budget",
            Self::StageFiltered => "stage_filtered",
            Self::SemanticUnavailable => "semantic_unavailable",
            Self::Timeout => "timeout",
            Self::UnsupportedFeature => "unsupported_feature",
            Self::Unknown => "unknown",
        }
    }

    /// Parses a fallback-reason label; unrecognized values map to `Unknown`.
    pub fn from_label(value: &str) -> Self {
        match value {
            "none" => Self::None,
            "precise_absent" => Self::PreciseAbsent,
            "resource_budget" => Self::ResourceBudget,
            "stage_filtered" => Self::StageFiltered,
            "semantic_unavailable" => Self::SemanticUnavailable,
            "timeout" => Self::Timeout,
            "unsupported_feature" => Self::UnsupportedFeature,
            _ => Self::Unknown,
        }
    }
}

/// Repository cardinality for a normalized workload invocation.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadRepositoryScopeKind {
    Unspecified,
    Single,
    Multi,
}

impl WorkloadRepositoryScopeKind {
    /// Snake-case wire label for repository cardinality on a workload.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Unspecified => "unspecified",
            Self::Single => "single",
            Self::Multi => "multi",
        }
    }
}

/// Bounded repository scope metadata attached to workload provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkloadRepositoryScope {
    pub scope: WorkloadRepositoryScopeKind,
    pub repository_count: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub repository_ids: Vec<String>,
}

impl WorkloadRepositoryScope {
    /// Builds scope metadata; truncates id list and id text for workload payload bounds.
    pub fn new(repository_scope: WorkloadRepositoryScopeKind, repository_ids: Vec<String>) -> Self {
        let repository_count = repository_ids.len();
        let repository_ids = bounded_repository_ids(repository_ids, WORKLOAD_REPOSITORY_ID_LIMIT);

        Self {
            scope: repository_scope,
            repository_count,
            repository_ids,
        }
    }

    /// Empty scope when no repository context was attached to the invocation.
    pub fn unspecified() -> Self {
        Self::new(WorkloadRepositoryScopeKind::Unspecified, Vec::new())
    }

    /// Infers single/multi/unspecified scope from repository ids with payload bounds applied.
    pub fn from_repository_ids(repository_ids: &[String]) -> Self {
        match repository_ids.len() {
            0 => Self::unspecified(),
            1 => Self::new(
                WorkloadRepositoryScopeKind::Single,
                vec![bounded_text(&repository_ids[0], WORKLOAD_TEXT_LIMIT)],
            ),
            _ => Self::new(
                WorkloadRepositoryScopeKind::Multi,
                repository_ids
                    .iter()
                    .map(|id| bounded_text(id, WORKLOAD_TEXT_LIMIT))
                    .collect(),
            ),
        }
    }

    /// Snake-case cardinality label for the attached repository scope.
    pub fn scope_label(&self) -> &'static str {
        self.scope.as_str()
    }
}

/// Timing and cardinality sample for one workload attribution stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkloadStageSample {
    pub elapsed_us: u64,
    pub input_count: u64,
    pub output_count: u64,
}

impl WorkloadStageSample {
    /// Builds a stage timing/cardinality sample from already-bounded u64 counters.
    pub const fn new(elapsed_us: u64, input_count: u64, output_count: u64) -> Self {
        Self {
            elapsed_us,
            input_count,
            output_count,
        }
    }

    /// Convenience alias for constructing samples from u64 stage counters.
    pub fn from_u64s(elapsed_us: u64, input_count: u64, output_count: u64) -> Self {
        Self {
            elapsed_us,
            input_count,
            output_count,
        }
    }

    /// Converts usize stage counters into a sample, saturating at `u64::MAX`.
    pub fn bounded_from_usizes(elapsed_us: usize, input_count: usize, output_count: usize) -> Self {
        Self {
            elapsed_us: bounded_to_u64(elapsed_us),
            input_count: bounded_to_u64(input_count),
            output_count: bounded_to_u64(output_count),
        }
    }

    /// True when the stage recorded no elapsed time and no input/output traffic.
    pub fn is_zero(self) -> bool {
        self.elapsed_us == 0 && self.input_count == 0 && self.output_count == 0
    }
}

impl Default for WorkloadStageSample {
    fn default() -> Self {
        Self::new(0, 0, 0)
    }
}

/// Stage-level timing and cardinality samples for workload attribution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WorkloadStageAttribution {
    pub candidate_intake: WorkloadStageSample,
    pub freshness_validation: WorkloadStageSample,
    pub scan: WorkloadStageSample,
    pub witness_scoring: WorkloadStageSample,
    pub graph_expansion: WorkloadStageSample,
    pub semantic_retrieval: WorkloadStageSample,
    pub anchor_blending: WorkloadStageSample,
    pub document_aggregation: WorkloadStageSample,
    pub final_diversification: WorkloadStageSample,
}

impl WorkloadStageAttribution {
    /// Zeroed attribution used when stage timing was not collected.
    pub const fn empty() -> Self {
        Self {
            candidate_intake: WorkloadStageSample::new(0, 0, 0),
            freshness_validation: WorkloadStageSample::new(0, 0, 0),
            scan: WorkloadStageSample::new(0, 0, 0),
            witness_scoring: WorkloadStageSample::new(0, 0, 0),
            graph_expansion: WorkloadStageSample::new(0, 0, 0),
            semantic_retrieval: WorkloadStageSample::new(0, 0, 0),
            anchor_blending: WorkloadStageSample::new(0, 0, 0),
            document_aggregation: WorkloadStageSample::new(0, 0, 0),
            final_diversification: WorkloadStageSample::new(0, 0, 0),
        }
    }

    /// Records candidate intake stage timing and cardinality.
    pub fn with_candidate_intake(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.candidate_intake =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records freshness-validation stage timing and cardinality.
    pub fn with_freshness_validation(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.freshness_validation =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records lexical/scan stage timing and cardinality.
    pub fn with_scan(mut self, elapsed_us: usize, input_count: usize, output_count: usize) -> Self {
        self.scan = WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records path-witness scoring stage timing and cardinality.
    pub fn with_witness_scoring(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.witness_scoring =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records graph-expansion stage timing and cardinality.
    pub fn with_graph_expansion(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.graph_expansion =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records semantic-retrieval stage timing and cardinality.
    pub fn with_semantic_retrieval(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.semantic_retrieval =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records multi-channel anchor blending stage timing and cardinality.
    pub fn with_anchor_blending(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.anchor_blending =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records document-aggregation stage timing and cardinality.
    pub fn with_document_aggregation(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.document_aggregation =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// Records final diversification stage timing and cardinality.
    pub fn with_final_diversification(
        mut self,
        elapsed_us: usize,
        input_count: usize,
        output_count: usize,
    ) -> Self {
        self.final_diversification =
            WorkloadStageSample::bounded_from_usizes(elapsed_us, input_count, output_count);
        self
    }

    /// True when every stage sample is zero (no attribution recorded).
    pub fn is_empty(&self) -> bool {
        self == &Self::empty()
    }
}

impl Default for WorkloadStageAttribution {
    fn default() -> Self {
        Self::empty()
    }
}

/// Canonical workload summary attached to provenance events and MCP responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NormalizedWorkloadMetadata {
    pub tool_family: WorkloadToolFamily,
    pub tool_class: WorkloadToolClass,
    pub repository_scope: WorkloadRepositoryScope,
    pub precision_mode: WorkloadPrecisionMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<WorkloadFallbackReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason_detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_attribution: Option<WorkloadStageAttribution>,
}

impl NormalizedWorkloadMetadata {
    /// Builds workload metadata from an MCP tool name, scope, and precision tier.
    pub fn new(
        tool_name: &str,
        repository_scope: WorkloadRepositoryScope,
        precision_mode: WorkloadPrecisionMode,
    ) -> Self {
        let tool_class = WorkloadToolClass::from_tool_name(tool_name);
        Self {
            tool_family: tool_class.family(),
            tool_class,
            repository_scope,
            precision_mode,
            fallback_reason: None,
            fallback_reason_detail: None,
            stage_attribution: None,
        }
    }

    /// Attaches a recovery/fallback reason with optional bounded detail text.
    pub fn with_fallback_reason(
        mut self,
        fallback_reason: WorkloadFallbackReason,
        detail: Option<String>,
    ) -> Self {
        self.fallback_reason = Some(fallback_reason);
        self.fallback_reason_detail = detail.map(|value| bounded_text(&value, WORKLOAD_TEXT_LIMIT));
        self
    }

    /// Builds metadata while inferring repository scope from the id list.
    pub fn from_repository_ids(
        tool_name: &str,
        repository_ids: &[String],
        precision_mode: WorkloadPrecisionMode,
    ) -> Self {
        Self::new(
            tool_name,
            WorkloadRepositoryScope::from_repository_ids(repository_ids),
            precision_mode,
        )
    }

    /// Replaces the repository scope attached to this workload record.
    pub fn with_repository_scope(mut self, repository_scope: WorkloadRepositoryScope) -> Self {
        self.repository_scope = repository_scope;
        self
    }

    /// Sets or clears bounded free-form fallback detail without changing the reason code.
    pub fn with_fallback_reason_detail(mut self, detail: Option<String>) -> Self {
        self.fallback_reason_detail = detail.map(|value| bounded_text(&value, WORKLOAD_TEXT_LIMIT));
        self
    }

    /// Overrides the precision tier after graph/semantic recovery decisions.
    pub fn with_precision_mode(mut self, precision_mode: WorkloadPrecisionMode) -> Self {
        self.precision_mode = precision_mode;
        self
    }

    /// Attaches stage-level timing samples for workload attribution.
    pub fn with_stage_attribution(mut self, stage_attribution: WorkloadStageAttribution) -> Self {
        self.stage_attribution = Some(stage_attribution);
        self
    }

    /// True when a fallback reason was recorded for this invocation.
    pub fn has_fallback(&self) -> bool {
        self.fallback_reason.is_some()
    }

    /// Snake-case repository-scope cardinality label.
    pub fn repository_scope_label(&self) -> &'static str {
        self.repository_scope.scope_label()
    }

    /// Serializes to JSON for MCP/provenance payloads, with a minimal fallback on error.
    pub fn as_payload_value(&self) -> Value {
        serde_json::to_value(self).unwrap_or_else(|_| {
            json!({
                "tool_family": self.tool_family.as_str(),
                "tool_class": self.tool_class.as_str(),
                "precision_mode": self.precision_mode.as_str(),
                "repository_scope": {
                    "scope": self.repository_scope.scope.as_str(),
                    "repository_count": self.repository_scope.repository_count,
                },
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_precision_from_str() {
        assert_eq!(
            WorkloadPrecisionMode::from_label("exact"),
            WorkloadPrecisionMode::Exact
        );
        assert_eq!(
            WorkloadPrecisionMode::from_label("heuristic"),
            WorkloadPrecisionMode::Heuristic
        );
        assert_eq!(
            WorkloadPrecisionMode::from_label("missing"),
            WorkloadPrecisionMode::Unknown
        );
    }

    #[test]
    fn fallback_reason_from_str() {
        assert_eq!(
            WorkloadFallbackReason::from_label("precise_absent"),
            WorkloadFallbackReason::PreciseAbsent
        );
        assert_eq!(
            WorkloadFallbackReason::from_label("resource_budget"),
            WorkloadFallbackReason::ResourceBudget
        );
        assert_eq!(
            WorkloadFallbackReason::from_label("unknown_reason"),
            WorkloadFallbackReason::Unknown
        );
    }

    #[test]
    fn scope_from_repository_ids_is_bounded() {
        let scope = WorkloadRepositoryScope::from_repository_ids(&[
            "repo-one".to_owned(),
            "repo-two".to_owned(),
        ]);
        assert_eq!(scope.scope, WorkloadRepositoryScopeKind::Multi);
        assert_eq!(scope.repository_count, 2);
    }

    #[test]
    fn normalized_workload_metadata_builder() {
        let metadata = NormalizedWorkloadMetadata::from_repository_ids(
            "search_text",
            &["repo-one".to_owned()],
            WorkloadPrecisionMode::Heuristic,
        )
        .with_fallback_reason(
            WorkloadFallbackReason::PreciseAbsent,
            Some("no matches".to_owned()),
        );

        assert_eq!(metadata.tool_class, WorkloadToolClass::LiteralLookup);
        assert_eq!(metadata.precision_mode, WorkloadPrecisionMode::Heuristic);
        assert!(metadata.has_fallback());
        assert_eq!(metadata.repository_scope.repository_count, 1);
    }

    #[test]
    fn call_hierarchy_tool_class_is_stable() {
        assert_eq!(
            WorkloadToolClass::from_tool_name("incoming_calls"),
            WorkloadToolClass::CallHierarchy
        );
        assert_eq!(
            WorkloadToolClass::from_tool_name("outgoing_calls"),
            WorkloadToolClass::CallHierarchy
        );
    }
}
