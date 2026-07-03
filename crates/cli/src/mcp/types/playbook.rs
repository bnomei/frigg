//! Playbook MCP wire types for playbook run, replay, and citation composition tools.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mcp::advanced::deep_search::{
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

/// Parameters for `playbook_run`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookRunParams {
    pub playbook: PlaybookContract,
}

/// Response from `playbook_run` containing the executed trace artifact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookRunResponse {
    pub trace_artifact: PlaybookTraceArtifactContract,
}

/// Parameters for `playbook_replay`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookReplayParams {
    pub playbook: PlaybookContract,
    pub expected_trace_artifact: PlaybookTraceArtifactContract,
}

/// Response from `playbook_replay` comparing expected and replayed traces.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookReplayResponse {
    pub matches: bool,
    pub diff: Option<String>,
    pub replayed_trace_artifact: PlaybookTraceArtifactContract,
}

/// Parameters for `playbook_compose_citations`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookComposeCitationsParams {
    pub trace_artifact: PlaybookTraceArtifactContract,
    pub answer: Option<String>,
}

/// Response from `playbook_compose_citations` with claim-linked file spans.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookComposeCitationsResponse {
    pub citation_payload: PlaybookCitationPayloadContract,
}

/// MCP wire contract for a playbook.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookContract {
    pub playbook_id: String,
    pub steps: Vec<PlaybookStepContract>,
}

/// One tool invocation step in a playbook contract.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookStepContract {
    pub step_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub params: Value,
}

/// MCP wire contract for a playbook trace artifact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookTraceArtifactContract {
    pub trace_schema: String,
    pub playbook_id: String,
    pub step_count: usize,
    pub steps: Vec<PlaybookTraceStepContract>,
}

/// One executed step recorded in a playbook trace artifact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookTraceStepContract {
    pub step_index: usize,
    pub step_id: String,
    pub tool_name: String,
    pub params_json: String,
    pub outcome: PlaybookTraceOutcomeContract,
}

/// Success or error outcome for one traced playbook tool call.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum PlaybookTraceOutcomeContract {
    Ok {
        response_json: String,
    },
    Err {
        code: String,
        message: String,
        error_code: Option<String>,
    },
}

/// Claim-linked citation payload composed from a playbook trace artifact.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookCitationPayloadContract {
    pub answer_schema: String,
    pub playbook_id: String,
    pub answer: String,
    pub claims: Vec<PlaybookClaimContract>,
    pub citations: Vec<PlaybookCitationContract>,
}

/// One answer claim backed by one or more file-span citations.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookClaimContract {
    pub claim_id: String,
    pub text: String,
    pub citation_ids: Vec<String>,
}

/// One repository file-span citation tied to a traced tool call.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookCitationContract {
    pub citation_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub repository_id: String,
    pub path: String,
    pub span: PlaybookFileSpanContract,
}

/// 1-based file span used by playbook citation contracts.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct PlaybookFileSpanContract {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

impl From<PlaybookRunParams> for InternalDeepSearchPlaybook {
    fn from(value: PlaybookRunParams) -> Self {
        value.playbook.into()
    }
}

impl From<InternalDeepSearchTraceArtifact> for PlaybookRunResponse {
    fn from(value: InternalDeepSearchTraceArtifact) -> Self {
        Self {
            trace_artifact: value.into(),
        }
    }
}

impl PlaybookReplayParams {
    pub fn into_internal(self) -> (InternalDeepSearchPlaybook, InternalDeepSearchTraceArtifact) {
        (self.playbook.into(), self.expected_trace_artifact.into())
    }
}

impl From<InternalDeepSearchReplayCheck> for PlaybookReplayResponse {
    fn from(value: InternalDeepSearchReplayCheck) -> Self {
        Self {
            matches: value.matches,
            diff: value.diff,
            replayed_trace_artifact: value.replayed.into(),
        }
    }
}

impl PlaybookComposeCitationsParams {
    pub fn into_internal(self) -> (InternalDeepSearchTraceArtifact, Option<String>) {
        (self.trace_artifact.into(), self.answer)
    }
}

impl From<InternalDeepSearchCitationPayload> for PlaybookComposeCitationsResponse {
    fn from(value: InternalDeepSearchCitationPayload) -> Self {
        Self {
            citation_payload: value.into(),
        }
    }
}

