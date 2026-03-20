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
    assert!(first.repositories[0].storage.is_some());
    assert!(first.repositories[0].health.is_some());
    assert_eq!(
        first.repositories[0]
            .health
            .as_ref()
            .map(|health| health.lexical.state),
        second.repositories[0]
            .health
            .as_ref()
            .map(|health| health.lexical.state)
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
            path: Some(nested_path.display().to_string()),
            repository_id: None,
            set_default: None,
            resolve_mode: None,
        }))
        .await
        .expect("workspace_attach should succeed for fixture file path")
        .0;
    assert_eq!(first.repository.repository_id, "repo-001");
    assert_eq!(first.resolution, WorkspaceResolveMode::GitRoot);
    assert!(first.session_default);
    assert_eq!(first.action, WorkspaceAttachAction::AttachedFresh);
    assert_ne!(first.storage.index_state, WorkspaceStorageIndexState::Error);
    assert!(matches!(
        first.precise.state,
        WorkspacePreciseState::Ok
            | WorkspacePreciseState::Unavailable
            | WorkspacePreciseState::Partial
            | WorkspacePreciseState::Failed
    ));
    assert!(
        first.precise.generation_action.is_some(),
        "workspace_attach should always expose a top-level precise generation action summary"
    );
    if first.precise.failure_tool.is_some() {
        assert!(
            first.precise.failure_summary.is_some() || first.precise.failure_class.is_some(),
            "precise failures should surface a summary or typed failure class"
        );
    }
    assert!(first.repository.storage.is_none());
    assert!(first.repository.health.is_some());
    let serialized: serde_json::Value =
        serde_json::to_value(&first).expect("workspace_attach response should serialize");
    assert!(serialized.get("storage").is_some());
    assert!(
        serialized
            .get("repository")
            .and_then(|value| value.get("storage"))
            .is_none(),
        "workspace_attach should keep storage only at the top level"
    );

    let second = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(fixture_root().display().to_string()),
            repository_id: None,
            set_default: Some(false),
            resolve_mode: None,
        }))
        .await
        .expect("workspace_attach should reuse existing root")
        .0;
    assert_eq!(
        second.repository.repository_id,
        first.repository.repository_id
    );
    assert_eq!(second.action, WorkspaceAttachAction::ReusedWorkspace);

    let current = server
        .workspace_current(Parameters(WorkspaceCurrentParams {}))
        .await
        .expect("workspace_current should succeed")
        .0;
    assert!(current.session_default);
    let current_repository = current
        .repository
        .as_ref()
        .expect("workspace_current should return attached repository");
    assert_eq!(current_repository.repository_id, "repo-001");
    assert!(current_repository.health.is_some());
    assert_eq!(current.repositories.len(), 1);
    assert_eq!(current.repositories[0].repository_id, "repo-001");
    assert!(current.precise.is_some());
    let runtime = current
        .runtime
        .as_ref()
        .expect("workspace_current should expose runtime status");
    assert_eq!(runtime.profile, RuntimeProfile::StdioEphemeral);
    assert!(!runtime.persistent_state_available);
    assert!(!runtime.watch_active);
    assert_eq!(runtime.status_tool, "workspace_current");
    assert!(
        runtime
            .recent_provenance
            .iter()
            .any(|event| event.tool_name == "workspace_attach"),
        "workspace_current should surface recent provenance from prior attach"
    );
}

#[tokio::test]
async fn workspace_attach_reports_schema_only_storage_as_uninitialized() {
    let workspace_root = temp_workspace_root("workspace-attach-schema-only-storage");
    fs::create_dir_all(workspace_root.join("src")).expect("workspace src dir should be creatable");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn attached_only() -> &'static str { \"fixture\" }\n",
    )
    .expect("workspace source file should be writable");

    let db_path = ensure_provenance_db_parent_dir(&workspace_root)
        .expect("workspace storage path should resolve");
    let storage = Storage::new(db_path);
    storage
        .initialize()
        .expect("schema-only workspace storage should initialize");

    let mut config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: None,
            resolve_mode: Some(WorkspaceResolveMode::Direct),
        }))
        .await
        .expect("workspace_attach should succeed for schema-only storage")
        .0;

    assert_eq!(
        response.storage.index_state,
        WorkspaceStorageIndexState::Uninitialized
    );
    assert!(response.storage.exists);
    assert!(
        response.storage.initialized,
        "storage summary should still report that the schema exists even when no manifest snapshot has been indexed"
    );
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .map(|health| health.lexical.state),
        Some(WorkspaceIndexComponentState::Missing)
    );
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .and_then(|health| health.lexical.reason.as_deref()),
        Some("missing_manifest_snapshot")
    );
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .map(|health| health.semantic.state),
        Some(WorkspaceIndexComponentState::Missing)
    );
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .and_then(|health| health.semantic.reason.as_deref()),
        Some("missing_manifest_snapshot")
    );
    let serialized: serde_json::Value =
        serde_json::to_value(&response).expect("workspace_attach response should serialize");
    assert!(
        serialized
            .pointer("/repository/health/lexical/artifact_count")
            .is_none(),
        "unknown lexical artifact counts should be omitted instead of serialized as null"
    );
    assert!(
        serialized
            .pointer("/repository/health/semantic/artifact_count")
            .is_none(),
        "unknown semantic artifact counts should be omitted instead of serialized as null"
    );

    fs::remove_dir_all(&workspace_root).expect("temporary workspace should clean up");
}

