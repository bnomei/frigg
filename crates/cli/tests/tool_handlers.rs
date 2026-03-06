#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{
    DocumentSymbolsParams, FindDeclarationsParams, FindImplementationsParams, FindReferencesParams,
    GoToDefinitionParams, IncomingCallsParams, ListRepositoriesParams, OutgoingCallsParams,
    ReadFileParams, SearchHybridParams, SearchPatternType, SearchStructuralParams,
    SearchSymbolParams, SearchTextParams, WorkspaceAttachParams, WorkspaceCurrentParams,
    WorkspaceResolveMode, WorkspaceStorageIndexState,
};
use frigg::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeProvider};
use frigg::storage::{
    ManifestEntry, Storage, ensure_provenance_db_parent_dir, resolve_provenance_db_path,
};
use protobuf::{EnumOrUnknown, Message};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    SymbolInformation as ScipSymbolInformationProto,
};

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repos/manifest-determinism")
}

fn server_for_fixture() -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![fixture_root()])
        .expect("fixture root must produce valid config");
    FriggMcpServer::new(config)
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

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-mcp-tool-handlers-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn server_for_workspace_root(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    FriggMcpServer::new(config)
}

fn server_for_config(config: FriggConfig) -> FriggMcpServer {
    config
        .validate_for_serving()
        .expect("test config must validate for serving");
    FriggMcpServer::new(config)
}

fn server_for_workspace_root_with_max_file_bytes(
    workspace_root: &Path,
    max_file_bytes: usize,
) -> FriggMcpServer {
    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    config.max_file_bytes = max_file_bytes;
    FriggMcpServer::new(config)
}

fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
    system_time
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
}

fn seed_manifest_snapshot(
    workspace_root: &Path,
    repository_id: &str,
    snapshot_id: &str,
    paths: &[&str],
) {
    let db_path =
        ensure_provenance_db_parent_dir(workspace_root).expect("manifest storage path should work");
    let resolved_db_path =
        resolve_provenance_db_path(workspace_root).expect("manifest db path should resolve");
    assert_eq!(db_path, resolved_db_path);

    let storage = Storage::new(db_path);
    storage
        .initialize()
        .expect("manifest storage should initialize");

    let mut manifest_entries = paths
        .iter()
        .map(|path| {
            let metadata = fs::metadata(workspace_root.join(path))
                .expect("manifest snapshot path should exist for test");
            ManifestEntry {
                path: (*path).to_owned(),
                sha256: format!("hash-{path}"),
                size_bytes: metadata.len(),
                mtime_ns: metadata.modified().ok().and_then(system_time_to_unix_nanos),
            }
        })
        .collect::<Vec<_>>();
    manifest_entries.sort_by(|left, right| left.path.cmp(&right.path));
    manifest_entries.dedup_by(|left, right| left.path == right.path);

    storage
        .upsert_manifest(repository_id, snapshot_id, &manifest_entries)
        .expect("manifest snapshot should persist");
}

fn write_scip_fixture(workspace_root: &Path, file_name: &str, payload: &str) {
    let fixture_dir = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&fixture_dir).expect("failed to create scip fixture directory");
    fs::write(fixture_dir.join(file_name), payload).expect("failed to write scip fixture payload");
}

fn write_scip_protobuf_fixture(workspace_root: &Path, file_name: &str) {
    let fixture_dir = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&fixture_dir).expect("failed to create scip fixture directory");

    let mut index = ScipIndexProto::new();
    let mut document = ScipDocumentProto::new();
    document.relative_path = "src/lib.rs".to_owned();

    let mut definition = ScipOccurrenceProto::new();
    definition.symbol = "scip-rust pkg repo#User".to_owned();
    definition.range = vec![0, 11, 15];
    definition.symbol_roles = 1;
    document.occurrences.push(definition);

    let mut reference = ScipOccurrenceProto::new();
    reference.symbol = "scip-rust pkg repo#User".to_owned();
    reference.range = vec![2, 31, 35];
    reference.symbol_roles = 8;
    document.occurrences.push(reference);

    let mut symbol = ScipSymbolInformationProto::new();
    symbol.symbol = "scip-rust pkg repo#User".to_owned();
    symbol.display_name = "User".to_owned();
    symbol.kind = EnumOrUnknown::from_i32(7);
    document.symbols.push(symbol);

    index.documents.push(document);
    let payload = index
        .write_to_bytes()
        .expect("protobuf fixture payload should serialize");
    fs::write(fixture_dir.join(file_name), payload)
        .expect("failed to write scip protobuf fixture payload");
}

fn cleanup_workspace_root(workspace_root: &Path) {
    let _ = fs::remove_dir_all(workspace_root);
}

