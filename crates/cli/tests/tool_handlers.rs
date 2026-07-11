#![allow(clippy::panic)]

//! Integration tests for MCP tool handlers wired through the production server stack.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use frigg::domain::model::{ReferenceMatchKind, stable_repository_id_for_root};
use frigg::mcp::types::{
    DocumentSymbolsParams, DocumentSymbolsResponse, ExploreAnchor, ExploreCursor, ExploreOperation,
    ExploreParams, FindDeclarationsParams, FindDeclarationsResponse, FindImplementationsParams,
    FindImplementationsResponse, FindReferencesParams, FindReferencesResponse,
    GoToDefinitionParams, GoToDefinitionResponse, ImpactBundleParams, IncomingCallsParams,
    IncomingCallsResponse, InspectSyntaxTreeResponse, ListFilesParams, ListRepositoriesParams,
    NavigationMode, OutgoingCallsParams, OutgoingCallsResponse, ReadFileParams, ReadMatchParams,
    ReadPresentationMode, RecoveryFields, ResponseMode, ResultCompleteness, ResultUnit,
    SearchHybridParams, SearchHybridQueryShape, SearchHybridRankReason, SearchPatternType,
    SearchStructuralParams, SearchStructuralResponse, SearchSymbolParams, SearchSymbolPathClass,
    SearchSymbolResponse, SearchTextParams, StructuralResultMode, SyntaxTreeNodeItem,
    WorkspaceAttachAction, WorkspaceAttachParams, WorkspaceCurrentParams, WorkspaceParams,
    WorkspacePreciseState, WorkspaceResolveMode, WorkspaceStorageIndexState,
};
use frigg::mcp::{FriggMcpServer, ToolCallDisplayEvent, ToolCallDisplayStatus};
use frigg::settings::{
    FriggConfig, RuntimeProfile, SemanticRuntimeConfig, SemanticRuntimeCredentials,
    SemanticRuntimeProvider,
};
use frigg::storage::{
    DEFAULT_VECTOR_DIMENSIONS, ManifestEntry, SemanticChunkEmbeddingRecord, Storage,
    ensure_provenance_db_parent_dir, resolve_provenance_db_path,
};
use protobuf::{EnumOrUnknown, Message};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ErrorCode};
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    SymbolInformation as ScipSymbolInformationProto,
};
use serde::de::DeserializeOwned;

fn write_fixture_workspace(root: &Path) {
    fs::create_dir_all(root.join("src/nested")).expect("failed to create fixture source tree");
    fs::create_dir_all(root.join("logs")).expect("failed to create fixture log tree");
    fs::create_dir_all(root.join(".git")).expect("failed to create fixture git root");
    fs::write(
        root.join("README.md"),
        "# Manifest Determinism Fixture\n\nThis fixture is used by MCP tool tests.\n",
    )
    .expect("failed to seed fixture README");
    fs::write(
        root.join("src/lib.rs"),
        "pub fn greeting() -> &'static str {\n    \"hello from fixture\"\n}\n",
    )
    .expect("failed to seed fixture source");
    fs::write(root.join("src/nested/data.txt"), "alpha\nbeta\ngamma\n")
        .expect("failed to seed fixture nested data");
    fs::write(root.join("src/ignored.tmp"), "temporary artifact\n")
        .expect("failed to seed fixture tmp file");
    fs::write(
        root.join("logs/build.log"),
        "this log file should be ignored by .gitignore\n",
    )
    .expect("failed to seed fixture log");
    fs::write(root.join(".gitignore"), "*.tmp\n*.log\n.DS_Store\n")
        .expect("failed to seed fixture ignore file");
}

fn fresh_fixture_root(test_name: &str) -> PathBuf {
    let root = temp_workspace_root(test_name);
    write_fixture_workspace(&root);
    root
}

async fn server_for_fixture() -> FriggMcpServer {
    let config =
        FriggConfig::from_workspace_roots(vec![fresh_fixture_root("tool-handlers-fixture-server")])
            .expect("fixture root must produce valid config");
    let server = FriggMcpServer::new(config);
    attach_session_repositories(&server).await;
    server
}

async fn attach_session_repositories(server: &FriggMcpServer) {
    let Ok(listed) = server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
    else {
        return;
    };
    for repository in listed.0.repositories {
        let _ = server
            .workspace_attach(Parameters(WorkspaceAttachParams {
                path: None,
                repository_id: Some(repository.repository_id),
                set_default: Some(true),
                resolve_mode: None,
                wait_for_precise: Some(false),
            }))
            .await;
    }
}

async fn public_repository_id(server: &FriggMcpServer) -> String {
    server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed")
        .0
        .repositories
        .first()
        .expect("test server should expose one repository")
        .repository_id
        .clone()
}

fn stable_public_repository_id_for_root(root: &Path) -> String {
    stable_repository_id_for_root(root).0
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

fn error_data_field<'a>(error: &'a rmcp::ErrorData, key: &str) -> &'a serde_json::Value {
    error
        .data
        .as_ref()
        .and_then(|value| value.get(key))
        .unwrap_or_else(|| panic!("expected structured error data field `{key}`"))
}

