//! Navigation MCP wire types: references, definitions, declarations, implementations, and call hierarchy.

use super::{MetadataObject, ResponseMode, ResultCompleteness, TargetRef};
use crate::domain::model::{GeneratedStructuralFollowUp, ReferenceMatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `find_references`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesParams {
    pub target: Option<TargetRef>,
    /// Symbol query.
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    /// Source path for location-aware resolution.
    pub path: Option<String>,
    /// 1-based source line.
    pub line: Option<usize>,
    /// 1-based source column.
    pub column: Option<usize>,
    /// Include definition rows.
    pub include_definition: Option<bool>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Opaque v2 continuation returned by a prior identical request.
    pub continuation: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// Precision mode reported by navigation tools for the resolved target and match set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigationMode {
    Precise,
    PrecisePartial,
    HeuristicNoPrecise,
    UnavailableNoPrecise,
}

/// Whether navigation target resolution produced one symbol or requires disambiguation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigationTargetSelectionStatus {
    Resolved,
    DisambiguationRequired,
}

/// Target-resolution summary shared by navigation tool responses.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationTargetSelectionSummary {
    pub status: NavigationTargetSelectionStatus,
    pub symbol_query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_stable_symbol_id: Option<String>,
    pub candidate_count: usize,
    pub same_rank_candidate_count: usize,
    pub ambiguous_query: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidates: Vec<crate::domain::model::SymbolMatch>,
}