fn rewrite_file_with_new_mtime(path: &Path, contents: &str) {
    let before = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(system_time_to_unix_nanos);

    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(20));
        fs::write(path, contents).expect("rewritten fixture file should persist");
        let after = fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_to_unix_nanos);
        if after != before {
            return;
        }
    }

    panic!("fixture file mtime did not advance after rewrite");
}

#[tokio::test]
async fn core_list_repositories_is_deterministic() {
    let server = server_for_fixture();

    let first = server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed")
        .0;
    let second = server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed")
        .0;

    assert_eq!(first.repositories.len(), second.repositories.len());
    assert_eq!(first.repositories.len(), 1);
    assert_eq!(first.repositories[0].repository_id, "repo-001");
    assert_eq!(
        first.repositories[0].repository_id,
        second.repositories[0].repository_id
    );
    assert_eq!(
        first.repositories[0].display_name,
        second.repositories[0].display_name
    );
    assert_eq!(
        first.repositories[0].root_path,
        second.repositories[0].root_path
    );
}

#[tokio::test]
async fn workspace_attach_reuses_git_root_and_sets_session_default() {
    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );
    let nested_path = fixture_root().join("src/lib.rs");

    let first = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: nested_path.display().to_string(),
            set_default: None,
            resolve_mode: None,
        }))
        .await
        .expect("workspace_attach should succeed for fixture file path")
        .0;
    assert_eq!(first.repository.repository_id, "repo-001");
    assert_eq!(first.resolution, WorkspaceResolveMode::GitRoot);
    assert!(first.session_default);
    assert_ne!(first.storage.index_state, WorkspaceStorageIndexState::Error);

    let second = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: fixture_root().display().to_string(),
            set_default: Some(false),
            resolve_mode: None,
        }))
        .await
        .expect("workspace_attach should reuse existing root")
        .0;
    assert_eq!(second.repository.repository_id, first.repository.repository_id);

    let current = server
        .workspace_current(Parameters(WorkspaceCurrentParams {}))
        .await
        .expect("workspace_current should succeed")
        .0;
    assert!(current.session_default);
    assert_eq!(
        current
            .repository
            .expect("workspace_current should return attached repository")
            .repository_id,
        "repo-001"
    );
}

#[tokio::test]
async fn workspace_session_default_scopes_search_text_without_repository_hint() {
    let root_a = temp_workspace_root("workspace-default-a");
    let root_b = temp_workspace_root("workspace-default-b");
    fs::create_dir_all(root_a.join("src")).expect("workspace a src dir should be creatable");
    fs::create_dir_all(root_b.join("src")).expect("workspace b src dir should be creatable");
    fs::write(root_a.join("src/lib.rs"), "pub fn shared_marker() { /* repo_a */ }\n")
        .expect("workspace a source should write");
    fs::write(root_b.join("src/lib.rs"), "pub fn shared_marker() { /* repo_b */ }\n")
        .expect("workspace b source should write");

    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );

    let attached_a = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: root_a.display().to_string(),
            set_default: Some(false),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
        }))
        .await
        .expect("workspace_attach should attach repo a")
        .0;
    let attached_b = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: root_b.display().to_string(),
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
        }))
        .await
        .expect("workspace_attach should attach repo b and set default")
        .0;

    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "shared_marker".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: None,
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_text should honor session default")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, attached_b.repository.repository_id);
    assert_ne!(
        response.matches[0].repository_id,
        attached_a.repository.repository_id
    );

    cleanup_workspace_root(&root_a);
    cleanup_workspace_root(&root_b);
}

#[tokio::test]
async fn workspace_read_file_without_attached_repositories_returns_remediation() {
    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );

    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "README.md".to_owned(),
            repository_id: None,
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
    {
        Ok(_) => panic!("read_file should fail without attached repositories"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(error_code_tag(&error), Some("resource_not_found"));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(|value| value.as_str()),
        Some("workspace_attach")
    );
}