fn structured_tool_result<T: DeserializeOwned>(result: CallToolResult) -> T {
    let structured = result
        .structured_content
        .unwrap_or_else(|| panic!("expected structured_content in tool result"));
    serde_json::from_value(structured)
        .unwrap_or_else(|err| panic!("structured_content should deserialize: {err}"))
}

fn tool_result_text(result: &CallToolResult) -> &str {
    result
        .content
        .first()
        .and_then(|content| content.as_text())
        .map(|text| text.text.as_str())
        .unwrap_or_else(|| panic!("expected first tool result content item to be text"))
}

fn assert_omits_absent_metadata_and_note<T: serde::Serialize>(tool_name: &str, response: &T) {
    let value = serde_json::to_value(response)
        .unwrap_or_else(|err| panic!("{tool_name} response should serialize: {err}"));
    assert!(
        value.get("metadata").is_none(),
        "{tool_name} should omit absent metadata instead of serializing null"
    );
    assert!(
        value.get("note").is_none(),
        "{tool_name} should omit absent note instead of serializing null"
    );
}

#[test]
fn metadata_note_responses_omit_absent_fields_on_wire() {
    assert_omits_absent_metadata_and_note(
        "search_symbol",
        &SearchSymbolResponse {
            matches: Vec::new(),
            completeness: ResultCompleteness::complete(ResultUnit::Symbol, 0, 0)
                .expect("empty symbol completeness"),
            result_handle: None,
            handle_scope: None,
            handle_expires: None,
            latency_class: None,
            metadata: None,
            note: None,
            recovery: RecoveryFields::default(),
        },
    );
    assert_omits_absent_metadata_and_note(
        "find_references",
        &FindReferencesResponse {
            total_matches: 0,
            matches: Vec::new(),
            result_handle: None,
            handle_scope: None,
            handle_expires: None,
            mode: NavigationMode::UnavailableNoPrecise,
            target_selection: None,
            metadata: None,
            note: None,
            recovery: RecoveryFields::default(),
        },
    );
    assert_omits_absent_metadata_and_note(
        "go_to_definition",
        &GoToDefinitionResponse {
            matches: Vec::new(),
            result_handle: None,
            handle_scope: None,
            handle_expires: None,
            mode: NavigationMode::UnavailableNoPrecise,
            target_selection: None,
            metadata: None,
            note: None,
            location_warning: None,
            ambiguous_location: None,
            recovery: RecoveryFields::default(),
        },
    );
    assert_omits_absent_metadata_and_note(
        "find_declarations",
        &FindDeclarationsResponse {
            matches: Vec::new(),
            result_handle: None,
            mode: NavigationMode::UnavailableNoPrecise,
            target_selection: None,
            metadata: None,
            note: None,

            recovery: RecoveryFields::default(),
        },
    );
    assert_omits_absent_metadata_and_note(
        "find_implementations",
        &FindImplementationsResponse {
            matches: Vec::new(),
            result_handle: None,
            mode: NavigationMode::UnavailableNoPrecise,
            target_selection: None,
            metadata: None,
            note: None,

            recovery: RecoveryFields::default(),
        },
    );
    assert_omits_absent_metadata_and_note(
        "incoming_calls",
        &IncomingCallsResponse {
            matches: Vec::new(),
            result_handle: None,
            mode: NavigationMode::UnavailableNoPrecise,
            availability: None,
            target_selection: None,
            metadata: None,
            note: None,

            recovery: RecoveryFields::default(),
        },
    );
    assert_omits_absent_metadata_and_note(
        "outgoing_calls",
        &OutgoingCallsResponse {
            matches: Vec::new(),
            result_handle: None,
            mode: NavigationMode::UnavailableNoPrecise,
            availability: None,
            target_selection: None,
            metadata: None,
            note: None,

            trust: frigg::mcp::types::NavigationEdgeTrust::Provisional,
            trust_note: frigg::mcp::types::OUTGOING_CALLS_TRUST_NOTE.to_owned(),
            recovery: RecoveryFields::default(),
        },
    );
    assert_omits_absent_metadata_and_note(
        "document_symbols",
        &DocumentSymbolsResponse {
            total_symbols: 0,
            returned: 0,
            truncated: false,
            resume_from: None,
            completeness: ResultCompleteness::complete(ResultUnit::DocumentSymbol, 0, 0).unwrap(),
            top_level_only: true,
            symbols: Vec::new(),
            result_handle: None,
            metadata: None,
            note: None,
        },
    );

    let node = SyntaxTreeNodeItem {
        kind: "source_file".to_owned(),
        named: true,
        path: "src/lib.rs".to_owned(),
        line: 1,
        column: 1,
        end_line: 1,
        end_column: 1,
        excerpt: String::new(),
    };
    assert_omits_absent_metadata_and_note(
        "inspect_syntax_tree",
        &InspectSyntaxTreeResponse {
            repository_id: "repo-001".to_owned(),
            path: "src/lib.rs".to_owned(),
            language: "rust".to_owned(),
            focus: node,
            ancestors: Vec::new(),
            children: Vec::new(),
            ancestors_completeness: ResultCompleteness::complete(ResultUnit::SyntaxNode, 0, 0)
                .unwrap(),
            children_completeness: ResultCompleteness::complete(ResultUnit::SyntaxNode, 0, 0)
                .unwrap(),
            follow_up_structural: Vec::new(),
            metadata: None,
            note: None,
        },
    );
    assert_omits_absent_metadata_and_note(
        "search_structural",
        &SearchStructuralResponse {
            matches: Vec::new(),
            result_mode: StructuralResultMode::Matches,
            completeness: ResultCompleteness::complete(ResultUnit::StructuralMatch, 0, 0).unwrap(),
            metadata: None,
            note: None,
        },
    );
}

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::current_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join(".codex-cache")
        .join("tool-handler-tests")
        .join(format!(
            "frigg-mcp-tool-handlers-{test_name}-{}-{nanos_since_epoch}",
            std::process::id()
        ))
}

