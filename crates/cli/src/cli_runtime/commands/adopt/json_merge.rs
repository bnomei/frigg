use serde_json::{Map, Value, json};

pub(crate) const DEFAULT_MCP_SERVER_URL: &str = "http://127.0.0.1:37444/mcp";
pub(crate) const MCP_SERVER_KEY: &str = "frigg";
const MCP_SERVERS_KEY: &str = "mcpServers";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpEntryState {
    Missing,
    Desired,
    Diverged,
}

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

pub(crate) fn desired_mcp_server() -> Value {
    json!({
        "type": "http",
        "url": DEFAULT_MCP_SERVER_URL,
    })
}

pub(crate) fn classify_mcp_entry(contents: &str) -> Result<McpEntryState, McpJsonError> {
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

    if *existing == desired_mcp_server() {
        Ok(McpEntryState::Desired)
    } else {
        Ok(McpEntryState::Diverged)
    }
}

pub(crate) fn desired_mcp_config() -> Value {
    let mut servers = Map::new();
    servers.insert(MCP_SERVER_KEY.to_owned(), desired_mcp_server());

    let mut root = Map::new();
    root.insert(MCP_SERVERS_KEY.to_owned(), Value::Object(servers));
    Value::Object(root)
}

pub(crate) fn upsert_mcp_server(
    contents: Option<&str>,
    force: bool,
) -> Result<McpJsonEdit, McpJsonError> {
    let Some(contents) = contents else {
        return serialize_changed(desired_mcp_config());
    };

    let mut root = parse_object_root(contents)?;
    let servers = ensure_servers_object(&mut root)?;
    match servers.get(MCP_SERVER_KEY) {
        Some(existing) if *existing == desired_mcp_server() => return Ok(McpJsonEdit::Unchanged),
        Some(_) if !force => return Ok(McpJsonEdit::Skipped),
        _ => {}
    }

    servers.insert(MCP_SERVER_KEY.to_owned(), desired_mcp_server());
    serialize_if_changed(Value::Object(root), contents)
}

pub(crate) fn remove_mcp_server(contents: &str, force: bool) -> Result<McpJsonEdit, McpJsonError> {
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
        Some(existing) if *existing == desired_mcp_server() || force => {}
        Some(_) => return Ok(McpJsonEdit::Skipped),
        None => return Ok(McpJsonEdit::Unchanged),
    }

    servers.remove(MCP_SERVER_KEY);
    serialize_if_changed(Value::Object(root), contents)
}

fn parse_object_root(contents: &str) -> Result<Map<String, Value>, McpJsonError> {
    let value: Value = serde_json::from_str(contents).map_err(McpJsonError::Parse)?;
    value.as_object().cloned().ok_or(McpJsonError::InvalidShape(
        "MCP config root must be a JSON object",
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
    use super::{
        DEFAULT_MCP_SERVER_URL, MCP_SERVER_KEY, McpEntryState, McpJsonEdit, classify_mcp_entry,
        desired_mcp_config, desired_mcp_server, remove_mcp_server, upsert_mcp_server,
    };

    #[test]
    fn adopt_json_merge_defaults_to_loopback_http() {
        assert_eq!(MCP_SERVER_KEY, "frigg");
        assert_eq!(DEFAULT_MCP_SERVER_URL, "http://127.0.0.1:37444/mcp");
    }

    #[test]
    fn adopt_json_merge_desired_config_has_frigg_server_key() {
        let config = desired_mcp_config();

        assert_eq!(
            config["mcpServers"][MCP_SERVER_KEY],
            desired_mcp_server(),
            "desired config should contain the fixed Frigg MCP entry"
        );
    }

    #[test]
    fn adopt_json_merge_classifies_missing_desired_and_diverged_entries() {
        assert_eq!(
            classify_mcp_entry(r#"{"mcpServers":{"other":{"url":"http://localhost"}}}"#)
                .expect("parse missing"),
            McpEntryState::Missing
        );
        assert_eq!(
            classify_mcp_entry(
                r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:37444/mcp"}}}"#
            )
            .expect("parse desired"),
            McpEntryState::Desired
        );
        assert_eq!(
            classify_mcp_entry(r#"{"mcpServers":{"frigg":{"command":"frigg"}}}"#)
                .expect("parse diverged"),
            McpEntryState::Diverged
        );
    }

    #[test]
    fn adopt_json_merge_creates_missing_config() {
        assert_eq!(
            upsert_mcp_server(None, false).expect("create config"),
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
        )
        .expect("merge config");

        let McpJsonEdit::Changed(updated) = edit else {
            panic!("expected changed edit");
        };
        let value: serde_json::Value = serde_json::from_str(&updated).expect("parse updated");
        assert_eq!(value["unrelated"], true);
        assert_eq!(value["mcpServers"]["other"]["command"], "other");
        assert_eq!(value["mcpServers"][MCP_SERVER_KEY], desired_mcp_server());
    }

    #[test]
    fn adopt_json_merge_skips_diverged_frigg_without_force() {
        assert_eq!(
            upsert_mcp_server(
                Some(r#"{"mcpServers":{"frigg":{"command":"frigg"}}}"#),
                false
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
        )
        .expect("force merge config");

        let McpJsonEdit::Changed(updated) = edit else {
            panic!("expected changed edit");
        };
        let value: serde_json::Value = serde_json::from_str(&updated).expect("parse updated");
        assert_eq!(value["mcpServers"]["frigg"], desired_mcp_server());
        assert_eq!(value["mcpServers"]["other"]["url"], "x");
    }

    #[test]
    fn adopt_json_merge_removes_only_frigg_on_uninstall() {
        let edit = remove_mcp_server(
            r#"{"mcpServers":{"frigg":{"type":"http","url":"http://127.0.0.1:37444/mcp"},"other":{"url":"x"}},"unrelated":1}"#,
            false,
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
        let err = upsert_mcp_server(Some("{not json"), false).expect_err("reject malformed JSON");

        assert!(err.to_string().contains("invalid JSON"));
    }

    #[test]
    fn adopt_json_merge_rejects_non_object_root() {
        let err = upsert_mcp_server(Some("[]"), false).expect_err("reject non-object root");

        assert_eq!(err.to_string(), "MCP config root must be a JSON object");
    }

    #[test]
    fn adopt_json_merge_rejects_non_object_mcp_servers() {
        let err = upsert_mcp_server(Some(r#"{"mcpServers":[]}"#), false)
            .expect_err("reject non-object mcpServers");

        assert_eq!(
            err.to_string(),
            "mcpServers must be a JSON object when present"
        );
    }
}
