#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use frigg::domain::model::ReferenceMatchKind;
use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::{
    DocumentSymbolsParams, ExploreAnchor, ExploreCursor, ExploreOperation, ExploreParams,
    FindDeclarationsParams, FindImplementationsParams, FindReferencesParams, GoToDefinitionParams,
    IncomingCallsParams, ListRepositoriesParams, NavigationMode, OutgoingCallsParams,
    ReadFileParams, SearchHybridParams, SearchPatternType, SearchStructuralParams,
    SearchSymbolParams, SearchSymbolPathClass, SearchTextParams, WorkspaceAttachAction,
    WorkspaceAttachParams, WorkspaceCurrentParams, WorkspaceIndexComponentState,
    WorkspacePreciseState, WorkspaceResolveMode, WorkspaceStorageIndexState,
};
use frigg::settings::{
    FriggConfig, RuntimeProfile, SemanticRuntimeConfig, SemanticRuntimeProvider,
};
use frigg::storage::{
    DEFAULT_VECTOR_DIMENSIONS, ManifestEntry, SemanticChunkEmbeddingRecord, Storage,
    ensure_provenance_db_parent_dir, resolve_provenance_db_path,
};
use protobuf::{EnumOrUnknown, Message};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    SymbolInformation as ScipSymbolInformationProto,
};

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

fn server_for_fixture() -> FriggMcpServer {
    let config =
        FriggConfig::from_workspace_roots(vec![fresh_fixture_root("tool-handlers-fixture-server")])
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

fn error_data_field<'a>(error: &'a rmcp::ErrorData, key: &str) -> &'a serde_json::Value {
    error
        .data
        .as_ref()
        .and_then(|value| value.get(key))
        .unwrap_or_else(|| panic!("expected structured error data field `{key}`"))
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

fn extended_runtime_server_for_workspace_root(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    FriggMcpServer::new_with_runtime_options(config, false, true)
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
#[path = "tool_handlers/navigation.rs"]
mod navigation;
#[path = "tool_handlers/references.rs"]
mod references;
#[path = "tool_handlers/search_symbol.rs"]
mod search_symbol;
#[path = "tool_handlers/structural.rs"]
mod structural;
#[path = "tool_handlers/workspace.rs"]
mod workspace;