#[tokio::test]
async fn core_list_repositories_fails_with_typed_error_when_provenance_persistence_fails_by_default()
 {
    let workspace_root = temp_workspace_root("list-repositories-provenance-strict");
    fs::create_dir_all(&workspace_root).expect("failed to create temporary workspace root");
    fs::write(workspace_root.join(".frigg"), "blocked")
        .expect("failed to seed blocking provenance path fixture");
    let server = server_for_workspace_root(&workspace_root);

    let error = match server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
    {
        Ok(_) => panic!("strict mode should fail when provenance persistence fails"),
        Err(error) => error,
    };

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

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_read_file_returns_typed_not_found_error() {
    let server = server_for_fixture();
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "missing-file.txt".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
    {
        Ok(_) => panic!("missing file should return typed error"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(error_code_tag(&error), Some("resource_not_found"));
    assert_eq!(retryable_tag(&error), Some(false));
}

#[tokio::test]
async fn core_read_file_returns_repository_relative_canonical_path() {
    let server = server_for_fixture();
    let absolute_path = fixture_root().join("src/lib.rs");
    let absolute_response = server
        .read_file(Parameters(ReadFileParams {
            path: absolute_path.display().to_string(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
        .expect("absolute read_file path under workspace root should resolve")
        .0;
    assert_eq!(absolute_response.repository_id, "repo-001");
    assert_eq!(absolute_response.path, "src/lib.rs");
    assert!(
        !Path::new(&absolute_response.path).is_absolute(),
        "read_file path contract must be repository-relative"
    );

    let relative_response = server
        .read_file(Parameters(ReadFileParams {
            path: "./src/../src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
        .expect("relative read_file path under workspace root should resolve")
        .0;
    assert_eq!(relative_response.repository_id, "repo-001");
    assert_eq!(relative_response.path, "src/lib.rs");
    assert_eq!(relative_response.path, absolute_response.path);
}

#[tokio::test]
async fn core_read_file_supports_line_range_slicing() {
    let server = server_for_fixture();
    let response = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(128),
            line_start: Some(2),
            line_end: Some(2),
        }))
        .await
        .expect("line-range slice should succeed")
        .0;

    assert_eq!(response.repository_id, "repo-001");
    assert_eq!(response.path, "src/lib.rs");
    assert_eq!(response.content, "    \"hello from fixture\"");
    assert_eq!(response.bytes, response.content.as_bytes().len());
}

#[tokio::test]
async fn core_read_file_line_range_can_bypass_full_file_size_limit() {
    let workspace_root = temp_workspace_root("read-file-line-range-max-bytes");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "abcdefghijklmnopqrstuvwxyz\nok\nabcdefghijklmnopqrstuvwxyz\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(8),
            line_start: Some(2),
            line_end: Some(2),
        }))
        .await
        .expect("line-range slice should apply max_bytes to returned slice content")
        .0;

    assert_eq!(response.content, "ok");
    assert_eq!(response.bytes, 2);
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_read_file_line_range_preserves_lossy_utf8_behavior() {
    let workspace_root = temp_workspace_root("read-file-line-range-lossy-utf8");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        b"alpha\nbeta \xFF\nomega\n".as_slice(),
    )
    .expect("failed to seed temporary fixture source");

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(64),
            line_start: Some(2),
            line_end: Some(2),
        }))
        .await
        .expect("lossy utf8 line-range slice should succeed")
        .0;

    assert_eq!(response.content, "beta \u{fffd}");
    assert_eq!(response.bytes, response.content.len());
    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_read_file_rejects_invalid_line_range_payload() {
    let server = server_for_fixture();
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(128),
            line_start: Some(3),
            line_end: Some(2),
        }))
        .await
    {
        Ok(_) => panic!("invalid line range should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
}

#[tokio::test]
async fn core_search_text_literal_scoped_to_repository() {
    let server = server_for_fixture();
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "hello from fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(10),
        }))
        .await
        .expect("literal search should succeed")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].path, "src/lib.rs");
}

#[tokio::test]
async fn core_search_text_regex_mode_executes_regex_search() {
    let server = server_for_fixture();
    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "hello\\s+from\\s+fixture".to_owned(),
            pattern_type: Some(SearchPatternType::Regex),
            repository_id: Some("repo-001".to_owned()),
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("regex mode should execute search")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].path, "src/lib.rs");
}

#[tokio::test]
async fn core_search_hybrid_returns_deterministic_matches_and_note_metadata() {
    let server = server_for_fixture();
    let first = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("search_hybrid should succeed")
        .0;
    let second = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("search_hybrid should be deterministic")
        .0;

    assert_eq!(first.matches, second.matches);
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].repository_id, "repo-001");
    assert_eq!(first.matches[0].path, "src/lib.rs");
    assert_eq!(first.semantic_requested, Some(false));
    assert_eq!(first.semantic_enabled, Some(false));
    assert_eq!(first.semantic_status.as_deref(), Some("disabled"));
    assert_eq!(
        first.semantic_reason.as_deref(),
        Some("semantic channel disabled by request toggle")
    );
    assert!(
        first.matches[0].blended_score >= 0.0,
        "hybrid blended score should be non-negative"
    );

    let structured: serde_json::Value =
        serde_json::to_value(&first).expect("search_hybrid response should serialize");
    assert_eq!(structured["semantic_status"], "disabled");
    assert_eq!(structured["semantic_enabled"], false);
    assert_eq!(structured["semantic_requested"], false);
    assert_eq!(
        structured["semantic_reason"],
        "semantic channel disabled by request toggle"
    );

    let note = first
        .note
        .as_ref()
        .expect("search_hybrid should emit deterministic note metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("search_hybrid note should be valid JSON");
    assert_eq!(note_json["semantic_status"], "disabled");
    assert_eq!(note_json["semantic_enabled"], false);
    assert_eq!(note_json["semantic_requested"], false);
    assert_eq!(
        structured["semantic_status"], note_json["semantic_status"],
        "top-level semantic status should stay aligned with note metadata"
    );
    assert_eq!(
        structured["semantic_reason"], note_json["semantic_reason"],
        "top-level semantic reason should stay aligned with note metadata"
    );
}

