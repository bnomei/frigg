#![allow(clippy::panic)]

//! Integration smoke tests for RMCP's service/protocol layer around Frigg's server handler.

use frigg::mcp::FriggMcpServer;
use frigg::settings::FriggConfig;
use rmcp::{
    ClientHandler, ServiceError, ServiceExt,
    model::{
        ClientInfo, ContentBlock, ErrorCode, ErrorData, GetPromptRequestParams, ProtocolVersion,
        ReadResourceRequestParams, ResourceContents, Tool,
    },
};
use serde_json::Value;

const SUPPORT_MATRIX_RESOURCE_URI: &str = "frigg://policy/support-matrix.json";
const SEMANTIC_MODELS_RESOURCE_URI: &str = "frigg://policy/semantic-models.json";
const SHELL_REPLACEMENT_MAP_RESOURCE_URI: &str = "frigg://policy/shell-replacement-map.json";
const ROUTING_GUIDE_PROMPT_NAME: &str = "frigg-routing-guide";
const MISSING_POLICY_RESOURCE_URI: &str = "frigg://policy/missing.json";
const MAX_TOOL_DESCRIPTION_CHARS: usize = 140;
const MAX_SCHEMA_DESCRIPTION_CHARS: usize = 180;

#[derive(Debug, Clone)]
struct VersionedClient {
    protocol_version: ProtocolVersion,
}

impl ClientHandler for VersionedClient {
    fn get_info(&self) -> ClientInfo {
        let mut info = ClientInfo::default();
        info.protocol_version = self.protocol_version.clone();
        info
    }
}

#[tokio::test]
async fn rmcp_service_routes_policy_resources_and_prompts() {
    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config must be valid");
    let server = FriggMcpServer::new(config);
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);

    let server_handle = tokio::spawn(async move {
        let service = server
            .serve(server_transport)
            .await
            .expect("server should initialize over duplex transport");
        service
            .waiting()
            .await
            .expect("server service should finish cleanly");
    });

    let client =
        ().serve(client_transport)
            .await
            .expect("client should initialize over duplex transport");

    let tools = client
        .list_tools(None)
        .await
        .expect("tools/list should route through RMCP");
    assert_tools_list_descriptions_are_concise(&tools.tools);
    assert_collection_contracts_are_discoverable(&tools.tools);

    let read_file = tool_named(&tools.tools, "read_file");
    let read_file_annotations = read_file
        .annotations
        .as_ref()
        .expect("read_file should publish annotations");
    assert_eq!(read_file_annotations.read_only_hint, Some(true));
    assert_eq!(read_file_annotations.destructive_hint, Some(false));
    assert_eq!(read_file_annotations.idempotent_hint, Some(true));
    assert_schema_object(&read_file.input_schema, "read_file inputSchema");

    let workspace = tool_named(&tools.tools, "workspace");
    let workspace_annotations = workspace
        .annotations
        .as_ref()
        .expect("workspace should publish annotations");
    assert_eq!(workspace_annotations.read_only_hint, Some(true));
    assert_eq!(workspace_annotations.destructive_hint, Some(false));
    assert_eq!(workspace_annotations.idempotent_hint, Some(false));
    assert_schema_object(&workspace.input_schema, "workspace inputSchema");

    let document_symbols = tool_named(&tools.tools, "document_symbols");
    let output_schema = document_symbols
        .output_schema
        .as_ref()
        .expect("document_symbols should publish outputSchema");
    assert_schema_object(output_schema, "document_symbols outputSchema");
    let metadata_schema = output_schema
        .get("properties")
        .and_then(Value::as_object)
        .and_then(|properties| properties.get("metadata"))
        .expect("document_symbols outputSchema should include metadata");
    assert_eq!(
        metadata_schema.get("type"),
        Some(&Value::String("object".to_owned())),
        "document_symbols metadata outputSchema should be an object"
    );

    let resources = client
        .list_resources(None)
        .await
        .expect("resources/list should route through RMCP");
    assert!(
        resources
            .resources
            .iter()
            .any(|resource| resource.uri == SUPPORT_MATRIX_RESOURCE_URI),
        "resources/list should expose the support matrix resource"
    );
    assert!(
        resources
            .resources
            .iter()
            .any(|resource| resource.uri == SEMANTIC_MODELS_RESOURCE_URI),
        "resources/list should expose the semantic-models scoreboard resource"
    );
    assert!(
        resources
            .resources
            .iter()
            .any(|resource| resource.uri == SHELL_REPLACEMENT_MAP_RESOURCE_URI),
        "resources/list should expose the shell replacement map resource"
    );

    let support_matrix = client
        .read_resource(ReadResourceRequestParams::new(SUPPORT_MATRIX_RESOURCE_URI))
        .await
        .expect("resources/read should route through RMCP");
    let ResourceContents::TextResourceContents { uri, text, .. } = &support_matrix.contents[0]
    else {
        panic!("support matrix should be returned as text resource contents");
    };
    assert_eq!(uri, SUPPORT_MATRIX_RESOURCE_URI);
    assert!(
        text.contains("\"schema_id\": \"frigg.policy.support_matrix.v4\""),
        "support matrix should contain the expected policy schema"
    );

    let semantic_models = client
        .read_resource(ReadResourceRequestParams::new(SEMANTIC_MODELS_RESOURCE_URI))
        .await
        .expect("resources/read should route semantic-models through RMCP");
    let ResourceContents::TextResourceContents { uri, text, .. } = &semantic_models.contents[0]
    else {
        panic!("semantic models should be returned as text resource contents");
    };
    assert_eq!(uri, SEMANTIC_MODELS_RESOURCE_URI);
    assert!(
        text.contains("\"schema_id\": \"frigg.policy.semantic_models.v1\""),
        "semantic models should contain the expected policy schema"
    );
    assert!(
        text.contains("\"quality_scores\": \"curated\""),
        "semantic models must use curated catalog quality, not a live leaderboard"
    );

    let replacement_map = client
        .read_resource(ReadResourceRequestParams::new(
            SHELL_REPLACEMENT_MAP_RESOURCE_URI,
        ))
        .await
        .expect("resources/read should route shell replacement map through RMCP");
    let ResourceContents::TextResourceContents { uri, text, .. } = &replacement_map.contents[0]
    else {
        panic!("shell replacement map should be returned as text resource contents");
    };
    assert_eq!(uri, SHELL_REPLACEMENT_MAP_RESOURCE_URI);
    assert!(
        text.contains("\"schema_id\": \"frigg.policy.shell_replacement_map.v1\""),
        "shell replacement map should contain the expected policy schema"
    );

    let prompts = client
        .list_prompts(None)
        .await
        .expect("prompts/list should route through RMCP");
    assert!(
        prompts
            .prompts
            .iter()
            .any(|prompt| prompt.name == ROUTING_GUIDE_PROMPT_NAME),
        "prompts/list should expose the routing guide prompt"
    );

    let routing_prompt = client
        .get_prompt(GetPromptRequestParams::new(ROUTING_GUIDE_PROMPT_NAME))
        .await
        .expect("prompts/get should route through RMCP");
    assert!(
        routing_prompt.messages.iter().any(|message| {
            matches!(
                &message.content,
                ContentBlock::ResourceLink(resource)
                    if resource.uri == SUPPORT_MATRIX_RESOURCE_URI
            )
        }),
        "routing prompt should link the support matrix resource"
    );

    client.cancel().await.expect("client should cancel");
    server_handle.await.expect("server task should join");
}

