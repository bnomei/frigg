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

pub const PUBLIC_READ_ONLY_TOOL_NAMES: [&str; 16] = [
    "list_repositories",
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
    pub query: String,
    pub pattern_type: Option<SearchPatternType>,
    pub repository_id: Option<String>,
    pub path_regex: Option<String>,
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
    pub query: String,
    pub repository_id: Option<String>,
    pub language: Option<String>,
    pub limit: Option<usize>,
    pub weights: Option<SearchHybridChannelWeightsParams>,
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
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchSymbolParams {
    pub query: String,
    pub repository_id: Option<String>,
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
    fn schema_search_hybrid_contract_matches_wrappers() {
        assert_contract::<SearchHybridParams, SearchHybridResponse>(
            "search_hybrid.v1.schema.json",
            "search_hybrid",
            "SearchHybridParams",
            "SearchHybridResponse",
        );
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