/// Response from `find_references` with navigation mode and optional target-selection notes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesResponse {
    pub total_matches: usize,
    pub matches: Vec<ReferenceMatch>,
    /// Canonical cardinality, coverage, and paging truth for reference rows.
    pub completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    /// Short scope label for `match_id` values (for example `nav`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_scope: Option<String>,
    /// Handle lifetime. Session-scoped handles use `"session"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_expires: Option<String>,
    pub mode: NavigationMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_selection: Option<NavigationTargetSelectionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Flattened recovery fields on empty reference results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Parameters for `go_to_definition` (default agent route for definition/body anchors).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionParams {
    pub target: Option<TargetRef>,
    /// Recommended: symbol name to resolve. Prefer this over path+line alone on dense lines.
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Opaque v2 continuation returned by a prior identical request.
    pub continuation: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// One resolved navigation location with optional structural follow-up suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<TargetRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_symbol_id: Option<String>,
    pub symbol: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub precision: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

/// Response from `go_to_definition`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionResponse {
    pub matches: Vec<NavigationLocation>,
    /// Canonical cardinality, coverage, and paging truth for definition rows.
    pub completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    /// Short scope label for `match_id` values (for example `nav`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_scope: Option<String>,
    /// Handle lifetime. Session-scoped handles use `"session"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handle_expires: Option<String>,
    pub mode: NavigationMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_selection: Option<NavigationTargetSelectionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Soft warning when path+line was used without `symbol` (generic or density-specific copy).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_warning: Option<String>,
    /// True when path+line without symbol may be wrong. Check this or location_warning before edits; prefer symbol= from search_symbol.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ambiguous_location: Option<bool>,
    /// Flattened recovery fields on empty definition results (and soft re-plan on ambiguous hits).
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Parameters for `find_declarations` (secondary to go_to_definition; use when decl vs def matters).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsParams {
    pub target: Option<TargetRef>,
    /// Preferred when the name is known (same as go_to_definition).
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Opaque v2 continuation returned by a prior identical request.
    pub continuation: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// Response from `find_declarations`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsResponse {
    pub matches: Vec<NavigationLocation>,
    /// Canonical cardinality, coverage, and paging truth for declaration rows.
    pub completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    pub mode: NavigationMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_selection: Option<NavigationTargetSelectionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Flattened recovery fields on empty declaration results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Parameters for `find_implementations`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsParams {
    pub target: Option<TargetRef>,
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Opaque v2 continuation returned by a prior identical request.
    pub continuation: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// One implementation or override location for a resolved symbol target.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImplementationMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<TargetRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_symbol_id: Option<String>,
    pub symbol: String,
    pub kind: Option<String>,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub relation: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    pub precision: Option<String>,
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

/// Response from `find_implementations`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsResponse {
    pub matches: Vec<ImplementationMatch>,
    /// Canonical cardinality, coverage, and paging truth for implementation rows.
    pub completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    pub mode: NavigationMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_selection: Option<NavigationTargetSelectionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Flattened recovery fields on empty implementation results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Parameters for `incoming_calls`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsParams {
    pub target: Option<TargetRef>,
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Opaque v2 continuation returned by a prior identical request.
    pub continuation: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// Parameters for `outgoing_calls`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsParams {
    pub target: Option<TargetRef>,
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Opaque v2 continuation returned by a prior identical request.
    pub continuation: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// One incoming or outgoing call edge in the call hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallHierarchyMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_ref: Option<TargetRef>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_stable_symbol_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_stable_symbol_id: Option<String>,
    pub source_symbol: String,
    pub target_symbol: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub relation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_signature: Option<String>,
    pub precision: Option<String>,
    pub call_path: Option<String>,
    pub call_line: Option<usize>,
    pub call_column: Option<usize>,
    pub call_end_line: Option<usize>,
    pub call_end_column: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

/// Response from `incoming_calls`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    /// Canonical cardinality, coverage, and paging truth for incoming call rows.
    pub completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    pub mode: NavigationMode,
    pub availability: Option<NavigationAvailability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_selection: Option<NavigationTargetSelectionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Flattened recovery fields on empty caller results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Trust tier for nav edges. Outgoing_calls is always provisional today; do not invent verified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigationEdgeTrust {
    Provisional,
    Verified,
}

/// Always-on compact honesty copy for `outgoing_calls` (EXP-nav-outgoing-honesty B).
pub const OUTGOING_CALLS_TRUST_NOTE: &str = "Callee edges are provisional; confirm with read_file, find_references, or search_structural before asserting blast radius.";

/// Response from `outgoing_calls`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    /// Canonical cardinality, coverage, and paging truth for outgoing call rows.
    pub completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    pub mode: NavigationMode,
    pub availability: Option<NavigationAvailability>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_selection: Option<NavigationTargetSelectionSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    /// Full-mode diagnostic note only (stripped in compact). Prefer `trust` / `trust_note`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Machine-obvious trust tier. Outgoing callees are always `provisional` today.
    pub trust: NavigationEdgeTrust,
    /// Always-on compact honesty (not stripped in compact mode; skill-less hosts still see it).
    pub trust_note: String,
    /// Flattened recovery fields on empty callee results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

impl OutgoingCallsResponse {
    /// Default provisional honesty applied to every outgoing_calls response path.
    pub fn with_provisional_honesty(mut self) -> Self {
        self.trust = NavigationEdgeTrust::Provisional;
        self.trust_note = OUTGOING_CALLS_TRUST_NOTE.to_owned();
        self
    }
}

/// Availability note when call-hierarchy results depend on precise coverage.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationAvailability {
    pub status: String,
    pub reason: Option<String>,
    pub precise_required_for_complete_results: bool,
}

/// Parameters for `document_symbols`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsParams {
    pub target: Option<TargetRef>,
    pub path: String,
    pub repository_id: Option<String>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    /// Return only top-level symbols when true. Defaults to `true` when omitted.
    pub top_level_only: Option<bool>,
    /// Max outline rows to return. Omit for the default bounded page.
    pub limit: Option<usize>,
    /// Continuation offset returned as `resume_from` when the outline is truncated.
    pub resume_from: Option<usize>,
    /// Opaque v2 continuation. Cannot be combined with `resume_from`.
    pub continuation: Option<String>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// One document symbol row with optional nested children.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_symbol_id: Option<String>,
    pub symbol: String,
    pub kind: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
    pub children: Vec<DocumentSymbolItem>,
}

/// Response from `document_symbols`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsResponse {
    pub symbols: Vec<DocumentSymbolItem>,
    /// Total outline symbols before pagination (after top_level_only filtering).
    pub total_symbols: usize,
    /// Number of symbols returned in this page.
    pub returned: usize,
    /// True when more outline rows remain after this page.
    pub truncated: bool,
    /// Continuation offset for the next page when `truncated` is true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_from: Option<usize>,
    /// Canonical cardinality and paging truth for outline rows.
    pub completeness: super::ResultCompleteness,
    /// Echo of the effective top_level_only setting.
    pub top_level_only: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Parameters for `inspect_syntax_tree`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectSyntaxTreeParams {
    pub path: String,
    pub repository_id: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub max_ancestors: Option<usize>,
    pub max_children: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
}

/// One syntax-tree node with source span and excerpt.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SyntaxTreeNodeItem {
    pub kind: String,
    pub named: bool,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
}

/// Response from `inspect_syntax_tree` around a focused AST node.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectSyntaxTreeResponse {
    pub repository_id: String,
    pub path: String,
    pub language: String,
    pub focus: SyntaxTreeNodeItem,
    pub ancestors: Vec<SyntaxTreeNodeItem>,
    pub children: Vec<SyntaxTreeNodeItem>,
    /// Independent completeness for the bounded ancestor collection.
    pub ancestors_completeness: super::ResultCompleteness,
    /// Independent completeness for the bounded child collection.
    pub children_completeness: super::ResultCompleteness,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Parameters for optional `impact_bundle` convenience composition.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ImpactBundleParams {
    pub target: Option<TargetRef>,
    /// Symbol name to resolve impact for (required).
    pub symbol: String,
    /// Path class for the initial symbol lookup. Defaults to `runtime`.
    pub path_class: Option<crate::mcp::types::SearchSymbolPathClass>,
    /// Optional repository scope.
    pub repository_id: Option<String>,
    /// Force include implementations even when kind is not trait/interface.
    pub include_implementations: Option<bool>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// Section role for a path tally row in `ImpactBundleSummary.top_paths`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ImpactBundlePathRole {
    /// Selected/primary symbol hit (first search_symbol match used for nav composition).
    Symbol,
    Reference,
    IncomingCall,
    Implementation,
}

impl ImpactBundlePathRole {
    /// Tie-break priority under equal counts (lower sorts earlier after count desc).
    fn plan_priority(self) -> u8 {
        match self {
            Self::Symbol => 0,
            Self::Reference => 1,
            Self::IncomingCall => 2,
            Self::Implementation => 3,
        }
    }
}

/// Path tally row for compact impact planning (EXP-nav-impact-shape B).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImpactBundlePathTally {
    pub path: String,
    pub role: ImpactBundlePathRole,
    pub count: usize,
}

