//! Search and exploration MCP wire types: text, hybrid, symbol, structural, and `explore` contracts.

use std::collections::BTreeMap;

use super::{
    MetadataObject, ReadPresentationMode, RecoveryFields, ResponseMode, SuggestedNext,
    ZeroHitReason, ZeroHitScope,
};
use crate::domain::{
    ChannelHealthStatus, EvidenceAnchor, PathClass, SourceClass, model::SymbolMatch,
    model::TextMatch,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    /// Canonical repository-relative path.
    pub path: String,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    /// Explorer mode.
    pub operation: ExploreOperation,
    /// Search query for `probe` or `refine`.
    pub query: Option<String>,
    /// Match mode for `query`.
    pub pattern_type: Option<SearchPatternType>,
    /// Anchor used by `zoom` and `refine`.
    pub anchor: Option<ExploreAnchor>,
    /// Context lines around anchors and matches.
    pub context_lines: Option<usize>,
    /// Max match rows to return.
    pub max_matches: Option<usize>,
    /// Continuation cursor for `probe` or `refine`.
    pub resume_from: Option<ExploreCursor>,
    /// Read-surface presentation mode.
    pub presentation_mode: Option<ReadPresentationMode>,
    /// Include context-efficiency metadata; requires JSON presentation.
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
    /// Text query to match. Literal by default.
    #[serde(alias = "pattern")]
    pub query: String,
    /// Match mode for `query`.
    pub pattern_type: Option<SearchPatternType>,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    /// Repository-relative path regex filter.
    pub path_regex: Option<String>,
    /// Max returned matches.
    pub limit: Option<usize>,
    /// Context lines around matches.
    pub context_lines: Option<usize>,
    /// Force case-sensitive matching.
    pub case_sensitive: Option<bool>,
    /// Force case-insensitive matching.
    pub ignore_case: Option<bool>,
    /// Match whole words.
    pub word: Option<bool>,
    /// Return at most one hit row per file.
    pub files_with_matches: Option<bool>,
    /// Return counts and omit match rows.
    pub count_only: Option<bool>,
    /// Repository-relative include glob.
    pub glob: Option<String>,
    /// Repository-relative exclude glob.
    pub exclude_glob: Option<String>,
    /// Include hidden path segments.
    pub include_hidden: Option<bool>,
    /// Max returned hits per file.
    pub max_count_per_file: Option<usize>,
    /// Collapse repeated paths.
    pub collapse_by_file: Option<bool>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
    /// Include context-efficiency metadata.
    pub include_context_efficiency: Option<bool>,
}

/// Response from `search_text` with optional `result_handle` for `read_match`.
///
/// Empty results may include flattened recovery fields (`error_code`,
/// `correction_hint`, `related_tools`, `suggested_next`, optional
/// `zero_hit_reason`, `scope`, `index`) for compact-mode re-planning
/// (`FUT-006` / `FUT-016`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextResponse {
    pub total_matches: usize,
    pub matches: Vec<TextMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    /// Short scope label for `match_id` values (for example `search`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_scope: Option<String>,
    /// Handle lifetime. Session-scoped handles use `"session"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_expires: Option<String>,
    /// Echo of `count_only` when the request asked for counts without match rows (`FUT-009`).
    /// When true, empty `matches[]` is intentional — read `total_matches`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count_only: Option<bool>,
    /// Approximate search latency class for agent tool-cost guidance (`FUT-017` partial).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_class: Option<LatencyClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SearchTextMetadata>,
    /// Shared recovery composer fields; omitted when empty so existing clients stay compatible.
    /// Applied scope echo lives on `recovery.scope` (`ZeroHitScope`) when path filters are set.
    #[serde(flatten, default)]
    pub recovery: RecoveryFields,
}

/// Coarse latency/cost class for compact search responses (`FUT-017`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum LatencyClass {
    Hot,
    Warm,
    Cold,
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

/// Context-efficiency metadata returned when requested.
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
    /// Discovery query.
    pub query: String,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    /// Optional language filter.
    pub language: Option<String>,
    /// Optional max matches.
    pub limit: Option<usize>,
    /// Optional channel-weight overrides.
    pub weights: Option<SearchHybridChannelWeightsParams>,
    /// Optional semantic-channel toggle.
    pub semantic: Option<bool>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
    /// Include context-efficiency metadata.
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
    /// Path-class hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_class: Option<PathClass>,
    /// Source-class hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_class: Option<SourceClass>,
    /// Surface-family hints.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub surface_families: Vec<String>,
    /// Live-navigation hint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub navigation_hint: Option<SearchHybridNavigationHint>,
    /// Strongest rank signals.
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

