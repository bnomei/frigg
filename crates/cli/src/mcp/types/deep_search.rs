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