/// Always-on cardinality summary for `impact_bundle`. Plan from summary first; lists/handles for proof. Counts mirror returned lists; composition uses the first symbol hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ImpactBundleSummary {
    pub symbols_count: usize,
    pub references_count: usize,
    pub incoming_calls_count: usize,
    pub implementations_count: usize,
    pub implementations_included: bool,
    pub references_mode: NavigationMode,
    pub incoming_calls_mode: NavigationMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations_mode: Option<NavigationMode>,
    /// Highest-count path×role tallies (global cap 8); empty when no hits.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_paths: Vec<ImpactBundlePathTally>,
    /// True when more path×role tallies existed than the top_paths cap.
    pub top_paths_truncated: bool,
}

/// Composed impact: hits + refs + callers (+ optional impls). One next-step channel via flattened suggested_next only.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImpactBundleResponse {
    pub symbol: String,
    pub path_class: String,
    /// Always-on plan-friendly counts / modes / top paths (EXP-nav-impact-shape B).
    pub summary: ImpactBundleSummary,
    pub symbols: Vec<crate::domain::model::SymbolMatch>,
    /// Cardinality and coverage for the symbol lookup section.
    pub symbols_completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols_result_handle: Option<String>,
    pub references: Vec<crate::domain::model::ReferenceMatch>,
    /// Cardinality and coverage for the references section.
    pub references_completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references_result_handle: Option<String>,
    pub references_mode: NavigationMode,
    pub incoming_calls: Vec<CallHierarchyMatch>,
    /// Cardinality and coverage for the incoming-call section.
    pub incoming_calls_completeness: ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_calls_result_handle: Option<String>,
    pub incoming_calls_mode: NavigationMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implementations: Vec<ImplementationMatch>,
    /// Cardinality and coverage for implementations when that section was explicitly included.
    /// `None` is a policy omission, not hidden truncation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations_completeness: Option<ResultCompleteness>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations_result_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations_mode: Option<NavigationMode>,
    pub implementations_included: bool,
    /// Aggregate truth across every included section. It never upgrades a child section's
    /// coverage or truncation state.
    pub completeness: ResultCompleteness,
    /// Flattened recovery + **only** `suggested_next` channel (success and zero-hit paths).
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

