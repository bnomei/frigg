//! Search and exploration MCP wire types: text, hybrid, symbol, structural, and `explore` contracts.

use std::collections::BTreeMap;

use super::{MetadataObject, ReadPresentationMode, ResponseMode};
use crate::domain::{
    ChannelHealthStatus, EvidenceAnchor, PathClass, SourceClass, model::SymbolMatch,
    model::TextMatch,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Literal or safe-regex matching mode for text and explore queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchPatternType {
    Literal,
    Regex,
}

/// In-file explorer mode: scan, zoom to an anchor window, or refine within an anchor scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ExploreOperation {
    Probe,
    Zoom,
    Refine,
}

/// 1-based source anchor bounding an explore zoom or refine window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreAnchor {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// 1-based continuation cursor for paginated explore probe or refine scans.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreCursor {
    pub line: usize,
    pub column: usize,
}

/// Inclusive 1-based line window inside one repository file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreLineWindow {
    pub start_line: usize,
    pub end_line: usize,
}

/// Bounded source excerpt returned by explore zoom or match windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreWindow {
    pub start_line: usize,
    pub end_line: usize,
    pub bytes: usize,
    pub content: String,
}

/// One explore match row with excerpt, anchor, and local context window.
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

/// Explorer execution metadata including effective limits and optional context-efficiency stats.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ExploreMetadata {
    pub lossy_utf8: bool,
    pub effective_context_lines: usize,
    pub effective_max_matches: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_efficiency: Option<ContextEfficiencyMetadata>,
}

/// Parameters for the extended `explore` tool.
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
    /// Read-surface presentation mode. Zoom defaults to raw text content; probe/refine default to JSON.
    pub presentation_mode: Option<ReadPresentationMode>,
    /// Include bounded context-efficiency metadata in the response. Requires JSON presentation.
    pub include_context_efficiency: Option<bool>,
}

/// Response from the extended `explore` tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
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

/// Parameters for `search_text`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
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
    /// Optional inline excerpt window expansion for simple review flows. Omit to keep one-line excerpts.
    pub context_lines: Option<usize>,
    /// Optional bound on returned hits per file after lexical matching.
    pub max_matches_per_file: Option<usize>,
    /// Optional repeated-path collapse mode for noisy lexical result sets.
    pub collapse_by_file: Option<bool>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
    /// Include bounded context-efficiency metadata in the response. Defaults to false.
    pub include_context_efficiency: Option<bool>,
}

/// Response from `search_text` with optional `result_handle` for `read_match`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextResponse {
    pub total_matches: usize,
    pub matches: Vec<TextMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SearchTextMetadata>,
}

/// Lexical search backend mix reported in text and hybrid metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchLexicalBackendMetadata {
    Native,
    Ripgrep,
    Mixed,
}

/// Optional metadata returned by `search_text`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextMetadata {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_backend: Option<SearchLexicalBackendMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_backend_note: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_efficiency: Option<ContextEfficiencyMetadata>,
}

/// Bounded context-efficiency metadata returned by read and search tools when requested.
///
/// Estimates compare indexed corpus size against returned match and source bytes so callers
/// can judge how much repository context a tool response avoided loading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ContextEfficiencyMetadata {
    pub indexed_readable_files: usize,
    pub indexed_readable_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_min_mtime_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub indexed_max_mtime_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_input_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_output_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_match_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_unique_paths: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_unique_file_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub returned_source_bytes_estimate: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_file_context_saved_bytes_estimate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_file_context_saved_percent_estimate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus_context_saved_bytes_estimate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus_context_saved_percent_estimate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub corpus_narrowing_ratio_estimate: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query_duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub narrowing_ratio_estimate: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_attribution: Option<ContextEfficiencyStageAttribution>,
}

/// Candidate narrowing counts attributed to one context-efficiency measurement stage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContextEfficiencyStageAttribution {
    pub candidate_input_count: usize,
    pub candidate_output_count: usize,
}

