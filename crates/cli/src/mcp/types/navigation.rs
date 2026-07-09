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
    /// Flattened recovery fields on empty reference results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Parameters for `go_to_definition`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionParams {
    /// Recommended: symbol name to resolve. Prefer this over path+line alone on dense lines.
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
    /// Soft warning when path+line without `symbol` looks dense/ambiguous.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_warning: Option<String>,
    /// Flattened recovery fields on empty definition results.
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
    /// Flattened recovery fields on empty declaration results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
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
    /// Flattened recovery fields on empty implementation results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
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
    /// Flattened recovery fields on empty caller results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
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
    /// Flattened recovery fields on empty callee results.
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
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
    /// Return only top-level symbols when true. Defaults to `true` when omitted.
    pub top_level_only: Option<bool>,
    /// Max outline rows to return. Omit for the default bounded page.
    pub limit: Option<usize>,
    /// Continuation offset returned as `resume_from` when the outline is truncated.
    pub resume_from: Option<usize>,
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

/// Composed impact response: symbol hits + references + callers (+ optional impls).
///
/// **One next-step channel:** follow-ups live only in flattened
/// [`RecoveryFields::suggested_next`] (same pattern as `search_batch`). There is no
/// second top-level `suggested_next` field — dual channels caused serde ambiguity and
/// agents reading full mode for “more next steps” found nothing extra.
///
/// Compact (default) keeps: composed match arrays, handles, modes, and recovery/next.
/// Full `response_mode` is forwarded to child search/nav calls for diagnostics only;
/// it does not add a second recovery channel on the bundle itself.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImpactBundleResponse {
    pub symbol: String,
    pub path_class: String,
    pub symbols: Vec<crate::domain::model::SymbolMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbols_result_handle: Option<String>,
    pub references: Vec<crate::domain::model::ReferenceMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub references_result_handle: Option<String>,
    pub references_mode: NavigationMode,
    pub incoming_calls: Vec<CallHierarchyMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub incoming_calls_result_handle: Option<String>,
    pub incoming_calls_mode: NavigationMode,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implementations: Vec<ImplementationMatch>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations_result_handle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implementations_mode: Option<NavigationMode>,
    pub implementations_included: bool,
    /// Flattened recovery + **only** `suggested_next` channel (success and zero-hit paths).
    #[serde(flatten, default)]
    pub recovery: super::RecoveryFields,
}

/// Optional evidence-packet claim witness shape for review/security.
///
/// Agents may assemble multi-claim packets from search/nav/read results using this shape.
/// Not a live MCP tool response — documentation and optional typing helper only.
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

#[cfg(test)]
mod tests {
    use super::{
        EvidencePacket, EvidencePacketClaim, ImpactBundleResponse, NavigationMode,
    };
    use crate::mcp::types::{RecoveryFields, SuggestedNext};

    #[test]
    fn impact_bundle_response_single_suggested_next_channel() {
        // Success-shaped: next steps only via flattened recovery (no dual top-level field).
        let success = ImpactBundleResponse {
            symbol: "catalog_entries".to_owned(),
            path_class: "runtime".to_owned(),
            symbols: Vec::new(),
            symbols_result_handle: Some("symbols:h1".to_owned()),
            references: Vec::new(),
            references_result_handle: Some("refs:h1".to_owned()),
            references_mode: NavigationMode::Precise,
            incoming_calls: Vec::new(),
            incoming_calls_result_handle: None,
            incoming_calls_mode: NavigationMode::Precise,
            implementations: Vec::new(),
            implementations_result_handle: None,
            implementations_mode: None,
            implementations_included: false,
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
            .expect("flattened recovery must expose suggested_next once");
        assert_eq!(next.len(), 2);
        assert!(
            value.as_object().expect("object").keys().filter(|k| *k == "suggested_next").count()
                == 1,
            "exactly one suggested_next key after serialize"
        );
        // Compact-relevant fields stay present on success.
        assert_eq!(value["symbol"], "catalog_entries");
        assert_eq!(value["symbols_result_handle"], "symbols:h1");
        assert_eq!(value["references_result_handle"], "refs:h1");
        assert!(value.get("error_code").is_none());

        // Zero-hit shaped recovery still single-channel.
        let zero = ImpactBundleResponse {
            symbol: "missing_sym".to_owned(),
            path_class: "runtime".to_owned(),
            symbols: Vec::new(),
            symbols_result_handle: None,
            references: Vec::new(),
            references_result_handle: None,
            references_mode: NavigationMode::UnavailableNoPrecise,
            incoming_calls: Vec::new(),
            incoming_calls_result_handle: None,
            incoming_calls_mode: NavigationMode::UnavailableNoPrecise,
            implementations: Vec::new(),
            implementations_result_handle: None,
            implementations_mode: None,
            implementations_included: false,
            recovery: RecoveryFields {
                error_code: Some("ZERO_HIT".to_owned()),
                message: Some("no symbol hits".to_owned()),
                suggested_next: vec![SuggestedNext::tool("search_symbol")
                    .with_symbol("missing_sym")
                    .with_reason("retry")],
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
        // Round-trip: deserializing must not invent a second channel field.
        assert!(zero_value.get("recovery").is_none());
        let back: ImpactBundleResponse =
            serde_json::from_value(zero_value).expect("deserialize impact");
        assert_eq!(back.recovery.suggested_next.len(), 1);
        assert_eq!(back.recovery.error_code.as_deref(), Some("ZERO_HIT"));

        // Flattened recovery: suggested_next appears at top level JSON, not nested under recovery.
        assert!(value.get("recovery").is_none());
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
        // Skill-documented multi-claim envelope.
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
