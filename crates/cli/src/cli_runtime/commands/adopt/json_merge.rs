pub(crate) const DEFAULT_MCP_SERVER_URL: &str = "http://127.0.0.1:37444/mcp";
pub(crate) const MCP_SERVER_KEY: &str = "frigg";

#[cfg(test)]
mod tests {
    use super::{DEFAULT_MCP_SERVER_URL, MCP_SERVER_KEY};

    #[test]
    fn adopt_json_merge_defaults_to_loopback_http() {
        assert_eq!(MCP_SERVER_KEY, "frigg");
        assert_eq!(DEFAULT_MCP_SERVER_URL, "http://127.0.0.1:37444/mcp");
    }
}