/// Discovery-to-navigation summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridUtilitySummary {
    /// Count of useful live-navigation pivots.
    pub pivotable_match_count: usize,
    /// One-based rank of the best pivot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_rank: Option<usize>,
    /// Canonical path of the best pivot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_path: Option<String>,
    /// Repository id for the best pivot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_repository_id: Option<String>,
    /// True when symbol follow-up is likely useful.
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

/// Timing and candidate counts for one stage.
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

/// Stage-by-stage execution counters.
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

/// Per-repository freshness metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseFreshnessRepositoryMetadata {
    pub repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    pub manifest: String,
    pub semantic: String,
    pub dirty_root: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cacheable_reason: Option<String>,
    pub candidate_source: String,
    pub using_live_walk: bool,
    pub refresh_in_progress: bool,
    #[serde(default)]
    pub active_index_tasks: Vec<Value>,
    pub recommended_client_behavior: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Runtime cache freshness basis.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResponseFreshnessBasisMetadata {
    pub mode: String,
    pub cacheable: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repositories: Vec<ResponseFreshnessRepositoryMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runtime_cache_contract: Option<Value>,
}

/// Semantic accelerator health.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridSemanticAcceleratorMetadata {
    pub tier: String,
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<ChannelHealthStatus>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Language-specific semantic capabilities.
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

/// Classified query shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchHybridQueryShape {
    BroadNaturalLanguage,
    CodeShaped,
    Neutral,
}

/// Exact symbol or text pivot assistance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridExactPivotAssistance {
    pub applied: bool,
    pub exact_symbol_hit_count: usize,
    pub exact_text_hit_count: usize,
    pub boosted_match_count: usize,
}

/// Diagnostics and optional telemetry for `search_hybrid`.
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
    /// Discovery-to-navigation summary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub utility: Option<SearchHybridUtilitySummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_efficiency: Option<ContextEfficiencyMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_debug: Option<ResponseFreshnessBasisMetadata>,
}

/// Response from `search_hybrid`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridResponse {
    pub matches: Vec<SearchHybridMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    /// Short scope label for `match_id` values (for example `hybrid`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_scope: Option<String>,
    /// Handle lifetime. Session-scoped handles use `"session"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_expires: Option<String>,
    /// Always-on compact note: hybrid is discovery-only, not final proof (`FUT-010`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ranking_note: Option<String>,
    /// Best live-navigation pivot path when available (`FUT-010`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub best_pivot_path: Option<String>,
    /// Approximate search latency class for agent tool-cost guidance (`FUT-017`).
    /// Hybrid is typically `warm` or `cold` (discovery path; allowed slower than exact).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_class: Option<LatencyClass>,
    /// Diagnostics metadata; compact mode omits it unless context-efficiency is requested.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<SearchHybridMetadata>,
    /// Flattened recovery fields (`suggested_next`, zero-hit) for compact re-planning
    /// (`FUT-006` / `FUT-010` / `FUT-016`).
    #[serde(flatten, default)]
    pub recovery: RecoveryFields,
}

/// Parameters for `search_symbol`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolParams {
    /// Symbol name to search.
    pub query: String,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    /// Optional path class filter.
    pub path_class: Option<SearchSymbolPathClass>,
    /// Repository-relative path regex filter.
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
    /// Short scope label for `match_id` values (for example `symbols`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_scope: Option<String>,
    /// Handle lifetime. Session-scoped handles use `"session"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_expires: Option<String>,
    /// Approximate search latency class for agent tool-cost guidance (`FUT-017`).
    /// Known-name symbol lookup is typically `hot` when scoped/runtime-first.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_class: Option<LatencyClass>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Flattened recovery fields on empty symbol results (`FUT-006` / `FUT-016`).
    #[serde(flatten, default)]
    pub recovery: RecoveryFields,
}

/// Path-class filter for symbol search over runtime, project, or support files.
///
/// Default when omitted is [`SearchSymbolPathClass::Runtime`] (runtime-first, tests
/// require opt-in via [`SearchSymbolPathClass::Support`] or [`SearchSymbolPathClass::Any`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchSymbolPathClass {
    Runtime,
    Project,
    Support,
    /// Opt-in: all path classes (runtime, project, and support/tests).
    Any,
}

impl SearchSymbolPathClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Runtime => "runtime",
            Self::Project => "project",
            Self::Support => "support",
            Self::Any => "any",
        }
    }

    /// True when this filter restricts to a single concrete path class.
    pub fn is_concrete_filter(self) -> bool {
        !matches!(self, Self::Any)
    }
}

/// Probe kind for multi-hypothesis `search_batch` (`FUT-008`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchBatchProbeKind {
    Text,
    Symbol,
    Hybrid,
}

