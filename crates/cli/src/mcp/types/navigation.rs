//! Navigation MCP wire types: references, definitions, declarations, implementations, and call hierarchy.

use super::{MetadataObject, ResponseMode};
use crate::domain::model::{GeneratedStructuralFollowUp, ReferenceMatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Parameters for `find_references`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesParams {
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
    /// Flattened recovery fields on empty reference results (`FUT-006` / `FUT-016`).
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Parameters for `go_to_definition`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// One resolved navigation location with optional structural follow-up suggestions.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationLocation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
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
    /// Flattened recovery fields on empty definition results (`FUT-006` / `FUT-016`).
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Parameters for `find_declarations`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// Response from `find_declarations`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsResponse {
    pub matches: Vec<NavigationLocation>,
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
}

/// Parameters for `find_implementations`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// One implementation or override location for a resolved symbol target.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImplementationMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
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
}

/// Parameters for `incoming_calls`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// Parameters for `outgoing_calls`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
    /// Response detail profile. Omit to default to `compact`.
    pub response_mode: Option<ResponseMode>,
}

/// One incoming or outgoing call edge in the call hierarchy.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallHierarchyMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
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
}

/// Response from `outgoing_calls`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
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
    pub path: String,
    pub repository_id: Option<String>,
    /// Include structural follow-up suggestions.
    pub include_follow_up_structural: Option<bool>,
    /// Return only top-level symbols when true.
    pub top_level_only: Option<bool>,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    #[schemars(schema_with = "super::metadata_object_field_schema")]
    pub metadata: Option<MetadataObject>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}
