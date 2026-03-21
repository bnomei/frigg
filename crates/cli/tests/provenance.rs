use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{
    ExploreOperation, ExploreParams, FindReferencesParams, ListRepositoriesParams, ReadFileParams,
    SearchHybridParams, SearchPatternType, SearchSymbolParams, SearchTextParams,
};
use frigg::settings::FriggConfig;
use frigg::storage::Storage;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;
use serde_json::Value;

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

fn build_multilang_workspace_fixture(test_name: &str) -> PathBuf {
    let root = temp_workspace_root(test_name);
    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).expect("failed to create multi-language fixture source directory");
    fs::write(
        src_dir.join("App.tsx"),
        "export class DashboardCard {\n\
             render() { return <Card />; }\n\
         }\n",
    )
    .expect("failed to seed typescript fixture source file");
    fs::write(
        src_dir.join("app.py"),
        "class PyService:\n\
             def run(self) -> str:\n\
                 return \"ok\"\n",
    )
    .expect("failed to seed python fixture source file");
    fs::write(
        src_dir.join("main.go"),
        "package main\n\
         type GoService struct{}\n",
    )
    .expect("failed to seed go fixture source file");
    fs::write(src_dir.join("Main.kt"), "class KotlinService\n")
        .expect("failed to seed kotlin fixture source file");
    fs::write(
        src_dir.join("init.lua"),
        "function luaRun()\n\
             return \"ok\"\n\
         end\n",
    )
    .expect("failed to seed lua fixture source file");
    fs::write(
        src_dir.join("app.nim"),
        "proc nimHelper(): string =\n\
           \"ok\"\n",
    )
    .expect("failed to seed nim fixture source file");
    fs::write(src_dir.join("main.roc"), "rocGreet = \\name -> name\n")
        .expect("failed to seed roc fixture source file");
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

fn extended_runtime_server_for_workspace(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace fixture should produce valid config");
    FriggMcpServer::new_with_runtime_options(config, false, true)
}

fn cleanup_workspace(workspace_root: &Path) {
    let _ = fs::remove_dir_all(workspace_root);
}

async fn public_repository_id(server: &FriggMcpServer) -> String {
    server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed")
        .0
        .repositories
        .into_iter()
        .next()
        .expect("server should expose one repository")
        .repository_id
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
    let repository_id = public_repository_id(&server).await;

    server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed");
    server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some(repository_id.clone()),
            max_bytes: None,
            line_start: None,
            line_end: None,
            presentation_mode: None,
        }))
        .await
        .expect("read_file should succeed");
    server
        .search_text(Parameters(SearchTextParams {
            query: "hello provenance".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some(repository_id.clone()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(10),
            ..Default::default()
        }))
        .await
        .expect("search_text should succeed");

    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("search_symbol should succeed");

    server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("greeting".to_owned()),
            repository_id: Some(repository_id),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            include_follow_up_structural: None,
            limit: Some(5),
            ..Default::default()
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
    let repository_id = public_repository_id(&server).await;
    let long_query = "q".repeat(2_048);

    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: long_query,
            repository_id: Some(repository_id),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
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
async fn provenance_search_symbol_records_baseline_language_queries() {
    let workspace_root = build_multilang_workspace_fixture("search-symbol-multilang");
    let server = server_for_workspace(&workspace_root);
    let repository_id = public_repository_id(&server).await;

    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "DashboardCard".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("typescript search_symbol should succeed");
    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "PyService".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("python search_symbol should succeed");
    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "GoService".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("go search_symbol should succeed");
    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "KotlinService".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("kotlin search_symbol should succeed");
    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "luaRun".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("lua search_symbol should succeed");
    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "nimHelper".to_owned(),
            repository_id: Some(repository_id.clone()),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("nim search_symbol should succeed");
    server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "rocGreet".to_owned(),
            repository_id: Some(repository_id),
            path_class: None,
            path_regex: None,
            limit: Some(5),
            ..Default::default()
        }))
        .await
        .expect("roc search_symbol should succeed");

    let storage = Storage::new(storage_path_for_workspace(&workspace_root));
    let rows = storage
        .load_provenance_events_for_tool("search_symbol", 10)
        .expect("expected search_symbol provenance rows");
    let queries = rows
        .iter()
        .map(|row| {
            serde_json::from_str::<Value>(&row.payload_json)
                .expect("failed to parse search_symbol provenance payload")
        })
        .map(|payload| {
            payload["params"]["query"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        })
        .collect::<Vec<_>>();

    assert!(
        queries.iter().any(|query| query == "DashboardCard"),
        "missing typescript search_symbol provenance query: {queries:?}"
    );
    assert!(
        queries.iter().any(|query| query == "PyService"),
        "missing python search_symbol provenance query: {queries:?}"
    );
    assert!(
        queries.iter().any(|query| query == "GoService"),
        "missing go search_symbol provenance query: {queries:?}"
    );
    assert!(
        queries.iter().any(|query| query == "KotlinService"),
        "missing kotlin search_symbol provenance query: {queries:?}"
    );
    assert!(
        queries.iter().any(|query| query == "luaRun"),
        "missing lua search_symbol provenance query: {queries:?}"
    );
    assert!(
        queries.iter().any(|query| query == "nimHelper"),
        "missing nim search_symbol provenance query: {queries:?}"
    );
    assert!(
        queries.iter().any(|query| query == "rocGreet"),
        "missing roc search_symbol provenance query: {queries:?}"
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
            ..Default::default()
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
async fn provenance_extended_explore_invocations_include_scope_metadata() {
    let workspace_root = build_workspace_fixture("explore-runtime-provenance");
    let server = extended_runtime_server_for_workspace(&workspace_root);
    let repository_id = public_repository_id(&server).await;

    server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some(repository_id),
            operation: ExploreOperation::Probe,
            query: Some("provenance".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(1),
            resume_from: None,
            presentation_mode: None,
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
    let repository_id = public_repository_id(&server).await;

    server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello provenance".to_owned(),
            repository_id: Some(repository_id),
            language: Some("rust".to_owned()),
            limit: Some(5),
            weights: None,
            semantic: Some(false),
            ..Default::default()
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
