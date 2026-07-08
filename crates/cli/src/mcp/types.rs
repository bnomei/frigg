//! Wire contracts for Frigg's public MCP tool surface. These types keep workspace lifecycle,
//! search, navigation, and health-reporting semantics explicit so server code, tests, and schema
//! generation all describe the same external API.

use std::borrow::Cow;
use std::ops::Deref;

use schemars::{JsonSchema, Schema, SchemaGenerator};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Full public MCP tool manifest when playbook tools are compiled in.
#[cfg(feature = "playbook")]
pub const PUBLIC_TOOL_NAMES: &[&str] = &[
    "workspace",
    "list_files",
    "read_file",
    "read_match",
    "explore",
    "search_text",
    "search_hybrid",
    "search_symbol",
    "search_batch",
    "find_references",
    "go_to_definition",
    "find_declarations",
    "find_implementations",
    "incoming_calls",
    "outgoing_calls",
    "document_symbols",
    "inspect_syntax_tree",
    "search_structural",
    "impact_bundle",
    "playbook_run",
    "playbook_replay",
    "playbook_compose_citations",
];
/// Full public MCP tool manifest for the default build without playbook tools.
#[cfg(not(feature = "playbook"))]
pub const PUBLIC_TOOL_NAMES: &[&str] = &[
    "workspace",
    "list_files",
    "read_file",
    "read_match",
    "explore",
    "search_text",
    "search_hybrid",
    "search_symbol",
    "search_batch",
    "find_references",
    "go_to_definition",
    "find_declarations",
    "find_implementations",
    "incoming_calls",
    "outgoing_calls",
    "document_symbols",
    "inspect_syntax_tree",
    "search_structural",
    "impact_bundle",
];
/// Public tools declared read-only at the MCP hint layer. Frigg treats session state and ignored
/// `.frigg/` state changes as read-only for this source-safety contract.
#[cfg(feature = "playbook")]
pub const PUBLIC_READ_ONLY_TOOL_NAMES: &[&str] = &[
    "workspace",
    "list_files",
    "read_file",
    "read_match",
    "explore",
    "search_text",
    "search_hybrid",
    "search_symbol",
    "search_batch",
    "find_references",
    "go_to_definition",
    "find_declarations",
    "find_implementations",
    "incoming_calls",
    "outgoing_calls",
    "document_symbols",
    "inspect_syntax_tree",
    "search_structural",
    "impact_bundle",
    "playbook_run",
    "playbook_replay",
    "playbook_compose_citations",
];
/// Public tools declared read-only at the MCP hint layer for builds without playbook tools.
#[cfg(not(feature = "playbook"))]
pub const PUBLIC_READ_ONLY_TOOL_NAMES: &[&str] = &[
    "workspace",
    "list_files",
    "read_file",
    "read_match",
    "explore",
    "search_text",
    "search_hybrid",
    "search_symbol",
    "search_batch",
    "find_references",
    "go_to_definition",
    "find_declarations",
    "find_implementations",
    "incoming_calls",
    "outgoing_calls",
    "document_symbols",
    "inspect_syntax_tree",
    "search_structural",
    "impact_bundle",
];
/// Public tools whose behavior depends on per-session workspace attachment state.
pub const PUBLIC_SESSION_STATEFUL_TOOL_NAMES: [&str; 1] = ["workspace"];
/// Public tools that can change ignored `.frigg/` maintenance state and therefore require confirmation.
pub const PUBLIC_WRITE_TOOL_NAMES: [&str; 0] = [];
/// Request parameter name used by write-capable tools to require explicit confirmation.
pub const WRITE_CONFIRM_PARAM: &str = "confirm";
/// Stable MCP error code returned when a write-capable tool lacks required confirmation.
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

/// JSON Schema helper for optional `metadata` fields on MCP read responses.
pub fn metadata_object_field_schema(generator: &mut SchemaGenerator) -> Schema {
    MetadataObject::json_schema(generator)
}

/// Response detail profile for search and navigation tools that support compact versus full payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    Compact,
    Full,
}

/// Presentation contract for bounded file reads: plain MCP text bytes versus structured JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReadPresentationMode {
    /// Return only the selected source bytes as MCP text `content[]`. Do not prepend headers or
    /// attach `structuredContent`; request JSON mode for path, line, byte, or efficiency metadata.
    Text,
    /// Return a structured JSON object with explicit `content`, path, byte, and metadata fields.
    Json,
    /// Return MCP text with `LINE|content` prefixes for citation-trained agents (`FUT-012`).
    /// Line numbers are 1-based and align with the selected window's `start_line`.
    Citation,
}

#[path = "types/navigation.rs"]
mod navigation;
#[cfg(feature = "playbook")]
#[path = "types/playbook.rs"]
mod playbook;
#[path = "types/recovery.rs"]
mod recovery;
#[path = "types/repository.rs"]
mod repository;
#[path = "types/search.rs"]
mod search;
#[path = "types/workspace.rs"]
mod workspace;

pub use navigation::*;
#[cfg(feature = "playbook")]
pub use playbook::*;
pub use recovery::*;
pub use repository::*;
pub use search::*;
pub use workspace::*;
