//! Dogfood-shaped anchor: public MCP tool manifest (fixture, not live product).
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
];

pub const PUBLIC_READ_ONLY_TOOL_NAMES: &[&str] = PUBLIC_TOOL_NAMES;
pub const TOOL_SURFACE_PROFILE_ENV: &str = "FRIGG_MCP_TOOL_SURFACE_PROFILE";
