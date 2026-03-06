use crate::domain::model::{ReferenceMatch, SymbolMatch, TextMatch};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mcp::deep_search::{
    DeepSearchCitation as InternalDeepSearchCitation,
    DeepSearchCitationPayload as InternalDeepSearchCitationPayload,
    DeepSearchClaim as InternalDeepSearchClaim, DeepSearchFileSpan as InternalDeepSearchFileSpan,
    DeepSearchPlaybook as InternalDeepSearchPlaybook,
    DeepSearchPlaybookStep as InternalDeepSearchPlaybookStep,
    DeepSearchReplayCheck as InternalDeepSearchReplayCheck,
    DeepSearchTraceArtifact as InternalDeepSearchTraceArtifact,
    DeepSearchTraceOutcome as InternalDeepSearchTraceOutcome,
    DeepSearchTraceStep as InternalDeepSearchTraceStep,
};

pub const PUBLIC_READ_ONLY_TOOL_NAMES: [&str; 18] = [
    "list_repositories",
    "workspace_attach",
    "workspace_current",
    "read_file",
    "search_text",
    "search_hybrid",
    "search_symbol",
    "find_references",
    "go_to_definition",
    "find_declarations",
    "find_implementations",
    "incoming_calls",
    "outgoing_calls",
    "document_symbols",
    "search_structural",
    "deep_search_run",
    "deep_search_replay",
    "deep_search_compose_citations",
];
pub const WRITE_CONFIRM_PARAM: &str = "confirm";
pub const WRITE_CONFIRMATION_REQUIRED_ERROR_CODE: &str = "confirmation_required";

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RepositorySummary {
    pub repository_id: String,
    pub display_name: String,
    pub root_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListRepositoriesResponse {
    pub repositories: Vec<RepositorySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct ListRepositoriesParams {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceResolveMode {
    GitRoot,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceStorageIndexState {
    MissingDb,
    Uninitialized,
    Ready,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceStorageSummary {
    pub db_path: String,
    pub exists: bool,
    pub initialized: bool,
    pub index_state: WorkspaceStorageIndexState,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceAttachParams {
    /// File or directory path to attach. Relative paths resolve against the Frigg server process cwd.
    pub path: String,
    /// Whether to make the attached repository the session default. Omit to default to `true`.
    pub set_default: Option<bool>,
    /// Workspace resolution strategy. Omit to prefer the enclosing Git root before falling back to the direct directory.
    pub resolve_mode: Option<WorkspaceResolveMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceAttachResponse {
    pub repository: RepositorySummary,
    pub resolved_from: String,
    pub resolution: WorkspaceResolveMode,
    pub session_default: bool,
    pub storage: WorkspaceStorageSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
pub struct WorkspaceCurrentParams {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WorkspaceCurrentResponse {
    pub repository: Option<RepositorySummary>,
    pub session_default: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileParams {
    pub path: String,
    pub repository_id: Option<String>,
    pub max_bytes: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadFileResponse {
    pub repository_id: String,
    pub path: String,
    pub bytes: usize,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SearchPatternType {
    Literal,
    Regex,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextParams {
    /// Text or regex pattern to search for. Leading and trailing whitespace is trimmed.
    pub query: String,
    /// Match mode for `query`. Omit for exact literal search or set `regex` for safe-regex search.
    pub pattern_type: Option<SearchPatternType>,
    /// Optional repository scope from `list_repositories`. Omit to search every configured repository.
    pub repository_id: Option<String>,
    /// Optional safe regex applied to canonical repository-relative paths before files are searched.
    /// Use this to narrow broad queries to code, docs, or runtime slices.
    pub path_regex: Option<String>,
    /// Optional max matches. Frigg clamps the effective limit to the server search budget.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchTextResponse {
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
    /// Natural-language or exact phrase query for broad doc/runtime retrieval.
    /// Expect mixed contracts, README, runtime, or tests for broad questions;
    /// when you need concrete implementation anchors, follow with `search_symbol`
    /// or `search_text` plus scoped `path_regex`.
    pub query: String,
    /// Optional repository scope from `list_repositories`. Omit to search every configured repository.
    pub repository_id: Option<String>,
    /// Optional language filter for source-backed follow-up, for example `rust` when runtime files should outrank docs-only evidence.
    pub language: Option<String>,
    /// Optional max matches. Frigg clamps the effective limit to the server search budget.
    pub limit: Option<usize>,
    /// Optional channel-weight overrides when a client needs deterministic lexical/graph/semantic tradeoffs.
    pub weights: Option<SearchHybridChannelWeightsParams>,
    /// Optional semantic-channel toggle. Omit to use the active runtime configuration.
    pub semantic: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridMatch {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub excerpt: String,
    pub blended_score: f32,
    pub lexical_score: f32,
    pub graph_score: f32,
    pub semantic_score: f32,
    pub lexical_sources: Vec<String>,
    pub graph_sources: Vec<String>,
    pub semantic_sources: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchHybridResponse {
    pub matches: Vec<SearchHybridMatch>,
    /// Whether semantic retrieval was requested after config defaults were applied.
    pub semantic_requested: Option<bool>,
    /// Whether semantic retrieval actually contributed to the successful response.
    pub semantic_enabled: Option<bool>,
    /// Semantic channel outcome (`ok`, `disabled`, or `degraded`) for this response.
    pub semantic_status: Option<String>,
    /// Deterministic explanation when the semantic channel is disabled or degraded.
    pub semantic_reason: Option<String>,
    /// JSON-encoded compatibility metadata mirroring the semantic fields plus diagnostics.
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolParams {
    /// API/type/function name to search in indexed Rust/PHP symbols.
    /// Use this after broad `search_text` or `search_hybrid` results when you
    /// know the runtime anchor you want to inspect.
    pub query: String,
    /// Optional repository scope from `list_repositories`. Omit to search every configured repository.
    pub repository_id: Option<String>,
    /// Optional max matches. Frigg clamps the effective limit to the server search budget.
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolResponse {
    pub matches: Vec<SymbolMatch>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesParams {
    pub symbol: String,
    pub repository_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesResponse {
    pub matches: Vec<ReferenceMatch>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GoToDefinitionResponse {
    pub matches: Vec<NavigationLocation>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDeclarationsResponse {
    pub matches: Vec<NavigationLocation>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ImplementationMatch {
    pub symbol: String,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub relation: Option<String>,
    pub precision: Option<String>,
    pub fallback_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindImplementationsResponse {
    pub matches: Vec<ImplementationMatch>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsParams {
    pub symbol: Option<String>,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct IncomingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct OutgoingCallsResponse {
    pub matches: Vec<CallHierarchyMatch>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsParams {
    pub path: String,
    pub repository_id: Option<String>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DocumentSymbolsResponse {
    pub symbols: Vec<DocumentSymbolItem>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralParams {
    pub query: String,
    pub language: Option<String>,
    pub repository_id: Option<String>,
    pub path_regex: Option<String>,
    pub limit: Option<usize>,
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
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchStructuralResponse {
    pub matches: Vec<StructuralMatch>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchRunParams {
    pub playbook: DeepSearchPlaybookContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchRunResponse {
    pub trace_artifact: DeepSearchTraceArtifactContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchReplayParams {
    pub playbook: DeepSearchPlaybookContract,
    pub expected_trace_artifact: DeepSearchTraceArtifactContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchReplayResponse {
    pub matches: bool,
    pub diff: Option<String>,
    pub replayed_trace_artifact: DeepSearchTraceArtifactContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchComposeCitationsParams {
    pub trace_artifact: DeepSearchTraceArtifactContract,
    pub answer: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchComposeCitationsResponse {
    pub citation_payload: DeepSearchCitationPayloadContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchPlaybookContract {
    pub playbook_id: String,
    pub steps: Vec<DeepSearchPlaybookStepContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchPlaybookStepContract {
    pub step_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchTraceArtifactContract {
    pub trace_schema: String,
    pub playbook_id: String,
    pub step_count: usize,
    pub steps: Vec<DeepSearchTraceStepContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchTraceStepContract {
    pub step_index: usize,
    pub step_id: String,
    pub tool_name: String,
    pub params_json: String,
    pub outcome: DeepSearchTraceOutcomeContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeepSearchTraceOutcomeContract {
    Ok {
        response_json: String,
    },
    Err {
        code: String,
        message: String,
        error_code: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchCitationPayloadContract {
    pub answer_schema: String,
    pub playbook_id: String,
    pub answer: String,
    pub claims: Vec<DeepSearchClaimContract>,
    pub citations: Vec<DeepSearchCitationContract>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchClaimContract {
    pub claim_id: String,
    pub text: String,
    pub citation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchCitationContract {
    pub citation_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub repository_id: String,
    pub path: String,
    pub span: DeepSearchFileSpanContract,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct DeepSearchFileSpanContract {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl From<DeepSearchRunParams> for InternalDeepSearchPlaybook {
    fn from(value: DeepSearchRunParams) -> Self {
        value.playbook.into()
    }
}

impl From<InternalDeepSearchTraceArtifact> for DeepSearchRunResponse {
    fn from(value: InternalDeepSearchTraceArtifact) -> Self {
        Self {
            trace_artifact: value.into(),
        }
    }
}

impl DeepSearchReplayParams {
    pub fn into_internal(self) -> (InternalDeepSearchPlaybook, InternalDeepSearchTraceArtifact) {
        (self.playbook.into(), self.expected_trace_artifact.into())
    }
}

impl From<InternalDeepSearchReplayCheck> for DeepSearchReplayResponse {
    fn from(value: InternalDeepSearchReplayCheck) -> Self {
        Self {
            matches: value.matches,
            diff: value.diff,
            replayed_trace_artifact: value.replayed.into(),
        }
    }
}

impl DeepSearchComposeCitationsParams {
    pub fn into_internal(self) -> (InternalDeepSearchTraceArtifact, Option<String>) {
        (self.trace_artifact.into(), self.answer)
    }
}

impl From<InternalDeepSearchCitationPayload> for DeepSearchComposeCitationsResponse {
    fn from(value: InternalDeepSearchCitationPayload) -> Self {
        Self {
            citation_payload: value.into(),
        }
    }
}

impl From<DeepSearchPlaybookContract> for InternalDeepSearchPlaybook {
    fn from(value: DeepSearchPlaybookContract) -> Self {
        Self {
            playbook_id: value.playbook_id,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<InternalDeepSearchPlaybook> for DeepSearchPlaybookContract {
    fn from(value: InternalDeepSearchPlaybook) -> Self {
        Self {
            playbook_id: value.playbook_id,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<DeepSearchPlaybookStepContract> for InternalDeepSearchPlaybookStep {
    fn from(value: DeepSearchPlaybookStepContract) -> Self {
        Self {
            step_id: value.step_id,
            tool_name: value.tool_name,
            params: value.params,
        }
    }
}

impl From<InternalDeepSearchPlaybookStep> for DeepSearchPlaybookStepContract {
    fn from(value: InternalDeepSearchPlaybookStep) -> Self {
        Self {
            step_id: value.step_id,
            tool_name: value.tool_name,
            params: value.params,
        }
    }
}

impl From<DeepSearchTraceArtifactContract> for InternalDeepSearchTraceArtifact {
    fn from(value: DeepSearchTraceArtifactContract) -> Self {
        Self {
            trace_schema: value.trace_schema,
            playbook_id: value.playbook_id,
            step_count: value.step_count,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<InternalDeepSearchTraceArtifact> for DeepSearchTraceArtifactContract {
    fn from(value: InternalDeepSearchTraceArtifact) -> Self {
        Self {
            trace_schema: value.trace_schema,
            playbook_id: value.playbook_id,
            step_count: value.step_count,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<DeepSearchTraceStepContract> for InternalDeepSearchTraceStep {
    fn from(value: DeepSearchTraceStepContract) -> Self {
        Self {
            step_index: value.step_index,
            step_id: value.step_id,
            tool_name: value.tool_name,
            params_json: value.params_json,
            outcome: value.outcome.into(),
        }
    }
}

impl From<InternalDeepSearchTraceStep> for DeepSearchTraceStepContract {
    fn from(value: InternalDeepSearchTraceStep) -> Self {
        Self {
            step_index: value.step_index,
            step_id: value.step_id,
            tool_name: value.tool_name,
            params_json: value.params_json,
            outcome: value.outcome.into(),
        }
    }
}

impl From<DeepSearchTraceOutcomeContract> for InternalDeepSearchTraceOutcome {
    fn from(value: DeepSearchTraceOutcomeContract) -> Self {
        match value {
            DeepSearchTraceOutcomeContract::Ok { response_json } => Self::Ok { response_json },
            DeepSearchTraceOutcomeContract::Err {
                code,
                message,
                error_code,
            } => Self::Err {
                code,
                message,
                error_code,
            },
        }
    }
}

impl From<InternalDeepSearchTraceOutcome> for DeepSearchTraceOutcomeContract {
    fn from(value: InternalDeepSearchTraceOutcome) -> Self {
        match value {
            InternalDeepSearchTraceOutcome::Ok { response_json } => Self::Ok { response_json },
            InternalDeepSearchTraceOutcome::Err {
                code,
                message,
                error_code,
            } => Self::Err {
                code,
                message,
                error_code,
            },
        }
    }
}

impl From<DeepSearchCitationPayloadContract> for InternalDeepSearchCitationPayload {
    fn from(value: DeepSearchCitationPayloadContract) -> Self {
        Self {
            answer_schema: value.answer_schema,
            playbook_id: value.playbook_id,
            answer: value.answer,
            claims: value.claims.into_iter().map(Into::into).collect(),
            citations: value.citations.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<InternalDeepSearchCitationPayload> for DeepSearchCitationPayloadContract {
    fn from(value: InternalDeepSearchCitationPayload) -> Self {
        Self {
            answer_schema: value.answer_schema,
            playbook_id: value.playbook_id,
            answer: value.answer,
            claims: value.claims.into_iter().map(Into::into).collect(),
            citations: value.citations.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<DeepSearchClaimContract> for InternalDeepSearchClaim {
    fn from(value: DeepSearchClaimContract) -> Self {
        Self {
            claim_id: value.claim_id,
            text: value.text,
            citation_ids: value.citation_ids,
        }
    }
}

impl From<InternalDeepSearchClaim> for DeepSearchClaimContract {
    fn from(value: InternalDeepSearchClaim) -> Self {
        Self {
            claim_id: value.claim_id,
            text: value.text,
            citation_ids: value.citation_ids,
        }
    }
}

impl From<DeepSearchCitationContract> for InternalDeepSearchCitation {
    fn from(value: DeepSearchCitationContract) -> Self {
        Self {
            citation_id: value.citation_id,
            tool_call_id: value.tool_call_id,
            tool_name: value.tool_name,
            repository_id: value.repository_id,
            path: value.path,
            span: value.span.into(),
        }
    }
}

impl From<InternalDeepSearchCitation> for DeepSearchCitationContract {
    fn from(value: InternalDeepSearchCitation) -> Self {
        Self {
            citation_id: value.citation_id,
            tool_call_id: value.tool_call_id,
            tool_name: value.tool_name,
            repository_id: value.repository_id,
            path: value.path,
            span: value.span.into(),
        }
    }
}

impl From<DeepSearchFileSpanContract> for InternalDeepSearchFileSpan {
    fn from(value: DeepSearchFileSpanContract) -> Self {
        Self {
            start_line: value.start_line,
            start_column: value.start_column,
            end_line: value.end_line,
            end_column: value.end_column,
        }
    }
}

impl From<InternalDeepSearchFileSpan> for DeepSearchFileSpanContract {
    fn from(value: InternalDeepSearchFileSpan) -> Self {
        Self {
            start_line: value.start_line,
            start_column: value.start_column,
            end_line: value.end_line,
            end_column: value.end_column,
        }
    }
}

#[cfg(test)]
#[allow(clippy::panic)]
mod schema_tests {
    use std::collections::BTreeSet;
    use std::ffi::OsStr;
    use std::fs;
    use std::path::PathBuf;

    use schemars::{JsonSchema, schema_for};
    use serde::Deserialize;

    use super::*;

    #[derive(Debug, Deserialize)]
    struct ToolSchemaDoc {
        schema_id: String,
        tool_name: String,
        input_wrapper: String,
        output_wrapper: String,
        input_fields: Vec<String>,
        input_required: Vec<String>,
        output_fields: Vec<String>,
        output_required: Vec<String>,
        #[serde(default)]
        contract_notes: Vec<String>,
        #[serde(default)]
        nested_contracts: Option<Value>,
        #[serde(default)]
        step_tool_schema_refs: Vec<StepToolSchemaRefDoc>,
        #[serde(default)]
        input_example: Option<Value>,
        #[serde(default)]
        output_example: Option<Value>,
    }

    #[derive(Debug, Deserialize)]
    struct StepToolSchemaRefDoc {
        tool_name: String,
        schema_file: String,
        params_wrapper: String,
        response_wrapper: String,
    }

    fn docs_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/tools/v1")
    }

    fn read_doc(file_name: &str) -> ToolSchemaDoc {
        let path = docs_dir().join(file_name);
        let raw = fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("failed to read schema doc {}: {err}", path.display()));
        serde_json::from_str::<ToolSchemaDoc>(&raw)
            .unwrap_or_else(|err| panic!("failed to parse schema doc {}: {err}", path.display()))
    }

    fn field_set<T: JsonSchema>() -> BTreeSet<String> {
        let schema_json =
            serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
        schema_json
            .get("properties")
            .and_then(|value| value.as_object())
            .map(|props| props.keys().cloned().collect())
            .unwrap_or_default()
    }

    fn required_set<T: JsonSchema>() -> BTreeSet<String> {
        let schema_json =
            serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
        schema_json
            .get("required")
            .and_then(|value| value.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| item.as_str().map(ToOwned::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    }

    fn to_set(values: &[String]) -> BTreeSet<String> {
        values.iter().cloned().collect()
    }

    fn property_description<T: JsonSchema>(field: &str) -> Option<String> {
        let schema_json =
            serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
        schema_json
            .get("properties")
            .and_then(|value| value.get(field))
            .and_then(|value| value.get("description"))
            .and_then(|value| value.as_str())
            .map(ToOwned::to_owned)
    }

    fn property_schema<T: JsonSchema>(field: &str) -> Value {
        let schema_json =
            serde_json::to_value(schema_for!(T)).expect("failed to serialize generated schema");
        schema_json
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|props| props.get(field))
            .cloned()
            .unwrap_or_else(|| panic!("missing schema property `{field}`"))
    }

    fn schema_allows_type(schema: &Value, expected: &str) -> bool {
        schema.get("type").and_then(Value::as_str) == Some(expected)
            || schema
                .get("type")
                .and_then(Value::as_array)
                .is_some_and(|items| items.iter().any(|item| item.as_str() == Some(expected)))
            || schema
                .get("anyOf")
                .and_then(Value::as_array)
                .is_some_and(|variants| {
                    variants
                        .iter()
                        .any(|variant| schema_allows_type(variant, expected))
                })
            || schema
                .get("oneOf")
                .and_then(Value::as_array)
                .is_some_and(|variants| {
                    variants
                        .iter()
                        .any(|variant| schema_allows_type(variant, expected))
                })
    }

    fn assert_optional_string_property<T: JsonSchema>(field: &str) {
        let property = property_schema::<T>(field);
        assert!(
            schema_allows_type(&property, "string"),
            "expected `{field}` to allow string transport, got schema: {property}"
        );
        assert!(
            !schema_allows_type(&property, "object"),
            "expected `{field}` to avoid object transport, got schema: {property}"
        );
    }

    fn assert_contract<TInput: JsonSchema, TOutput: JsonSchema>(
        file_name: &str,
        tool_name: &str,
        input_wrapper: &str,
        output_wrapper: &str,
    ) {
        let doc = read_doc(file_name);
        assert_eq!(doc.schema_id, format!("frigg.tools.{tool_name}.v1"));
        assert_eq!(doc.tool_name, tool_name);
        assert_eq!(doc.input_wrapper, input_wrapper);
        assert_eq!(doc.output_wrapper, output_wrapper);
        assert_eq!(to_set(&doc.input_fields), field_set::<TInput>());
        assert_eq!(to_set(&doc.input_required), required_set::<TInput>());
        assert_eq!(to_set(&doc.output_fields), field_set::<TOutput>());
        assert_eq!(to_set(&doc.output_required), required_set::<TOutput>());
    }

    fn assert_examples_parse<TInput, TOutput>(file_name: &str)
    where
        TInput: for<'de> Deserialize<'de>,
        TOutput: for<'de> Deserialize<'de>,
    {
        let doc = read_doc(file_name);
        assert!(
            !doc.contract_notes.is_empty(),
            "{file_name} should publish contract_notes for nested deep-search payload guidance"
        );
        assert!(
            doc.nested_contracts.is_some(),
            "{file_name} should publish nested_contracts guidance"
        );

        let input_example = doc
            .input_example
            .unwrap_or_else(|| panic!("{file_name} should publish an input_example"));
        serde_json::from_value::<TInput>(input_example)
            .unwrap_or_else(|err| panic!("failed to parse input_example in {file_name}: {err}"));

        let output_example = doc
            .output_example
            .unwrap_or_else(|| panic!("{file_name} should publish an output_example"));
        serde_json::from_value::<TOutput>(output_example)
            .unwrap_or_else(|err| panic!("failed to parse output_example in {file_name}: {err}"));
    }

    fn nested_strings(doc: &ToolSchemaDoc, pointer: &str) -> BTreeSet<String> {
        doc.nested_contracts
            .as_ref()
            .unwrap_or_else(|| panic!("missing nested_contracts for {pointer}"))
            .pointer(pointer)
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("missing string array at nested_contracts{pointer}"))
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .unwrap_or_else(|| panic!("expected string at nested_contracts{pointer}"))
                    .to_owned()
            })
            .collect()
    }

    fn assert_step_tool_schema_refs(file_name: &str) {
        let doc = read_doc(file_name);
        let actual = doc
            .step_tool_schema_refs
            .iter()
            .map(|entry| {
                (
                    entry.tool_name.as_str(),
                    entry.schema_file.as_str(),
                    entry.params_wrapper.as_str(),
                    entry.response_wrapper.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let expected = vec![
            (
                "list_repositories",
                "list_repositories.v1.schema.json",
                "ListRepositoriesParams",
                "ListRepositoriesResponse",
            ),
            (
                "read_file",
                "read_file.v1.schema.json",
                "ReadFileParams",
                "ReadFileResponse",
            ),
            (
                "search_text",
                "search_text.v1.schema.json",
                "SearchTextParams",
                "SearchTextResponse",
            ),
            (
                "search_symbol",
                "search_symbol.v1.schema.json",
                "SearchSymbolParams",
                "SearchSymbolResponse",
            ),
            (
                "find_references",
                "find_references.v1.schema.json",
                "FindReferencesParams",
                "FindReferencesResponse",
            ),
        ];
        assert_eq!(
            actual, expected,
            "{file_name} step_tool_schema_refs drifted from the allowed deep-search step surface"
        );

        for entry in &doc.step_tool_schema_refs {
            let schema_path = docs_dir().join(&entry.schema_file);
            assert!(
                schema_path.exists(),
                "referenced schema file {} does not exist",
                schema_path.display()
            );
        }
    }

    fn assert_deep_search_stdio_setup_notes(file_name: &str) {
        let doc = read_doc(file_name);
        let notes = doc.contract_notes.join(" ");
        for required in [
            "FRIGG_MCP_TOOL_SURFACE_PROFILE=extended",
            "RUST_LOG=error",
            "--watch-mode off",
            "list_repositories",
        ] {
            assert!(
                notes.contains(required),
                "{file_name} contract_notes should mention `{required}`: {notes}"
            );
        }
    }

    fn assert_run_nested_contracts(file_name: &str) {
        let doc = read_doc(file_name);
        assert_eq!(
            nested_strings(&doc, "/playbook/required"),
            required_set::<DeepSearchPlaybookContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/playbook/step_required"),
            required_set::<DeepSearchPlaybookStepContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/playbook/allowed_step_tools"),
            [
                "find_references",
                "list_repositories",
                "read_file",
                "search_symbol",
                "search_text",
            ]
            .into_iter()
            .map(ToOwned::to_owned)
            .collect()
        );
        assert_eq!(
            nested_strings(&doc, "/trace_artifact/required"),
            required_set::<DeepSearchTraceArtifactContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/trace_artifact/step_required"),
            required_set::<DeepSearchTraceStepContract>()
        );
    }

    fn assert_replay_nested_contracts(file_name: &str) {
        let doc = read_doc(file_name);
        assert_eq!(
            nested_strings(&doc, "/playbook/required"),
            required_set::<DeepSearchPlaybookContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/playbook/step_required"),
            required_set::<DeepSearchPlaybookStepContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/expected_trace_artifact/required"),
            required_set::<DeepSearchTraceArtifactContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/replay_response/required"),
            required_set::<DeepSearchReplayResponse>()
        );
    }

    fn assert_citation_nested_contracts(file_name: &str) {
        let doc = read_doc(file_name);
        assert_eq!(
            nested_strings(&doc, "/trace_artifact/required"),
            required_set::<DeepSearchTraceArtifactContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/citation_payload/required"),
            required_set::<DeepSearchCitationPayloadContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/citation_payload/claim_required"),
            required_set::<DeepSearchClaimContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/citation_payload/citation_required"),
            required_set::<DeepSearchCitationContract>()
        );
        assert_eq!(
            nested_strings(&doc, "/citation_payload/span_required"),
            required_set::<DeepSearchFileSpanContract>()
        );
    }

    #[test]
    fn schema_list_repositories_contract_matches_wrappers() {
        assert_contract::<ListRepositoriesParams, ListRepositoriesResponse>(
            "list_repositories.v1.schema.json",
            "list_repositories",
            "ListRepositoriesParams",
            "ListRepositoriesResponse",
        );
    }

    #[test]
    fn schema_workspace_attach_contract_matches_wrappers() {
        assert_contract::<WorkspaceAttachParams, WorkspaceAttachResponse>(
            "workspace_attach.v1.schema.json",
            "workspace_attach",
            "WorkspaceAttachParams",
            "WorkspaceAttachResponse",
        );
    }

    #[test]
    fn schema_workspace_current_contract_matches_wrappers() {
        assert_contract::<WorkspaceCurrentParams, WorkspaceCurrentResponse>(
            "workspace_current.v1.schema.json",
            "workspace_current",
            "WorkspaceCurrentParams",
            "WorkspaceCurrentResponse",
        );
    }

    #[test]
    fn schema_read_file_contract_matches_wrappers() {
        assert_contract::<ReadFileParams, ReadFileResponse>(
            "read_file.v1.schema.json",
            "read_file",
            "ReadFileParams",
            "ReadFileResponse",
        );
    }

    #[test]
    fn schema_search_text_contract_matches_wrappers() {
        assert_contract::<SearchTextParams, SearchTextResponse>(
            "search_text.v1.schema.json",
            "search_text",
            "SearchTextParams",
            "SearchTextResponse",
        );
    }

    #[test]
    fn schema_search_text_includes_scoping_guidance() {
        let repository_id = property_description::<SearchTextParams>("repository_id")
            .expect("repository_id should expose a schema description");
        assert!(
            repository_id.contains("list_repositories"),
            "repository_id description should mention list_repositories guidance: {repository_id}"
        );

        let path_regex = property_description::<SearchTextParams>("path_regex")
            .expect("path_regex should expose a schema description");
        assert!(
            path_regex.contains("canonical repository-relative paths"),
            "path_regex description should mention canonical repository-relative paths: {path_regex}"
        );
        assert!(
            path_regex.contains("code, docs, or runtime slices"),
            "path_regex description should explain scoping guidance: {path_regex}"
        );
    }

    #[test]
    fn schema_search_hybrid_contract_matches_wrappers() {
        assert_contract::<SearchHybridParams, SearchHybridResponse>(
            "search_hybrid.v1.schema.json",
            "search_hybrid",
            "SearchHybridParams",
            "SearchHybridResponse",
        );
    }

    #[test]
    fn schema_search_hybrid_includes_follow_up_guidance() {
        let query = property_description::<SearchHybridParams>("query")
            .expect("query should expose a schema description");
        assert!(
            query.contains("search_symbol"),
            "search_hybrid.query description should mention search_symbol follow-up guidance: {query}"
        );
        assert!(
            query.contains("path_regex"),
            "search_hybrid.query description should mention scoped search_text path_regex guidance: {query}"
        );

        let note = property_description::<SearchHybridResponse>("note")
            .expect("note should expose a schema description");
        assert!(
            note.contains("JSON-encoded"),
            "search_hybrid.note description should mention JSON-encoded compatibility metadata: {note}"
        );
        assert!(
            note.contains("diagnostics"),
            "search_hybrid.note description should mention diagnostics metadata: {note}"
        );
    }

    #[test]
    fn schema_search_hybrid_note_remains_string_encoded() {
        assert_optional_string_property::<SearchHybridResponse>("note");
    }

    #[test]
    fn schema_search_symbol_contract_matches_wrappers() {
        assert_contract::<SearchSymbolParams, SearchSymbolResponse>(
            "search_symbol.v1.schema.json",
            "search_symbol",
            "SearchSymbolParams",
            "SearchSymbolResponse",
        );
    }

    #[test]
    fn schema_search_symbol_includes_runtime_pivot_guidance() {
        let query = property_description::<SearchSymbolParams>("query")
            .expect("query should expose a schema description");
        assert!(
            query.contains("search_hybrid"),
            "search_symbol.query description should mention search_hybrid follow-up guidance: {query}"
        );
        assert!(
            query.contains("runtime anchor"),
            "search_symbol.query description should explain runtime-anchor usage: {query}"
        );
    }

    #[test]
    fn schema_find_references_contract_matches_wrappers() {
        assert_contract::<FindReferencesParams, FindReferencesResponse>(
            "find_references.v1.schema.json",
            "find_references",
            "FindReferencesParams",
            "FindReferencesResponse",
        );
    }

    #[test]
    fn schema_go_to_definition_contract_matches_wrappers() {
        assert_contract::<GoToDefinitionParams, GoToDefinitionResponse>(
            "go_to_definition.v1.schema.json",
            "go_to_definition",
            "GoToDefinitionParams",
            "GoToDefinitionResponse",
        );
    }

    #[test]
    fn schema_find_declarations_contract_matches_wrappers() {
        assert_contract::<FindDeclarationsParams, FindDeclarationsResponse>(
            "find_declarations.v1.schema.json",
            "find_declarations",
            "FindDeclarationsParams",
            "FindDeclarationsResponse",
        );
    }

    #[test]
    fn schema_find_implementations_contract_matches_wrappers() {
        assert_contract::<FindImplementationsParams, FindImplementationsResponse>(
            "find_implementations.v1.schema.json",
            "find_implementations",
            "FindImplementationsParams",
            "FindImplementationsResponse",
        );
    }

    #[test]
    fn schema_incoming_calls_contract_matches_wrappers() {
        assert_contract::<IncomingCallsParams, IncomingCallsResponse>(
            "incoming_calls.v1.schema.json",
            "incoming_calls",
            "IncomingCallsParams",
            "IncomingCallsResponse",
        );
    }

    #[test]
    fn schema_outgoing_calls_contract_matches_wrappers() {
        assert_contract::<OutgoingCallsParams, OutgoingCallsResponse>(
            "outgoing_calls.v1.schema.json",
            "outgoing_calls",
            "OutgoingCallsParams",
            "OutgoingCallsResponse",
        );
    }

    #[test]
    fn schema_document_symbols_contract_matches_wrappers() {
        assert_contract::<DocumentSymbolsParams, DocumentSymbolsResponse>(
            "document_symbols.v1.schema.json",
            "document_symbols",
            "DocumentSymbolsParams",
            "DocumentSymbolsResponse",
        );
    }

    #[test]
    fn schema_search_structural_contract_matches_wrappers() {
        assert_contract::<SearchStructuralParams, SearchStructuralResponse>(
            "search_structural.v1.schema.json",
            "search_structural",
            "SearchStructuralParams",
            "SearchStructuralResponse",
        );
    }

    #[test]
    fn schema_deep_search_run_contract_matches_wrappers() {
        assert_contract::<DeepSearchRunParams, DeepSearchRunResponse>(
            "deep_search_run.v1.schema.json",
            "deep_search_run",
            "DeepSearchRunParams",
            "DeepSearchRunResponse",
        );
    }

    #[test]
    fn schema_deep_search_replay_contract_matches_wrappers() {
        assert_contract::<DeepSearchReplayParams, DeepSearchReplayResponse>(
            "deep_search_replay.v1.schema.json",
            "deep_search_replay",
            "DeepSearchReplayParams",
            "DeepSearchReplayResponse",
        );
    }

    #[test]
    fn schema_deep_search_compose_citations_contract_matches_wrappers() {
        assert_contract::<DeepSearchComposeCitationsParams, DeepSearchComposeCitationsResponse>(
            "deep_search_compose_citations.v1.schema.json",
            "deep_search_compose_citations",
            "DeepSearchComposeCitationsParams",
            "DeepSearchComposeCitationsResponse",
        );
    }

    #[test]
    fn schema_deep_search_run_examples_parse_against_wrappers() {
        assert_examples_parse::<DeepSearchRunParams, DeepSearchRunResponse>(
            "deep_search_run.v1.schema.json",
        );
    }

    #[test]
    fn schema_deep_search_run_contract_notes_and_step_refs_stay_in_sync() {
        assert_deep_search_stdio_setup_notes("deep_search_run.v1.schema.json");
        assert_step_tool_schema_refs("deep_search_run.v1.schema.json");
        assert_run_nested_contracts("deep_search_run.v1.schema.json");
    }

    #[test]
    fn schema_deep_search_replay_examples_parse_against_wrappers() {
        assert_examples_parse::<DeepSearchReplayParams, DeepSearchReplayResponse>(
            "deep_search_replay.v1.schema.json",
        );
    }

    #[test]
    fn schema_deep_search_replay_contract_notes_and_step_refs_stay_in_sync() {
        assert_deep_search_stdio_setup_notes("deep_search_replay.v1.schema.json");
        assert_step_tool_schema_refs("deep_search_replay.v1.schema.json");
        assert_replay_nested_contracts("deep_search_replay.v1.schema.json");
    }

    #[test]
    fn schema_deep_search_compose_citations_examples_parse_against_wrappers() {
        assert_examples_parse::<DeepSearchComposeCitationsParams, DeepSearchComposeCitationsResponse>(
            "deep_search_compose_citations.v1.schema.json",
        );
    }

    #[test]
    fn schema_deep_search_compose_citations_contract_notes_and_step_refs_stay_in_sync() {
        assert_deep_search_stdio_setup_notes("deep_search_compose_citations.v1.schema.json");
        assert_step_tool_schema_refs("deep_search_compose_citations.v1.schema.json");
        assert_citation_nested_contracts("deep_search_compose_citations.v1.schema.json");
    }

    #[test]
    fn schema_docs_presence_for_read_only_tools() {
        let base = docs_dir();
        let expected = PUBLIC_READ_ONLY_TOOL_NAMES
            .iter()
            .map(|name| format!("{name}.v1.schema.json"))
            .collect::<BTreeSet<_>>();
        let actual = fs::read_dir(&base)
            .unwrap_or_else(|err| {
                panic!("failed to read schema docs dir {}: {err}", base.display())
            })
            .map(|entry| {
                entry
                    .unwrap_or_else(|err| panic!("failed to read schema docs dir entry: {err}"))
                    .path()
            })
            .filter(|path| path.extension() == Some(OsStr::new("json")))
            .filter_map(|path| {
                path.file_name()
                    .and_then(|name| name.to_str().map(ToOwned::to_owned))
            })
            .collect::<BTreeSet<_>>();

        assert_eq!(
            actual, expected,
            "public tool schema file set drifted; update tests/contracts intentionally before adding tools"
        );
    }

    #[test]
    fn schema_core_read_only_input_fields_exclude_confirm_param() {
        let confirm = WRITE_CONFIRM_PARAM.to_owned();
        let input_field_sets = [
            field_set::<ListRepositoriesParams>(),
            field_set::<WorkspaceAttachParams>(),
            field_set::<WorkspaceCurrentParams>(),
            field_set::<ReadFileParams>(),
            field_set::<SearchTextParams>(),
            field_set::<SearchHybridParams>(),
            field_set::<SearchSymbolParams>(),
            field_set::<FindReferencesParams>(),
            field_set::<GoToDefinitionParams>(),
            field_set::<FindDeclarationsParams>(),
            field_set::<FindImplementationsParams>(),
            field_set::<IncomingCallsParams>(),
            field_set::<OutgoingCallsParams>(),
            field_set::<DocumentSymbolsParams>(),
            field_set::<SearchStructuralParams>(),
            field_set::<DeepSearchRunParams>(),
            field_set::<DeepSearchReplayParams>(),
            field_set::<DeepSearchComposeCitationsParams>(),
        ];

        for fields in input_field_sets {
            assert!(
                !fields.contains(&confirm),
                "read-only tool params must not expose `{}` before a write-surface contract upgrade",
                WRITE_CONFIRM_PARAM
            );
        }
    }

    #[test]
    fn schema_write_surface_policy_markers_are_present_in_contract_docs() {
        let tools_readme_path = docs_dir().join("README.md");
        let tools_readme = fs::read_to_string(&tools_readme_path).unwrap_or_else(|err| {
            panic!(
                "failed to read tools contract README {}: {err}",
                tools_readme_path.display()
            )
        });
        for marker in [
            "write_surface_policy: v1",
            "current_public_tool_surface: read_only",
            "write_confirm_required: true",
            "write_confirm_semantics: reject_missing_or_false_confirm_before_side_effects",
            "write_safety_invariant_workspace_boundary: required",
            "write_safety_invariant_path_traversal_defense: required",
            "write_safety_invariant_regex_budget_limits: required",
            "write_safety_invariant_typed_deterministic_errors: required",
        ] {
            assert!(
                tools_readme.contains(marker),
                "tools contract README is missing policy marker `{marker}`"
            );
        }

        let confirm_param_marker = format!("write_confirm_param: {WRITE_CONFIRM_PARAM}");
        assert!(
            tools_readme.contains(&confirm_param_marker),
            "tools contract README is missing policy marker `{confirm_param_marker}`"
        );
        let confirm_error_marker =
            format!("write_confirm_failure_error_code: {WRITE_CONFIRMATION_REQUIRED_ERROR_CODE}");
        assert!(
            tools_readme.contains(&confirm_error_marker),
            "tools contract README is missing policy marker `{confirm_error_marker}`"
        );

        let errors_path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contracts/errors.md");
        let errors_doc = fs::read_to_string(&errors_path).unwrap_or_else(|err| {
            panic!(
                "failed to read errors contract {}: {err}",
                errors_path.display()
            )
        });
        for marker in [
            "write_surface_policy: v1",
            "write_confirm_required: true",
            "write_no_side_effect_without_confirm: true",
        ] {
            assert!(
                errors_doc.contains(marker),
                "errors contract is missing policy marker `{marker}`"
            );
        }
        assert!(
            errors_doc.contains(&confirm_param_marker),
            "errors contract is missing policy marker `{confirm_param_marker}`"
        );
        assert!(
            errors_doc.contains(&confirm_error_marker),
            "errors contract is missing policy marker `{confirm_error_marker}`"
        );
    }
}