impl From<PlaybookContract> for InternalDeepSearchPlaybook {
    fn from(value: PlaybookContract) -> Self {
        Self {
            playbook_id: value.playbook_id,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<InternalDeepSearchPlaybook> for PlaybookContract {
    fn from(value: InternalDeepSearchPlaybook) -> Self {
        Self {
            playbook_id: value.playbook_id,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<PlaybookStepContract> for InternalDeepSearchPlaybookStep {
    fn from(value: PlaybookStepContract) -> Self {
        Self {
            step_id: value.step_id,
            tool_name: value.tool_name,
            params: value.params,
        }
    }
}

impl From<InternalDeepSearchPlaybookStep> for PlaybookStepContract {
    fn from(value: InternalDeepSearchPlaybookStep) -> Self {
        Self {
            step_id: value.step_id,
            tool_name: value.tool_name,
            params: value.params,
        }
    }
}

impl From<PlaybookTraceArtifactContract> for InternalDeepSearchTraceArtifact {
    fn from(value: PlaybookTraceArtifactContract) -> Self {
        Self {
            trace_schema: value.trace_schema,
            playbook_id: value.playbook_id,
            step_count: value.step_count,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<InternalDeepSearchTraceArtifact> for PlaybookTraceArtifactContract {
    fn from(value: InternalDeepSearchTraceArtifact) -> Self {
        Self {
            trace_schema: value.trace_schema,
            playbook_id: value.playbook_id,
            step_count: value.step_count,
            steps: value.steps.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<PlaybookTraceStepContract> for InternalDeepSearchTraceStep {
    fn from(value: PlaybookTraceStepContract) -> Self {
        Self {
            step_index: value.step_index,
            step_id: value.step_id,
            tool_name: value.tool_name,
            params_json: value.params_json,
            outcome: value.outcome.into(),
        }
    }
}

impl From<InternalDeepSearchTraceStep> for PlaybookTraceStepContract {
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

impl From<PlaybookTraceOutcomeContract> for InternalDeepSearchTraceOutcome {
    fn from(value: PlaybookTraceOutcomeContract) -> Self {
        match value {
            PlaybookTraceOutcomeContract::Ok { response_json } => Self::Ok { response_json },
            PlaybookTraceOutcomeContract::Err {
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

impl From<InternalDeepSearchTraceOutcome> for PlaybookTraceOutcomeContract {
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

impl From<PlaybookCitationPayloadContract> for InternalDeepSearchCitationPayload {
    fn from(value: PlaybookCitationPayloadContract) -> Self {
        Self {
            answer_schema: value.answer_schema,
            playbook_id: value.playbook_id,
            answer: value.answer,
            claims: value.claims.into_iter().map(Into::into).collect(),
            citations: value.citations.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<InternalDeepSearchCitationPayload> for PlaybookCitationPayloadContract {
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

impl From<PlaybookClaimContract> for InternalDeepSearchClaim {
    fn from(value: PlaybookClaimContract) -> Self {
        Self {
            claim_id: value.claim_id,
            text: value.text,
            citation_ids: value.citation_ids,
        }
    }
}

impl From<InternalDeepSearchClaim> for PlaybookClaimContract {
    fn from(value: InternalDeepSearchClaim) -> Self {
        Self {
            claim_id: value.claim_id,
            text: value.text,
            citation_ids: value.citation_ids,
        }
    }
}

impl From<PlaybookCitationContract> for InternalDeepSearchCitation {
    fn from(value: PlaybookCitationContract) -> Self {
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

impl From<InternalDeepSearchCitation> for PlaybookCitationContract {
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

impl From<PlaybookFileSpanContract> for InternalDeepSearchFileSpan {
    fn from(value: PlaybookFileSpanContract) -> Self {
        Self {
            start_line: value.start_line,
            start_column: value.start_column,
            end_line: value.end_line,
            end_column: value.end_column,
        }
    }
}

impl From<InternalDeepSearchFileSpan> for PlaybookFileSpanContract {
    fn from(value: InternalDeepSearchFileSpan) -> Self {
        Self {
            start_line: value.start_line,
            start_column: value.start_column,
            end_line: value.end_line,
            end_column: value.end_column,
        }
    }
}