impl ImpactBundleResponse {
    /// Build the always-on summary from current list sections (global top_paths cap 8).
    pub fn compute_summary(
        symbols: &[crate::domain::model::SymbolMatch],
        references: &[crate::domain::model::ReferenceMatch],
        incoming_calls: &[CallHierarchyMatch],
        implementations: &[ImplementationMatch],
        references_mode: NavigationMode,
        incoming_calls_mode: NavigationMode,
        implementations_mode: Option<NavigationMode>,
        implementations_included: bool,
    ) -> ImpactBundleSummary {
        const TOP_PATHS_CAP: usize = 8;
        use std::collections::BTreeMap;

        let mut tallies: BTreeMap<(String, ImpactBundlePathRole), usize> = BTreeMap::new();
        if let Some(m) = symbols.first() {
            *tallies
                .entry((m.path.clone(), ImpactBundlePathRole::Symbol))
                .or_default() += 1;
        }
        for m in references {
            *tallies
                .entry((m.path.clone(), ImpactBundlePathRole::Reference))
                .or_default() += 1;
        }
        for m in incoming_calls {
            *tallies
                .entry((m.path.clone(), ImpactBundlePathRole::IncomingCall))
                .or_default() += 1;
        }
        for m in implementations {
            *tallies
                .entry((m.path.clone(), ImpactBundlePathRole::Implementation))
                .or_default() += 1;
        }

        let total_tallies = tallies.len();
        let mut top_paths: Vec<ImpactBundlePathTally> = tallies
            .into_iter()
            .map(|((path, role), count)| ImpactBundlePathTally { path, role, count })
            .collect();
        top_paths.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.role.plan_priority().cmp(&b.role.plan_priority()))
                .then_with(|| a.path.cmp(&b.path))
        });
        let top_paths_truncated = total_tallies > TOP_PATHS_CAP;
        top_paths.truncate(TOP_PATHS_CAP);

        ImpactBundleSummary {
            symbols_count: symbols.len(),
            references_count: references.len(),
            incoming_calls_count: incoming_calls.len(),
            implementations_count: implementations.len(),
            implementations_included,
            references_mode,
            incoming_calls_mode,
            implementations_mode,
            top_paths,
            top_paths_truncated,
        }
    }

    /// Recompute `summary` from the response's current lists/modes.
    pub fn with_computed_summary(mut self) -> Self {
        self.summary = Self::compute_summary(
            &self.symbols,
            &self.references,
            &self.incoming_calls,
            &self.implementations,
            self.references_mode,
            self.incoming_calls_mode,
            self.implementations_mode,
            self.implementations_included,
        );
        self
    }

    /// Preserve section truth in the bundle-level envelope. The aggregate's unit is the number
    /// of included sections rather than a misleading sum of heterogeneous row units.
    pub fn aggregate_completeness(sections: &[&ResultCompleteness]) -> ResultCompleteness {
        let returned = sections.len();
        let truncated = sections.iter().any(|section| section.truncated);
        let complete = sections.iter().all(|section| section.complete);
        let mut truncation_reasons = Vec::new();
        let mut incomplete_reasons = Vec::new();
        for section in sections {
            if section.truncated {
                truncation_reasons.push(super::ResultTruncationReason::ChildLimit);
            }
            if !section.complete {
                incomplete_reasons.push(super::ResultIncompleteReason::ChildIncomplete);
                incomplete_reasons.extend(section.incomplete_reasons.iter().copied());
            }
        }
        ResultCompleteness::try_new(
            super::ResultUnit::ImpactSection,
            returned,
            Some(returned),
            complete,
            truncated,
            truncation_reasons,
            incomplete_reasons,
            None,
        )
        .expect("impact aggregate completeness must preserve valid child state")
    }
}

/// Optional evidence-packet claim shape for review/security (not a live MCP tool response).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvidencePacketClaim {
    pub claim: String,
    pub tool: String,
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
}

/// Optional multi-claim evidence packet envelope for review/security.
///
/// Mirrors the skill-documented JSON shape; not a live MCP tool response.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct EvidencePacket {
    pub claims: Vec<EvidencePacketClaim>,
}

/// Whether `search_structural` returns grouped match rows or raw capture rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructuralResultMode {
    Matches,
    Captures,
}

/// Capture-selection policy used to derive a structural match anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum StructuralAnchorSelection {
    PrimaryCapture,
    MatchCapture,
    FirstUsefulNamedCapture,
    FirstCapture,
    CaptureRow,
}

/// Parameters for `search_structural`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralParams {
    pub query: String,
    pub language: Option<String>,
    pub repository_id: Option<String>,
    pub path_regex: Option<String>,
    pub limit: Option<usize>,
    /// Grouped or raw result shape.
    pub result_mode: Option<StructuralResultMode>,
    /// Anchor capture name for grouped results.
    pub primary_capture: Option<String>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    /// Opaque v2 continuation. Structural results are replayed deterministically after lookup.
    pub continuation: Option<String>,
}

/// One named capture from a structural query match.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StructuralCaptureItem {
    pub name: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
}

/// One structural query match with anchor capture and follow-up suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StructuralMatch {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
    pub anchor_capture_name: Option<String>,
    pub anchor_selection: StructuralAnchorSelection,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub captures: Vec<StructuralCaptureItem>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

