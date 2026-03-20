//! Wire contracts for Frigg's public MCP tool surface. These types keep workspace lifecycle,
//! search, navigation, and health-reporting semantics explicit so server code, tests, and schema
//! generation all describe the same external API.

pub const PUBLIC_TOOL_NAMES: [&str; 23] = [
    "list_repositories",
    "workspace_attach",
    "workspace_detach",
    "workspace_prepare",
    "workspace_reindex",
    "workspace_current",
    "read_file",
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
pub const PUBLIC_READ_ONLY_TOOL_NAMES: [&str; 19] = [
    "list_repositories",
    "workspace_current",
    "read_file",
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
