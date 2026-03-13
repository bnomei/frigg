use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mcp::server::FriggMcpServer;

const DEEP_SEARCH_ALLOWED_STEP_TOOLS: [&str; 5] = [
    "list_repositories",
    "read_file",
    "search_text",
    "search_symbol",
    "find_references",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchPlaybook {
    pub playbook_id: String,
    pub steps: Vec<DeepSearchPlaybookStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchPlaybookStep {
    pub step_id: String,
    pub tool_name: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchTraceArtifact {
    pub trace_schema: String,
    pub playbook_id: String,
    pub step_count: usize,
    pub steps: Vec<DeepSearchTraceStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchTraceStep {
    pub step_index: usize,
    pub step_id: String,
    pub tool_name: String,
    pub params_json: String,
    pub outcome: DeepSearchTraceOutcome,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DeepSearchTraceOutcome {
    Ok {
        response_json: String,
    },
    Err {
        code: String,
        message: String,
        error_code: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchReplayCheck {
    pub matches: bool,
    pub diff: Option<String>,
    pub replayed: DeepSearchTraceArtifact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchCitationPayload {
    pub answer_schema: String,
    pub playbook_id: String,
    pub answer: String,
    pub claims: Vec<DeepSearchClaim>,
    pub citations: Vec<DeepSearchCitation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchClaim {
    pub claim_id: String,
    pub text: String,
    pub citation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchCitation {
    pub citation_id: String,
    pub tool_call_id: String,
    pub tool_name: String,
    pub repository_id: String,
    pub path: String,
    pub span: DeepSearchFileSpan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeepSearchFileSpan {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Clone)]
pub struct DeepSearchHarness {
    server: FriggMcpServer,
}

#[path = "deep_search/runtime.rs"]
mod runtime;
#[cfg(test)]
#[path = "deep_search/tests.rs"]
mod tests;