#[tokio::test]
async fn rmcp_resource_not_found_error_code_follows_protocol_version() {
    let legacy_error = read_missing_policy_resource_error(ProtocolVersion::V_2025_11_25).await;
    assert_eq!(legacy_error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(
        structured_error_code(&legacy_error),
        Some("resource_not_found"),
        "legacy clients should still receive Frigg's structured error class"
    );

    let sep_2164_error = read_missing_policy_resource_error(ProtocolVersion::V_2026_07_28).await;
    assert_eq!(sep_2164_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        structured_error_code(&sep_2164_error),
        Some("resource_not_found"),
        "newer clients get RMCP's remapped JSON-RPC code but keep Frigg's structured error class"
    );
}

async fn read_missing_policy_resource_error(protocol_version: ProtocolVersion) -> ErrorData {
    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config must be valid");
    let server = FriggMcpServer::new(config);
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);

    let server_handle = tokio::spawn(async move {
        let service = server
            .serve(server_transport)
            .await
            .expect("server should initialize over duplex transport");
        service
            .waiting()
            .await
            .expect("server service should finish cleanly");
    });

    let client = VersionedClient { protocol_version }
        .serve(client_transport)
        .await
        .expect("client should initialize over duplex transport");

    let error = client
        .read_resource(ReadResourceRequestParams::new(MISSING_POLICY_RESOURCE_URI))
        .await
        .expect_err("missing policy resource should error");

    client.cancel().await.expect("client should cancel");
    server_handle.await.expect("server task should join");

    match error {
        ServiceError::McpError(data) => data,
        other => panic!("expected MCP error for missing policy resource, got: {other:?}"),
    }
}

fn tool_named<'a>(tools: &'a [Tool], name: &str) -> &'a Tool {
    tools
        .iter()
        .find(|tool| tool.name == name)
        .unwrap_or_else(|| panic!("tools/list should expose `{name}`"))
}

fn assert_schema_object(schema: &serde_json::Map<String, Value>, label: &str) {
    assert_eq!(
        schema.get("type"),
        Some(&Value::String("object".to_owned())),
        "{label} should be an object schema"
    );
}