async fn server_for_workspace_root(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new(config);
    attach_session_repositories(&server).await;
    server
}

async fn extended_runtime_server_for_workspace_root(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_runtime_options(config, true);
    attach_session_repositories(&server).await;
    server
}

fn server_for_config(config: FriggConfig) -> FriggMcpServer {
    config
        .validate_for_serving()
        .expect("test config must validate for serving");
    FriggMcpServer::new(config)
}

fn server_for_config_with_semantic_runtime_credentials(
    config: FriggConfig,
    credentials: SemanticRuntimeCredentials,
) -> FriggMcpServer {
    config
        .validate_for_serving()
        .expect("test config must validate for serving");
    FriggMcpServer::new_with_semantic_runtime_credentials(config, credentials)
}

async fn server_for_workspace_root_with_max_file_bytes(
    workspace_root: &Path,
    max_file_bytes: usize,
) -> FriggMcpServer {
    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    config.max_file_bytes = max_file_bytes;
    config.full_scip_ingest = false;
    let server = FriggMcpServer::new(config);
    attach_session_repositories(&server).await;
    server
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

fn seed_semantic_embeddings(
    workspace_root: &Path,
    repository_id: &str,
    snapshot_id: &str,
    records: &[SemanticChunkEmbeddingRecord],
) {
    let db_path =
        ensure_provenance_db_parent_dir(workspace_root).expect("semantic storage path should work");
    let resolved_db_path =
        resolve_provenance_db_path(workspace_root).expect("semantic db path should resolve");
    assert_eq!(db_path, resolved_db_path);

    let storage = Storage::new(db_path);
    storage
        .initialize()
        .expect("semantic storage should initialize");
    storage
        .replace_semantic_embeddings_for_repository(
            repository_id,
            snapshot_id,
            records
                .first()
                .map(|record| record.provider.as_str())
                .expect("semantic seed records require a provider"),
            records
                .first()
                .map(|record| record.model.as_str())
                .expect("semantic seed records require a model"),
            records,
        )
        .expect("semantic embeddings should persist");
}

fn semantic_record(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    chunk_index: usize,
    embedding: Vec<f32>,
) -> SemanticChunkEmbeddingRecord {
    let mut embedding = embedding;
    embedding.resize(DEFAULT_VECTOR_DIMENSIONS, 0.0);
    SemanticChunkEmbeddingRecord {
        chunk_id: format!("chunk-{}-{chunk_index}", path.replace('/', "_")),
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        path: path.to_owned(),
        language: "rust".to_owned(),
        chunk_index,
        start_line: 1,
        end_line: 1,
        provider: "openai".to_owned(),
        model: "text-embedding-3-small".to_owned(),
        trace_id: Some("trace-001".to_owned()),
        content_hash_blake3: format!("hash-{}-{chunk_index}", path.replace('/', "_")),
        content_text: path.to_owned(),
        embedding,
    }
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

#[path = "tool_handlers/core.rs"]
mod core;
#[path = "tool_handlers/document_symbols.rs"]
mod document_symbols;
#[path = "tool_handlers/freshness_ignore.rs"]
mod freshness_ignore;
#[path = "tool_handlers/handles_futura.rs"]
mod handles_futura;
#[path = "tool_handlers/navigation.rs"]
mod navigation;
#[path = "tool_handlers/proof_handle_producers.rs"]
mod proof_handle_producers;
#[path = "tool_handlers/references.rs"]
mod references;
#[path = "tool_handlers/search_batch.rs"]
mod search_batch;
#[path = "tool_handlers/search_symbol.rs"]
mod search_symbol;
#[path = "tool_handlers/search_text_futura.rs"]
mod search_text_futura;
#[path = "tool_handlers/structural.rs"]
mod structural;
#[path = "tool_handlers/workspace.rs"]
mod workspace;
