use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::mcp::types::{
    DeepSearchComposeCitationsParams, DeepSearchPlaybookContract, DeepSearchPlaybookStepContract,
    DeepSearchReplayParams, DeepSearchRunParams, ExploreOperation, ExploreParams,
    FindReferencesParams, ListRepositoriesParams, ReadFileParams, SearchHybridParams,
    SearchPatternType, SearchSymbolParams, SearchTextParams,
};
use frigg::mcp::{DeepSearchHarness, FriggMcpServer};
use frigg::settings::FriggConfig;
use frigg::storage::Storage;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;
use serde_json::{Value, json};

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-mcp-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn build_workspace_fixture(test_name: &str) -> PathBuf {
    let root = temp_workspace_root(test_name);
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create workspace fixture source directory");
    fs::write(
        src_dir.join("lib.rs"),
        "pub fn greeting() -> &'static str { \"hello provenance\" }\n",
    )
    .expect("failed to seed fixture source file");
    root
}

fn storage_path_for_workspace(workspace_root: &Path) -> PathBuf {
    workspace_root.join(".frigg").join("storage.sqlite3")
}

fn server_for_workspace(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace fixture should produce valid config");
    FriggMcpServer::new(config)
}

fn deep_search_runtime_server_for_workspace(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace fixture should produce valid config");
    FriggMcpServer::new_with_runtime_options(config, false, true)
}

fn extended_runtime_server_for_workspace(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace fixture should produce valid config");
    FriggMcpServer::new_with_runtime_options(config, false, true)
}

fn build_deep_search_workspace_fixture(test_name: &str) -> PathBuf {
    let root = temp_workspace_root(test_name);
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create deep-search workspace fixture");
    fs::write(
        src_dir.join("lib.rs"),
        "pub fn greeting() -> &'static str { \"hello replay\" }\n\
         pub fn callsite() { let _ = greeting(); }\n",
    )
    .expect("failed to seed deep-search workspace source file");
    root
}

fn deep_search_playbook_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/playbooks/deep-search-replay-basic.playbook.json")
}

fn cleanup_workspace(workspace_root: &Path) {
    let _ = fs::remove_dir_all(workspace_root);
}

fn error_code_tag(error: &rmcp::ErrorData) -> Option<&str> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
}

fn retryable_tag(error: &rmcp::ErrorData) -> Option<bool> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("retryable"))
        .and_then(|value| value.as_bool())
}