fn assert_tools_list_descriptions_are_concise(tools: &[Tool]) {
    for tool in tools {
        let description = tool.description.as_deref().unwrap_or("");
        assert!(
            description.chars().count() <= MAX_TOOL_DESCRIPTION_CHARS,
            "tool `{}` description is too long: {description}",
            tool.name
        );

        assert_schema_descriptions_are_concise(
            &Value::Object(tool.input_schema.as_ref().clone()),
            &format!("{} inputSchema", tool.name),
        );
        if let Some(output_schema) = &tool.output_schema {
            assert_schema_descriptions_are_concise(
                &Value::Object(output_schema.as_ref().clone()),
                &format!("{} outputSchema", tool.name),
            );
        }
    }
}

/// Keep the public `tools/list` contract aligned with the canonical completeness envelope.
/// This covers the schema projection, the v2 continuation input surface, and the concise
/// descriptions agents receive before issuing a bounded collection request.
fn assert_collection_contracts_are_discoverable(tools: &[Tool]) {
    const PAGED_COLLECTIONS: &[&str] = &[
        "list_files",
        "explore",
        "search_text",
        "search_symbol",
        "search_batch",
        "find_references",
        "go_to_definition",
        "find_declarations",
        "find_implementations",
        "incoming_calls",
        "outgoing_calls",
        "document_symbols",
        "search_structural",
    ];
    const COMPLETENESS_OUTPUTS: &[(&str, &[&str])] = &[
        ("list_files", &["completeness"]),
        ("explore", &["completeness"]),
        ("search_text", &["completeness"]),
        ("search_hybrid", &["completeness"]),
        ("search_symbol", &["completeness"]),
        ("search_batch", &["completeness"]),
        ("find_references", &["completeness"]),
        ("go_to_definition", &["completeness"]),
        ("find_declarations", &["completeness"]),
        ("find_implementations", &["completeness"]),
        ("incoming_calls", &["completeness"]),
        ("outgoing_calls", &["completeness"]),
        ("document_symbols", &["completeness"]),
        (
            "inspect_syntax_tree",
            &["ancestors_completeness", "children_completeness"],
        ),
        ("search_structural", &["completeness"]),
        (
            "impact_bundle",
            &[
                "symbols_completeness",
                "references_completeness",
                "incoming_calls_completeness",
                "completeness",
            ],
        ),
    ];

    for tool_name in PAGED_COLLECTIONS {
        let tool = tool_named(tools, tool_name);
        let input_properties =
            schema_properties(&tool.input_schema, &format!("{tool_name} inputSchema"));
        assert!(
            input_properties.contains_key("continuation"),
            "{tool_name} must expose the canonical v2 continuation input"
        );
        let description = tool.description.as_deref().unwrap_or("");
        assert!(
            description.contains("completeness") && description.contains("continuation"),
            "{tool_name} description must name canonical completeness and continuation behavior: {description}"
        );
    }

    for (tool_name, required_properties) in COMPLETENESS_OUTPUTS {
        let tool = tool_named(tools, tool_name);
        let output_schema = tool
            .output_schema
            .as_ref()
            .unwrap_or_else(|| panic!("{tool_name} should publish an outputSchema"));
        let output_properties =
            schema_properties(output_schema, &format!("{tool_name} outputSchema"));
        for property in *required_properties {
            assert!(
                output_properties.contains_key(*property),
                "{tool_name} outputSchema must expose `{property}`"
            );
        }
    }

    let hybrid_description = tool_named(tools, "search_hybrid")
        .description
        .as_deref()
        .unwrap_or("");
    assert!(
        hybrid_description.contains("incomplete")
            && hybrid_description.contains("non-exhaustive")
            && hybrid_description.contains("continuation"),
        "search_hybrid description must not overclaim exhaustive continuation coverage: {hybrid_description}"
    );

    for tool_name in ["inspect_syntax_tree", "impact_bundle", "search_batch"] {
        let description = tool_named(tools, tool_name)
            .description
            .as_deref()
            .unwrap_or("");
        assert!(
            description.contains("completeness"),
            "{tool_name} description must disclose bounded collection completeness: {description}"
        );
    }
}

fn schema_properties<'a>(
    schema: &'a serde_json::Map<String, Value>,
    label: &str,
) -> &'a serde_json::Map<String, Value> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{label} should define object properties"))
}

fn assert_schema_descriptions_are_concise(value: &Value, label: &str) {
    match value {
        Value::Object(map) => {
            if let Some(description) = map.get("description").and_then(Value::as_str) {
                assert!(
                    description.chars().count() <= MAX_SCHEMA_DESCRIPTION_CHARS,
                    "{label} has an overlong schema description: {description}"
                );
            }
            for (key, child) in map {
                assert_schema_descriptions_are_concise(child, &format!("{label}.{key}"));
            }
        }
        Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                assert_schema_descriptions_are_concise(child, &format!("{label}[{index}]"));
            }
        }
        _ => {}
    }
}

fn structured_error_code(error: &ErrorData) -> Option<&str> {
    error.data.as_ref()?.get("error_code")?.as_str()
}