/// Response from `search_structural`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralResponse {
    pub matches: Vec<StructuralMatch>,
    pub result_mode: StructuralResultMode,
    /// Canonical cardinality and paging truth for the selected structural row unit.
    pub completeness: super::ResultCompleteness,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::{
        EvidencePacket, EvidencePacketClaim, ImpactBundlePathRole, ImpactBundleResponse,
        ImpactBundleSummary, NavigationMode,
    };
    use crate::mcp::types::{
        RecoveryFields, ResultCompleteness, ResultIncompleteReason, ResultTruncationReason,
        ResultUnit, SuggestedNext,
    };

    fn empty_impact_summary(
        references_mode: NavigationMode,
        incoming_calls_mode: NavigationMode,
    ) -> ImpactBundleSummary {
        ImpactBundleResponse::compute_summary(
            &[],
            &[],
            &[],
            &[],
            references_mode,
            incoming_calls_mode,
            None,
            false,
        )
    }

    #[test]
    fn with_provisional_honesty_forces_provisional_on_wire() {
        use super::{
            NavigationEdgeTrust, NavigationMode, OUTGOING_CALLS_TRUST_NOTE, OutgoingCallsResponse,
        };
        use crate::mcp::types::{RecoveryFields, ResultCompleteness, ResultUnit};

        let response = OutgoingCallsResponse {
            completeness: ResultCompleteness::complete(ResultUnit::OutgoingCall, 0, 0)
                .expect("empty fixture is complete"),
            matches: Vec::new(),
            result_handle: None,
            mode: NavigationMode::HeuristicNoPrecise,
            availability: None,
            target_selection: None,
            metadata: None,
            note: Some("full diagnostic only".to_owned()),
            trust: NavigationEdgeTrust::Verified,
            trust_note: String::new(),
            recovery: RecoveryFields::default(),
        }
        .with_provisional_honesty();
        assert_eq!(response.trust, NavigationEdgeTrust::Provisional);
        assert_eq!(response.trust_note, OUTGOING_CALLS_TRUST_NOTE);

        let value = serde_json::to_value(&response).expect("serialize");
        assert_eq!(value["trust"], "provisional");
        assert_eq!(value["trust_note"], OUTGOING_CALLS_TRUST_NOTE);
        assert_eq!(value["note"], "full diagnostic only");
    }

    #[test]
    fn go_to_definition_ambiguous_location_serializes_when_true() {
        use super::GoToDefinitionResponse;
        use crate::mcp::types::{NavigationMode, RecoveryFields, ResultCompleteness, ResultUnit};
        use serde_json::json;

        let response = GoToDefinitionResponse {
            completeness: ResultCompleteness::try_new(
                ResultUnit::Definition,
                0,
                Some(0),
                false,
                false,
                vec![],
                vec![crate::mcp::types::ResultIncompleteReason::NavigationHeuristicCoverage],
                None,
            )
            .expect("heuristic fixture is internally consistent"),
            matches: Vec::new(),
            result_handle: None,
            handle_scope: None,
            handle_expires: None,
            mode: NavigationMode::HeuristicNoPrecise,
            target_selection: None,
            metadata: None,
            note: None,
            location_warning: Some("dense line".to_owned()),
            ambiguous_location: Some(true),
            recovery: RecoveryFields {
                correction_hint: Some("retry with symbol".to_owned()),
                ..RecoveryFields::default()
            },
        };
        let value = serde_json::to_value(&response).expect("serialize");
        assert_eq!(value["ambiguous_location"], json!(true));
        assert_eq!(value["location_warning"], "dense line");
        assert_eq!(value["correction_hint"], "retry with symbol");

        let quiet = GoToDefinitionResponse {
            completeness: ResultCompleteness::complete(ResultUnit::Definition, 0, 0)
                .expect("empty precise fixture is complete"),
            matches: Vec::new(),
            result_handle: None,
            handle_scope: None,
            handle_expires: None,
            mode: NavigationMode::Precise,
            target_selection: None,
            metadata: None,
            note: None,
            location_warning: None,
            ambiguous_location: None,
            recovery: RecoveryFields::default(),
        };
        let quiet_value = serde_json::to_value(&quiet).expect("serialize quiet");
        assert!(quiet_value.get("ambiguous_location").is_none());
        assert!(quiet_value.get("location_warning").is_none());
    }

    #[test]
    fn impact_bundle_response_single_suggested_next_channel() {
        let success = ImpactBundleResponse {
            symbol: "catalog_entries".to_owned(),
            path_class: "runtime".to_owned(),
            summary: empty_impact_summary(NavigationMode::Precise, NavigationMode::Precise),
            symbols: Vec::new(),
            symbols_completeness: ResultCompleteness::complete(ResultUnit::Symbol, 0, 0)
                .expect("empty symbols fixture is complete"),
            symbols_result_handle: Some("symbols:h1".to_owned()),
            references: Vec::new(),
            references_completeness: ResultCompleteness::complete(ResultUnit::Reference, 0, 0)
                .expect("empty references fixture is complete"),
            references_result_handle: Some("refs:h1".to_owned()),
            references_mode: NavigationMode::Precise,
            incoming_calls: Vec::new(),
            incoming_calls_completeness: ResultCompleteness::complete(
                ResultUnit::IncomingCall,
                0,
                0,
            )
            .expect("empty incoming fixture is complete"),
            incoming_calls_result_handle: None,
            incoming_calls_mode: NavigationMode::Precise,
            implementations: Vec::new(),
            implementations_completeness: None,
            implementations_result_handle: None,
            implementations_mode: None,
            implementations_included: false,
            completeness: ResultCompleteness::complete(ResultUnit::ImpactSection, 3, 3)
                .expect("all included fixture sections are complete"),
            recovery: RecoveryFields {
                suggested_next: vec![
                    SuggestedNext::tool("read_match").with_reason("proof clusters"),
                    SuggestedNext::tool("search_text")
                        .with_query("catalog_entries")
                        .with_path_regex("^tests/")
                        .with_reason("tests pass"),
                ],
                ..RecoveryFields::default()
            },
        };
        let value = serde_json::to_value(&success).expect("serialize success impact");
        let next = value["suggested_next"]
            .as_array()
            .expect("flattened recovery must expose suggested_next");
        assert_eq!(next.len(), 2);
        assert_eq!(
            next[0]["tool"], "read_match",
            "success next steps serialize from recovery.suggested_next"
        );
        assert_eq!(value["symbol"], "catalog_entries");
        assert_eq!(value["symbols_result_handle"], "symbols:h1");
        assert_eq!(value["references_result_handle"], "refs:h1");
        assert!(value.get("summary").is_some());
        assert_eq!(value["summary"]["references_count"], 0);
        assert!(value.get("error_code").is_none());
        assert!(value.get("recovery").is_none());

        let zero = ImpactBundleResponse {
            symbol: "missing_sym".to_owned(),
            path_class: "runtime".to_owned(),
            summary: empty_impact_summary(
                NavigationMode::UnavailableNoPrecise,
                NavigationMode::UnavailableNoPrecise,
            ),
            symbols: Vec::new(),
            symbols_completeness: ResultCompleteness::try_new(
                ResultUnit::Symbol,
                0,
                None,
                false,
                false,
                vec![],
                vec![ResultIncompleteReason::NavigationUnavailable],
                None,
            )
            .expect("unavailable symbols fixture is valid"),
            symbols_result_handle: None,
            references: Vec::new(),
            references_completeness: ResultCompleteness::try_new(
                ResultUnit::Reference,
                0,
                None,
                false,
                false,
                vec![],
                vec![ResultIncompleteReason::NavigationUnavailable],
                None,
            )
            .expect("unavailable references fixture is valid"),
            references_result_handle: None,
            references_mode: NavigationMode::UnavailableNoPrecise,
            incoming_calls: Vec::new(),
            incoming_calls_completeness: ResultCompleteness::try_new(
                ResultUnit::IncomingCall,
                0,
                None,
                false,
                false,
                vec![],
                vec![ResultIncompleteReason::NavigationUnavailable],
                None,
            )
            .expect("unavailable incoming fixture is valid"),
            incoming_calls_result_handle: None,
            incoming_calls_mode: NavigationMode::UnavailableNoPrecise,
            implementations: Vec::new(),
            implementations_completeness: None,
            implementations_result_handle: None,
            implementations_mode: None,
            implementations_included: false,
            completeness: ResultCompleteness::try_new(
                ResultUnit::ImpactSection,
                3,
                Some(3),
                false,
                false,
                vec![],
                vec![ResultIncompleteReason::ChildIncomplete],
                None,
            )
            .expect("incomplete aggregate fixture is valid"),
            recovery: RecoveryFields {
                error_code: Some("ZERO_HIT".to_owned()),
                message: Some("no symbol hits".to_owned()),
                suggested_next: vec![
                    SuggestedNext::tool("search_symbol")
                        .with_symbol("missing_sym")
                        .with_reason("retry"),
                ],
                ..RecoveryFields::default()
            },
        };
        let zero_value = serde_json::to_value(&zero).expect("serialize zero impact");
        assert_eq!(
            zero_value["suggested_next"]
                .as_array()
                .map(|a| a.len())
                .unwrap_or(0),
            1
        );
        assert_eq!(zero_value["error_code"], "ZERO_HIT");
        assert!(zero_value.get("recovery").is_none());
        let back: ImpactBundleResponse =
            serde_json::from_value(zero_value).expect("deserialize impact");
        assert_eq!(back.recovery.suggested_next.len(), 1);
        assert_eq!(back.recovery.error_code.as_deref(), Some("ZERO_HIT"));

        assert!(value.get("recovery").is_none());
    }

    #[test]
    fn impact_bundle_aggregate_completeness_preserves_child_truncation() {
        let complete = ResultCompleteness::complete(ResultUnit::Symbol, 1, 1)
            .expect("complete symbol section");
        let capped = ResultCompleteness::try_new(
            ResultUnit::Reference,
            10,
            Some(11),
            false,
            true,
            vec![ResultTruncationReason::PageLimit],
            vec![],
            Some("continuation-test".to_owned()),
        )
        .expect("capped reference section");
        let aggregate = ImpactBundleResponse::aggregate_completeness(&[&complete, &capped]);
        assert_eq!(aggregate.unit, ResultUnit::ImpactSection);
        assert_eq!(aggregate.returned, 2);
        assert_eq!(aggregate.total, Some(2));
        assert!(!aggregate.complete);
        assert!(aggregate.truncated);
        assert!(
            aggregate
                .truncation_reasons
                .contains(&ResultTruncationReason::ChildLimit)
        );
        assert!(
            aggregate
                .incomplete_reasons
                .contains(&ResultIncompleteReason::ChildIncomplete)
        );
        assert!(aggregate.continuation.is_none());
    }

    #[test]
    fn impact_bundle_summary_counts_and_top_paths() {
        use super::{CallHierarchyMatch, ImplementationMatch};
        use crate::domain::model::{ReferenceMatch, ReferenceMatchKind, SymbolMatch};

        let symbols = vec![SymbolMatch {
            match_id: None,
            target_ref: None,
            stable_symbol_id: None,
            repository_id: "r1".to_owned(),
            symbol: "target".to_owned(),
            kind: "function".to_owned(),
            path: "src/lib.rs".to_owned(),
            line: 1,
            column: Some(1),
            excerpt: None,
            path_class: Some("runtime".to_owned()),
            container: None,
            signature: None,
        }];
        let references = vec![
            ReferenceMatch {
                match_id: None,
                target_ref: None,
                stable_symbol_id: None,
                repository_id: "r1".to_owned(),
                symbol: "target".to_owned(),
                path: "src/a.rs".to_owned(),
                line: 2,
                column: 1,
                match_kind: ReferenceMatchKind::Reference,
                precision: None,
                fallback_reason: None,
                container: None,
                signature: None,
                follow_up_structural: Vec::new(),
            },
            ReferenceMatch {
                match_id: None,
                target_ref: None,
                stable_symbol_id: None,
                repository_id: "r1".to_owned(),
                symbol: "target".to_owned(),
                path: "src/a.rs".to_owned(),
                line: 5,
                column: 1,
                match_kind: ReferenceMatchKind::Reference,
                precision: None,
                fallback_reason: None,
                container: None,
                signature: None,
                follow_up_structural: Vec::new(),
            },
            ReferenceMatch {
                match_id: None,
                target_ref: None,
                stable_symbol_id: None,
                repository_id: "r1".to_owned(),
                symbol: "target".to_owned(),
                path: "src/b.rs".to_owned(),
                line: 3,
                column: 1,
                match_kind: ReferenceMatchKind::Reference,
                precision: None,
                fallback_reason: None,
                container: None,
                signature: None,
                follow_up_structural: Vec::new(),
            },
        ];
        let incoming = vec![CallHierarchyMatch {
            match_id: None,
            target_ref: None,
            source_stable_symbol_id: None,
            target_stable_symbol_id: None,
            source_symbol: "caller".to_owned(),
            target_symbol: "target".to_owned(),
            repository_id: "r1".to_owned(),
            path: "src/a.rs".to_owned(),
            line: 10,
            column: 1,
            relation: "calls".to_owned(),
            source_container: None,
            target_container: None,
            source_signature: None,
            target_signature: None,
            precision: None,
            call_path: None,
            call_line: None,
            call_column: None,
            call_end_line: None,
            call_end_column: None,
            follow_up_structural: Vec::new(),
        }];
        let implementations: Vec<ImplementationMatch> = Vec::new();

        let summary = ImpactBundleResponse::compute_summary(
            &symbols,
            &references,
            &incoming,
            &implementations,
            NavigationMode::Precise,
            NavigationMode::HeuristicNoPrecise,
            None,
            false,
        );
        assert_eq!(summary.symbols_count, 1);
        assert_eq!(summary.references_count, 3);
        assert_eq!(summary.incoming_calls_count, 1);
        assert_eq!(summary.implementations_count, 0);
        assert!(!summary.implementations_included);
        assert_eq!(summary.references_mode, NavigationMode::Precise);
        assert_eq!(
            summary.incoming_calls_mode,
            NavigationMode::HeuristicNoPrecise
        );
        assert!(
            summary.top_paths.iter().any(|p| {
                p.path == "src/a.rs" && p.role == ImpactBundlePathRole::Reference && p.count == 2
            }),
            "top_paths should tally reference paths: {:?}",
            summary.top_paths
        );
        assert!(
            summary.top_paths.first().is_some_and(|p| p.count >= 2),
            "highest tally should lead top_paths: {:?}",
            summary.top_paths
        );
        assert!(
            summary
                .top_paths
                .iter()
                .any(|p| p.role == ImpactBundlePathRole::Symbol && p.path == "src/lib.rs"),
            "selected symbol path should stay visible in top_paths: {:?}",
            summary.top_paths
        );
        assert!(!summary.top_paths_truncated);

        let value = serde_json::to_value(&summary).expect("serialize summary");
        assert_eq!(value["references_count"], 3);
        assert_eq!(value["references_mode"], "precise");
        assert_eq!(value["top_paths_truncated"], false);

        let many_refs: Vec<ReferenceMatch> = (0..10)
            .map(|i| ReferenceMatch {
                match_id: None,
                target_ref: None,
                stable_symbol_id: None,
                repository_id: "r1".to_owned(),
                symbol: "target".to_owned(),
                path: format!("src/p{i}.rs"),
                line: 1,
                column: 1,
                match_kind: ReferenceMatchKind::Reference,
                precision: None,
                fallback_reason: None,
                container: None,
                signature: None,
                follow_up_structural: Vec::new(),
            })
            .collect();
        let capped = ImpactBundleResponse::compute_summary(
            &symbols,
            &many_refs,
            &[],
            &[],
            NavigationMode::Precise,
            NavigationMode::Precise,
            None,
            false,
        );
        assert_eq!(capped.top_paths.len(), 8);
        assert!(capped.top_paths_truncated);
        assert!(
            capped
                .top_paths
                .iter()
                .any(|p| p.role == ImpactBundlePathRole::Symbol),
            "symbol anchor should survive cap under ties: {:?}",
            capped.top_paths
        );
    }

    #[test]
    fn evidence_packet_claim_serde_round_trip() {
        let claim = EvidencePacketClaim {
            claim: "catalog_entries registers callable operations".to_owned(),
            tool: "search_symbol".to_owned(),
            path: "src/catalog/mod.rs".to_owned(),
            start_line: 40,
            end_line: 72,
            match_id: Some("symbols:m1".to_owned()),
            result_handle: Some("result-000001".to_owned()),
        };

        let json = serde_json::to_string(&claim).expect("claim should serialize");
        let back: EvidencePacketClaim =
            serde_json::from_str(&json).expect("claim should deserialize");
        assert_eq!(back.claim, claim.claim);
        assert_eq!(back.tool, "search_symbol");
        assert_eq!(back.path, "src/catalog/mod.rs");
        assert_eq!(back.start_line, 40);
        assert_eq!(back.end_line, 72);
        assert_eq!(back.match_id.as_deref(), Some("symbols:m1"));
        assert_eq!(back.result_handle.as_deref(), Some("result-000001"));
    }

    #[test]
    fn evidence_packet_claim_omits_optional_handle_fields_when_none() {
        let claim = EvidencePacketClaim {
            claim: "path/line witness only".to_owned(),
            tool: "read_file".to_owned(),
            path: "src/lib.rs".to_owned(),
            start_line: 1,
            end_line: 3,
            match_id: None,
            result_handle: None,
        };
        let value = serde_json::to_value(&claim).expect("serialize");
        assert!(value.get("match_id").is_none());
        assert!(value.get("result_handle").is_none());
        assert_eq!(value["path"], "src/lib.rs");
        assert_eq!(value["tool"], "read_file");
    }

    #[test]
    fn evidence_packet_skill_shaped_multi_claim_json_deserializes() {
        let skill_json = r#"{
          "claims": [
            {
              "claim": "catalog_entries registers callable operations",
              "tool": "search_symbol",
              "path": "src/catalog/mod.rs",
              "start_line": 40,
              "end_line": 72,
              "match_id": "symbols:m1",
              "result_handle": "result-aaa"
            },
            {
              "claim": "caller reaches catalog_entries from the HTTP surface",
              "tool": "incoming_calls",
              "path": "src/http/routes.rs",
              "start_line": 88,
              "end_line": 110,
              "match_id": "nav:m2",
              "result_handle": "result-bbb"
            }
          ]
        }"#;

        let packet: EvidencePacket =
            serde_json::from_str(skill_json).expect("skill-shaped packet should deserialize");
        assert_eq!(packet.claims.len(), 2);
        assert_eq!(packet.claims[0].tool, "search_symbol");
        assert_eq!(packet.claims[0].path, "src/catalog/mod.rs");
        assert_eq!(packet.claims[0].start_line, 40);
        assert_eq!(packet.claims[1].tool, "incoming_calls");
        assert_eq!(packet.claims[1].match_id.as_deref(), Some("nav:m2"));

        let round = serde_json::to_value(&packet).expect("packet should re-serialize");
        assert_eq!(round["claims"].as_array().map(|a| a.len()), Some(2));
    }
}