/// Optional lexical, graph, and semantic weight overrides for `search_hybrid`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelWeightsParams {
    pub lexical: Option<f32>,
    pub graph: Option<f32>,
    pub semantic: Option<f32>,
}

/// Parameters for `search_hybrid`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridParams {
    /// Discovery-style natural-language or subsystem query.
    /// For direct exact strings or regexes use `search_text`; for known identifiers use `search_symbol`.
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
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
    /// Include bounded context-efficiency metadata in the response. Defaults to false.
    pub include_context_efficiency: Option<bool>,
}

/// One blended hybrid-search match with channel scores and navigation hints.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
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
    /// Concise explanation of the strongest signals that lifted this match.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rank_reasons: Vec<SearchHybridRankReason>,
}

/// Short explanation of the strongest signal that lifted a hybrid match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchHybridRankReason {
    ExactSymbolMatch,
    ExactTextMatch,
    StrongLexicalAnchor,
    GraphAdjacency,
    SemanticContribution,
    WitnessOnlyFallback,
}

/// Follow-up navigation affordances suggested for one hybrid match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridNavigationHint {
    /// True when the match is a reasonable first pivot for `read_file` or symbol follow-up.
    pub pivotable: bool,
    /// True when `document_symbols` is expected to be useful on this path.
    pub document_symbols: bool,
    /// True when symbol/anchor follow-up is likely to support `go_to_definition`.
    pub go_to_definition: bool,
}

/// Utility summary for turning hybrid discovery results into live navigation pivots.
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

/// One structured diagnostic emitted by a hybrid search channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridChannelDiagnostic {
    pub code: String,
    pub message: String,
}

/// Health and throughput metadata for one hybrid search channel.
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

/// Aggregate manifest walk and read diagnostic counts for hybrid search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridDiagnosticsSummary {
    pub walk: usize,
    pub read: usize,
    pub total: usize,
}

/// Timing and candidate counts for one hybrid-search pipeline stage.
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

/// Stage-by-stage attribution for one hybrid-search execution.
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

/// Per-repository freshness inputs recorded in search response metadata.
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

/// Freshness basis describing whether a search response is safe to reuse from the runtime cache.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseFreshnessBasisMetadata {
    pub mode: String,
    pub cacheable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<ResponseFreshnessRepositoryMetadata>,
}

/// Semantic accelerator tier and health reported for hybrid search.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridSemanticAcceleratorMetadata {
    pub tier: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ChannelHealthStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Language-specific semantic capabilities consulted during hybrid search.
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

/// Classified query shape used to tune hybrid channel weighting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchHybridQueryShape {
    BroadNaturalLanguage,
    CodeShaped,
    Neutral,
}

/// Exact symbol or text pivot assistance applied during hybrid ranking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridExactPivotAssistance {
    pub applied: bool,
    pub exact_symbol_hit_count: usize,
    pub exact_text_hit_count: usize,
    pub boosted_match_count: usize,
}

/// Structured diagnostics and freshness metadata for `search_hybrid`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridMetadata {
    pub channels: BTreeMap<String, SearchHybridChannelMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_backend: Option<SearchLexicalBackendMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lexical_backend_note: Option<String>,
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
    pub query_shape: Option<SearchHybridQueryShape>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exact_pivot_assistance: Option<SearchHybridExactPivotAssistance>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub witness_demotion_applied: Option<bool>,
    pub diagnostics_count: usize,
    pub diagnostics: SearchHybridDiagnosticsSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage_attribution: Option<SearchHybridStageAttribution>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_capability: Option<SearchHybridLanguageCapabilityMetadata>,
    /// Utility summary for discovery-to-navigation workflows.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utility: Option<SearchHybridUtilitySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_efficiency: Option<ContextEfficiencyMetadata>,
    pub freshness_basis: ResponseFreshnessBasisMetadata,
}

/// Response from `search_hybrid` including channel health and navigation hints.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridResponse {
    pub matches: Vec<SearchHybridMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
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