#[tokio::test]
async fn provenance_core_tool_invocations_are_persisted() {
    let workspace_root = build_workspace_fixture("core-invocations");
    let server = server_for_workspace(&workspace_root);

    server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed");
    server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
        .expect("read_file should succeed");
    server
        .search_text(Parameters(SearchTextParams {
            query: "hello provenance".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(10),
        }))
        .await
        .expect("search_text should succeed");

    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
        }))
        .await
        .expect("search_symbol should succeed");

    server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(5),
        }))
        .await
        .expect("find_references should return heuristic response");

    let storage = Storage::new(storage_path_for_workspace(&workspace_root));
    let list_rows = storage
        .load_provenance_events_for_tool("list_repositories", 10)
        .expect("expected list_repositories provenance rows");
    let read_rows = storage
        .load_provenance_events_for_tool("read_file", 10)
        .expect("expected read_file provenance rows");
    let search_text_rows = storage
        .load_provenance_events_for_tool("search_text", 10)
        .expect("expected search_text provenance rows");
    let search_symbol_rows = storage
        .load_provenance_events_for_tool("search_symbol", 10)
        .expect("expected search_symbol provenance rows");
    let find_references_rows = storage
        .load_provenance_events_for_tool("find_references", 10)
        .expect("expected find_references provenance rows");

    assert!(
        !list_rows.is_empty(),
        "missing list_repositories provenance"
    );
    assert!(!read_rows.is_empty(), "missing read_file provenance");
    assert!(
        !search_text_rows.is_empty(),
        "missing search_text provenance"
    );
    assert!(
        !search_symbol_rows.is_empty(),
        "missing search_symbol provenance"
    );
    assert!(
        !find_references_rows.is_empty(),
        "missing find_references provenance"
    );

    let symbol_payload = serde_json::from_str::<Value>(&search_symbol_rows[0].payload_json)
        .expect("failed to parse search_symbol provenance payload");
    assert_eq!(symbol_payload["outcome"]["status"].as_str(), Some("ok"));

    let references_payload = serde_json::from_str::<Value>(&find_references_rows[0].payload_json)
        .expect("failed to parse find_references provenance payload");
    assert_eq!(references_payload["outcome"]["status"].as_str(), Some("ok"));

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_bounded_text_fields_are_truncated() {
    let workspace_root = build_workspace_fixture("bounded-fields");
    let server = server_for_workspace(&workspace_root);
    let long_query = "q".repeat(2_048);

    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: long_query,
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
        }))
        .await
        .expect("search_symbol should succeed even when query does not match any symbol");

    let storage = Storage::new(storage_path_for_workspace(&workspace_root));
    let rows = storage
        .load_provenance_events_for_tool("search_symbol", 1)
        .expect("expected search_symbol provenance rows");
    assert_eq!(rows.len(), 1);

    let payload = serde_json::from_str::<Value>(&rows[0].payload_json)
        .expect("failed to parse search_symbol provenance payload");
    let stored_query = payload["params"]["query"]
        .as_str()
        .expect("expected params.query in provenance payload");
    assert!(
        stored_query.len() <= 515,
        "expected bounded query in provenance payload, got {} bytes",
        stored_query.len()
    );
    assert!(
        stored_query.ends_with("..."),
        "expected bounded query marker suffix"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_invalid_repository_hint_is_not_attributed_to_default_repository() {
    let workspace_root = build_workspace_fixture("invalid-repository-hint");
    let server = server_for_workspace(&workspace_root);
    let invalid_repository_id = "repo-999";

    server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
        .expect("list_repositories should succeed and initialize provenance storage");

    let error = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some(invalid_repository_id.to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
        }))
        .await
        .err()
        .expect("unknown repository_id should return typed resource_not_found");

    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("repository_id"))
            .and_then(|value| value.as_str()),
        Some(invalid_repository_id)
    );

    let storage_path = storage_path_for_workspace(&workspace_root);
    assert!(
        storage_path.exists(),
        "provenance storage should exist after successful read-only tool call"
    );
    let storage = Storage::new(storage_path);
    let rows = storage
        .load_provenance_events_for_tool("search_symbol", 10)
        .expect("provenance query should succeed");
    assert!(
        rows.is_empty(),
        "invalid repository hints must not be persisted against default repository"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_persistence_failures_are_strict_by_default_with_typed_error_metadata() {
    let workspace_root = build_workspace_fixture("strict-failure-default");
    fs::write(workspace_root.join(".frigg"), "blocked")
        .expect("failed to seed blocking provenance path fixture");
    let server = server_for_workspace(&workspace_root);

    let error = server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
        .err()
        .expect("strict mode should fail when provenance persistence fails");

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error_code_tag(&error),
        Some("provenance_persistence_failed")
    );
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("provenance_stage"))
            .and_then(|value| value.as_str()),
        Some("resolve_storage_path")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("tool_name"))
            .and_then(|value| value.as_str()),
        Some("list_repositories")
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_best_effort_mode_is_opt_in() {
    let workspace_root = build_workspace_fixture("best-effort-opt-in");
    fs::write(workspace_root.join(".frigg"), "blocked")
        .expect("failed to seed blocking provenance path fixture");
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace fixture should produce valid config");
    let server = FriggMcpServer::new_with_provenance_best_effort(config, true);

    let response = server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
        .expect("best-effort mode should not fail request on provenance persistence error")
        .0;

    assert_eq!(response.repositories.len(), 1);
    assert!(!storage_path_for_workspace(&workspace_root).exists());

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_deep_search_runtime_tool_invocations_emit_budget_metadata() {
    let workspace_root = build_deep_search_workspace_fixture("deep-search-runtime-provenance");
    let server = deep_search_runtime_server_for_workspace(&workspace_root);

    let playbook = DeepSearchHarness::load_playbook(&deep_search_playbook_path())
        .expect("deep-search playbook fixture must parse");
    let run = server
        .deep_search_run(Parameters(DeepSearchRunParams {
            playbook: playbook.clone().into(),
        }))
        .await
        .expect("deep_search_run should succeed")
        .0;
    let replay = server
        .deep_search_replay(Parameters(DeepSearchReplayParams {
            playbook: playbook.into(),
            expected_trace_artifact: run.trace_artifact.clone(),
        }))
        .await
        .expect("deep_search_replay should succeed")
        .0;
    let compose = server
        .deep_search_compose_citations(Parameters(DeepSearchComposeCitationsParams {
            trace_artifact: run.trace_artifact.clone(),
            answer: None,
        }))
        .await
        .expect("deep_search_compose_citations should succeed")
        .0;

    assert!(
        replay.matches,
        "expected deterministic replay match: {:?}",
        replay.diff
    );
    assert!(
        !compose.citation_payload.claims.is_empty(),
        "expected composed citation claims"
    );

    let storage = Storage::new(storage_path_for_workspace(&workspace_root));
    let run_rows = storage
        .load_provenance_events_for_tool("deep_search_run", 1)
        .expect("expected deep_search_run provenance rows");
    let replay_rows = storage
        .load_provenance_events_for_tool("deep_search_replay", 1)
        .expect("expected deep_search_replay provenance rows");
    let compose_rows = storage
        .load_provenance_events_for_tool("deep_search_compose_citations", 1)
        .expect("expected deep_search_compose_citations provenance rows");

    assert_eq!(run_rows.len(), 1);
    assert_eq!(replay_rows.len(), 1);
    assert_eq!(compose_rows.len(), 1);

    let run_payload = serde_json::from_str::<Value>(&run_rows[0].payload_json)
        .expect("failed to parse deep_search_run provenance payload");
    let replay_payload = serde_json::from_str::<Value>(&replay_rows[0].payload_json)
        .expect("failed to parse deep_search_replay provenance payload");
    let compose_payload = serde_json::from_str::<Value>(&compose_rows[0].payload_json)
        .expect("failed to parse deep_search_compose_citations provenance payload");

    for payload in [&run_payload, &replay_payload, &compose_payload] {
        assert_eq!(payload["outcome"]["status"].as_str(), Some("ok"));
        assert!(
            payload["source_refs"]["resource_budgets"]
                .as_array()
                .map(|entries| !entries.is_empty())
                .unwrap_or(false),
            "expected deep-search provenance to include resource_budgets entries"
        );
        assert!(
            payload["source_refs"]["resource_usage"]
                .as_array()
                .map(|entries| !entries.is_empty())
                .unwrap_or(false),
            "expected deep-search provenance to include resource_usage entries"
        );
    }

    let run_budget_tools = run_payload["source_refs"]["resource_budgets"]
        .as_array()
        .expect("run resource_budgets should be an array")
        .iter()
        .filter_map(|entry| entry.get("tool_name").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(
        run_budget_tools.contains(&"find_references"),
        "expected run budget metadata to include find_references step"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_deep_search_runtime_invalid_params_failure_is_typed_and_persisted() {
    let workspace_root = build_deep_search_workspace_fixture("deep-search-runtime-failure");
    let server = deep_search_runtime_server_for_workspace(&workspace_root);

    let error = match server
        .deep_search_run(Parameters(DeepSearchRunParams {
            playbook: DeepSearchPlaybookContract {
                playbook_id: "unsupported-step-tool".to_owned(),
                steps: vec![DeepSearchPlaybookStepContract {
                    step_id: "tool-001".to_owned(),
                    tool_name: "write_file".to_owned(),
                    params: json!({ "path": "src/lib.rs" }),
                }],
            },
        }))
        .await
    {
        Ok(_) => panic!("unsupported deep-search tool step should return invalid_params"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));

    let storage = Storage::new(storage_path_for_workspace(&workspace_root));
    let run_rows = storage
        .load_provenance_events_for_tool("deep_search_run", 1)
        .expect("expected deep_search_run provenance rows");
    assert_eq!(run_rows.len(), 1);
    let payload = serde_json::from_str::<Value>(&run_rows[0].payload_json)
        .expect("failed to parse deep_search_run provenance payload");

    assert_eq!(payload["outcome"]["status"].as_str(), Some("error"));
    assert_eq!(
        payload["outcome"]["error_code"].as_str(),
        Some("invalid_params")
    );
    assert!(
        payload["source_refs"]["resource_budgets"].is_array(),
        "expected resource_budgets key even on deep-search invalid_params failure"
    );
    assert!(
        payload["source_refs"]["resource_usage"].is_array(),
        "expected resource_usage key even on deep-search invalid_params failure"
    );

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_extended_explore_invocations_include_scope_metadata() {
    let workspace_root = build_workspace_fixture("explore-runtime-provenance");
    let server = extended_runtime_server_for_workspace(&workspace_root);

    server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: Some("provenance".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(1),
            resume_from: None,
        }))
        .await
        .expect("explore should succeed");

    let storage = Storage::new(storage_path_for_workspace(&workspace_root));
    let rows = storage
        .load_provenance_events_for_tool("explore", 1)
        .expect("expected explore provenance rows");
    assert_eq!(rows.len(), 1);

    let payload = serde_json::from_str::<Value>(&rows[0].payload_json)
        .expect("failed to parse explore provenance payload");
    assert_eq!(payload["outcome"]["status"].as_str(), Some("ok"));
    assert_eq!(payload["params"]["operation"].as_str(), Some("probe"));
    assert_eq!(
        payload["source_refs"]["resolved_path"].as_str(),
        Some("src/lib.rs")
    );
    assert_eq!(
        payload["source_refs"]["scan_scope"]["start_line"].as_u64(),
        Some(1)
    );
    assert_eq!(payload["source_refs"]["total_matches"].as_u64(), Some(1));
    assert_eq!(payload["source_refs"]["truncated"].as_bool(), Some(false));

    cleanup_workspace(&workspace_root);
}

#[tokio::test]
async fn provenance_search_hybrid_invocations_include_winning_anchor_metadata() {
    let workspace_root = build_workspace_fixture("search-hybrid-anchor-provenance");
    let server = server_for_workspace(&workspace_root);

    server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello provenance".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(5),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("search_hybrid should succeed");

    let storage = Storage::new(storage_path_for_workspace(&workspace_root));
    let rows = storage
        .load_provenance_events_for_tool("search_hybrid", 1)
        .expect("expected search_hybrid provenance rows");
    assert_eq!(rows.len(), 1);

    let payload = serde_json::from_str::<Value>(&rows[0].payload_json)
        .expect("failed to parse search_hybrid provenance payload");
    assert_eq!(payload["outcome"]["status"].as_str(), Some("ok"));
    assert_eq!(
        payload["source_refs"]["matches"]["top_matches"][0]["path"].as_str(),
        Some("src/lib.rs")
    );
    assert_eq!(
        payload["source_refs"]["matches"]["top_matches"][0]["anchor"]["kind"].as_str(),
        Some("text_span")
    );
    assert_eq!(
        payload["source_refs"]["matches"]["top_matches"][0]["anchor"]["start_line"].as_u64(),
        Some(1)
    );

    cleanup_workspace(&workspace_root);
}
