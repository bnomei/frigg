//! JSON merge helpers for Frigg MCP server entries and Claude PreToolUse hooks in project settings.
//!
//! Merges Frigg MCP server entries and Claude PreToolUse hooks while preserving unrelated project
//! JSON keys.

use serde_json::{Map, Value, json};

#[cfg(test)]
pub(crate) const DEFAULT_MCP_SERVER_URL: &str = "http://127.0.0.1:37444/mcp";
pub(crate) const MCP_SERVER_KEY: &str = "frigg";
const MCP_SERVERS_KEY: &str = "mcpServers";
const CLAUDE_HOOKS_KEY: &str = "hooks";
const CLAUDE_PRE_TOOL_USE_KEY: &str = "PreToolUse";
const CLAUDE_HOOK_MATCHER: &str = "Grep|Bash|Read";
const CLAUDE_HOOK_COMMAND: &str = "frigg hook pretooluse";

/// Classifies whether the desired Frigg MCP server entry is absent, current, or user-diverged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpEntryState {
    Missing,
    Desired,
    Diverged,
}

/// Classifies whether the desired Claude PreToolUse hook command is absent or already installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClaudeHookState {
    Missing,
    Desired,
}

/// Outcome of a JSON merge or removal attempt against an adopt target file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum McpJsonEdit {
    Changed(String),
    Unchanged,
    Skipped,
}

#[derive(Debug)]
pub(crate) enum McpJsonError {
    Parse(serde_json::Error),
    InvalidShape(&'static str),
    Serialize(serde_json::Error),
}

impl std::fmt::Display for McpJsonError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Parse(err) => write!(formatter, "invalid JSON: {err}"),
            Self::InvalidShape(message) => formatter.write_str(message),
            Self::Serialize(err) => write!(formatter, "JSON serialization failed: {err}"),
        }
    }
}

impl std::error::Error for McpJsonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Parse(err) | Self::Serialize(err) => Some(err),
            Self::InvalidShape(_) => None,
        }
    }
}

/// Returns the canonical Frigg MCP HTTP server entry written by adopt.
pub(crate) fn desired_mcp_server(mcp_server_url: &str) -> Value {
    json!({
        "type": "http",
        "url": mcp_server_url,
    })
}

/// Classifies the Frigg MCP server entry in existing `.mcp.json` or Cursor MCP config contents.
pub(crate) fn classify_mcp_entry(
    contents: &str,
    mcp_server_url: &str,
) -> Result<McpEntryState, McpJsonError> {
    let value: Value = serde_json::from_str(contents).map_err(McpJsonError::Parse)?;
    let root = value.as_object().ok_or(McpJsonError::InvalidShape(
        "MCP config root must be a JSON object",
    ))?;
    let Some(servers) = root.get(MCP_SERVERS_KEY) else {
        return Ok(McpEntryState::Missing);
    };
    let Some(servers) = servers.as_object() else {
        return Err(McpJsonError::InvalidShape(
            "mcpServers must be a JSON object when present",
        ));
    };
    let Some(existing) = servers.get(MCP_SERVER_KEY) else {
        return Ok(McpEntryState::Missing);
    };

    if *existing == desired_mcp_server(mcp_server_url) {
        Ok(McpEntryState::Desired)
    } else {
        Ok(McpEntryState::Diverged)
    }
}

pub(crate) fn desired_mcp_config(mcp_server_url: &str) -> Value {
    let mut servers = Map::new();
    servers.insert(
        MCP_SERVER_KEY.to_owned(),
        desired_mcp_server(mcp_server_url),
    );

    let mut root = Map::new();
    root.insert(MCP_SERVERS_KEY.to_owned(), Value::Object(servers));
    Value::Object(root)
}

/// Inserts or updates the Frigg MCP server entry while preserving unrelated JSON keys.
pub(crate) fn upsert_mcp_server(
    contents: Option<&str>,
    force: bool,
    mcp_server_url: &str,
) -> Result<McpJsonEdit, McpJsonError> {
    let Some(contents) = contents else {
        return serialize_changed(desired_mcp_config(mcp_server_url));
    };

    let mut root = parse_object_root(contents)?;
    let servers = ensure_servers_object(&mut root)?;
    match servers.get(MCP_SERVER_KEY) {
        Some(existing) if *existing == desired_mcp_server(mcp_server_url) => {
            return Ok(McpJsonEdit::Unchanged);
        }
        Some(_) if !force => return Ok(McpJsonEdit::Skipped),
        _ => {}
    }

    servers.insert(
        MCP_SERVER_KEY.to_owned(),
        desired_mcp_server(mcp_server_url),
    );
    serialize_if_changed(Value::Object(root), contents)
}