/// Parameters for `search_symbol`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
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
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// Response from `search_symbol` with optional `result_handle` for `read_match`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolResponse {
    pub matches: Vec<SymbolMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Path-class filter for symbol search over runtime, project, or support files.
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn context_efficiency_metadata_omits_unknown_optional_fields() {
        let value = serde_json::to_value(ContextEfficiencyMetadata {
            indexed_readable_files: 3,
            indexed_readable_bytes: 120,
            indexed_min_mtime_ns: None,
            indexed_max_mtime_ns: None,
            candidate_input_count: None,
            candidate_output_count: None,
            returned_match_count: Some(2),
            returned_unique_paths: Some(1),
            returned_unique_file_bytes: Some(80),
            returned_source_bytes_estimate: Some(20),
            matched_file_context_saved_bytes_estimate: Some(60),
            matched_file_context_saved_percent_estimate: Some(75.0),
            corpus_context_saved_bytes_estimate: Some(100),
            corpus_context_saved_percent_estimate: Some(83.33),
            corpus_narrowing_ratio_estimate: Some(6),
            query_duration_ms: Some(12),
            narrowing_ratio_estimate: Some(6),
            stage_attribution: None,
        })
        .expect("context-efficiency metadata should serialize");

        assert_eq!(
            value,
            json!({
                "indexed_readable_files": 3,
                "indexed_readable_bytes": 120,
                "returned_match_count": 2,
                "returned_unique_paths": 1,
                "returned_unique_file_bytes": 80,
                "returned_source_bytes_estimate": 20,
                "matched_file_context_saved_bytes_estimate": 60,
                "matched_file_context_saved_percent_estimate": 75.0,
                "corpus_context_saved_bytes_estimate": 100,
                "corpus_context_saved_percent_estimate": 83.33,
                "corpus_narrowing_ratio_estimate": 6,
                "query_duration_ms": 12,
                "narrowing_ratio_estimate": 6
            })
        );
    }

    #[test]
    fn context_efficiency_stage_attribution_is_typed() {
        let value = serde_json::to_value(ContextEfficiencyMetadata {
            indexed_readable_files: 1,
            indexed_readable_bytes: 10,
            indexed_min_mtime_ns: Some(100),
            indexed_max_mtime_ns: Some(200),
            candidate_input_count: Some(8),
            candidate_output_count: Some(3),
            returned_match_count: None,
            returned_unique_paths: None,
            returned_unique_file_bytes: None,
            returned_source_bytes_estimate: None,
            matched_file_context_saved_bytes_estimate: None,
            matched_file_context_saved_percent_estimate: None,
            corpus_context_saved_bytes_estimate: None,
            corpus_context_saved_percent_estimate: None,
            corpus_narrowing_ratio_estimate: None,
            query_duration_ms: None,
            narrowing_ratio_estimate: None,
            stage_attribution: Some(ContextEfficiencyStageAttribution {
                candidate_input_count: 8,
                candidate_output_count: 3,
            }),
        })
        .expect("context-efficiency metadata should serialize");

        assert_eq!(
            value["stage_attribution"]["candidate_input_count"],
            json!(8)
        );
        assert_eq!(
            value["stage_attribution"]["candidate_output_count"],
            json!(3)
        );
    }

    #[test]
    fn explore_params_accept_context_efficiency_opt_in() {
        let params: ExploreParams = serde_json::from_value(json!({
            "path": "src/lib.rs",
            "operation": "probe",
            "query": "needle",
            "include_context_efficiency": true
        }))
        .expect("explore params should accept include_context_efficiency");

        assert_eq!(params.include_context_efficiency, Some(true));
    }

    #[test]
    fn explore_metadata_omits_context_efficiency_by_default() {
        let value = serde_json::to_value(ExploreMetadata {
            lossy_utf8: false,
            effective_context_lines: 3,
            effective_max_matches: 8,
            context_efficiency: None,
        })
        .expect("explore metadata should serialize");

        assert!(value.get("context_efficiency").is_none());
    }
}