#[tokio::test]
async fn workspace_attach_reports_known_lexical_and_semantic_artifact_counts() {
    let workspace_root = temp_workspace_root("workspace-attach-artifact-counts");
    fs::create_dir_all(workspace_root.join("src")).expect("workspace src dir should be creatable");
    fs::write(
        workspace_root.join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("workspace source file should be writable");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn helper() -> &'static str { \"fixture\" }\n",
    )
    .expect("workspace source file should be writable");

    seed_manifest_snapshot(
        &workspace_root,
        "repo-001",
        "snapshot-001",
        &["src/main.rs", "src/lib.rs"],
    );
    seed_semantic_embeddings(
        &workspace_root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record("repo-001", "snapshot-001", "src/main.rs", 0, vec![1.0, 0.0]),
            semantic_record("repo-001", "snapshot-001", "src/lib.rs", 0, vec![0.6, 0.0]),
        ],
    );

    let mut config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: None,
            resolve_mode: Some(WorkspaceResolveMode::Direct),
        }))
        .await
        .expect("workspace_attach should succeed for indexed workspace")
        .0;

    let health = response
        .repository
        .health
        .as_ref()
        .expect("workspace_attach should include health");
    assert_eq!(health.lexical.state, WorkspaceIndexComponentState::Ready);
    assert_eq!(health.lexical.artifact_count, Some(2));
    assert_eq!(health.semantic.state, WorkspaceIndexComponentState::Ready);
    assert_eq!(health.semantic.artifact_count, Some(2));

    let serialized: serde_json::Value =
        serde_json::to_value(&response).expect("workspace_attach response should serialize");
    assert_eq!(
        serialized.pointer("/repository/health/lexical/artifact_count"),
        Some(&serde_json::Value::from(2))
    );
    assert_eq!(
        serialized.pointer("/repository/health/semantic/artifact_count"),
        Some(&serde_json::Value::from(2))
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn workspace_session_default_scopes_search_text_without_repository_hint() {
    let root_a = temp_workspace_root("workspace-default-a");
    let root_b = temp_workspace_root("workspace-default-b");
    fs::create_dir_all(root_a.join("src")).expect("workspace a src dir should be creatable");
    fs::create_dir_all(root_b.join("src")).expect("workspace b src dir should be creatable");
    fs::write(
        root_a.join("src/lib.rs"),
        "pub fn shared_marker() { /* repo_a */ }\n",
    )
    .expect("workspace a source should write");
    fs::write(
        root_b.join("src/lib.rs"),
        "pub fn shared_marker() { /* repo_b */ }\n",
    )
    .expect("workspace b source should write");

    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );

    let attached_a = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(root_a.display().to_string()),
            repository_id: None,
            set_default: Some(false),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
        }))
        .await
        .expect("workspace_attach should attach repo a")
        .0;
    let attached_b = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(root_b.display().to_string()),
            repository_id: None,
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
    assert_eq!(
        response.matches[0].repository_id,
        attached_b.repository.repository_id
    );
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
    assert_eq!(
        error.message, "line_end must be greater than or equal to line_start",
        "invalid read_file line ranges should preserve the typed invalid_params message"
    );
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
    assert_eq!(response.total_matches, 1);
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
async fn core_search_hybrid_returns_deterministic_matches_and_metadata_only() {
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
    assert_eq!(first.matches[0].line, 2);
    assert_eq!(first.matches[0].column, 6);
    assert_eq!(
        first.matches[0]
            .anchor
            .as_ref()
            .map(|anchor| anchor.start_line),
        Some(2)
    );
    assert_eq!(
        first.matches[0]
            .anchor
            .as_ref()
            .map(|anchor| anchor.start_column),
        Some(6)
    );
    assert_eq!(first.semantic_requested, None);
    assert_eq!(first.semantic_enabled, None);
    assert_eq!(first.semantic_status, None);
    assert_eq!(first.semantic_hit_count, None);
    assert_eq!(first.semantic_match_count, None);
    assert_eq!(first.semantic_reason, None);
    assert_eq!(first.warning, None);
    assert_eq!(first.note, None);
    assert!(
        first.matches[0].blended_score >= 0.0,
        "hybrid blended score should be non-negative"
    );

    let structured: serde_json::Value =
        serde_json::to_value(&first).expect("search_hybrid response should serialize");
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("requested_language"))
            .and_then(|value| value.as_str()),
        Some("rust")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("semantic_chunking"))
            .and_then(|value| value.as_str()),
        Some("optional_accelerator")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("capabilities"))
            .and_then(|value| value.get("symbol_corpus"))
            .and_then(|value| value.as_str()),
        Some("core")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("tier"))
            .and_then(|value| value.as_str()),
        Some("optional_accelerator")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_capability"))
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("state"))
            .and_then(|value| value.as_str()),
        Some("disabled_by_request")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("channels"))
            .and_then(|value| value.get("lexical_manifest"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("ok")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("channels"))
            .and_then(|value| value.get("path_surface_witness"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("filtered")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("channels"))
            .and_then(|value| value.get("semantic"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("disabled")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        Some("disabled")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_enabled"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_requested"))
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_candidate_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_hit_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_match_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_reason"))
            .and_then(|value| value.as_str()),
        Some("semantic channel disabled by request toggle")
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("warning"))
            .and_then(|value| value.as_str()),
        Some(
            "semantic retrieval is disabled; results are ranked from lexical and graph signals only (semantic channel disabled by request toggle)"
        )
    );
    for field in [
        "semantic_requested",
        "semantic_enabled",
        "semantic_status",
        "semantic_reason",
        "semantic_hit_count",
        "semantic_match_count",
        "warning",
        "note",
    ] {
        assert!(
            structured.get(field).is_none(),
            "search_hybrid should omit duplicate top-level field `{field}` when metadata is present"
        );
    }
    assert!(
        structured
            .get("metadata")
            .and_then(|value| value.get("stage_attribution"))
            .and_then(|value| value.get("candidate_intake"))
            .and_then(|value| value.get("output_count"))
            .and_then(|value| value.as_u64())
            .is_some_and(|value| value >= 1),
        "search_hybrid metadata should expose candidate intake counts"
    );
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("stage_attribution"))
            .and_then(|value| value.get("semantic_retrieval"))
            .and_then(|value| value.get("output_count"))
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert!(
        structured
            .get("metadata")
            .and_then(|value| value.get("stage_attribution"))
            .and_then(|value| value.get("scan"))
            .and_then(|value| value.get("elapsed_us"))
            .and_then(|value| value.as_u64())
            .is_some(),
        "search_hybrid metadata should expose additive stage attribution"
    );
    let second_metadata = second
        .metadata
        .as_ref()
        .map(|metadata| serde_json::to_value(metadata).expect("metadata should serialize"));
    assert_eq!(
        structured
            .get("metadata")
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        second_metadata
            .as_ref()
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        "metadata-only semantic status should remain deterministic"
    );
    let freshness_cacheable = structured
        .get("metadata")
        .and_then(|value| value.get("freshness_basis"))
        .and_then(|value| value.get("cacheable"))
        .and_then(|value| value.as_bool());
    if freshness_cacheable == Some(true) {
        assert_eq!(
            structured
                .get("metadata")
                .and_then(|value| value.get("stage_attribution")),
            second_metadata
                .as_ref()
                .and_then(|value| value.get("stage_attribution")),
            "cacheable search_hybrid responses should keep stage attribution stable within the session"
        );
    } else {
        assert!(
            second_metadata
                .as_ref()
                .and_then(|value| value.get("stage_attribution"))
                .and_then(|value| value.get("scan"))
                .and_then(|value| value.get("elapsed_us"))
                .and_then(|value| value.as_u64())
                .is_some(),
            "non-cacheable search_hybrid responses should still report stage attribution on repeated calls"
        );
    }
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
    assert_eq!(
        error.message, "query must not be empty",
        "empty search_hybrid queries should return the typed invalid_params message"
    );
}

#[tokio::test]
async fn core_search_hybrid_surfaces_degraded_warning_when_semantic_runtime_fails_non_strict() {
    let mut config = FriggConfig::from_workspace_roots(vec![fixture_root()])
        .expect("fixture root must produce valid config");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(true),
        }))
        .await
        .expect("non-strict semantic startup failure should degrade, not hard-fail")
        .0;

    assert_eq!(response.semantic_requested, None);
    assert_eq!(response.semantic_enabled, None);
    assert_eq!(response.semantic_status, None);
    assert_eq!(response.semantic_hit_count, None);
    assert_eq!(response.semantic_match_count, None);
    assert_eq!(response.note, None);
    let metadata = serde_json::to_value(
        response
            .metadata
            .as_ref()
            .expect("search_hybrid should emit structured metadata"),
    )
    .expect("metadata should serialize");
    assert_eq!(
        metadata
            .get("channels")
            .and_then(|value| value.get("semantic"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("degraded")
    );
    assert_eq!(
        metadata
            .get("semantic_status")
            .and_then(|value| value.as_str()),
        Some("degraded")
    );
    assert_eq!(
        metadata
            .get("semantic_enabled")
            .and_then(|value| value.as_bool()),
        Some(false)
    );
    assert_eq!(
        metadata
            .get("semantic_requested")
            .and_then(|value| value.as_bool()),
        Some(true)
    );
    assert_eq!(
        metadata
            .get("semantic_candidate_count")
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        metadata
            .get("semantic_hit_count")
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        metadata
            .get("semantic_match_count")
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert!(
        metadata
            .get("semantic_reason")
            .and_then(|value| value.as_str())
            .is_some_and(|reason| reason.contains("semantic runtime validation failed")),
        "degraded semantic reason should explain the validation failure"
    );
    assert!(
        metadata
            .get("warning")
            .and_then(|value| value.as_str())
            .is_some_and(|warning| warning.starts_with(
                "semantic retrieval is degraded; semantic contribution may be partial"
            )),
        "degraded search_hybrid response should emit a clear warning"
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_chunking"))
            .and_then(|value| value.as_str()),
        Some("optional_accelerator")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("state"))
            .and_then(|value| value.as_str()),
        Some("degraded_runtime")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("status"))
            .and_then(|value| value.as_str()),
        Some("degraded")
    );
}

