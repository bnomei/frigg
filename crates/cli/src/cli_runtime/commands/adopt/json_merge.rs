pub(crate) const DEFAULT_MCP_SERVER_URL: &str = "http://127.0.0.1:37444/mcp";
pub(crate) const MCP_SERVER_KEY: &str = "frigg";

use serde_json::{Value, json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpEntryState {
    Missing,
    Desired,
    Diverged,
}

pub(crate) fn desired_mcp_server() -> Value {
    json!({
        "type": "http",
        "url": DEFAULT_MCP_SERVER_URL,
    })
}

pub(crate) fn classify_mcp_entry(contents: &str) -> Result<McpEntryState, serde_json::Error> {
    let value: Value = serde_json::from_str(contents)?;
    let Some(servers) = value.get("mcpServers").and_then(Value::as_object) else {
        return Ok(McpEntryState::Missing);
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

#[cfg(test)]
pub(crate) fn desired_mcp_config() -> Value {
    use serde_json::Map;

    let mut servers = Map::new();
    servers.insert(MCP_SERVER_KEY.to_owned(), desired_mcp_server());

    let mut root = Map::new();
    root.insert("mcpServers".to_owned(), Value::Object(servers));
    Value::Object(root)
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_MCP_SERVER_URL, MCP_SERVER_KEY, McpEntryState, classify_mcp_entry,
        desired_mcp_config, desired_mcp_server,
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
}