/// Removes the Frigg MCP server entry, skipping diverged entries unless `force` is set.
pub(crate) fn remove_mcp_server(
    contents: &str,
    force: bool,
    mcp_server_url: &str,
) -> Result<McpJsonEdit, McpJsonError> {
    let mut root = parse_object_root(contents)?;
    let Some(servers) = root.get_mut(MCP_SERVERS_KEY) else {
        return Ok(McpJsonEdit::Unchanged);
    };
    let Some(servers) = servers.as_object_mut() else {
        return Err(McpJsonError::InvalidShape(
            "mcpServers must be a JSON object when present",
        ));
    };

    match servers.get(MCP_SERVER_KEY) {
        Some(existing) if *existing == desired_mcp_server(mcp_server_url) || force => {}
        Some(_) => return Ok(McpJsonEdit::Skipped),
        None => return Ok(McpJsonEdit::Unchanged),
    }

    servers.remove(MCP_SERVER_KEY);
    serialize_if_changed(Value::Object(root), contents)
}

pub(crate) fn desired_claude_hook_command() -> Value {
    json!({
        "type": "command",
        "command": CLAUDE_HOOK_COMMAND,
        "timeout": 5,
    })
}

fn desired_claude_pre_tool_use_entry() -> Value {
    json!({
        "matcher": CLAUDE_HOOK_MATCHER,
        "hooks": [desired_claude_hook_command()],
    })
}

/// Classifies whether the Frigg PreToolUse hook is present in Claude settings JSON.
pub(crate) fn classify_claude_hook(contents: &str) -> Result<ClaudeHookState, McpJsonError> {
    let root = parse_object_root(contents)?;
    if contains_desired_claude_hook(&Value::Object(root)) {
        Ok(ClaudeHookState::Desired)
    } else {
        Ok(ClaudeHookState::Missing)
    }
}

/// Inserts the Frigg PreToolUse hook command while preserving sibling Claude settings and hooks.
pub(crate) fn upsert_claude_hook(contents: Option<&str>) -> Result<McpJsonEdit, McpJsonError> {
    let Some(contents) = contents else {
        return serialize_changed(json!({
            CLAUDE_HOOKS_KEY: {
                CLAUDE_PRE_TOOL_USE_KEY: [desired_claude_pre_tool_use_entry()],
            },
        }));
    };

    let mut root = parse_object_root(contents)?;
    let pre_tool_use = ensure_pre_tool_use_array(&mut root)?;
    if pre_tool_use_contains_desired_hook(pre_tool_use) {
        return Ok(McpJsonEdit::Unchanged);
    }

    if let Some(entry) = pre_tool_use.iter_mut().find(|entry| {
        entry
            .get("matcher")
            .and_then(Value::as_str)
            .is_some_and(|matcher| matcher == CLAUDE_HOOK_MATCHER)
            && entry.get("hooks").is_some_and(Value::is_array)
    }) {
        let hooks = entry
            .get_mut("hooks")
            .and_then(Value::as_array_mut)
            .expect("entry hook array was checked above");
        hooks.push(desired_claude_hook_command());
    } else {
        pre_tool_use.push(desired_claude_pre_tool_use_entry());
    }

    serialize_if_changed(Value::Object(root), contents)
}

pub(crate) fn remove_claude_hook(contents: &str) -> Result<McpJsonEdit, McpJsonError> {
    let mut root = parse_object_root(contents)?;
    let Some(hooks) = root.get_mut(CLAUDE_HOOKS_KEY) else {
        return Ok(McpJsonEdit::Unchanged);
    };
    let hooks = hooks.as_object_mut().ok_or(McpJsonError::InvalidShape(
        "hooks must be a JSON object when present",
    ))?;
    let Some(pre_tool_use) = hooks.get_mut(CLAUDE_PRE_TOOL_USE_KEY) else {
        return Ok(McpJsonEdit::Unchanged);
    };
    let pre_tool_use = pre_tool_use
        .as_array_mut()
        .ok_or(McpJsonError::InvalidShape(
            "hooks.PreToolUse must be a JSON array when present",
        ))?;

    if !pre_tool_use_contains_desired_hook(pre_tool_use) {
        return Ok(McpJsonEdit::Unchanged);
    }

    for entry in pre_tool_use.iter_mut().filter(|entry| {
        entry
            .get("matcher")
            .and_then(Value::as_str)
            .is_some_and(|matcher| matcher == CLAUDE_HOOK_MATCHER)
    }) {
        let Some(hook_commands) = entry.get_mut("hooks").and_then(Value::as_array_mut) else {
            continue;
        };
        hook_commands.retain(|hook| *hook != desired_claude_hook_command());
    }

    serialize_if_changed(Value::Object(root), contents)
}