#[tokio::test]
async fn core_search_hybrid_marks_unsupported_semantic_language_filters_as_unavailable() {
    let mut config = FriggConfig::from_workspace_roots(vec![fixture_root()])
        .expect("fixture root must produce valid config");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "hello from fixture".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("typescript".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(true),
        }))
        .await
        .expect("unsupported semantic language filters should degrade to metadata, not fail")
        .0;

    let metadata = serde_json::to_value(
        response
            .metadata
            .as_ref()
            .expect("search_hybrid should emit structured metadata"),
    )
    .expect("metadata should serialize");
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_chunking"))
            .and_then(|value| value.as_str()),
        Some("unsupported")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("capabilities"))
            .and_then(|value| value.get("symbol_corpus"))
            .and_then(|value| value.as_str()),
        Some("core")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("tier"))
            .and_then(|value| value.as_str()),
        Some("unsupported")
    );
    assert_eq!(
        metadata
            .get("semantic_capability")
            .and_then(|value| value.get("semantic_accelerator"))
            .and_then(|value| value.get("state"))
            .and_then(|value| value.as_str()),
        Some("unsupported_language")
    );
    assert_eq!(
        metadata
            .get("semantic_status")
            .and_then(|value| value.as_str()),
        Some("unavailable")
    );
    assert_eq!(
        metadata
            .get("semantic_reason")
            .and_then(|value| value.as_str()),
        Some("requested language filter 'typescript' does not support semantic_chunking")
    );
    assert!(
        metadata
            .get("warning")
            .and_then(|value| value.as_str())
            .is_some_and(|warning| warning.contains("semantic retrieval is unavailable")),
        "unsupported semantic language filters should surface an unavailable warning"
    );
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
async fn extended_explore_probe_zoom_and_refine_are_deterministic() {
    let workspace_root = temp_workspace_root("explore-deterministic");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create explorer fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn demo() {\n\
         \x20\x20\x20\x20let needle_alpha = 1;\n\
         \x20\x20\x20\x20let helper_alpha = needle_alpha;\n\
         \x20\x20\x20\x20let needle_beta = 2;\n\
         \x20\x20\x20\x20let helper_beta = needle_beta;\n\
         \x20\x20\x20\x20let needle_gamma = 3;\n\
         }\n",
    )
    .expect("failed to seed explorer fixture");

    let server = extended_runtime_server_for_workspace_root(&workspace_root);
    let probe_params = ExploreParams {
        path: "src/lib.rs".to_owned(),
        repository_id: Some("repo-001".to_owned()),
        operation: ExploreOperation::Probe,
        query: Some("let needle_".to_owned()),
        pattern_type: Some(SearchPatternType::Literal),
        anchor: None,
        context_lines: Some(1),
        max_matches: Some(2),
        resume_from: None,
    };

    let first = server
        .explore(Parameters(probe_params.clone()))
        .await
        .expect("explore probe should succeed")
        .0;
    let second = server
        .explore(Parameters(probe_params))
        .await
        .expect("explore probe should be deterministic")
        .0;
    assert_eq!(first, second);
    assert_eq!(first.total_lines, 7);
    assert_eq!(first.total_matches, 3);
    assert_eq!(first.matches.len(), 2);
    assert!(first.truncated);
    assert_eq!(
        first.resume_from.as_ref().map(|cursor| cursor.line),
        Some(6)
    );
    assert_eq!(
        first.resume_from.as_ref().map(|cursor| cursor.column),
        Some(5)
    );
    assert_eq!(first.matches[0].window.start_line, 1);
    assert_eq!(first.matches[0].window.end_line, 3);
    assert_eq!(first.matches[1].window.start_line, 3);
    assert_eq!(first.matches[1].window.end_line, 5);

    let resumed = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: Some("let needle_".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: None,
            context_lines: Some(1),
            max_matches: Some(2),
            resume_from: first.resume_from.clone(),
        }))
        .await
        .expect("explore probe resume should succeed")
        .0;
    assert_eq!(resumed.total_matches, 1);
    assert_eq!(resumed.matches.len(), 1);
    assert!(!resumed.truncated);
    assert_eq!(resumed.matches[0].start_line, 6);

    let anchor = first.matches[1].anchor.clone();
    let zoom = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Zoom,
            query: None,
            pattern_type: None,
            anchor: Some(anchor.clone()),
            context_lines: Some(1),
            max_matches: None,
            resume_from: None,
        }))
        .await
        .expect("explore zoom should succeed")
        .0;
    assert_eq!(zoom.total_matches, 0);
    assert!(zoom.matches.is_empty());
    assert!(!zoom.truncated);
    assert_eq!(
        zoom.window.as_ref().map(|window| window.start_line),
        Some(3)
    );
    assert_eq!(zoom.window.as_ref().map(|window| window.end_line), Some(5));

    let refine = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Refine,
            query: Some("helper_".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: Some(anchor),
            context_lines: Some(1),
            max_matches: Some(5),
            resume_from: None,
        }))
        .await
        .expect("explore refine should succeed")
        .0;
    assert_eq!(refine.scan_scope.start_line, 3);
    assert_eq!(refine.scan_scope.end_line, 5);
    assert_eq!(refine.total_matches, 2);
    assert_eq!(refine.matches.len(), 2);
    assert_eq!(refine.matches[0].start_line, 3);
    assert_eq!(refine.matches[1].start_line, 5);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn extended_explore_rejects_invalid_mode_payloads() {
    let workspace_root = temp_workspace_root("explore-invalid-payloads");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create explorer invalid fixture");
    fs::write(src_root.join("lib.rs"), "pub fn demo() {}\n")
        .expect("failed to seed explorer invalid fixture");

    let server = extended_runtime_server_for_workspace_root(&workspace_root);
    let probe_error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Probe,
            query: None,
            pattern_type: None,
            anchor: None,
            context_lines: None,
            max_matches: None,
            resume_from: None,
        }))
        .await
        .err()
        .expect("probe without query should fail");
    assert_eq!(probe_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&probe_error), Some("invalid_params"));
    assert_eq!(probe_error.message, "query must not be empty");

    let zoom_error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Zoom,
            query: Some("demo".to_owned()),
            pattern_type: None,
            anchor: None,
            context_lines: None,
            max_matches: None,
            resume_from: None,
        }))
        .await
        .err()
        .expect("zoom with query should fail");
    assert_eq!(zoom_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&zoom_error), Some("invalid_params"));
    assert_eq!(zoom_error.message, "query is not allowed for zoom");

    let refine_error = server
        .explore(Parameters(ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            operation: ExploreOperation::Refine,
            query: Some("demo".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            anchor: Some(ExploreAnchor {
                start_line: 1,
                start_column: 8,
                end_line: 1,
                end_column: 12,
            }),
            context_lines: Some(0),
            max_matches: Some(1),
            resume_from: Some(ExploreCursor { line: 2, column: 1 }),
        }))
        .await
        .err()
        .expect("refine with resume_from outside scan scope should fail");
    assert_eq!(refine_error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&refine_error), Some("invalid_params"));
    assert_eq!(
        refine_error.message,
        "resume_from must stay within the refine scan scope"
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
            path_class: None,
            path_regex: None,
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
            path_class: None,
            path_regex: None,
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
async fn search_symbol_returns_blade_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-blade");
    let blade_root = workspace_root.join("resources/views/components/dashboard");
    fs::create_dir_all(&blade_root).expect("failed to create temporary blade fixture");
    fs::write(
        blade_root.join("panel.blade.php"),
        "@section('hero')\n\
         @props(['title' => 'Dashboard'])\n\
         <x-slot:icon />\n\
         <livewire:orders.table />\n",
    )
    .expect("failed to seed temporary blade fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "dashboard.panel".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should return blade component matches")
        .0;

    assert!(
        response
            .matches
            .iter()
            .any(|matched| matched.symbol == "dashboard.panel" && matched.kind == "component")
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.path == "resources/views/components/dashboard/panel.blade.php")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_returns_typescript_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-typescript");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("App.tsx"),
        "type Props = {};\n\
         export class DashboardCard {\n\
             title: string;\n\
             render(_props: Props) { return <Card />; }\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "DashboardCard".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should return typescript matches")
        .0;

    assert!(
        response
            .matches
            .iter()
            .any(|matched| matched.symbol == "DashboardCard" && matched.kind == "class")
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.path == "src/App.tsx")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_returns_python_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-python");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary python fixture");
    fs::write(
        src_root.join("app.py"),
        concat!(
            "type Alias = str\n",
            "class Service:\n",
            "    def run(self) -> Alias:\n",
            "        return \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary python fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "Service".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should return python matches")
        .0;

    assert!(
        response
            .matches
            .iter()
            .any(|matched| matched.symbol == "Service" && matched.kind == "class")
    );
    assert!(
        response
            .matches
            .iter()
            .all(|matched| matched.path == "src/app.py")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_returns_additional_language_symbols_from_runtime_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-additional-languages");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary multi-language fixture");
    fs::write(
        src_root.join("main.go"),
        concat!(
            "package main\n",
            "type GoService struct{}\n",
            "func goHelper() string { return \"ok\" }\n",
        ),
    )
    .expect("failed to seed temporary go fixture");
    fs::write(
        src_root.join("Main.kt"),
        concat!(
            "class KotlinService\n",
            "fun kotlinHelper(): String = \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary kotlin fixture");
    fs::write(
        src_root.join("init.lua"),
        concat!("function luaRun()\n", "    return \"ok\"\n", "end\n",),
    )
    .expect("failed to seed temporary lua fixture");
    fs::write(
        src_root.join("app.nim"),
        concat!("proc nimHelper(): string =\n", "  \"ok\"\n",),
    )
    .expect("failed to seed temporary nim fixture");
    fs::write(
        src_root.join("main.roc"),
        concat!("UserId := U64\n", "rocGreet = \\name -> name\n",),
    )
    .expect("failed to seed temporary roc fixture");
    let server = server_for_workspace_root(&workspace_root);

    for (query, expected_kind, expected_path) in [
        ("GoService", "struct", "src/main.go"),
        ("KotlinService", "class", "src/Main.kt"),
        ("luaRun", "function", "src/init.lua"),
        ("nimHelper", "function", "src/app.nim"),
        ("rocGreet", "function", "src/main.roc"),
    ] {
        let response = server
            .search_symbol(Parameters(SearchSymbolParams {
                query: query.to_owned(),
                repository_id: Some("repo-001".to_owned()),
                path_class: None,
                path_regex: None,
                limit: Some(20),
            }))
            .await
            .expect("search_symbol should return baseline-language matches")
            .0;

        assert!(
            response.matches.iter().any(|matched| {
                matched.symbol == query
                    && matched.kind == expected_kind
                    && matched.path == expected_path
            }),
            "expected {query} {expected_kind} match in {expected_path}, got {:?}",
            response
                .matches
                .iter()
                .map(|matched| (&matched.symbol, &matched.kind, &matched.path))
                .collect::<Vec<_>>()
        );
    }

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_resolves_php_canonical_queries() {
    let workspace_root = temp_workspace_root("search-symbol-php-canonical");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("OrderHandler.php"),
        "<?php\n\
         namespace App\\Handlers;\n\
         class OrderHandler {\n\
             public function handle(): void {}\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let class_response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "App\\Handlers\\OrderHandler".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should resolve canonical php class queries")
        .0;
    assert!(
        class_response.matches.iter().any(|matched| {
            matched.symbol == "OrderHandler"
                && matched.kind == "class"
                && matched.path == "src/OrderHandler.php"
        }),
        "expected canonical class query to resolve class symbol"
    );

    let method_response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "App\\Handlers\\OrderHandler::handle".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should resolve canonical php member queries")
        .0;
    assert!(
        method_response.matches.iter().any(|matched| {
            matched.symbol == "handle"
                && matched.kind == "method"
                && matched.path == "src/OrderHandler.php"
        }),
        "expected canonical member query to resolve method symbol"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_prefers_runtime_paths_within_same_lexical_rank() {
    let workspace_root = temp_workspace_root("search-symbol-runtime-first");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::create_dir_all(workspace_root.join("tests")).expect("failed to create tests fixture");
    fs::create_dir_all(workspace_root.join("benches")).expect("failed to create benches fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub fn run() {}\n")
        .expect("failed to write runtime symbol fixture");
    fs::write(workspace_root.join("tests/support.rs"), "pub fn run() {}\n")
        .expect("failed to write tests symbol fixture");
    fs::write(workspace_root.join("benches/bench.rs"), "pub fn run() {}\n")
        .expect("failed to write bench symbol fixture");

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "run".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should succeed")
        .0;

    let paths = response
        .matches
        .iter()
        .map(|matched| matched.path.as_str())
        .collect::<Vec<_>>();
    assert_eq!(paths[0], "src/lib.rs");
    assert!(paths.contains(&"tests/support.rs"));
    assert!(paths.contains(&"benches/bench.rs"));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_symbol_filters_by_path_class_and_path_regex() {
    let workspace_root = temp_workspace_root("search-symbol-filters");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::create_dir_all(workspace_root.join("tests")).expect("failed to create tests fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn run() {}\n\
         pub fn helper() {}\n",
    )
    .expect("failed to write runtime source");
    fs::write(workspace_root.join("tests/support.rs"), "pub fn run() {}\n")
        .expect("failed to write support source");

    let server = server_for_workspace_root(&workspace_root);
    let support_only = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "run".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: Some(SearchSymbolPathClass::Support),
            path_regex: None,
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should respect path_class filter")
        .0;
    assert_eq!(support_only.matches.len(), 1);
    assert_eq!(support_only.matches[0].path, "tests/support.rs");

    let runtime_slice = server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "run".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: Some(r"^src/.*\.rs$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_symbol should respect path_regex filter")
        .0;
    assert_eq!(runtime_slice.matches.len(), 1);
    assert_eq!(runtime_slice.matches[0].path, "src/lib.rs");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn core_search_symbol_rejects_abusive_path_regex_with_typed_invalid_params() {
    let server = server_for_fixture();
    let abusive_path_regex = "a".repeat(600);

    let error = match server
        .search_symbol(Parameters(SearchSymbolParams {
            query: "greeting".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            path_class: None,
            path_regex: Some(abusive_path_regex.clone()),
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("abusive search_symbol path_regex should be rejected"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert!(
        error.message.contains("invalid path_regex"),
        "path_regex validation should produce a typed invalid_params message"
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
async fn search_symbol_rebuilds_stale_manifest_snapshot_before_reusing_cached_corpus() {
    let workspace_root = temp_workspace_root("search-symbol-stale-manifest-snapshot");
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
            path_class: None,
            path_regex: None,
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
            path_class: None,
            path_regex: None,
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
            path_class: None,
            path_regex: None,
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
    let workspace_root = temp_workspace_root("search-symbol-stale-manifest-edit");
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
            path_class: None,
            path_regex: None,
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
            path_class: None,
            path_regex: None,
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
            path_class: None,
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_symbol should not reuse stale corpus matches")
        .0;
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_text_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("search-text-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_text(Parameters(SearchTextParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("initial search_text call should succeed")
        .0;
    assert_eq!(first.total_matches, 1);
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .search_text(Parameters(SearchTextParams {
            query: "beta_beta".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_text should bypass stale cache after edit")
        .0;
    assert_eq!(second.total_matches, 1);
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert!(second.matches[0].excerpt.contains("beta_beta"));

    let stale = server
        .search_text(Parameters(SearchTextParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            pattern_type: Some(SearchPatternType::Literal),
            path_regex: None,
            limit: Some(10),
        }))
        .await
        .expect("search_text should not reuse stale cached matches")
        .0;
    assert_eq!(stale.total_matches, 0);
    assert!(stale.matches.is_empty());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_hybrid_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("search-hybrid-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("initial search_hybrid call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "beta_beta".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("search_hybrid should bypass stale cache after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert!(second.matches[0].excerpt.contains("beta_beta"));
    assert_eq!(
        second
            .metadata
            .as_ref()
            .map(|metadata| serde_json::to_value(metadata).expect("metadata should serialize"))
            .as_ref()
            .and_then(|metadata| metadata.get("freshness_basis"))
            .and_then(|value| value.get("cacheable"))
            .and_then(|value| value.as_bool()),
        Some(false),
        "stale manifest-backed queries should surface non-cacheable freshness metadata until a fresh snapshot exists"
    );

    let stale = server
        .search_hybrid(Parameters(SearchHybridParams {
            query: "alpha".to_owned(),
            repository_id: Some("repo-001".to_owned()),
            language: Some("rust".to_owned()),
            limit: Some(10),
            weights: None,
            semantic: Some(false),
        }))
        .await
        .expect("search_hybrid should not reuse stale cached matches")
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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should return heuristic references")
        .0;

    assert!(
        response.matches.len() >= 2,
        "expected at least two deterministic heuristic references"
    );
    assert_eq!(response.total_matches, response.matches.len());
    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);
    assert_eq!(response.matches[0].column, 25);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit heuristic metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("find_references should emit typed metadata"),
        &note_json
    );
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
async fn find_references_includes_definition_when_requested_by_default() {
    let workspace_root = temp_workspace_root("find-references-include-definition");
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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: None,
            limit: Some(20),
        }))
        .await
        .expect("find_references should return heuristic references with a definition row")
        .0;

    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);
    assert_eq!(
        response
            .matches
            .first()
            .expect("definition row should be present")
            .match_kind,
        ReferenceMatchKind::Definition
    );
    assert!(
        response
            .matches
            .iter()
            .any(|entry| entry.match_kind == ReferenceMatchKind::Reference)
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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise references first")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 32);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise references from protobuf scip")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches[0].repository_id, "repo-001");
    assert_eq!(response.matches[0].symbol, "User");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 32);
    assert_eq!(
        response.matches[0].match_kind,
        ReferenceMatchKind::Reference
    );

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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should fall back to heuristic references")
        .0;

    assert!(
        response.matches.len() >= 2,
        "expected deterministic heuristic fallback references"
    );
    assert_eq!(response.mode, NavigationMode::HeuristicNoPrecise);

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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
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
            symbol: Some("invalid_params".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
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
async fn find_references_precise_results_stay_pinned_to_runtime_target_selection() {
    let workspace_root = temp_workspace_root("find-references-precise-target-pinning");
    let src_root = workspace_root.join("src");
    let benches_root = workspace_root.join("benches");
    fs::create_dir_all(&src_root).expect("failed to create runtime fixture");
    fs::create_dir_all(&benches_root).expect("failed to create bench fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn try_execute() {}\n\
         pub fn runtime_caller() { try_execute(); }\n",
    )
    .expect("failed to seed runtime source file");
    fs::write(
        benches_root.join("runtime_bottlenecks.rs"),
        "pub fn try_execute() {}\n\
         pub fn bench_caller() { try_execute(); }\n",
    )
    .expect("failed to seed bench source file");
    write_scip_fixture(
        &workspace_root,
        "target-pinning.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "range": [0, 7, 18], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#runtime_caller", "range": [1, 7, 21], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "range": [1, 26, 37], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#z_runtime_try_execute",
                  "display_name": "try_execute",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#runtime_caller",
                  "display_name": "runtime_caller",
                  "kind": "function",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "is_reference": true }
                  ]
                }
              ]
            },
            {
              "relative_path": "benches/runtime_bottlenecks.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#a_bench_try_execute", "range": [0, 7, 18], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#bench_caller", "range": [1, 7, 19], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#a_bench_try_execute", "range": [1, 24, 35], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#a_bench_try_execute",
                  "display_name": "try_execute",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#bench_caller",
                  "display_name": "bench_caller",
                  "kind": "function",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#a_bench_try_execute", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("try_execute".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should pin precise results to the selected runtime target")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "try_execute");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit target selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "symbol");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/lib.rs");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
    );
    assert_eq!(note_json["precise"]["reference_count"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_matches_precise_typescript_symbols_without_display_names() {
    let workspace_root = temp_workspace_root("find-references-typescript-symbol-tail");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("auth.ts"),
        "const requireServerUser = () => {};\n\
         export function handler() {\n\
             requireServerUser();\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    write_scip_fixture(
        &workspace_root,
        "typescript-tail.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "range": [0, 6, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:handler.",
                  "range": [1, 16, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "range": [2, 4, 21],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "display_name": "",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:handler.",
                  "display_name": "handler",
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
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve precise TypeScript references")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "requireServerUser");
    assert_eq!(response.matches[0].path, "src/auth.ts");
    assert_eq!(response.matches[0].line, 3);
    assert_eq!(response.matches[0].column, 5);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_precise_symbol"],
        "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser."
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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
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
            symbol: Some("User".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
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
async fn find_references_prefers_location_resolution_when_symbol_and_location_are_both_supplied() {
    let workspace_root = temp_workspace_root("find-references-location-precedence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.php"),
        "<?php\nfunction alpha() {}\nfunction beta() {}\nalpha();\nbeta();\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.php".to_owned()),
            line: Some(3),
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should prefer location resolution")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "beta");
    assert_eq!(response.matches[0].path, "src/lib.php");
    assert_eq!(response.matches[0].line, 5);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location");
    assert_eq!(note_json["target_selection"]["selected_symbol"], "beta");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_resolves_location_only_requests() {
    let workspace_root = temp_workspace_root("find-references-location-only");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.php"),
        "<?php\nfunction alpha() {}\nfunction beta() {}\nalpha();\nbeta();\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .find_references(Parameters(FindReferencesParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.php".to_owned()),
            line: Some(3),
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
        .expect("find_references should resolve location-only requests")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "beta");
    assert_eq!(response.matches[0].path, "src/lib.php");
    assert_eq!(response.matches[0].line, 5);

    let note = response
        .note
        .as_ref()
        .expect("find_references should emit selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_references note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location");
    assert_eq!(note_json["target_selection"]["selected_symbol"], "beta");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn find_references_rejects_requests_without_symbol_or_location() {
    let server = server_for_fixture();
    let error = match server
        .find_references(Parameters(FindReferencesParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            include_definition: Some(false),
            limit: Some(20),
        }))
        .await
    {
        Ok(_) => panic!("find_references should reject requests without a symbol or location"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(error_code_tag(&error), Some("invalid_params"));
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error.message, "either `symbol` or (`path` + `line`) is required",
        "find_references should emit the typed invalid_params message when neither a symbol nor a location is provided"
    );
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
    assert_eq!(response.matches[0].kind.as_deref(), Some("struct"));
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit precision metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("go_to_definition should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["heuristic"], false);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("go-to-definition-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(10),
        }))
        .await
        .expect("initial go_to_definition call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "alpha");
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("beta_beta".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(10),
        }))
        .await
        .expect("go_to_definition should bypass stale cache after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "beta_beta");
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert_eq!(
        second
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("freshness_basis"))
            .and_then(|value| value.get("cacheable"))
            .and_then(|value| value.as_bool()),
        Some(false),
        "stale manifest-backed navigation should surface non-cacheable freshness metadata until a fresh snapshot exists"
    );

    let stale = match server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(10),
        }))
        .await
    {
        Ok(_) => panic!("go_to_definition should not reuse stale cached matches"),
        Err(error) => error,
    };
    assert_eq!(error_code_tag(&stale), Some("resource_not_found"));
    assert_eq!(retryable_tag(&stale), Some(false));

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
async fn navigation_go_to_definition_rust_use_path_prefers_imported_symbol_over_same_file_name() {
    let workspace_root = temp_workspace_root("go-to-definition-rust-use-import");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create rust fixture");
    fs::write(src_root.join("worker.rs"), "pub fn helper() {}\n")
        .expect("failed to seed imported helper fixture");
    let use_line = "use crate::worker::helper;\n";
    fs::write(
        src_root.join("app.rs"),
        format!("pub fn helper() {{}}\n{use_line}pub fn call() {{ helper(); }}\n"),
    )
    .expect("failed to seed ambiguous import fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/app.rs".to_owned()),
            line: Some(2),
            column: Some(use_line.find("helper").expect("import token present") + 1),
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should prefer the imported Rust symbol at use sites")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "helper");
    assert_eq!(response.matches[0].path, "src/worker.rs");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit location-token metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_rust");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_rust_reexport_alias_resolves_underlying_symbol() {
    let workspace_root = temp_workspace_root("go-to-definition-rust-reexport-alias");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create rust fixture");
    fs::write(src_root.join("worker.rs"), "pub fn helper() {}\n")
        .expect("failed to seed imported helper fixture");
    let reexport_line = "pub use crate::worker::helper as local_helper;\n";
    fs::write(
        src_root.join("lib.rs"),
        format!("{reexport_line}pub fn local_helper() {{}}\n"),
    )
    .expect("failed to seed re-export alias fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            line: Some(1),
            column: Some(
                reexport_line
                    .find("local_helper")
                    .expect("alias token present")
                    + 1,
            ),
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should resolve the underlying re-exported Rust symbol")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "helper");
    assert_eq!(response.matches[0].path, "src/worker.rs");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit location-token metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_rust");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_rust_method_call_prefers_impl_method_over_free_function() {
    let workspace_root = temp_workspace_root("go-to-definition-rust-method-vs-function");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create rust fixture");
    let call_line = "    fn call(&self) { self.render(); }\n";
    fs::write(
        src_root.join("lib.rs"),
        format!(
            "fn render() {{}}\n\
             trait Renderer {{ fn render(&self); }}\n\
             struct App;\n\
             impl Renderer for App {{\n\
                 fn render(&self) {{}}\n\
{call_line}\
             }}\n"
        ),
    )
    .expect("failed to seed rust method fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: None,
            repository_id: Some("repo-001".to_owned()),
            path: Some("src/lib.rs".to_owned()),
            line: Some(6),
            column: Some(call_line.rfind("render").expect("method token present") + 1),
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should prefer the impl method at a Rust field call site")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "render");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 5);
    assert_eq!(response.matches[0].kind.as_deref(), Some("method"));
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit location-token metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["resolution_source"], "location_token_rust");

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_prefers_runtime_paths_for_ambiguous_exact_name_queries() {
    let workspace_root = temp_workspace_root("go-to-definition-runtime-first");
    let src_root = workspace_root.join("src");
    let benches_root = workspace_root.join("benches");
    fs::create_dir_all(&src_root).expect("failed to create runtime fixture");
    fs::create_dir_all(&benches_root).expect("failed to create bench fixture");
    fs::write(src_root.join("lib.rs"), "pub fn try_execute() {}\n")
        .expect("failed to seed runtime fixture source");
    fs::write(
        benches_root.join("runtime_bottlenecks.rs"),
        "pub fn try_execute() {}\n",
    )
    .expect("failed to seed bench fixture source");

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("try_execute".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should prefer runtime code for ambiguous exact-name queries")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "try_execute");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit target selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/lib.rs");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
    );
    assert_eq!(note_json["target_selection"]["ambiguous_query"], true);
    assert_eq!(note_json["target_selection"]["candidate_count"], 2);
    assert_eq!(
        note_json["target_selection"]["same_rank_candidate_count"],
        2
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_go_to_definition_precise_results_stay_pinned_to_runtime_target_selection() {
    let workspace_root = temp_workspace_root("go-to-definition-precise-target-pinning");
    let src_root = workspace_root.join("src");
    let benches_root = workspace_root.join("benches");
    fs::create_dir_all(&src_root).expect("failed to create runtime fixture");
    fs::create_dir_all(&benches_root).expect("failed to create bench fixture");
    fs::write(src_root.join("lib.rs"), "pub fn try_execute() {}\n")
        .expect("failed to seed runtime fixture source");
    fs::write(
        benches_root.join("runtime_bottlenecks.rs"),
        "pub fn try_execute() {}\n",
    )
    .expect("failed to seed bench fixture source");
    write_scip_fixture(
        &workspace_root,
        "go_to_definition_target_pinning.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#z_runtime_try_execute", "range": [0, 7, 18], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#z_runtime_try_execute",
                  "display_name": "try_execute",
                  "kind": "function",
                  "relationships": []
                }
              ]
            },
            {
              "relative_path": "benches/runtime_bottlenecks.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#a_bench_try_execute", "range": [0, 7, 18], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#a_bench_try_execute",
                  "display_name": "try_execute",
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
        .go_to_definition(Parameters(GoToDefinitionParams {
            symbol: Some("try_execute".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("go_to_definition should keep precise definitions pinned to the selected runtime target")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "try_execute");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("go_to_definition should emit target selection metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("go_to_definition note should be valid JSON");
    assert_eq!(note_json["target_selection"]["selected_path"], "src/lib.rs");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
    );
    assert_eq!(note_json["precision"], "precise");

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
async fn navigation_find_declarations_does_not_reuse_stale_manifest_scoped_cache_after_edit() {
    let workspace_root = temp_workspace_root("find-declarations-stale-manifest-edit");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    let lib_path = src_root.join("lib.rs");
    fs::write(&lib_path, "pub fn alpha() {}\n").expect("failed to seed initial source");
    seed_manifest_snapshot(&workspace_root, "repo-001", "snapshot-001", &["src/lib.rs"]);

    let server = server_for_workspace_root(&workspace_root);
    let first = server
        .find_declarations(Parameters(FindDeclarationsParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(10),
        }))
        .await
        .expect("initial find_declarations call should succeed")
        .0;
    assert_eq!(first.matches.len(), 1);
    assert_eq!(first.matches[0].symbol, "alpha");
    assert_eq!(first.matches[0].path, "src/lib.rs");

    rewrite_file_with_new_mtime(&lib_path, "pub fn beta_beta() {}\n");

    let second = server
        .find_declarations(Parameters(FindDeclarationsParams {
            symbol: Some("beta_beta".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(10),
        }))
        .await
        .expect("find_declarations should bypass stale cache after edit")
        .0;
    assert_eq!(second.matches.len(), 1);
    assert_eq!(second.matches[0].symbol, "beta_beta");
    assert_eq!(second.matches[0].path, "src/lib.rs");
    assert_eq!(
        second
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("freshness_basis"))
            .and_then(|value| value.get("cacheable"))
            .and_then(|value| value.as_bool()),
        Some(false),
        "stale manifest-backed declaration lookup should surface non-cacheable freshness metadata until a fresh snapshot exists"
    );

    let stale = match server
        .find_declarations(Parameters(FindDeclarationsParams {
            symbol: Some("alpha".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(10),
        }))
        .await
    {
        Ok(_) => panic!("find_declarations should not reuse stale cached matches"),
        Err(error) => error,
    };
    assert_eq!(error_code_tag(&stale), Some("resource_not_found"));
    assert_eq!(retryable_tag(&stale), Some(false));

    cleanup_workspace_root(&workspace_root);
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
         pub fn serve() {}\n\
         pub fn consumer() { serve(); let _ = ServiceMarker; }\n\
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
                { "symbol": "scip-rust pkg repo#serve", "range": [3, 7, 12], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#consumer", "range": [4, 7, 15], "symbol_roles": 1 }
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
                    { "symbol": "scip-rust pkg repo#Service", "is_reference": true },
                    { "symbol": "scip-rust pkg repo#serve", "is_reference": true }
                  ]
                },
                {
                  "symbol": "scip-rust pkg repo#serve",
                  "display_name": "serve",
                  "kind": "function",
                  "relationships": []
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
    assert_eq!(outgoing.matches[0].target_symbol, "serve");
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
async fn navigation_find_implementations_uses_precise_occurrences_when_relationships_are_absent() {
    let workspace_root = temp_workspace_root("navigation-implementations-precise-occurrences");
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
        "impl-occurrences.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Service", "range": [2, 5, 12], "symbol_roles": 8 }
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
        .expect("find_implementations should derive precise implementations from occurrences")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "Impl");
    assert_eq!(response.matches[0].path, "src/lib.rs");
    assert_eq!(response.matches[0].line, 2);
    assert_eq!(response.matches[0].column, 12);
    assert_eq!(
        response.matches[0].relation.as_deref(),
        Some("implementation")
    );
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));

    let note = response
        .note
        .as_ref()
        .expect("find_implementations should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("find_implementations note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_selection"]["selected_path_class"],
        "runtime"
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
    assert_eq!(response.mode, NavigationMode::Precise);
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
async fn navigation_incoming_calls_marks_callable_precise_occurrences_as_calls() {
    let workspace_root = temp_workspace_root("navigation-incoming-precise-call-sites");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn callee() {}\n\
         pub fn caller() {\n\
             callee();\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "incoming-calls.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#callee", "range": [0, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#caller", "range": [1, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#callee", "range": [2, 4, 10], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#callee",
                  "display_name": "callee",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#caller",
                  "display_name": "caller",
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
            symbol: Some("callee".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("incoming_calls should classify callable precise references as calls")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "caller");
    assert_eq!(response.matches[0].target_symbol, "callee");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));
    assert_eq!(response.matches[0].call_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));
    assert_eq!(response.matches[0].call_end_line, Some(3));
    assert_eq!(response.matches[0].call_end_column, Some(11));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_incoming_calls_matches_precise_typescript_symbols_without_display_names() {
    let workspace_root = temp_workspace_root("navigation-incoming-typescript-symbol-tail");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("auth.ts"),
        "const requireServerUser = () => {};\n\
         export function handler() {\n\
             requireServerUser();\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    write_scip_fixture(
        &workspace_root,
        "typescript-incoming.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "range": [0, 6, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:handler.",
                  "range": [1, 16, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "range": [2, 4, 21],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser.",
                  "display_name": "",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/auth.ts:handler.",
                  "display_name": "handler",
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
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("incoming_calls should resolve precise TypeScript callers")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "handler");
    assert_eq!(response.matches[0].target_symbol, "requireServerUser");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));
    assert_eq!(
        response.matches[0].call_path.as_deref(),
        Some("src/auth.ts")
    );
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));

    let note = response
        .note
        .as_ref()
        .expect("incoming_calls should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("incoming_calls note should be valid JSON");
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(
        note_json["target_precise_symbol"],
        "scip-typescript npm app 1.0.0 src/auth.ts:requireServerUser."
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_incoming_calls_marks_unspecified_typescript_occurrences_as_calls() {
    let workspace_root =
        temp_workspace_root("navigation-incoming-typescript-unspecified-callable-kind");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("auth.ts"),
        "export function requireServerUser() {}\n\
         export function handler() {\n\
             requireServerUser();\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    write_scip_fixture(
        &workspace_root,
        "typescript-incoming-unspecified.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [0, 16, 33],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "range": [1, 16, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [2, 4, 21],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "display_name": "",
                  "kind": "unspecified_kind",
                  "relationships": []
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "display_name": "",
                  "kind": "unspecified_kind",
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
            symbol: Some("requireServerUser".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("incoming_calls should classify explicit TypeScript call sites as calls")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "handler");
    assert_eq!(response.matches[0].target_symbol, "requireServerUser");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(
        response.matches[0].call_path.as_deref(),
        Some("src/auth.ts")
    );
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_outgoing_calls_uses_precise_occurrences_when_relationships_are_absent() {
    let workspace_root = temp_workspace_root("navigation-outgoing-precise-occurrences");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn alpha() {}\n\
         pub fn beta() {}\n\
         pub const GAMMA: usize = 1;\n\
         pub struct Marker;\n\
         pub fn caller() {\n\
             alpha();\n\
             beta();\n\
             let _ = GAMMA;\n\
             let _ = Marker;\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");
    write_scip_fixture(
        &workspace_root,
        "outgoing.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#alpha", "range": [0, 7, 12], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#beta", "range": [1, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#GAMMA", "range": [2, 10, 15], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Marker", "range": [3, 11, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#caller", "range": [4, 7, 13], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#alpha", "range": [5, 4, 9], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#beta", "range": [6, 4, 8], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#GAMMA", "range": [7, 11, 16], "symbol_roles": 8 },
                { "symbol": "scip-rust pkg repo#Marker", "range": [8, 11, 17], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#alpha",
                  "display_name": "alpha",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#beta",
                  "display_name": "beta",
                  "kind": "function",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#GAMMA",
                  "display_name": "GAMMA",
                  "kind": "constant",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Marker",
                  "display_name": "Marker",
                  "kind": "struct",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#caller",
                  "display_name": "caller",
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
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("outgoing_calls should derive precise callees from precise references")
        .0;

    assert_eq!(response.matches.len(), 2);
    assert_eq!(response.matches[0].source_symbol, "caller");
    assert_eq!(response.matches[0].target_symbol, "alpha");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].precision.as_deref(), Some("precise"));
    assert_eq!(response.matches[0].call_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(response.matches[0].call_line, Some(6));
    assert_eq!(response.matches[0].call_column, Some(5));
    assert_eq!(response.matches[0].call_end_line, Some(6));
    assert_eq!(response.matches[0].call_end_column, Some(10));
    assert_eq!(response.matches[1].source_symbol, "caller");
    assert_eq!(response.matches[1].target_symbol, "beta");
    assert_eq!(response.matches[1].relation, "calls");
    assert_eq!(response.matches[1].precision.as_deref(), Some("precise"));
    assert_eq!(response.matches[1].call_path.as_deref(), Some("src/lib.rs"));
    assert_eq!(response.matches[1].call_line, Some(7));
    assert_eq!(response.matches[1].call_column, Some(5));
    assert_eq!(response.matches[1].call_end_line, Some(7));
    assert_eq!(response.matches[1].call_end_column, Some(9));

    let note = response
        .note
        .as_ref()
        .expect("outgoing_calls should emit precise metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("outgoing_calls note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("outgoing_calls should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["precision"], "precise");
    assert_eq!(note_json["precise"]["outgoing_count"], 2);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_outgoing_calls_matches_typescript_callees_with_unspecified_kind() {
    let workspace_root =
        temp_workspace_root("navigation-outgoing-typescript-unspecified-callable-kind");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("auth.ts"),
        "export function requireServerUser() {}\n\
         export function handler() {\n\
             requireServerUser();\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    write_scip_fixture(
        &workspace_root,
        "typescript-outgoing-unspecified.json",
        r#"{
          "documents": [
            {
              "relative_path": "src/auth.ts",
              "occurrences": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [0, 16, 33],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "range": [1, 16, 23],
                  "symbol_roles": 1
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "range": [2, 4, 21],
                  "symbol_roles": 8
                }
              ],
              "symbols": [
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/requireServerUser().",
                  "display_name": "",
                  "kind": "unspecified_kind",
                  "relationships": []
                },
                {
                  "symbol": "scip-typescript npm app 1.0.0 src/`auth.ts`/handler().",
                  "display_name": "",
                  "kind": "unspecified_kind",
                  "relationships": []
                }
              ]
            }
          ]
        }"#,
    );

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("handler".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("outgoing_calls should keep explicit TypeScript call sites when kind data is weak")
        .0;

    assert_eq!(response.mode, NavigationMode::Precise);
    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].source_symbol, "handler");
    assert_eq!(response.matches[0].target_symbol, "requireServerUser");
    assert_eq!(response.matches[0].relation, "calls");
    assert_eq!(response.matches[0].path, "src/auth.ts");
    assert_eq!(response.matches[0].line, 1);
    assert_eq!(response.matches[0].column, 17);
    assert_eq!(
        response.matches[0].call_path.as_deref(),
        Some("src/auth.ts")
    );
    assert_eq!(response.matches[0].call_line, Some(3));
    assert_eq!(response.matches[0].call_column, Some(5));

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn navigation_outgoing_calls_heuristic_fallback_keeps_empty_set_instead_of_widening_to_non_callable_refs()
 {
    let workspace_root = temp_workspace_root("navigation-outgoing-heuristic-callable-only");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "pub fn alpha() {}\n\
         pub const GAMMA: usize = 1;\n\
         pub struct Marker;\n\
         pub fn caller() {\n\
             alpha();\n\
             let _ = GAMMA;\n\
             let _ = Marker;\n\
         }\n",
    )
    .expect("failed to seed temporary fixture source");

    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .outgoing_calls(Parameters(OutgoingCallsParams {
            symbol: Some("caller".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("outgoing_calls should keep an empty heuristic result instead of widening")
        .0;

    assert!(response.matches.is_empty());

    let note = response
        .note
        .as_ref()
        .expect("outgoing_calls should emit heuristic metadata");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("outgoing_calls note should be valid JSON");
    assert_eq!(note_json["precision"], "heuristic");
    assert_eq!(note_json["fallback_reason"], "precise_absent");

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
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["source"],
        "tree_sitter"
    );

    let note = response
        .note
        .as_ref()
        .expect("document_symbols should emit metadata note");
    let note_json: serde_json::Value =
        serde_json::from_str(note).expect("document_symbols note should be valid JSON");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["source"], "tree_sitter");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn document_symbols_returns_php_metadata_evidence_counts() {
    let workspace_root = temp_workspace_root("document-symbols-php-evidence");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary php fixture");
    fs::write(
        src_root.join("OrderListener.php"),
        "<?php\n\
         namespace App\\Listeners;\n\
         \n\
         use App\\Attributes\\AsListener;\n\
         use App\\Contracts\\Dispatcher;\n\
         use App\\Exceptions\\OrderException;\n\
         use App\\Handlers\\OrderHandler;\n\
         \n\
         #[AsListener]\n\
         final class OrderListener\n\
         {\n\
             public function __construct(\n\
                 private Dispatcher $dispatcher,\n\
                 private OrderHandler $handler,\n\
             ) {}\n\
         \n\
             #[AsListener]\n\
             public function boot(Dispatcher $dispatcher): void\n\
             {\n\
                 $listener = new OrderHandler();\n\
                 $class = OrderHandler::class;\n\
                 $callable = [OrderHandler::class, 'handle'];\n\
                 dispatch(handler: $listener, options: ['queue' => 'orders']);\n\
         \n\
                 try {\n\
                     $dispatcher->dispatch($listener);\n\
                 } catch (OrderException $exception) {\n\
                     report($exception);\n\
                 }\n\
             }\n\
         }\n",
    )
    .expect("failed to seed temporary php fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/OrderListener.php".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return php outline metadata")
        .0;

    let php_metadata = response
        .metadata
        .as_ref()
        .expect("document_symbols should emit typed metadata")
        .get("php")
        .expect("php metadata summary should be present");
    assert!(
        php_metadata["canonical_name_count"]
            .as_u64()
            .expect("canonical name count should be numeric")
            >= 3
    );
    assert!(
        php_metadata["type_evidence_count"]
            .as_u64()
            .expect("type evidence count should be numeric")
            >= 3
    );
    assert!(
        php_metadata["target_evidence_count"]
            .as_u64()
            .expect("target evidence count should be numeric")
            >= 4
    );
    assert!(
        php_metadata["literal_evidence_count"]
            .as_u64()
            .expect("literal evidence count should be numeric")
            >= 2
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_typescript_outline_for_tsx_files() {
    let workspace_root = temp_workspace_root("document-symbols-typescript");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary typescript fixture");
    fs::write(
        src_root.join("App.tsx"),
        "type Props = {};\n\
         export function App(_props: Props) {\n\
             return <Card />;\n\
         }\n",
    )
    .expect("failed to seed temporary typescript fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/App.tsx".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return typescript outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Props" && symbol.kind == "type_alias")
    );
    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "App" && symbol.kind == "function")
    );
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "typescript"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_python_outline() {
    let workspace_root = temp_workspace_root("document-symbols-python");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary python fixture");
    fs::write(
        src_root.join("app.py"),
        concat!(
            "type Alias = str\n",
            "class Service:\n",
            "    def run(self) -> None:\n",
            "        pass\n",
            "\n",
            "def helper() -> Alias:\n",
            "    return \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary python fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/app.py".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return python outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Alias" && symbol.kind == "type_alias")
    );
    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Service" && symbol.kind == "class")
    );
    assert!(
        response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "run" && symbol.kind == "method")
    );
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "python"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_additional_baseline_language_outlines() {
    let workspace_root = temp_workspace_root("document-symbols-additional-baselines");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary baseline fixtures");
    fs::write(
        src_root.join("main.go"),
        concat!(
            "package main\n",
            "type Service struct{}\n",
            "func helper() string { return \"ok\" }\n",
        ),
    )
    .expect("failed to seed temporary go fixture");
    fs::write(
        src_root.join("App.kt"),
        concat!(
            "class Service {\n",
            "    fun run(): String = \"ok\"\n",
            "}\n",
            "typealias Alias = String\n",
        ),
    )
    .expect("failed to seed temporary kotlin fixture");
    fs::write(
        src_root.join("init.lua"),
        concat!("function Service.run()\n", "    return \"ok\"\n", "end\n",),
    )
    .expect("failed to seed temporary lua fixture");
    fs::write(
        src_root.join("main.roc"),
        concat!("UserId := U64\n", "greet = \\name -> name\n",),
    )
    .expect("failed to seed temporary roc fixture");
    fs::write(
        src_root.join("main.nim"),
        concat!(
            "type Service = object\n",
            "proc helper(): string =\n",
            "  \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary nim fixture");
    let server = server_for_workspace_root(&workspace_root);

    let go_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/main.go".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return go outline")
        .0;
    assert!(
        go_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "main" && symbol.kind == "module")
    );
    assert_eq!(
        go_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "go"
    );

    let kotlin_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/App.kt".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return kotlin outline")
        .0;
    assert!(
        kotlin_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Service" && symbol.kind == "class")
    );
    assert!(
        kotlin_response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "run" && symbol.kind == "method")
    );
    assert_eq!(
        kotlin_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "kotlin"
    );

    let lua_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/init.lua".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return lua outline")
        .0;
    assert!(
        lua_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "run" && symbol.kind == "function")
    );
    assert_eq!(
        lua_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "lua"
    );

    let roc_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/main.roc".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return roc outline")
        .0;
    assert!(
        roc_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "UserId" && symbol.kind == "type_alias")
    );
    assert!(
        roc_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "greet" && symbol.kind == "function")
    );
    assert_eq!(
        roc_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "roc"
    );

    let nim_response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/main.nim".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return nim outline")
        .0;
    assert!(
        nim_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "Service" && symbol.kind == "struct")
    );
    assert!(
        nim_response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "helper" && symbol.kind == "function")
    );
    assert_eq!(
        nim_response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["language"],
        "nim"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_hierarchy_for_nested_symbols() {
    let workspace_root = temp_workspace_root("document-symbols-hierarchy");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary fixture");
    fs::write(
        src_root.join("lib.rs"),
        "mod inner {\n    pub fn nested() {}\n}\n",
    )
    .expect("failed to seed temporary fixture source");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return nested outline")
        .0;

    assert_eq!(response.symbols.len(), 1);
    assert_eq!(response.symbols[0].symbol, "inner");
    assert_eq!(response.symbols[0].children.len(), 1);
    assert_eq!(response.symbols[0].children[0].symbol, "nested");
    assert_eq!(
        response.symbols[0].children[0].container.as_deref(),
        Some("inner")
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn document_symbols_returns_blade_outline() {
    let workspace_root = temp_workspace_root("document-symbols-blade");
    let blade_root = workspace_root.join("resources/views/components/dashboard");
    fs::create_dir_all(&blade_root).expect("failed to create temporary blade fixture");
    fs::write(
        blade_root.join("panel.blade.php"),
        "@section('hero')\n\
         @props(['title' => 'Dashboard'])\n\
         @aware(['tone'])\n\
         <x-slot:icon />\n\
         <x-alert.banner />\n\
         <livewire:orders.table />\n\
         <flux:button wire:click=\"save\" wire:model.live=\"state\" />\n",
    )
    .expect("failed to seed temporary blade fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .document_symbols(Parameters(DocumentSymbolsParams {
            path: "resources/views/components/dashboard/panel.blade.php".to_owned(),
            repository_id: Some("repo-001".to_owned()),
        }))
        .await
        .expect("document_symbols should return blade outline")
        .0;

    assert!(
        response
            .symbols
            .iter()
            .any(|symbol| symbol.symbol == "components.dashboard.panel" && symbol.kind == "module")
    );
    assert!(
        response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "dashboard.panel" && symbol.kind == "component")
    );
    assert!(
        response
            .symbols
            .iter()
            .flat_map(|symbol| symbol.children.iter())
            .any(|symbol| symbol.symbol == "hero" && symbol.kind == "section")
    );
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("document_symbols should emit typed metadata")["source"],
        "tree_sitter"
    );
    let blade_metadata = response
        .metadata
        .as_ref()
        .expect("document_symbols should emit typed metadata")
        .get("blade")
        .expect("blade metadata summary should be present");
    assert_eq!(blade_metadata["relations_detected"], 1);
    assert_eq!(
        blade_metadata["livewire_components"],
        serde_json::json!(["orders.table"])
    );
    assert_eq!(
        blade_metadata["wire_directives"],
        serde_json::json!(["wire:click", "wire:model.live"])
    );
    assert_eq!(
        blade_metadata["flux_components"],
        serde_json::json!(["flux:button"])
    );
    assert_eq!(blade_metadata["flux_registry_version"], "2026-03-08-mvp");

    cleanup_workspace_root(&workspace_root);
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
    assert_eq!(
        error_data_field(&error, "supported_extensions"),
        &serde_json::json!([
            ".rs",
            ".php",
            ".blade.php",
            ".ts",
            ".tsx",
            ".py",
            ".go",
            ".kt",
            ".kts",
            ".lua",
            ".roc",
            ".nim",
            ".nims"
        ])
    );
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
    assert_eq!(
        first
            .metadata
            .as_ref()
            .expect("search_structural should emit typed metadata"),
        &note_json
    );
    assert_eq!(note_json["source"], "tree_sitter_query");
    assert_eq!(note_json["heuristic"], false);
}

#[tokio::test]
async fn search_structural_returns_deterministic_blade_matches() {
    let workspace_root = temp_workspace_root("search-structural-blade");
    let blade_root = workspace_root.join("resources/views/components/dashboard");
    fs::create_dir_all(&blade_root).expect("failed to create temporary blade fixture");
    fs::write(
        blade_root.join("panel.blade.php"),
        "<x-slot:icon />\n\
         <x-alert.banner />\n\
         <livewire:orders.table />\n\
         <flux:button wire:click=\"save\" wire:model.live=\"state\" />\n",
    )
    .expect("failed to seed temporary blade fixture");
    let server = server_for_workspace_root(&workspace_root);

    let first = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(self_closing_tag (tag_name) @tag)".to_owned(),
            language: Some("blade".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"panel\.blade\.php$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return blade matches")
        .0;
    let second = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(self_closing_tag (tag_name) @tag)".to_owned(),
            language: Some("blade".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"panel\.blade\.php$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should deterministically return blade matches")
        .0;

    assert_eq!(first.matches.len(), second.matches.len());
    assert!(!first.matches.is_empty());
    assert!(
        first
            .matches
            .iter()
            .any(|matched| matched.excerpt == "x-slot:icon"),
        "expected blade structural match for x-slot tag"
    );
    let blade_metadata = first
        .metadata
        .as_ref()
        .expect("search_structural should emit typed metadata")
        .get("blade")
        .expect("blade aggregate metadata should be present");
    assert_eq!(
        blade_metadata["livewire_components"],
        serde_json::json!(["orders.table"])
    );
    assert_eq!(
        blade_metadata["wire_directives"],
        serde_json::json!(["wire:click", "wire:model.live"])
    );
    assert_eq!(
        blade_metadata["flux_components"],
        serde_json::json!(["flux:button"])
    );
    assert_eq!(blade_metadata["flux_registry_version"], "2026-03-08-mvp");
    assert_eq!(blade_metadata["relations_detected"], 1);

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_returns_typescript_tsx_matches() {
    let workspace_root = temp_workspace_root("search-structural-typescript");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary tsx fixture");
    fs::write(
        src_root.join("App.tsx"),
        "export function App() {\n\
             return <Card />;\n\
         }\n",
    )
    .expect("failed to seed temporary tsx fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(jsx_self_closing_element) @jsx".to_owned(),
            language: Some("tsx".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"App\.tsx$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return typescript matches")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].path, "src/App.tsx");
    assert_eq!(response.matches[0].excerpt, "<Card />");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("search_structural should emit typed metadata")["language"],
        "typescript"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_returns_python_matches() {
    let workspace_root = temp_workspace_root("search-structural-python");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary python fixture");
    fs::write(
        src_root.join("app.py"),
        concat!(
            "def first():\n",
            "    return 1\n",
            "\n",
            "def second():\n",
            "    return 2\n",
        ),
    )
    .expect("failed to seed temporary python fixture");
    let server = server_for_workspace_root(&workspace_root);

    let response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_definition) @fn".to_owned(),
            language: Some("py".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"app\.py$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return python matches")
        .0;

    assert_eq!(response.matches.len(), 2);
    assert_eq!(response.matches[0].path, "src/app.py");
    assert_eq!(
        response
            .metadata
            .as_ref()
            .expect("search_structural should emit typed metadata")["language"],
        "python"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_returns_additional_baseline_language_matches() {
    let workspace_root = temp_workspace_root("search-structural-additional-baselines");
    let src_root = workspace_root.join("src");
    fs::create_dir_all(&src_root).expect("failed to create temporary baseline fixtures");
    fs::write(
        src_root.join("main.go"),
        concat!("package main\n", "func helper() string { return \"ok\" }\n",),
    )
    .expect("failed to seed temporary go fixture");
    fs::write(
        src_root.join("App.kt"),
        concat!(
            "class Service {\n",
            "    fun run(): String = \"ok\"\n",
            "}\n",
            "fun helper(): String = \"ok\"\n",
        ),
    )
    .expect("failed to seed temporary kotlin fixture");
    fs::write(
        src_root.join("init.lua"),
        concat!("function Service.run()\n", "    return \"ok\"\n", "end\n",),
    )
    .expect("failed to seed temporary lua fixture");
    fs::write(
        src_root.join("main.roc"),
        concat!("greet = \\name -> name\n", "id = 1\n",),
    )
    .expect("failed to seed temporary roc fixture");
    fs::write(
        src_root.join("main.nim"),
        concat!("proc helper(): string =\n", "  \"ok\"\n",),
    )
    .expect("failed to seed temporary nim fixture");
    let server = server_for_workspace_root(&workspace_root);

    let go_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_declaration) @fn".to_owned(),
            language: Some("golang".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"main\.go$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return go matches")
        .0;
    assert_eq!(go_response.matches.len(), 1);
    assert_eq!(
        go_response.metadata.as_ref().expect("typed metadata")["language"],
        "go"
    );

    let kotlin_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_declaration) @fn".to_owned(),
            language: Some("kt".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"App\.kt$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return kotlin matches")
        .0;
    assert_eq!(kotlin_response.matches.len(), 2);
    assert_eq!(
        kotlin_response.metadata.as_ref().expect("typed metadata")["language"],
        "kotlin"
    );

    let lua_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_declaration) @fn".to_owned(),
            language: Some("lua".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"init\.lua$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return lua matches")
        .0;
    assert_eq!(lua_response.matches.len(), 1);
    assert_eq!(
        lua_response.metadata.as_ref().expect("typed metadata")["language"],
        "lua"
    );

    let roc_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(value_declaration) @value".to_owned(),
            language: Some("roc".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"main\.roc$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return roc matches")
        .0;
    assert_eq!(roc_response.matches.len(), 2);
    assert_eq!(
        roc_response.metadata.as_ref().expect("typed metadata")["language"],
        "roc"
    );

    let nim_response = server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(proc_declaration) @proc".to_owned(),
            language: Some("nims".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path_regex: Some(r"main\.nim$".to_owned()),
            limit: Some(20),
        }))
        .await
        .expect("search_structural should return nim matches")
        .0;
    assert_eq!(nim_response.matches.len(), 1);
    assert_eq!(
        nim_response.metadata.as_ref().expect("typed metadata")["language"],
        "nim"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn search_structural_rejects_unsupported_language_with_typed_error() {
    let server = server_for_fixture();
    let error = match server
        .search_structural(Parameters(SearchStructuralParams {
            query: "(function_item) @fn".to_owned(),
            language: Some("java".to_owned()),
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
    assert_eq!(
        error_data_field(&error, "supported_languages"),
        &serde_json::json!([
            "rust",
            "php",
            "blade",
            "typescript",
            "python",
            "go",
            "kotlin",
            "lua",
            "roc",
            "nim"
        ])
    );
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
