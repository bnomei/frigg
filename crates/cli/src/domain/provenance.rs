use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SourceRef {
    pub source_type: String,
    pub repository_id: Option<String>,
    pub path: Option<String>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProvenanceEvent {
    pub trace_id: String,
    pub tool_name: String,
    pub created_at: DateTime<Utc>,
    pub source_refs: Vec<SourceRef>,
}