fn parse_object_root(contents: &str) -> Result<Map<String, Value>, McpJsonError> {
    let value: Value = serde_json::from_str(contents).map_err(McpJsonError::Parse)?;
    value.as_object().cloned().ok_or(McpJsonError::InvalidShape(
        "MCP config root must be a JSON object",
    ))
}

fn contains_desired_claude_hook(root: &Value) -> bool {
    root.get(CLAUDE_HOOKS_KEY)
        .and_then(|hooks| hooks.get(CLAUDE_PRE_TOOL_USE_KEY))
        .and_then(Value::as_array)
        .is_some_and(|pre_tool_use| pre_tool_use_contains_desired_hook(pre_tool_use))
}

fn pre_tool_use_contains_desired_hook(pre_tool_use: &[Value]) -> bool {
    pre_tool_use.iter().any(|entry| {
        entry
            .get("matcher")
            .and_then(Value::as_str)
            .is_some_and(|matcher| matcher == CLAUDE_HOOK_MATCHER)
            && entry
                .get("hooks")
                .and_then(Value::as_array)
                .is_some_and(|hooks| {
                    hooks
                        .iter()
                        .any(|hook| *hook == desired_claude_hook_command())
                })
    })
}

fn ensure_pre_tool_use_array(
    root: &mut Map<String, Value>,
) -> Result<&mut Vec<Value>, McpJsonError> {
    if !root.contains_key(CLAUDE_HOOKS_KEY) {
        root.insert(CLAUDE_HOOKS_KEY.to_owned(), Value::Object(Map::new()));
    }

    let hooks = root
        .get_mut(CLAUDE_HOOKS_KEY)
        .and_then(Value::as_object_mut)
        .ok_or(McpJsonError::InvalidShape(
            "hooks must be a JSON object when present",
        ))?;

    if !hooks.contains_key(CLAUDE_PRE_TOOL_USE_KEY) {
        hooks.insert(CLAUDE_PRE_TOOL_USE_KEY.to_owned(), Value::Array(Vec::new()));
    }

    hooks
        .get_mut(CLAUDE_PRE_TOOL_USE_KEY)
        .and_then(Value::as_array_mut)
        .ok_or(McpJsonError::InvalidShape(
            "hooks.PreToolUse must be a JSON array when present",
        ))
}

fn ensure_servers_object(
    root: &mut Map<String, Value>,
) -> Result<&mut Map<String, Value>, McpJsonError> {
    if !root.contains_key(MCP_SERVERS_KEY) {
        root.insert(MCP_SERVERS_KEY.to_owned(), Value::Object(Map::new()));
    }

    root.get_mut(MCP_SERVERS_KEY)
        .and_then(Value::as_object_mut)
        .ok_or(McpJsonError::InvalidShape(
            "mcpServers must be a JSON object when present",
        ))
}

fn serialize_changed(value: Value) -> Result<McpJsonEdit, McpJsonError> {
    serialize_value(value).map(McpJsonEdit::Changed)
}

fn serialize_if_changed(value: Value, original: &str) -> Result<McpJsonEdit, McpJsonError> {
    let serialized = serialize_value(value)?;
    if serialized == original {
        Ok(McpJsonEdit::Unchanged)
    } else {
        Ok(McpJsonEdit::Changed(serialized))
    }
}

