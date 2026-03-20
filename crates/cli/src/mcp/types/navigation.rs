use crate::domain::model::{GeneratedStructuralFollowUp, ReferenceMatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesParams {
    /// Optional symbol query. Omit when resolving the target by source location.
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    /// Optional source path used for deterministic location-aware target resolution.
    pub path: Option<String>,
    /// Optional 1-based line used for deterministic location-aware target resolution.
    pub line: Option<usize>,
    /// Optional 1-based column used for deterministic location-aware target resolution.
    pub column: Option<usize>,
    /// Whether definition rows should be included in the returned reference set. Omit to default to `true`.
    pub include_definition: Option<bool>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each anchored match.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NavigationMode {
    Precise,
    PrecisePartial,
    HeuristicNoPrecise,
    UnavailableNoPrecise,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesResponse {
    pub total_matches: usize,
    pub matches: Vec<ReferenceMatch>,
    pub mode: NavigationMode,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each anchored match.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationLocation {
    pub symbol: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub kind: Option<String>,
    pub precision: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionResponse {
    pub matches: Vec<NavigationLocation>,
    pub mode: NavigationMode,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each anchored match.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsResponse {
    pub matches: Vec<NavigationLocation>,
    pub mode: NavigationMode,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each anchored match.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImplementationMatch {
    pub symbol: String,
    pub kind: Option<String>,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub relation: Option<String>,
    pub precision: Option<String>,
    pub fallback_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsResponse {
    pub matches: Vec<ImplementationMatch>,
    pub mode: NavigationMode,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each anchored match.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each anchored match.
    pub include_follow_up_structural: Option<bool>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CallHierarchyMatch {
    pub source_symbol: String,
    pub target_symbol: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub relation: String,
    pub precision: Option<String>,
    pub call_path: Option<String>,
    pub call_line: Option<usize>,
    pub call_column: Option<usize>,
    pub call_end_line: Option<usize>,
    pub call_end_column: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    pub mode: NavigationMode,
    pub availability: Option<NavigationAvailability>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    pub mode: NavigationMode,
    pub availability: Option<NavigationAvailability>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NavigationAvailability {
    pub status: String,
    pub reason: Option<String>,
    pub precise_required_for_complete_results: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsParams {
    pub path: String,
    pub repository_id: Option<String>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each anchored symbol.
    pub include_follow_up_structural: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolItem {
    pub symbol: String,
    pub kind: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: Option<usize>,
    pub end_column: Option<usize>,
    pub container: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
    pub children: Vec<DocumentSymbolItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsResponse {
    pub symbols: Vec<DocumentSymbolItem>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InspectSyntaxTreeParams {
    pub path: String,
    pub repository_id: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub max_ancestors: Option<usize>,
    pub max_children: Option<usize>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from the focused AST node.
    pub include_follow_up_structural: Option<bool>,
}

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
    pub metadata: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralParams {
    pub query: String,
    pub language: Option<String>,
    pub repository_id: Option<String>,
    pub path_regex: Option<String>,
    pub limit: Option<usize>,
    /// Optional opt-in for best-effort replayable `search_structural` suggestions derived from each matched AST node.
    pub include_follow_up_structural: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct StructuralMatch {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub end_line: usize,
    pub end_column: usize,
    pub excerpt: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralResponse {
    pub matches: Vec<StructuralMatch>,
    pub metadata: Option<Value>,
    pub note: Option<String>,
}
