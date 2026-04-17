//! Wire contracts for Frigg's public MCP tool surface. These types keep workspace lifecycle,
//! search, navigation, and health-reporting semantics explicit so server code, tests, and schema
//! generation all describe the same external API.

use std::borrow::Cow;
use std::ops::Deref;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const PUBLIC_TOOL_NAMES: [&str; 24] = [
    "list_repositories",
    "workspace_attach",
    "workspace_detach",
    "workspace_prepare",
    "workspace_reindex",
    "workspace_current",
    "read_file",
    "read_match",
    "explore",
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
    "inspect_syntax_tree",
    "search_structural",
    "deep_search_run",
    "deep_search_replay",
    "deep_search_compose_citations",
];
/// Public tools that are guaranteed not to mutate workspace or repository state.
pub const PUBLIC_READ_ONLY_TOOL_NAMES: [&str; 20] = [
    "list_repositories",
    "workspace_current",
    "read_file",
    "read_match",
    "explore",
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
    "inspect_syntax_tree",
    "search_structural",
    "deep_search_run",
    "deep_search_replay",
    "deep_search_compose_citations",
];
/// Public tools whose behavior depends on per-session workspace attachment state.
pub const PUBLIC_SESSION_STATEFUL_TOOL_NAMES: [&str; 2] = ["workspace_attach", "workspace_detach"];
/// Public tools that can change on-disk or persisted state and therefore require write-style
/// handling.
pub const PUBLIC_WRITE_TOOL_NAMES: [&str; 2] = ["workspace_prepare", "workspace_reindex"];
pub const WRITE_CONFIRM_PARAM: &str = "confirm";
pub const WRITE_CONFIRMATION_REQUIRED_ERROR_CODE: &str = "confirmation_required";

/// Object-only metadata payload used by several MCP read responses.
///
/// Frigg emits structured JSON objects here at runtime, and the explicit wrapper keeps the
/// generated tool `outputSchema` compatible with strict MCP clients that reject boolean
/// subschemas for `properties.metadata`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct MetadataObject(Value);

impl MetadataObject {
    pub fn into_inner(self) -> Value {
        self.0
    }
}

impl TryFrom<Value> for MetadataObject {
    type Error = &'static str;

    fn try_from(value: Value) -> Result<Self, Self::Error> {
        match value {
            Value::Object(_) => Ok(Self(value)),
            _ => Err("metadata payload must be a JSON object"),
        }
    }
}

impl From<MetadataObject> for Value {
    fn from(value: MetadataObject) -> Self {
        value.0
    }
}

impl Deref for MetadataObject {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl PartialEq<Value> for MetadataObject {
    fn eq(&self, other: &Value) -> bool {
        self.0 == *other
    }
}

impl PartialEq<MetadataObject> for Value {
    fn eq(&self, other: &MetadataObject) -> bool {
        *self == other.0
    }
}

impl JsonSchema for MetadataObject {
    fn inline_schema() -> bool {
        true
    }

    fn schema_name() -> Cow<'static, str> {
        "MetadataObject".into()
    }

    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        schemars::json_schema!({
            "type": "object"
        })
    }
}

pub fn metadata_object_field_schema(generator: &mut SchemaGenerator) -> Schema {
    MetadataObject::json_schema(generator)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    Compact,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadPresentationMode {
    Text,
    Json,
}

#[path = "types/deep_search.rs"]
mod deep_search;
#[path = "types/navigation.rs"]
mod navigation;
#[path = "types/repository.rs"]
mod repository;
#[path = "types/search.rs"]
mod search;
#[path = "types/workspace.rs"]
mod workspace;

pub use deep_search::*;
pub use navigation::*;
pub use repository::*;
pub use search::*;
pub use workspace::*;