fn serialize_value(value: Value) -> Result<String, McpJsonError> {
    let mut serialized = serde_json::to_string_pretty(&value).map_err(McpJsonError::Serialize)?;
    serialized.push('\n');
    Ok(serialized)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic, clippy::unwrap_used)]

    use serde_json::json;

    use super::{
        DEFAULT_MCP_SERVER_URL, MCP_SERVER_KEY, McpEntryState, McpJsonEdit, classify_claude_hook,
        classify_mcp_entry, desired_claude_hook_command, desired_mcp_config, desired_mcp_server,
        remove_claude_hook, remove_mcp_server, upsert_claude_hook, upsert_mcp_server,
    };

    #[test]
    fn adopt_json_merge_defaults_to_loopback_http() {
        assert_eq!(MCP_SERVER_KEY, "frigg");
        assert_eq!(DEFAULT_MCP_SERVER_URL, "http://127.0.0.1:37444/mcp");
    }

    #[test]
    fn adopt_json_merge_desired_config_has_frigg_server_key() {
        let config = desired_mcp_config(DEFAULT_MCP_SERVER_URL);

        assert_eq!(
            config["mcpServers"][MCP_SERVER_KEY],
            desired_mcp_server(DEFAULT_MCP_SERVER_URL),
            "desired config should contain the fixed Frigg MCP entry"
        );
    }

    #[test]
    fn desired_mcp_server_uses_resolved_http_endpoint_url() {
        let custom_url = "http://127.0.0.1:5000/mcp";
        assert_eq!(
            desired_mcp_server(custom_url),
            json!({
                "type": "http",
                "url": custom_url,
            })
        );
        assert_eq!(
            upsert_mcp_server(None, false, custom_url).expect("create custom config"),
            McpJsonEdit::Changed(
                "{\n  \"mcpServers\": {\n    \"frigg\": {\n      \"type\": \"http\",\n      \"url\": \"http://127.0.0.1:5000/mcp\"\n    }\n  }\n}\n"
                    .to_owned()
            )
        );
    }

    #[test]
    fn adopt_json_merge_classifies_missing_desired_and_diverged_entries() {
        assert_eq!(
            classify_mcp_entry(
                r#"{"mcpServers":{"other":{"url":"http://localhost"}}}"#,
                DEFAULT_MCP_SERVER_URL
            )
            .expect("parse missing"),
            McpEntryState::Missing
        );
        assert_eq!(
            classify_mcp_entry(
                r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:37444/mcp"}}}"#,
                DEFAULT_MCP_SERVER_URL
            )
            .expect("parse desired"),
            McpEntryState::Desired
        );
        assert_eq!(
            classify_mcp_entry(
                r#"{"mcpServers":{"frigg":{"command":"frigg"}}}"#,
                DEFAULT_MCP_SERVER_URL
            )
            .expect("parse diverged"),
            McpEntryState::Diverged
        );
    }

    #[test]
    fn adopt_json_merge_creates_missing_config() {
        assert_eq!(
            upsert_mcp_server(None, false, DEFAULT_MCP_SERVER_URL).expect("create config"),
            McpJsonEdit::Changed(
                "{\n  \"mcpServers\": {\n    \"frigg\": {\n      \"type\": \"http\",\n      \"url\": \"http://127.0.0.1:37444/mcp\"\n    }\n  }\n}\n"
                    .to_owned()
            )
        );
    }

    #[test]
    fn adopt_json_merge_adds_frigg_and_preserves_siblings() {
        let edit = upsert_mcp_server(
            Some(
                r#"{"unrelated":true,"mcpServers":{"other":{"command":"other","args":["serve"]}}}"#,
            ),
            false,
            DEFAULT_MCP_SERVER_URL,
        )
        .expect("merge config");

        let McpJsonEdit::Changed(updated) = edit else {
            panic!("expected changed edit");
        };
        let value: serde_json::Value = serde_json::from_str(&updated).expect("parse updated");
        assert_eq!(value["unrelated"], true);
        assert_eq!(value["mcpServers"]["other"]["command"], "other");
        assert_eq!(
            value["mcpServers"][MCP_SERVER_KEY],
            desired_mcp_server(DEFAULT_MCP_SERVER_URL)
        );
    }

    #[test]
    fn adopt_json_merge_skips_diverged_frigg_without_force() {
        assert_eq!(
            upsert_mcp_server(
                Some(r#"{"mcpServers":{"frigg":{"command":"frigg"}}}"#),
                false,
                DEFAULT_MCP_SERVER_URL
            )
            .expect("merge config"),
            McpJsonEdit::Skipped
        );
    }

    #[test]
    fn adopt_json_merge_forces_diverged_frigg() {
        let edit = upsert_mcp_server(
            Some(r#"{"mcpServers":{"frigg":{"command":"frigg"},"other":{"url":"x"}}}"#),
            true,
            DEFAULT_MCP_SERVER_URL,
        )
        .expect("force merge config");

        let McpJsonEdit::Changed(updated) = edit else {
            panic!("expected changed edit");
        };
        let value: serde_json::Value = serde_json::from_str(&updated).expect("parse updated");
        assert_eq!(
            value["mcpServers"]["frigg"],
            desired_mcp_server(DEFAULT_MCP_SERVER_URL)
        );
        assert_eq!(value["mcpServers"]["other"]["url"], "x");
    }

    #[test]
    fn adopt_json_merge_removes_only_frigg_on_uninstall() {
        let edit = remove_mcp_server(
            r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:37444/mcp"},"other":{"url":"x"}},"unrelated":1}"#,
            false,
            DEFAULT_MCP_SERVER_URL,
        )
        .expect("remove config");

        let McpJsonEdit::Changed(updated) = edit else {
            panic!("expected changed edit");
        };
        let value: serde_json::Value = serde_json::from_str(&updated).expect("parse updated");
        assert!(value["mcpServers"].get("frigg").is_none());
        assert_eq!(value["mcpServers"]["other"]["url"], "x");
        assert_eq!(value["unrelated"], 1);
    }

    #[test]
    fn adopt_json_merge_rejects_malformed_json_without_output() {
        let err = upsert_mcp_server(Some("{not json"), false, DEFAULT_MCP_SERVER_URL)
            .expect_err("reject malformed JSON");

        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn adopt_json_merge_rejects_non_object_root() {
        let err = upsert_mcp_server(Some("[]"), false, DEFAULT_MCP_SERVER_URL)
            .expect_err("reject non-object root");

        assert_eq!(err.to_string(), "MCP config root must be a JSON object");
    }

    #[test]
    fn adopt_json_merge_rejects_non_object_mcp_servers() {
        let err = upsert_mcp_server(Some(r#"{"mcpServers":[]}"#), false, DEFAULT_MCP_SERVER_URL)
            .expect_err("reject non-object mcpServers");

        assert_eq!(
            err.to_string(),
            "mcpServers must be a JSON object when present"
        );
    }

    #[test]
    fn adopt_json_merge_adds_claude_hook_and_preserves_siblings() {
        let edit = upsert_claude_hook(Some(
            r#"{"theme":"dark","hooks":{"PreToolUse":[{"matcher":"Write","hooks":[{"type":"command","command":"other"}]}],"PostToolUse":[{"matcher":"Bash","hooks":[]}]}}"#,
        ))
        .expect("merge claude settings");

        let McpJsonEdit::Changed(updated) = edit else {
            panic!("expected changed edit");
        };
        let value: serde_json::Value = serde_json::from_str(&updated).expect("parse updated");
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["hooks"]["PostToolUse"][0]["matcher"], "Bash");
        assert_eq!(value["hooks"]["PreToolUse"][0]["matcher"], "Write");
        assert_eq!(value["hooks"]["PreToolUse"][1]["matcher"], "Grep|Bash|Read");
        assert_eq!(
            value["hooks"]["PreToolUse"][1]["hooks"][0],
            desired_claude_hook_command()
        );
    }

    #[test]
    fn adopt_json_merge_claude_hook_is_idempotent() {
        let contents = r#"{"hooks":{"PreToolUse":[{"matcher":"Grep|Bash|Read","hooks":[{"type":"command","command":"frigg hook pretooluse","timeout":5}]}]}}"#;

        assert_eq!(
            classify_claude_hook(contents).expect("classify claude hook"),
            super::ClaudeHookState::Desired
        );
        assert_eq!(
            upsert_claude_hook(Some(contents)).expect("upsert claude hook"),
            McpJsonEdit::Unchanged
        );
    }

    #[test]
    fn adopt_json_merge_removes_only_frigg_claude_hook() {
        let edit = remove_claude_hook(
            r#"{"hooks":{"PreToolUse":[{"matcher":"Grep|Bash|Read","hooks":[{"type":"command","command":"other"},{"type":"command","command":"frigg hook pretooluse","timeout":5}]},{"matcher":"Write","hooks":[{"type":"command","command":"frigg hook pretooluse","timeout":5},{"type":"command","command":"write-hook"}]}]},"unrelated":true}"#,
        )
        .expect("remove claude hook");

        let McpJsonEdit::Changed(updated) = edit else {
            panic!("expected changed edit");
        };
        let value: serde_json::Value = serde_json::from_str(&updated).expect("parse updated");
        assert_eq!(value["unrelated"], true);
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            value["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            "other"
        );
        assert_eq!(value["hooks"]["PreToolUse"][1]["matcher"], "Write");
        assert_eq!(
            value["hooks"]["PreToolUse"][1]["hooks"][0],
            desired_claude_hook_command()
        );
        assert_eq!(
            value["hooks"]["PreToolUse"][1]["hooks"][1]["command"],
            "write-hook"
        );
    }

    #[test]
    fn adopt_json_merge_rejects_malformed_claude_settings_without_output() {
        let err = upsert_claude_hook(Some("{not json")).expect_err("reject malformed JSON");

        assert!(err.to_string().contains("invalid JSON"));
    }
}