impl SearchBatchProbeKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Symbol => "symbol",
            Self::Hybrid => "hybrid",
        }
    }
}

/// Merge strategy for `search_batch` results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum SearchBatchMergeMode {
    /// Prefer probes with more/stronger hits; symbol > text > hybrid tie-break.
    #[default]
    RankByProbeHitStrength,
}

/// One typed probe inside a `search_batch` request.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchBatchProbe {
    /// Stable probe id echoed on matches and probe_summary rows.
    pub id: String,
    /// Which underlying search tool to invoke.
    pub kind: SearchBatchProbeKind,
    /// Query / symbol / hybrid question text.
    pub query: String,
    /// Optional repository scope (overrides batch-level when set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// Optional path regex scope (text/symbol).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_regex: Option<String>,
    /// Optional include glob (text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    /// Optional path class (symbol; also echoed for text when set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path_class: Option<SearchSymbolPathClass>,
    /// Optional pattern type for text probes (default literal).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern_type: Option<SearchPatternType>,
}

/// Parameters for `search_batch` multi-probe search (`FUT-008`).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchBatchParams {
    /// 2..=8 typed probes executed and merged in one call.
    pub probes: Vec<SearchBatchProbe>,
    /// Merge policy. Defaults to `rank_by_probe_hit_strength`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge: Option<SearchBatchMergeMode>,
    /// Max merged match rows to return.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    /// Optional shared repository scope for probes that omit `repository_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_mode: Option<ResponseMode>,
    /// Continuation cursor for paginated batch results (match index).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<usize>,
}

/// One merged match row from `search_batch`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchBatchMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    /// Contributing probe ids (deduped multi-probe hits list all).
    pub probe_ids: Vec<String>,
    /// Primary probe kind that produced this row (first contributor).
    pub kind: SearchBatchProbeKind,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_class: Option<String>,
    /// Relative strength score used for ranking (higher is better).
    pub score: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
}

/// Per-probe summary row for `search_batch`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchBatchProbeSummary {
    pub id: String,
    pub kind: SearchBatchProbeKind,
    pub hits: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_hit_reason: Option<ZeroHitReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_hint: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_next: Vec<SuggestedNext>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ZeroHitScope>,
}

/// Response from `search_batch`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchBatchResponse {
    pub matches: Vec<SearchBatchMatch>,
    pub probe_summary: Vec<SearchBatchProbeSummary>,
    pub returned: usize,
    pub truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_expires: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latency_class: Option<LatencyClass>,
    /// Flattened recovery on all-zero batches and batch-level next steps.
    #[serde(flatten, default)]
    pub recovery: RecoveryFields,
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
            narrowing_ratio_estimate: Some(4),
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
                "narrowing_ratio_estimate": 4
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

    #[test]
    fn response_freshness_basis_round_trips_runtime_metadata() {
        let value = json!({
            "mode": "manifest_only",
            "cacheable": false,
            "repositories": [{
                "repository_id": "repo-001",
                "snapshot_id": "snapshot-abc",
                "manifest": "ready",
                "semantic": "ready",
                "dirty_root": false,
                "cacheable_reason": "refresh in progress",
                "candidate_source": "manifest",
                "using_live_walk": false,
                "refresh_in_progress": true,
                "active_index_tasks": [{
                    "kind": "manifest",
                    "status": "running"
                }],
                "recommended_client_behavior": "prefer_current_response",
                "provider": "openai",
                "model": "text-embedding-3-small"
            }],
            "runtime_cache_contract": {
                "cacheable": false,
                "invalidation_basis": "snapshot"
            }
        });

        let metadata: ResponseFreshnessBasisMetadata =
            serde_json::from_value(value.clone()).expect("freshness metadata should deserialize");
        let serialized =
            serde_json::to_value(metadata).expect("freshness metadata should serialize");

        assert_eq!(serialized, value);
    }

    #[test]
    fn response_freshness_repository_keeps_empty_active_index_tasks() {
        let value = json!({
            "repository_id": "repo-001",
            "snapshot_id": "snapshot-abc",
            "manifest": "ready",
            "semantic": "ready",
            "dirty_root": false,
            "candidate_source": "manifest_snapshot",
            "using_live_walk": false,
            "refresh_in_progress": false,
            "active_index_tasks": [],
            "recommended_client_behavior": "use_cached_frigg_results"
        });

        let metadata: ResponseFreshnessRepositoryMetadata = serde_json::from_value(value.clone())
            .expect("repository freshness metadata should deserialize");
        let serialized =
            serde_json::to_value(metadata).expect("repository freshness metadata should serialize");

        assert_eq!(serialized, value);
    }
}