#[tokio::test]
async fn core_search_hybrid_rejects_empty_query_with_typed_invalid_params() {
    let server = server_for_fixture();
    let error = match server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "   ".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: None,
            limit: Some(10),
            weights: None,
            semantic: None,
        }))
        .await
    {
        Ok(_) => panic!("empty search_hybrid query should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
}

#[tokio::test]
async fn core_search_hybrid_strict_semantic_requires_startup_credentials() {
    let mut config = FriggConfig::from_workspace_roots(vec![fixture_root()])
        .expect("fixture root must produce valid config");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: true,
    };
    let server = server_for_config(config);

    let error = match server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: None,
            limit: Some(10),
            weights: None,
            semantic: Some(true),
        }))
        .await
    {
        Ok(_) => panic!("strict semantic startup failure should return typed error"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(error_code_tag(&error), Some("unavailable"));
    assert_eq!(retryable_tag(&error), Some(true));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        Some("strict_failure")
    );
}

#[tokio::test]
async fn core_search_text_rejects_abusive_path_regex_with_typed_invalid_params() {
    let server = server_for_fixture();
    let abusive_path_regex = "a".repeat(600);
    let error = match server
        .search_text(Parameters(SearchTextParams {
            query: "hello".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(abusive_path_regex.clone()),
            limit: Some(10),
        }))
        .await
    {
        Ok(_) => panic!("abusive path_regex should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("invalid path_regex"),
        "unexpected error message: {}",
        error.message
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("path_regex"))
            .and_then(|value| value.as_str()),
        Some(abusive_path_regex.as_str())
    );
}

