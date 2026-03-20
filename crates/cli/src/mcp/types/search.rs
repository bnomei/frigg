use std::collections::BTreeMap;

use crate::domain::{
    ChannelHealthStatus, EvidenceAnchor, PathClass, SourceClass, model::SymbolMatch,
    model::TextMatch,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchPatternType {
    Literal,
    Regex,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExploreOperation {
    Probe,
    Zoom,
    Refine,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreAnchor {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreCursor {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreLineWindow {
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreWindow {
    pub start_line: usize,
    pub end_line: usize,
    pub bytes: usize,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreMatch {
    pub match_id: String,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
    pub window: ExploreWindow,
    pub anchor: ExploreAnchor,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreMetadata {
    pub lossy_utf8: bool,
    pub effective_context_lines: usize,
    pub effective_max_matches: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreParams {
    /// Artifact path using the same canonical repository-relative semantics as `read_file`.
    pub path: String,
    /// Optional repository scope from `list_repositories`.
    pub repository_id: Option<String>,
    /// Explorer mode: `probe` scans an artifact, `zoom` returns a bounded window, and `refine` searches only inside an anchor-derived window.
    pub operation: ExploreOperation,
    /// Search query for `probe` or `refine`. Leading and trailing whitespace is trimmed.
    pub query: Option<String>,
    /// Match mode for `query`. Omit for exact literal search or set `regex` for safe-regex search.
    pub pattern_type: Option<SearchPatternType>,
    /// Explicit anchor used by `zoom` and `refine`.
    pub anchor: Option<ExploreAnchor>,
    /// Context lines to include around anchors and match windows. Omit to use the explorer default.
    pub context_lines: Option<usize>,
    /// Max match rows to return. Omit to use the explorer default.
    pub max_matches: Option<usize>,
    /// Explicit continuation cursor for `probe` or `refine`.
    pub resume_from: Option<ExploreCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreResponse {
    pub repository_id: String,
    pub path: String,
    pub operation: ExploreOperation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern_type: Option<SearchPatternType>,
    pub total_lines: usize,
    pub scan_scope: ExploreLineWindow,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<ExploreWindow>,
    pub total_matches: usize,
    pub matches: Vec<ExploreMatch>,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<ExploreCursor>,
    pub metadata: ExploreMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextParams {
    /// Text or regex pattern to search for. Leading and trailing whitespace is trimmed.
    pub query: String,
    /// Match mode for `query`. Omit for exact literal search or set `regex` for safe-regex search.
    pub pattern_type: Option<SearchPatternType>,
    /// Optional repository scope from `list_repositories`.
    pub repository_id: Option<String>,
    /// Optional safe regex over canonical repository-relative paths.
    /// Use this to narrow code, docs, or runtime slices.
    pub path_regex: Option<String>,
    /// Optional max matches. Frigg clamps the effective limit to the server search budget.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextResponse {
    pub total_matches: usize,
    pub matches: Vec<TextMatch>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelWeightsParams {
    pub lexical: Option<f32>,
    pub graph: Option<f32>,
    pub semantic: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridParams {
    /// Broad natural-language or exact-phrase repository query.
    pub query: String,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    /// Optional language filter for source-backed follow-up.
    pub language: Option<String>,
    /// Optional max matches.
    pub limit: Option<usize>,
    /// Optional channel-weight overrides.
    pub weights: Option<SearchHybridChannelWeightsParams>,
    /// Optional semantic-channel toggle.
    pub semantic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridMatch {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub excerpt: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchor: Option<EvidenceAnchor>,
    pub blended_score: f32,
    pub lexical_score: f32,
    pub graph_score: f32,
    pub semantic_score: f32,
    pub lexical_sources: Vec<String>,
    pub graph_sources: Vec<String>,
    pub semantic_sources: Vec<String>,
    /// Generic path-class hint for choosing a first navigation pivot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_class: Option<PathClass>,
    /// Generic source-class hint from shared runtime/support/project classification.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_class: Option<SourceClass>,
    /// Generic surface-family hints such as `runtime`, `tests`, or `entrypoint`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_families: Vec<String>,
    /// Live-navigation hint describing whether this match is a good follow-up pivot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navigation_hint: Option<SearchHybridNavigationHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridNavigationHint {
    /// True when the match is a reasonable first pivot for `read_file` or symbol follow-up.
    pub pivotable: bool,
    /// True when `document_symbols` is expected to be useful on this path.
    pub document_symbols: bool,
    /// True when symbol/anchor follow-up is likely to support `go_to_definition`.
    pub go_to_definition: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridUtilitySummary {
    /// Count of returned matches that look like useful live-navigation pivots.
    pub pivotable_match_count: usize,
    /// One-based rank of the best generic pivot inside the returned result set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_rank: Option<usize>,
    /// Canonical path of the best generic pivot inside the returned result set.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_path: Option<String>,
    /// Repository id for the best generic pivot when cross-repository search is used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_repository_id: Option<String>,
    /// True when the returned set contains at least one pivot that likely supports symbol follow-up.
    pub symbol_navigation_ready: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelMetadata {
    pub status: ChannelHealthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub candidate_count: usize,
    pub hit_count: usize,
    pub match_count: usize,
    pub diagnostic_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<SearchHybridChannelDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridDiagnosticsSummary {
    pub walk: usize,
    pub read: usize,
    pub total: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridStageSample {
    pub elapsed_us: u64,
    pub input_count: usize,
    pub output_count: usize,
}

impl From<&crate::searcher::SearchStageSample> for SearchHybridStageSample {
    fn from(value: &crate::searcher::SearchStageSample) -> Self {
        Self {
            elapsed_us: value.elapsed_us,
            input_count: value.input_count,
            output_count: value.output_count,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridStageAttribution {
    pub candidate_intake: SearchHybridStageSample,
    pub freshness_validation: SearchHybridStageSample,
    pub scan: SearchHybridStageSample,
    pub witness_scoring: SearchHybridStageSample,
    pub graph_expansion: SearchHybridStageSample,
    pub semantic_retrieval: SearchHybridStageSample,
    pub anchor_blending: SearchHybridStageSample,
    pub document_aggregation: SearchHybridStageSample,
    pub final_diversification: SearchHybridStageSample,
}

impl From<&crate::searcher::SearchStageAttribution> for SearchHybridStageAttribution {
    fn from(value: &crate::searcher::SearchStageAttribution) -> Self {
        Self {
            candidate_intake: SearchHybridStageSample::from(&value.candidate_intake),
            freshness_validation: SearchHybridStageSample::from(&value.freshness_validation),
            scan: SearchHybridStageSample::from(&value.scan),
            witness_scoring: SearchHybridStageSample::from(&value.witness_scoring),
            graph_expansion: SearchHybridStageSample::from(&value.graph_expansion),
            semantic_retrieval: SearchHybridStageSample::from(&value.semantic_retrieval),
            anchor_blending: SearchHybridStageSample::from(&value.anchor_blending),
            document_aggregation: SearchHybridStageSample::from(&value.document_aggregation),
            final_diversification: SearchHybridStageSample::from(&value.final_diversification),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseFreshnessRepositoryMetadata {
    pub repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub manifest: String,
    pub semantic: String,
    pub dirty_root: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseFreshnessBasisMetadata {
    pub mode: String,
    pub cacheable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<ResponseFreshnessRepositoryMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridSemanticAcceleratorMetadata {
    pub tier: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ChannelHealthStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridLanguageCapabilityMetadata {
    pub requested_language: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub semantic_chunking: String,
    pub semantic_accelerator: SearchHybridSemanticAcceleratorMetadata,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub capabilities: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridMetadata {
    pub channels: BTreeMap<String, SearchHybridChannelMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_requested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_status: Option<ChannelHealthStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_candidate_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_hit_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_match_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_only_mode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    pub diagnostics_count: usize,
    pub diagnostics: SearchHybridDiagnosticsSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_attribution: Option<SearchHybridStageAttribution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_capability: Option<SearchHybridLanguageCapabilityMetadata>,
    /// Utility summary for discovery-to-navigation workflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utility: Option<SearchHybridUtilitySummary>,
    pub freshness_basis: ResponseFreshnessBasisMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridResponse {
    pub matches: Vec<SearchHybridMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_requested: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_status: Option<ChannelHealthStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_hit_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_match_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// Structured diagnostics payload for live responses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SearchHybridMetadata>,
    /// Human-readable summary note.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolParams {
    /// API, type, or function name to search in indexed symbols.
    pub query: String,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    /// Optional path class filter: `runtime`, `support`, or `project`.
    pub path_class: Option<SearchSymbolPathClass>,
    /// Optional safe regex over canonical repository-relative symbol paths.
    pub path_regex: Option<String>,
    /// Optional max matches.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolResponse {
    pub matches: Vec<SymbolMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchSymbolPathClass {
    Runtime,
    Project,
    Support,
}

impl SearchSymbolPathClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Project => "project",
            Self::Support => "support",
        }
    }
}