#[tokio::test]
async fn core_read_file_enforces_effective_max_bytes_clamp() {
    let workspace_root = temp_workspace_root("read-file-max-clamp");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(src_root.join("lib.rs"), "0123456789")
        .expect("failed to seed temporary fixture source");

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 4);
    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            max_bytes: Some(1024),
            line_start: None,
            line_end: None,
        }))
        .await
    {
        Ok(_) => panic!("effective max clamp should reject oversized file"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("max_bytes"))
            .and_then(|value| value.as_u64()),
        Some(4)
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("suggested_max_bytes"))
            .and_then(|value| value.as_u64()),
        Some(4)
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_search_symbol_returns_tree_sitter_matches() {
    let server = server_for_fixture();
    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "greeting");
    assert_eq!(response.matches[0].kind, "function");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 1);

    let note = response
        .note
        .as_ref()
        .expect("search_symbol should emit deterministic note metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("search_symbol note should be valid JSON");
    assert_eq!(note_json["source"], "tree_sitter");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn search_symbol_preserves_exact_case_prefix_and_infix_rank_order() {
    let workspace_root = temp_workspace_root("search-symbol-rank-order");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn target() {}\n\
         pub fn Target() {}\n\
         pub fn target_prefix() {}\n\
         pub fn other_target() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "target".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;

    let symbols = response
        .matches
        .iter()
        .map(|matched| matched.symbol.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        symbols,
        vec!["target", "Target", "target_prefix", "other_target"]
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_rebuilds_stale_manifest_snapshot_before_reusing_cached_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-stale-manifest");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let source_path = src_root.join("lib.rs");
    fs::write(&source_path, "pub fn old_name() {}\n")
        .expect("failed to seed temporary fixture source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "old_name".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed for warm snapshot")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "old_name");

    rewrite_file_with_new_mtime(&source_path, "pub fn new_name() {}\n");

    let second = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "new_name".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should rebuild stale snapshot")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "new_name");

    let stale = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "old_name".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should not keep stale symbol results")
        .0;
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_rebuilds_stale_manifest_backed_corpus_after_edit() {
    let workspace_root = temp_workspace_root("search-symbol-stale-manifest");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(10),
        }))
        .await
        .expect("initial search_symbol call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "alpha");

    fs::write(&lib_path, "pub fn beta_beta() {}\n").expect("failed to edit source in place");

    let second = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "beta_beta".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(10),
        }))
        .await
        .expect("search_symbol should rebuild stale corpus after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "beta_beta");

    let stale = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(10),
        }))
        .await
        .expect("search_symbol should not reuse stale corpus matches")
        .0;
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_find_references_returns_heuristic_metadata_and_matches() {
    let workspace_root = temp_workspace_root("find-references");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("find_references should return heuristic references")
        .0;

    assert!(
        response.matches.len() >= 2,
        "expected at least two deterministic heuristic references"
    );
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);
    assert_eq!(response.matches[0].column, 25);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit heuristic metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["heuristic"], true);
    assert_eq!(note_json["confidence"]["low"], response.matches.len());
    assert_eq!(note_json["resolution_source"], "symbol");
    assert_eq!(note_json["target_selection"]["ambiguous_query"], false);
    assert_eq!(note_json["target_selection"]["candidate_count"], 1);
    assert!(
        note_json["resource_budgets"]["source"]["max_file_bytes"]
            .as_u64()
            .is_some()
    );
    assert!(
        note_json["resource_usage"]["source"]["files_discovered"]
            .as_u64()
            .unwrap_or(0)
            >= 1
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn precision_precedence_find_references_prefers_precise_matches() {
    let workspace_root = temp_workspace_root("precision-precedence-precise");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn heuristic_marker() { let _ = User; }\n\
         pub fn precise_marker() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "references.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [2, 31, 35], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise references first")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 32);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["reference_count"], 1);
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert!(
        note_json["precise"]["candidate_directories"]
            .as_array()
            .is_some_and(|directories| !directories.is_empty())
    );
    assert!(
        note_json["precise"]["discovered_artifacts"]
            .as_array()
            .is_some_and(|artifacts| !artifacts.is_empty())
    );
    assert!(
        note_json["resource_usage"]["scip"]["artifacts_discovered_bytes"]
            .as_u64()
            .unwrap_or(0)
            > 0
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn precision_precedence_find_references_prefers_protobuf_scip_matches() {
    let workspace_root = temp_workspace_root("precision-precedence-precise-protobuf");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn heuristic_marker() { let _ = User; }\n\
         pub fn precise_marker() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_protobuf_fixture(&workspace_root, "references.scip");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise references from protobuf scip")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 32);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["reference_count"], 1);
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(
        note_json["precise"]["discovered_artifacts"][0]
            .as_str()
            .is_some_and(|path| path.ends_with(".frigg/scip/references.scip")),
        true
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn precision_precedence_find_references_falls_back_to_heuristic_when_precise_absent() {
    let workspace_root = temp_workspace_root("precision-precedence-heuristic");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("find_references should fall back to heuristic references")
        .0;

    assert!(
        response.matches.len() >= 2,
        "expected deterministic heuristic fallback references"
    );

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["heuristic"], true);
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise_absence_reason"],
        "no_scip_artifacts_discovered"
    );
    assert_eq!(note_json["precise"]["artifacts_discovered"], 0);
    assert_eq!(note_json["precise"]["artifacts_failed"], 0);
    assert_eq!(note_json["precise"]["reference_count"], 0);
    assert!(
        note_json["precise"]["candidate_directories"]
            .as_array()
            .is_some_and(|directories| directories.iter().any(|path| {
                path.as_str()
                    .is_some_and(|path| path.ends_with(".frigg/scip"))
            }))
    );
    assert_eq!(
        note_json["precise"]["discovered_artifacts"]
            .as_array()
            .map(|artifacts| artifacts.len()),
        Some(0)
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_reports_failed_scip_artifact_details_in_note_metadata() {
    let workspace_root = temp_workspace_root("find-references-failed-artifact-details");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(&workspace_root, "broken.json", "{ invalid json");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("find_references should fall back to heuristic references")
        .0;

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precision fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");

    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise_absence_reason"],
        "scip_artifact_ingest_failed"
    );
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "ingest_payload"
    );
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["artifact_label"]
            .as_str()
            .unwrap_or_default()
            .ends_with(".frigg/scip/broken.json"),
        true
    );
    assert!(
        note_json["precise"]["failed_artifacts"][0]["detail"]
            .as_str()
            .unwrap_or_default()
            .len()
            > 0,
        "expected parse failure detail in failed artifact metadata"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_reports_target_selection_metadata_for_ambiguous_symbol_queries() {
    let workspace_root = temp_workspace_root("find-references-ambiguous-symbol");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(src_root.join("a.rs"), "pub fn invalid_params() {}\n")
        .expect("failed to seed first source file");
    fs::write(src_root.join("b.rs"), "pub fn invalid_params() {}\n")
        .expect("failed to seed second source file");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "invalid_params".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("find_references should succeed with ambiguous symbol names")
        .0;

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit selection metadata in note");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "symbol");
    assert_eq!(note_json["target_selection"]["query"], "invalid_params");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/a.rs");
    assert_eq!(note_json["target_selection"]["selected_line"], 1);
    assert_eq!(note_json["target_selection"]["ambiguous_query"], true);
    assert_eq!(note_json["target_selection"]["candidate_count"], 2);
    assert_eq!(
        note_json["target_selection"]["same_rank_candidate_count"],
        2
    );
    assert_eq!(
        note_json["precise_absence_reason"],
        "no_scip_artifacts_discovered"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_retains_precise_matches_when_other_scip_artifact_exceeds_budget() {
    let workspace_root = temp_workspace_root("find-references-scip-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n",
    )
    .expect("failed to seed source fixture");
    write_scip_fixture(
        &workspace_root,
        "references.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [1, 27, 31], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("oversized SCIP artifact should retain partial precise references")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);
    let note = response
        .note
        .as_ref()
        .expect("find_references should emit partial precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise_partial");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "artifact_budget_bytes"
    );
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["artifact_label"]
            .as_str()
            .unwrap_or_default()
            .ends_with(".frigg/scip/oversized.json"),
        true
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_falls_back_when_partial_precise_absence_is_non_authoritative() {
    let workspace_root = temp_workspace_root("find-references-partial-absence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n",
    )
    .expect("failed to seed source fixture");
    write_scip_fixture(&workspace_root, "empty.json", r#"{ "documents": [] }"#);

    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("partial precise absence should fall back heuristically")
        .0;

    assert!(
        !response.matches.is_empty(),
        "heuristic fallback should still return lexical references"
    );
    let note = response
        .note
        .as_ref()
        .expect("find_references should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise_absence_reason"],
        "precise_partial_non_authoritative_absence"
    );
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_rejects_oversized_source_file_with_typed_timeout() {
    let workspace_root = temp_workspace_root("find-references-source-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn create_user() -> User { User }\n\
         pub fn use_user() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    fs::write(src_root.join("zzz_large.rs"), "x".repeat(256))
        .expect("failed to seed oversized source file");

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 8);
    let error = match server
        .find_references(Parameters(FindReferencesParams {
            symbol: "User".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("oversized source file should return typed timeout"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(error_code_tag(&error), Some("timeout"));
    assert_eq!(retryable_tag(&error), Some(true));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("budget_scope"))
            .and_then(|value| value.as_str()),
        Some("source")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("budget_code"))
            .and_then(|value| value.as_str()),
        Some("source_file_bytes")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_prefers_precise_matches() {
    let workspace_root = temp_workspace_root("go-to-definition-precise");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn caller() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [1, 33, 37], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should resolve precise definition")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, 12);
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_resolves_same_line_target_by_path_line_and_column() {
    let workspace_root = temp_workspace_root("go-to-definition-location-same-line");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.php"),
        "<?php function alpha() {} function beta() {}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.php".to_owned()),
            line: Some(1),
            column: Some(35),
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should resolve by location")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "beta");
    assert_eq!(response.matches[0].path, "src/lib.php");

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_degrades_when_any_scip_artifact_exceeds_budget() {
    let workspace_root = temp_workspace_root("go-to-definition-scip-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn caller() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#User", "range": [0, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#User", "range": [1, 33, 37], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should retain partial precise definitions")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(
        response.matches[0].precision.as_deref(),
        Some("precise_partial")
    );

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit partial precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["precision"], "precise_partial");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "artifact_budget_bytes"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_falls_back_when_partial_precise_has_no_target_match() {
    let workspace_root = temp_workspace_root("go-to-definition-partial-precise-absence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub struct User;\n\
         pub fn caller() { let _ = User; }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "other_symbol.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Admin", "range": [0, 0, 5], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Admin",
                  "display_name": "Admin",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );
    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should fall back when partial precise data lacks the target")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(
        note_json["precise_absence_reason"],
        "precise_partial_non_authoritative_absence"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_declarations_falls_back_to_heuristic_without_precise_data() {
    let server = server_for_fixture();
    let response = server
        .find_declarations(Parameters(FindDeclarationsParams {
            symbol: Some("greeting".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_declarations should return deterministic fallback")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "greeting");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("find_declarations should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_declarations note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["declaration_mode"], "definition_anchor_v1");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
}

#[tokio::test]
async fn navigation_find_implementations_falls_back_to_symbol_impl_heuristic() {
    let workspace_root = temp_workspace_root("navigation-implementations-heuristic");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should return deterministic heuristic fallback")
        .0;

    assert!(
        !response.matches.is_empty(),
        "expected heuristic implementation matches from symbol corpus fallback"
    );
    let first = &response.matches[0];
    assert_eq!(first.repository_id, "repo-001");
    assert_eq!(first.path, "src/lib.rs");
    assert_eq!(first.symbol, "Impl");
    assert_eq!(first.relation.as_deref(), Some("implements"));
    assert_eq!(first.precision.as_deref(), Some("heuristic"));
    assert_eq!(first.fallback_reason.as_deref(), Some("precise_absent"));

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit fallback metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");
    assert_eq!(
        note_json["precise"]["implementation_count"].as_u64(),
        Some(response.matches.len() as u64)
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_degrades_when_scip_artifact_exceeds_budget() {
    let workspace_root = temp_workspace_root("navigation-implementations-scip-budget");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "implementations.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Impl",
                  "display_name": "Impl",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                }
              ]
            }
          ]
        }"#,
    );

    let oversized_payload = format!(
        r#"{{
          "documents": [],
          "padding": "{}"
        }}"#,
        "x".repeat(4096)
    );
    write_scip_fixture(&workspace_root, "oversized.json", &oversized_payload);

    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 120);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should retain partial precise implementations")
        .0;

    assert!(
        !response.matches.is_empty(),
        "partial precise mode should still return implementation matches"
    );
    assert_eq!(
        response.matches[0].precision.as_deref(),
        Some("precise_partial")
    );
    assert_eq!(response.matches[0].fallback_reason, None);

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit partial precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "precise_partial");
    assert_eq!(note_json["heuristic"], false);
    assert_eq!(note_json["precise"]["coverage"], "partial");
    assert_eq!(note_json["precise"]["artifacts_ingested"], 1);
    assert_eq!(note_json["precise"]["artifacts_failed"], 1);
    assert_eq!(
        note_json["precise"]["failed_artifacts"][0]["stage"],
        "artifact_budget_bytes"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_implementations_and_call_hierarchy_prefer_precise_relationships() {
    let workspace_root = temp_workspace_root("navigation-precise-relationships");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct Impl;\n\
         impl Service for Impl {}\n\
         pub fn consumer() { let _ = ServiceMarker; }\n\
         pub struct ServiceMarker;\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "relationships.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#consumer", "range": [3, 7, 15], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Impl",
                  "display_name": "Impl",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#consumer",
                  "display_name": "consumer",
                  "kind": "function",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#,
    );
    let server = server_for_workspace_root(&workspace_root);

    let implementations = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should resolve precise relationships")
        .0;
    assert_eq!(implementations.matches.len(), 1);
    assert_eq!(implementations.matches[0].symbol, "Impl");
    assert_eq!(
        implementations.matches[0].relation.as_deref(),
        Some("implementation")
    );
    assert_eq!(
        implementations.matches[0].precision.as_deref(),
        Some("precise")
    );

    let incoming = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("incoming_calls should resolve precise relationships")
        .0;
    assert_eq!(incoming.matches.len(), 1);
    assert_eq!(incoming.matches[0].source_symbol, "consumer");
    assert_eq!(incoming.matches[0].target_symbol, "Service");
    assert_eq!(incoming.matches[0].relation, "calls");
    assert_eq!(incoming.matches[0].precision.as_deref(), Some("precise"));

    let outgoing = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("consumer".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("outgoing_calls should resolve precise relationships")
        .0;
    assert_eq!(outgoing.matches.len(), 1);
    assert_eq!(outgoing.matches[0].source_symbol, "consumer");
    assert_eq!(outgoing.matches[0].target_symbol, "Service");
    assert_eq!(outgoing.matches[0].relation, "calls");
    assert_eq!(outgoing.matches[0].precision.as_deref(), Some("precise"));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_find_implementations_prefers_relationship_bearing_precise_candidate_across_artifacts()
 {
    let workspace_root = temp_workspace_root("navigation-implementations-precise-overlay");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub struct ImplA;\n\
         impl Service for ImplA {}\n\
         pub struct ImplB;\n\
         impl Service for ImplB {}\n\
         pub struct ImplC;\n\
         impl Service for ImplC {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "a-canary.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#ImplA", "range": [1, 11, 16], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#ImplB", "range": [3, 11, 16], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#ImplC", "range": [5, 11, 16], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#ImplA",
                  "display_name": "ImplA",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#ImplB",
                  "display_name": "ImplB",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#ImplC",
                  "display_name": "ImplC",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                }
              ]
            }
          ]
        }"#,
    );
    write_scip_fixture(
        &workspace_root,
        "z-main.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "rust-analyzer cargo repo 0.1.0 svc/Service#", "range": [0, 10, 17], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "rust-analyzer cargo repo 0.1.0 svc/Service#",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should resolve precise overlay relationships")
        .0;

    assert_eq!(response.matches.len(), 3);
    assert_eq!(response.matches[0].symbol, "ImplA");
    assert_eq!(response.matches[1].symbol, "ImplB");
    assert_eq!(response.matches[2].symbol, "ImplC");
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.precision.as_deref() == Some("precise"))
    );

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_precise_symbol"],
        "scip-rust pkg repo#Service"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_incoming_calls_uses_precise_occurrences_when_relationships_are_absent() {
    let workspace_root = temp_workspace_root("navigation-incoming-precise-occurrences");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub trait Service {}\n\
         pub fn first(_service: &dyn Service) {}\n\
         pub fn second(_service: &dyn Service) {}\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "incoming.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Service", "range": [1, 28, 35], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#Service", "range": [2, 29, 36], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#first", "range": [1, 7, 12], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#second", "range": [2, 7, 13], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#first",
                  "display_name": "first",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#second",
                  "display_name": "second",
                  "kind": "function",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .incoming_calls(Parameters(IncomingCallsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("incoming_calls should derive precise callers from precise references")
        .0;

    assert_eq!(response.matches.len(), 2);
    assert_eq!(response.matches[0].source_symbol, "first");
    assert_eq!(response.matches[1].source_symbol, "second");
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.precision.as_deref() == Some("precise"))
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.relation == "refers_to")
    );

    let note = response
        .note
        .as_ref()
        .expect("incoming_calls should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("incoming_calls note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["precise"]["incoming_count"], 2);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_outline_for_supported_files() {
    let server = server_for_fixture();
    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "greeting" && symbol.kind == "function")
    );
    assert!(
        response
            .symbols
            .iter()
            .all(|symbol| symbol.path == "src/lib.rs" && symbol.repository_id == "repo-001")
    );

    let note = response
        .note
        .as_ref()
        .expect("document_symbols should emit metadata note");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("document_symbols note should be valid JSON");
    assert_eq!(note_json["source"], "tree_sitter");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn document_symbols_rejects_unsupported_extension_with_typed_error() {
    let server = server_for_fixture();
    let error = match server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "README.md".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
    {
        Ok(_) => panic!("unsupported document_symbols extension should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
}

#[tokio::test]
async fn document_symbols_rejects_over_budget_source_with_typed_error() {
    let workspace_root = temp_workspace_root("document-symbols-max-bytes");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(src_root.join("lib.rs"), "pub fn oversized_symbol() {}\n")
        .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root_with_max_file_bytes(&workspace_root, 8);

    let error = match server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
    {
        Ok(_) => panic!("over-budget document_symbols request should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));

    let data = error
        .data
        .as_ref()
        .expect("document_symbols over-budget error should carry structured data");
    assert_eq!(data["path"], "src/lib.rs");
    assert_eq!(data["max_bytes"], 8);
    assert!(
        data["bytes"]
            .as_u64()
            .expect("document_symbols bytes should be numeric")
            > 8
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_returns_deterministic_rust_matches() {
    let server = server_for_fixture();
    let first = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return matches")
        .0;
    let second = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"src/lib\.rs$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should be deterministic")
        .0;

    assert_eq!(
        first
            .matches
            .iter()
            .map(|matched| {
                (
                    matched.repository_id.clone(),
                    matched.path.clone(),
                    matched.line,
                    matched.column,
                    matched.end_line,
                    matched.end_column,
                    matched.excerpt.clone(),
                )
            })
            .collect::<Vec<_>>(),
        second
            .matches
            .iter()
            .map(|matched| {
                (
                    matched.repository_id.clone(),
                    matched.path.clone(),
                    matched.line,
                    matched.column,
                    matched.end_line,
                    matched.end_column,
                    matched.excerpt.clone(),
                )
            })
            .collect::<Vec<_>>()
    );
    assert!(!first.matches.is_empty());
    assert_eq!(first.matches[0].repository_id, "repo-001");
    assert_eq!(first.matches[0].path, "src/lib.rs");
    assert!(first.matches[0].line >= 1);

    let note = first
        .note
        .as_ref()
        .expect("search_structural should emit metadata note");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("search_structural note should be valid JSON");
    assert_eq!(note_json["source"], "tree_sitter_query");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn search_structural_rejects_unsupported_language_with_typed_error() {
    let server = server_for_fixture();
    let error = match server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("go".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: None,
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("unsupported structural search language should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
}

#[tokio::test]
async fn search_structural_rejects_invalid_query_with_typed_error() {
    let server = server_for_fixture();
    let error = match server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item @broken".to_owned(),
            language: Some("rust".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: None,
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("invalid structural query should fail"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert!(
        error.message.contains("invalid structural query"),
        "unexpected error message: {}",
        error.message
    );
}
