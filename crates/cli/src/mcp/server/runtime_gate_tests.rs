use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::domain::FriggError;
use crate::domain::model::TextMatch;
use crate::indexer::{FileMetadataDigest, ReindexMode, reindex_repository};
use crate::mcp::RuntimeTaskRegistry;
use crate::mcp::server_cache::{
    FindDeclarationsResponseCacheKey, GoToDefinitionResponseCacheKey, HeuristicReferenceCacheKey,
    RepositoryFreshnessCacheScope, RuntimeCacheFamily, RuntimeCacheFreshnessContract,
    RuntimeCacheResidency, RuntimeCacheReuseClass, SearchHybridResponseCacheKey,
    SearchSymbolResponseCacheKey, SearchTextResponseCacheKey,
};
use crate::mcp::tool_surface::{ToolSurfaceProfile, manifest_for_tool_surface_profile};
use crate::mcp::types::{
    FindDeclarationsResponse, GoToDefinitionResponse, RuntimeTaskKind, RuntimeTaskStatus,
    SearchHybridResponse, SearchSymbolResponse, SearchTextResponse, WorkspaceAttachParams,
    WorkspaceDetachParams, WorkspaceIndexComponentState, WorkspaceResolveMode,
};
use crate::searcher::ValidatedManifestCandidateCache;
use crate::settings::{
    FriggConfig, RuntimeProfile, RuntimeTransportKind, SemanticRuntimeConfig,
    SemanticRuntimeProvider, WatchConfig, WatchMode,
};
use crate::storage::{
    DEFAULT_VECTOR_DIMENSIONS, ManifestEntry, SemanticChunkEmbeddingRecord, Storage,
};
use crate::watch::maybe_start_watch_runtime;
use protobuf::{EnumOrUnknown, Message};
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::ErrorCode;
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    SymbolInformation as ScipSymbolInformationProto,
};
use serde_json::Value;

use super::{FriggMcpServer, ReadOnlyToolExecutionContext, RepositoryResponseCacheFreshnessMode};

fn fixture_config() -> FriggConfig {
    let workspace_root = std::env::current_dir()
        .expect("current working directory should exist for runtime gate tests");
    FriggConfig::from_workspace_roots(vec![workspace_root])
        .expect("runtime gate tests should build a valid FriggConfig")
}

fn to_set(values: Vec<String>) -> BTreeSet<String> {
    values.into_iter().collect()
}

fn extended_only_tool_names() -> Vec<String> {
    let core = manifest_for_tool_surface_profile(ToolSurfaceProfile::Core)
        .tool_names
        .into_iter()
        .collect::<BTreeSet<_>>();
    manifest_for_tool_surface_profile(ToolSurfaceProfile::Extended)
        .tool_names
        .into_iter()
        .filter(|tool_name| !core.contains(tool_name))
        .collect()
}

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-runtime-gate-tests-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn rewrite_fixture_file_with_mtime_tick(path: &Path, contents: &str) {
    let before = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(FriggMcpServer::system_time_to_unix_nanos);
    for _ in 0..8 {
        std::thread::sleep(Duration::from_millis(2));
        fs::write(path, contents).expect("failed to rewrite fixture file");
        let after = fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(FriggMcpServer::system_time_to_unix_nanos);
        if after != before {
            return;
        }
    }

    panic!("fixture file mtime did not advance after rewrite");
}

fn semantic_runtime_enabled_openai() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    }
}

#[test]
fn read_only_tool_execution_context_scopes_session_repository_with_manifest_freshness() {
    let workspace_root = temp_workspace_root("read-only-tool-context-manifest-freshness");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    let repository_id = workspace.repository_id.clone();

    let context = server
        .scoped_read_only_tool_execution_context(
            "search_text",
            None,
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("tool execution context should resolve current repository");

    assert_eq!(
        context.base,
        ReadOnlyToolExecutionContext {
            tool_name: "search_text",
            repository_hint: None,
        }
    );
    assert_eq!(context.scoped_repository_ids, vec![repository_id]);
    assert_eq!(context.scoped_workspaces.len(), 1);
    assert!(
        context.cache_freshness.scopes.is_some(),
        "scoped execution context should capture cache freshness inputs"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn runtime_cache_registry_classifies_snapshot_query_and_request_local_families() {
    let server = FriggMcpServer::new(fixture_config());

    let manifest = server.runtime_cache_policy(RuntimeCacheFamily::ValidatedManifestCandidate);
    assert_eq!(manifest.residency, RuntimeCacheResidency::ProcessWide);
    assert_eq!(
        manifest.reuse_class,
        RuntimeCacheReuseClass::SnapshotScopedReusable
    );
    assert_eq!(
        manifest.freshness_contract,
        RuntimeCacheFreshnessContract::RepositorySnapshot
    );
    assert!(manifest.dirty_root_bypass);
    assert_eq!(manifest.budget.max_entries, Some(128));

    let query_result = server.runtime_cache_policy(RuntimeCacheFamily::SearchHybridResponse);
    assert_eq!(query_result.residency, RuntimeCacheResidency::ProcessWide);
    assert_eq!(
        query_result.reuse_class,
        RuntimeCacheReuseClass::QueryResultMicroCache
    );
    assert_eq!(
        query_result.freshness_contract,
        RuntimeCacheFreshnessContract::RepositoryFreshnessScopes
    );
    assert!(query_result.dirty_root_bypass);
    assert_eq!(query_result.budget.max_bytes, Some(8 * 1024 * 1024));

    let request_local = server.runtime_cache_policy(RuntimeCacheFamily::SearcherProjectionStore);
    assert_eq!(request_local.residency, RuntimeCacheResidency::RequestLocal);
    assert_eq!(
        request_local.reuse_class,
        RuntimeCacheReuseClass::RequestLocalOnly
    );
    assert_eq!(
        request_local.freshness_contract,
        RuntimeCacheFreshnessContract::RequestLocal
    );
    assert_eq!(request_local.budget.max_entries, None);
    assert_eq!(request_local.budget.max_bytes, None);
}

#[test]
fn response_caches_respect_registry_entry_limits_and_track_evictions() {
    let server = FriggMcpServer::new(fixture_config());
    let search_text_limit = server
        .runtime_cache_policy(RuntimeCacheFamily::SearchTextResponse)
        .budget
        .max_entries
        .expect("search text cache should have an entry budget");
    let go_to_definition_limit = server
        .runtime_cache_policy(RuntimeCacheFamily::GoToDefinitionResponse)
        .budget
        .max_entries
        .expect("go-to-definition cache should have an entry budget");
    let scope = RepositoryFreshnessCacheScope {
        repository_id: "repo-001".to_owned(),
        snapshot_id: "snapshot-001".to_owned(),
        semantic_state: None,
        semantic_provider: None,
        semantic_model: None,
    };
    let empty_text_response = SearchTextResponse {
        total_matches: 0,
        matches: Vec::new(),
    };
    let empty_navigation_response = GoToDefinitionResponse {
        matches: Vec::new(),
        metadata: None,
        note: None,
    };

    for index in 0..=search_text_limit {
        server.cache_search_text_response(
            SearchTextResponseCacheKey {
                scoped_repository_ids: vec!["repo-001".to_owned()],
                freshness_scopes: vec![scope.clone()],
                query: format!("needle-{index}"),
                pattern_type: "literal",
                path_regex: None,
                limit: 10,
            },
            &empty_text_response,
            &Value::Null,
        );
    }
    assert_eq!(
        server
            .cache_state
            .search_text_response_cache
            .read()
            .expect("search text cache should not be poisoned")
            .len(),
        search_text_limit
    );
    assert_eq!(
        server.runtime_cache_telemetry(RuntimeCacheFamily::SearchTextResponse),
        crate::mcp::server_cache::RuntimeCacheTelemetry {
            hits: 0,
            misses: 0,
            bypasses: 0,
            inserts: search_text_limit + 1,
            evictions: 1,
            invalidations: 0,
        }
    );

    for index in 0..=go_to_definition_limit {
        server.cache_go_to_definition_response(
            GoToDefinitionResponseCacheKey {
                scoped_repository_ids: vec!["repo-001".to_owned()],
                freshness_scopes: vec![scope.clone()],
                repository_id: Some("repo-001".to_owned()),
                symbol: Some(format!("User{index}")),
                path: None,
                line: None,
                column: None,
                limit: 10,
            },
            &empty_navigation_response,
            &["repo-001".to_owned()],
            None,
            None,
            Some("heuristic"),
            Some("fixture"),
            10,
            0,
            0,
            0,
        );
    }
    assert_eq!(
        server
            .cache_state
            .go_to_definition_response_cache
            .read()
            .expect("go-to-definition cache should not be poisoned")
            .len(),
        go_to_definition_limit
    );
    assert_eq!(
        server.runtime_cache_telemetry(RuntimeCacheFamily::GoToDefinitionResponse),
        crate::mcp::server_cache::RuntimeCacheTelemetry {
            hits: 0,
            misses: 0,
            bypasses: 0,
            inserts: go_to_definition_limit + 1,
            evictions: 1,
            invalidations: 0,
        }
    );
}

#[test]
fn runtime_text_searchers_share_projection_store_service_across_requests() {
    let workspace_root = temp_workspace_root("runtime-shared-projection-store");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::create_dir_all(workspace_root.join(".github/workflows"))
        .expect("failed to create workflow fixture directory");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .expect("failed to write manifest fixture");
    fs::write(workspace_root.join("src/main.rs"), "fn main() {}\n")
        .expect("failed to write source fixture");
    fs::write(
        workspace_root.join(".github/workflows/ci.yml"),
        "name: ci\non: push\n",
    )
    .expect("failed to write workflow fixture");
    seed_manifest_snapshot(
        &workspace_root,
        "repo-001",
        "snapshot-001",
        &["Cargo.toml", "src/main.rs", ".github/workflows/ci.yml"],
    );

    let server = FriggMcpServer::new_with_runtime_options(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
        false,
        false,
    );
    assert_eq!(
        server
            .runtime_state
            .searcher_projection_store_service
            .entrypoint_surface_cache_len(),
        0
    );

    let first_searcher = server.runtime_text_searcher(server.config.as_ref().clone());
    let repository = first_searcher
        .first_repository_candidate_universe()
        .expect("expected manifest-backed repository");
    let first = first_searcher
        .load_or_build_entrypoint_surface_projections_for_repository(&repository, "snapshot-001")
        .expect("first request should load entrypoint projections");
    assert!(
        !first.is_empty(),
        "projection fixture should decode surfaces"
    );
    assert_eq!(
        server
            .runtime_state
            .searcher_projection_store_service
            .entrypoint_surface_cache_len(),
        1
    );

    let second_searcher = server.runtime_text_searcher(server.config.as_ref().clone());
    let second_repository = second_searcher
        .first_repository_candidate_universe()
        .expect("expected manifest-backed repository");
    let second = second_searcher
        .load_or_build_entrypoint_surface_projections_for_repository(
            &second_repository,
            "snapshot-001",
        )
        .expect("second request should reuse entrypoint projections");
    assert_eq!(&*first, &*second);
    assert_eq!(
        server
            .runtime_state
            .searcher_projection_store_service
            .entrypoint_surface_cache_len(),
        1
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn read_only_tool_execution_context_rejects_unknown_repository_hint() {
    let server = FriggMcpServer::new(fixture_config());
    let error = server
        .scoped_read_only_tool_execution_context(
            "search_text",
            Some("missing-repository".to_owned()),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect_err("unknown repository hints should fail during execution scoping");

    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    let detail = error
        .data
        .as_ref()
        .expect("missing repository errors should include metadata");
    assert_eq!(
        detail.get("error_code"),
        Some(&Value::String("resource_not_found".to_owned()))
    );
}

#[test]
fn read_only_navigation_tool_execution_context_scopes_explicit_repository() {
    let workspace_root = temp_workspace_root("read-only-navigation-tool-context-freshness");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    let repository_id = workspace.repository_id.clone();

    let context = server
        .scoped_read_only_tool_execution_context(
            "go_to_definition",
            Some(repository_id.clone()),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("navigation execution context should resolve explicit repository");

    assert_eq!(context.base.tool_name, "go_to_definition");
    assert_eq!(
        context.base.repository_hint.as_deref(),
        Some(repository_id.as_str())
    );
    assert_eq!(context.scoped_repository_ids, vec![repository_id]);
    assert!(
        context.cache_freshness.scopes.is_some(),
        "navigation execution context should capture cache freshness inputs"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

async fn wait_for_repository_answer_cache_eviction(
    server: &FriggMcpServer,
    repository_id: &str,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let summary_evicted = server.cached_repository_summary(repository_id).is_none();
        let text_evicted = server
            .cache_state
            .search_text_response_cache
            .read()
            .expect("search text response cache should not be poisoned")
            .is_empty();
        let hybrid_evicted = server
            .cache_state
            .search_hybrid_response_cache
            .read()
            .expect("search hybrid response cache should not be poisoned")
            .is_empty();
        let symbol_evicted = server
            .cache_state
            .search_symbol_response_cache
            .read()
            .expect("search symbol response cache should not be poisoned")
            .is_empty();
        let definition_evicted = server
            .cache_state
            .go_to_definition_response_cache
            .read()
            .expect("go-to-definition response cache should not be poisoned")
            .is_empty();
        let declarations_evicted = server
            .cache_state
            .find_declarations_response_cache
            .read()
            .expect("find declarations response cache should not be poisoned")
            .is_empty();
        let heuristic_evicted = server
            .cache_state
            .heuristic_reference_cache
            .read()
            .expect("heuristic reference cache should not be poisoned")
            .is_empty();

        if summary_evicted
            && text_evicted
            && hybrid_evicted
            && symbol_evicted
            && definition_evicted
            && declarations_evicted
            && heuristic_evicted
        {
            return true;
        }

        if tokio::time::Instant::now() >= deadline {
            return false;
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn seed_manifest_snapshot(
    workspace_root: &Path,
    repository_id: &str,
    snapshot_id: &str,
    paths: &[&str],
) {
    let db_path = crate::storage::ensure_provenance_db_parent_dir(workspace_root)
        .expect("manifest storage path should work");
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
                mtime_ns: metadata
                    .modified()
                    .ok()
                    .and_then(FriggMcpServer::system_time_to_unix_nanos),
            }
        })
        .collect::<Vec<_>>();
    manifest_entries.sort_by(|left, right| left.path.cmp(&right.path));
    manifest_entries.dedup_by(|left, right| left.path == right.path);

    storage
        .upsert_manifest(repository_id, snapshot_id, &manifest_entries)
        .expect("manifest snapshot should persist");
}

fn semantic_record(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
) -> SemanticChunkEmbeddingRecord {
    let mut embedding = vec![0.25, 0.75];
    embedding.resize(DEFAULT_VECTOR_DIMENSIONS, 0.0);
    SemanticChunkEmbeddingRecord {
        chunk_id: format!("chunk-{}", path.replace('/', "_")),
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        path: path.to_owned(),
        language: "rust".to_owned(),
        chunk_index: 0,
        start_line: 1,
        end_line: 1,
        provider: "openai".to_owned(),
        model: "text-embedding-3-small".to_owned(),
        trace_id: Some("trace-001".to_owned()),
        content_hash_blake3: format!("hash-{}", path.replace('/', "_")),
        content_text: path.to_owned(),
        embedding,
    }
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

#[test]
fn extended_only_tools_are_hidden_by_default_runtime_options() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false, false);
    let names = to_set(server.runtime_registered_tool_names());

    for tool_name in extended_only_tool_names() {
        assert!(
            !names.contains(&tool_name),
            "extended-only tool should not be registered by default: {tool_name}"
        );
    }
    assert!(
        names.contains("list_repositories"),
        "core tools should remain registered when extended-only tools are disabled"
    );
}

#[test]
fn extended_only_tools_are_registered_when_runtime_option_enabled() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false, true);
    let names = to_set(server.runtime_registered_tool_names());

    for tool_name in extended_only_tool_names() {
        assert!(
            names.contains(&tool_name),
            "extended-only tool should be registered when enabled: {tool_name}"
        );
    }
}

#[test]
fn server_info_enables_resources_and_prompts() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false, false);
    let info = <FriggMcpServer as rmcp::ServerHandler>::get_info(&server);

    assert!(info.capabilities.tools.is_some());
    assert!(info.capabilities.resources.is_some());
    assert!(info.capabilities.prompts.is_some());

    let instructions = info
        .instructions
        .expect("server info should publish MCP usage instructions");
    assert!(instructions.contains("call workspace_attach explicitly"));
    assert!(instructions.contains("no longer auto-attaches"));
    assert!(instructions.contains(super::SUPPORT_MATRIX_RESOURCE_URI));
    assert!(instructions.contains(super::ROUTING_GUIDE_PROMPT_NAME));
}

#[test]
fn server_starts_detached_when_started_without_startup_roots() {
    let workspace_root = temp_workspace_root("declared-roots-attach");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write workspace root fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    assert!(server.attached_workspaces().is_empty());
    assert!(server.current_repository_id().is_none());

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn workspace_attach_can_adopt_known_repository_id_for_new_session() {
    let workspace_root = temp_workspace_root("attach-known-repository-id");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Adopted;\n")
        .expect("failed to write workspace root fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("startup roots should register globally known workspaces");
    let session = server.clone_for_new_session();

    assert!(server.attached_workspaces().is_empty());
    assert!(session.attached_workspaces().is_empty());

    let response = session
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: None,
            repository_id: Some(workspace.repository_id.clone()),
            set_default: Some(true),
            resolve_mode: None,
        }))
        .await
        .expect("workspace_attach should adopt a known repository id")
        .0;

    assert_eq!(response.repository.repository_id, workspace.repository_id);
    assert!(response.session_default);
    assert_eq!(session.attached_workspaces().len(), 1);
    assert_eq!(
        session.current_repository_id().as_deref(),
        Some(workspace.repository_id.as_str())
    );
    assert_eq!(session.known_workspaces().len(), 1);
    assert!(server.attached_workspaces().is_empty());

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn workspace_detach_clears_session_default_and_preserves_known_workspace() {
    let workspace_root = temp_workspace_root("detach-preserves-known-workspace");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Detached;\n")
        .expect("failed to write workspace root fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("startup roots should register globally known workspaces");
    let session = server.clone_for_new_session();
    session
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: None,
            repository_id: Some(workspace.repository_id.clone()),
            set_default: Some(true),
            resolve_mode: None,
        }))
        .await
        .expect("workspace_attach should adopt a known repository id");

    let response = session
        .workspace_detach(Parameters(WorkspaceDetachParams {
            repository_id: None,
        }))
        .await
        .expect("workspace_detach should detach the session default repository")
        .0;

    assert_eq!(response.repository_id, workspace.repository_id);
    assert!(response.detached);
    assert!(!response.session_default);
    assert!(session.current_repository_id().is_none());
    assert!(session.attached_workspaces().is_empty());
    assert_eq!(session.known_workspaces().len(), 1);

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn watch_leases_follow_session_adoption_counts() {
    let workspace_root = temp_workspace_root("watch-lease-counts");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct LeaseCount;\n",
    )
    .expect("failed to write workspace root fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 25,
        retry_ms: 100,
    };
    let runtime_task_registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
    let validated_manifest_candidate_cache =
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    let server = FriggMcpServer::new_with_runtime(
        config.clone(),
        RuntimeProfile::StdioAttached,
        true,
        Arc::clone(&runtime_task_registry),
        Arc::clone(&validated_manifest_candidate_cache),
    );
    let runtime = Arc::new(
        maybe_start_watch_runtime(
            &config,
            RuntimeTransportKind::Stdio,
            runtime_task_registry,
            validated_manifest_candidate_cache,
            None,
        )
        .expect("watch runtime should start")
        .expect("watch runtime should be enabled"),
    );
    server.set_watch_runtime(Some(Arc::clone(&runtime)));
    let second_session = server.clone_for_new_session();
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("startup roots should register globally known workspaces");

    assert!(!runtime.lease_status(&workspace.repository_id).active);

    server
        .adopt_workspace(&workspace, true)
        .expect("first session should adopt workspace");
    assert_eq!(
        runtime.lease_status(&workspace.repository_id).lease_count,
        1
    );

    second_session
        .adopt_workspace(&workspace, true)
        .expect("second session should share the same watch lease");
    assert_eq!(
        runtime.lease_status(&workspace.repository_id).lease_count,
        2
    );

    server
        .detach_workspace(&workspace.repository_id)
        .expect("first session should detach workspace");
    assert_eq!(
        runtime.lease_status(&workspace.repository_id).lease_count,
        1
    );

    second_session
        .detach_workspace(&workspace.repository_id)
        .expect("second session should detach workspace");
    assert_eq!(
        runtime.lease_status(&workspace.repository_id).lease_count,
        0
    );
    assert!(!runtime.lease_status(&workspace.repository_id).active);

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_attach_invalidates_validated_manifest_candidate_cache() {
    let workspace_root = temp_workspace_root("attach-invalidates-manifest-cache");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Cached;\n")
        .expect("failed to write workspace root fixture");
    let root = workspace_root
        .canonicalize()
        .expect("workspace root should canonicalize");
    let source_path = root.join("src/lib.rs");
    let metadata = fs::metadata(&source_path).expect("source path should have metadata");

    let cache = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    cache
        .write()
        .expect("validated manifest candidate cache should not be poisoned")
        .store_validated(
            &root,
            "snapshot-001",
            &[FileMetadataDigest {
                path: source_path,
                size_bytes: metadata.len(),
                mtime_ns: None,
            }],
        );
    assert!(
        cache
            .read()
            .expect("validated manifest candidate cache should not be poisoned")
            .has_entry_for_root(&root)
    );

    let server = FriggMcpServer::new_with_runtime(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
        RuntimeProfile::StdioEphemeral,
        false,
        Arc::new(RwLock::new(RuntimeTaskRegistry::new())),
        Arc::clone(&cache),
    );

    let _ = server
        .attach_workspace_internal(&root, true, WorkspaceResolveMode::GitRoot)
        .expect("workspace attach should succeed");

    assert!(
        !cache
            .read()
            .expect("validated manifest candidate cache should not be poisoned")
            .has_entry_for_root(&root)
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_attach_invalidates_only_attached_repository_answer_caches() {
    let workspace_root = temp_workspace_root("attach-invalidates-only-attached-answer-caches");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Cached;\n")
        .expect("failed to write workspace root fixture");
    let root = workspace_root
        .canonicalize()
        .expect("workspace root should canonicalize");

    let server = FriggMcpServer::new_with_runtime(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
        RuntimeProfile::StdioEphemeral,
        false,
        Arc::new(RwLock::new(RuntimeTaskRegistry::new())),
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
    );

    let scope = |repository_id: &str, snapshot_id: &str| RepositoryFreshnessCacheScope {
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        semantic_state: None,
        semantic_provider: None,
        semantic_model: None,
    };
    let repo_001_scope = scope("repo-001", "snapshot-001");
    let repo_002_scope = scope("repo-002", "snapshot-002");

    let empty_text_response = SearchTextResponse {
        total_matches: 0,
        matches: Vec::<TextMatch>::new(),
    };
    let empty_hybrid_response = SearchHybridResponse {
        matches: Vec::new(),
        semantic_requested: None,
        semantic_enabled: None,
        semantic_status: None,
        semantic_reason: None,
        semantic_hit_count: None,
        semantic_match_count: None,
        warning: None,
        metadata: None,
        note: None,
    };
    let empty_symbol_response = SearchSymbolResponse {
        matches: Vec::new(),
        metadata: None,
        note: None,
    };
    let empty_navigation_response = GoToDefinitionResponse {
        matches: Vec::new(),
        metadata: None,
        note: None,
    };
    let empty_declarations_response = FindDeclarationsResponse {
        matches: Vec::new(),
        metadata: None,
        note: None,
    };

    server.cache_search_text_response(
        SearchTextResponseCacheKey {
            scoped_repository_ids: vec!["repo-001".to_owned()],
            freshness_scopes: vec![repo_001_scope.clone()],
            query: "needle".to_owned(),
            pattern_type: "literal",
            path_regex: None,
            limit: 10,
        },
        &empty_text_response,
        &Value::Null,
    );
    server.cache_search_text_response(
        SearchTextResponseCacheKey {
            scoped_repository_ids: vec!["repo-002".to_owned()],
            freshness_scopes: vec![repo_002_scope.clone()],
            query: "needle".to_owned(),
            pattern_type: "literal",
            path_regex: None,
            limit: 10,
        },
        &empty_text_response,
        &Value::Null,
    );
    server.cache_search_hybrid_response(
        SearchHybridResponseCacheKey {
            scoped_repository_ids: vec!["repo-001".to_owned()],
            freshness_scopes: vec![repo_001_scope.clone()],
            query: "runtime".to_owned(),
            language: None,
            limit: 10,
            semantic: None,
            lexical_weight_bits: 0,
            graph_weight_bits: 0,
            semantic_weight_bits: 0,
        },
        &empty_hybrid_response,
        &Value::Null,
    );
    server.cache_search_hybrid_response(
        SearchHybridResponseCacheKey {
            scoped_repository_ids: vec!["repo-002".to_owned()],
            freshness_scopes: vec![repo_002_scope.clone()],
            query: "runtime".to_owned(),
            language: None,
            limit: 10,
            semantic: None,
            lexical_weight_bits: 0,
            graph_weight_bits: 0,
            semantic_weight_bits: 0,
        },
        &empty_hybrid_response,
        &Value::Null,
    );
    server.cache_search_symbol_response(
        SearchSymbolResponseCacheKey {
            scoped_repository_ids: vec!["repo-001".to_owned()],
            freshness_scopes: vec![repo_001_scope.clone()],
            query: "User".to_owned(),
            path_class: None,
            path_regex: None,
            limit: 10,
        },
        &empty_symbol_response,
        &["repo-001".to_owned()],
        0,
        0,
        0,
        0,
        10,
    );
    server.cache_search_symbol_response(
        SearchSymbolResponseCacheKey {
            scoped_repository_ids: vec!["repo-002".to_owned()],
            freshness_scopes: vec![repo_002_scope.clone()],
            query: "User".to_owned(),
            path_class: None,
            path_regex: None,
            limit: 10,
        },
        &empty_symbol_response,
        &["repo-002".to_owned()],
        0,
        0,
        0,
        0,
        10,
    );
    server.cache_go_to_definition_response(
        GoToDefinitionResponseCacheKey {
            scoped_repository_ids: vec!["repo-001".to_owned()],
            freshness_scopes: vec![repo_001_scope.clone()],
            repository_id: Some("repo-001".to_owned()),
            symbol: Some("User".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: 10,
        },
        &empty_navigation_response,
        &["repo-001".to_owned()],
        None,
        None,
        None,
        None,
        10,
        0,
        0,
        0,
    );
    server.cache_go_to_definition_response(
        GoToDefinitionResponseCacheKey {
            scoped_repository_ids: vec!["repo-002".to_owned()],
            freshness_scopes: vec![repo_002_scope.clone()],
            repository_id: Some("repo-002".to_owned()),
            symbol: Some("User".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: 10,
        },
        &empty_navigation_response,
        &["repo-002".to_owned()],
        None,
        None,
        None,
        None,
        10,
        0,
        0,
        0,
    );
    server.cache_find_declarations_response(
        FindDeclarationsResponseCacheKey {
            scoped_repository_ids: vec!["repo-001".to_owned()],
            freshness_scopes: vec![repo_001_scope.clone()],
            repository_id: Some("repo-001".to_owned()),
            symbol: Some("User".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: 10,
        },
        &empty_declarations_response,
        &["repo-001".to_owned()],
        None,
        None,
        None,
        None,
        10,
        0,
        0,
        0,
    );
    server.cache_find_declarations_response(
        FindDeclarationsResponseCacheKey {
            scoped_repository_ids: vec!["repo-002".to_owned()],
            freshness_scopes: vec![repo_002_scope.clone()],
            repository_id: Some("repo-002".to_owned()),
            symbol: Some("User".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: 10,
        },
        &empty_declarations_response,
        &["repo-002".to_owned()],
        None,
        None,
        None,
        None,
        10,
        0,
        0,
        0,
    );
    server.cache_heuristic_references(
        HeuristicReferenceCacheKey {
            repository_id: "repo-001".to_owned(),
            symbol_id: "symbol-001".to_owned(),
            corpus_signature: "corpus-001".to_owned(),
            scip_signature: "scip-001".to_owned(),
        },
        Vec::new(),
        0,
        0,
        0,
    );
    server.cache_heuristic_references(
        HeuristicReferenceCacheKey {
            repository_id: "repo-002".to_owned(),
            symbol_id: "symbol-002".to_owned(),
            corpus_signature: "corpus-002".to_owned(),
            scip_signature: "scip-002".to_owned(),
        },
        Vec::new(),
        0,
        0,
        0,
    );

    let _ = server
        .attach_workspace_internal(&root, true, WorkspaceResolveMode::GitRoot)
        .expect("workspace attach should succeed");

    assert_eq!(
        server
            .cache_state
            .search_text_response_cache
            .read()
            .expect("search text cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .search_hybrid_response_cache
            .read()
            .expect("search hybrid cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .search_symbol_response_cache
            .read()
            .expect("search symbol cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .go_to_definition_response_cache
            .read()
            .expect("go_to_definition cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .find_declarations_response_cache
            .read()
            .expect("find_declarations cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .heuristic_reference_cache
            .read()
            .expect("heuristic reference cache should not be poisoned")
            .len(),
        1
    );
    assert!(
        server
            .cache_state
            .search_text_response_cache
            .read()
            .expect("search text cache should not be poisoned")
            .keys()
            .all(|key| key.scoped_repository_ids == ["repo-002".to_owned()]),
        "search text cache should retain only unaffected repository entries"
    );
    assert!(
        server
            .cache_state
            .search_hybrid_response_cache
            .read()
            .expect("search hybrid cache should not be poisoned")
            .keys()
            .all(|key| key.scoped_repository_ids == ["repo-002".to_owned()]),
        "search hybrid cache should retain only unaffected repository entries"
    );
    assert!(
        server
            .cache_state
            .search_symbol_response_cache
            .read()
            .expect("search symbol cache should not be poisoned")
            .keys()
            .all(|key| key.scoped_repository_ids == ["repo-002".to_owned()]),
        "search symbol cache should retain only unaffected repository entries"
    );
    assert!(
        server
            .cache_state
            .go_to_definition_response_cache
            .read()
            .expect("go_to_definition cache should not be poisoned")
            .keys()
            .all(|key| key.scoped_repository_ids == ["repo-002".to_owned()]),
        "go_to_definition cache should retain only unaffected repository entries"
    );
    assert!(
        server
            .cache_state
            .find_declarations_response_cache
            .read()
            .expect("find_declarations cache should not be poisoned")
            .keys()
            .all(|key| key.scoped_repository_ids == ["repo-002".to_owned()]),
        "find_declarations cache should retain only unaffected repository entries"
    );
    assert!(
        server
            .cache_state
            .heuristic_reference_cache
            .read()
            .expect("heuristic reference cache should not be poisoned")
            .keys()
            .all(|key| key.repository_id == "repo-002"),
        "heuristic reference cache should retain only unaffected repository entries"
    );
    assert_eq!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::SearchTextResponse)
            .invalidations,
        1
    );
    assert_eq!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::SearchHybridResponse)
            .invalidations,
        1
    );
    assert_eq!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::SearchSymbolResponse)
            .invalidations,
        1
    );
    assert_eq!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::GoToDefinitionResponse)
            .invalidations,
        1
    );
    assert_eq!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::FindDeclarationsResponse)
            .invalidations,
        1
    );
    assert_eq!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::HeuristicReference)
            .invalidations,
        1
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn watch_notify_invalidates_live_server_answer_caches() {
    let workspace_root = temp_workspace_root("watch-invalidates-live-answer-caches");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct WatchCache;\n",
    )
    .expect("failed to write workspace root fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("runtime gate tests should build a valid FriggConfig");
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 25,
        retry_ms: 100,
    };

    let declared_repository = config
        .repositories()
        .into_iter()
        .next()
        .expect("watch cache test should define one repository");
    let declared_root = PathBuf::from(&declared_repository.root_path);
    let db_path = crate::storage::ensure_provenance_db_parent_dir(&declared_root)
        .expect("manifest storage path should work");
    Storage::new(&db_path)
        .initialize()
        .expect("manifest storage should initialize");
    reindex_repository(
        &declared_repository.repository_id.0,
        &declared_root,
        &db_path,
        ReindexMode::ChangedOnly,
    )
    .expect("baseline changed-only reindex should succeed");

    let runtime_task_registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
    let validated_manifest_candidate_cache =
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    let server = FriggMcpServer::new_with_runtime(
        config.clone(),
        RuntimeProfile::StdioAttached,
        true,
        Arc::clone(&runtime_task_registry),
        Arc::clone(&validated_manifest_candidate_cache),
    );
    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        runtime_task_registry,
        validated_manifest_candidate_cache,
        Some(server.repository_cache_invalidation_callback()),
    )
    .expect("watch runtime should start")
    .expect("watch runtime should be enabled");
    tokio::time::sleep(Duration::from_millis(250)).await;

    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("runtime server should expose the startup workspace");
    runtime
        .acquire_lease(&workspace)
        .expect("runtime should start watching an adopted workspace");
    let summary = server.repository_summary(&workspace);
    let lexical_snapshot_id = summary
        .health
        .as_ref()
        .and_then(|health| health.lexical.snapshot_id.clone())
        .expect("baseline repository summary should report a lexical snapshot id");
    let scope = RepositoryFreshnessCacheScope {
        repository_id: workspace.repository_id.clone(),
        snapshot_id: lexical_snapshot_id,
        semantic_state: None,
        semantic_provider: None,
        semantic_model: None,
    };
    let empty_text_response = SearchTextResponse {
        total_matches: 0,
        matches: Vec::<TextMatch>::new(),
    };
    let empty_hybrid_response = SearchHybridResponse {
        matches: Vec::new(),
        semantic_requested: None,
        semantic_enabled: None,
        semantic_status: None,
        semantic_reason: None,
        semantic_hit_count: None,
        semantic_match_count: None,
        warning: None,
        metadata: None,
        note: None,
    };
    let empty_symbol_response = SearchSymbolResponse {
        matches: Vec::new(),
        metadata: None,
        note: None,
    };
    let empty_navigation_response = GoToDefinitionResponse {
        matches: Vec::new(),
        metadata: None,
        note: None,
    };
    let empty_declarations_response = FindDeclarationsResponse {
        matches: Vec::new(),
        metadata: None,
        note: None,
    };
    let empty_source_refs = serde_json::json!({});

    server.cache_search_text_response(
        SearchTextResponseCacheKey {
            scoped_repository_ids: vec![workspace.repository_id.clone()],
            freshness_scopes: vec![scope.clone()],
            query: "watch-cache".to_owned(),
            pattern_type: "literal",
            path_regex: None,
            limit: 5,
        },
        &empty_text_response,
        &empty_source_refs,
    );
    server.cache_search_hybrid_response(
        SearchHybridResponseCacheKey {
            scoped_repository_ids: vec![workspace.repository_id.clone()],
            freshness_scopes: vec![scope.clone()],
            query: "watch-cache".to_owned(),
            language: None,
            limit: 5,
            semantic: Some(false),
            lexical_weight_bits: 0.0f32.to_bits(),
            graph_weight_bits: 0.0f32.to_bits(),
            semantic_weight_bits: 0.0f32.to_bits(),
        },
        &empty_hybrid_response,
        &empty_source_refs,
    );
    server.cache_search_symbol_response(
        SearchSymbolResponseCacheKey {
            scoped_repository_ids: vec![workspace.repository_id.clone()],
            freshness_scopes: vec![scope.clone()],
            query: "WatchCache".to_owned(),
            path_class: None,
            path_regex: None,
            limit: 5,
        },
        &empty_symbol_response,
        &[workspace.repository_id.clone()],
        0,
        0,
        0,
        0,
        5,
    );
    server.cache_go_to_definition_response(
        GoToDefinitionResponseCacheKey {
            scoped_repository_ids: vec![workspace.repository_id.clone()],
            freshness_scopes: vec![scope.clone()],
            repository_id: Some(workspace.repository_id.clone()),
            symbol: Some("WatchCache".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: 5,
        },
        &empty_navigation_response,
        &[workspace.repository_id.clone()],
        Some("WatchCache"),
        None,
        Some("heuristic"),
        Some("fixture"),
        5,
        0,
        0,
        0,
    );
    server.cache_find_declarations_response(
        FindDeclarationsResponseCacheKey {
            scoped_repository_ids: vec![workspace.repository_id.clone()],
            freshness_scopes: vec![scope.clone()],
            repository_id: Some(workspace.repository_id.clone()),
            symbol: Some("WatchCache".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: 5,
        },
        &empty_declarations_response,
        &[workspace.repository_id.clone()],
        Some("WatchCache"),
        None,
        Some("heuristic"),
        Some("fixture"),
        5,
        0,
        0,
        0,
    );
    server.cache_heuristic_references(
        HeuristicReferenceCacheKey {
            repository_id: workspace.repository_id.clone(),
            symbol_id: "WatchCache".to_owned(),
            corpus_signature: "corpus-001".to_owned(),
            scip_signature: "scip-001".to_owned(),
        },
        Vec::new(),
        0,
        0,
        0,
    );

    assert!(
        server
            .cached_repository_summary(&workspace.repository_id)
            .is_some()
    );
    assert_eq!(
        server
            .cache_state
            .search_text_response_cache
            .read()
            .expect("search text response cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .search_hybrid_response_cache
            .read()
            .expect("search hybrid response cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .search_symbol_response_cache
            .read()
            .expect("search symbol response cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .go_to_definition_response_cache
            .read()
            .expect("go-to-definition response cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .find_declarations_response_cache
            .read()
            .expect("find declarations response cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server
            .cache_state
            .heuristic_reference_cache
            .read()
            .expect("heuristic reference cache should not be poisoned")
            .len(),
        1
    );

    fs::write(
        workspace_root.join("src/watch_cache.rs"),
        "pub fn watch_cache_dirty_event() {}\n",
    )
    .expect("creating a new source file should trigger notify backend");

    assert!(
        wait_for_repository_answer_cache_eviction(
            &server,
            &workspace.repository_id,
            Duration::from_secs(5),
        )
        .await,
        "watch notify should evict live repository-scoped answer caches"
    );
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::SearchTextResponse)
            .invalidations
            >= 1
    );
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::SearchHybridResponse)
            .invalidations
            >= 1
    );
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::SearchSymbolResponse)
            .invalidations
            >= 1
    );
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::GoToDefinitionResponse)
            .invalidations
            >= 1
    );
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::FindDeclarationsResponse)
            .invalidations
            >= 1
    );
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::HeuristicReference)
            .invalidations
            >= 1
    );

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn strict_semantic_failure_maps_to_unavailable_error_code() {
    let error = FriggMcpServer::map_frigg_error(FriggError::StrictSemanticFailure {
        reason: "provider outage".to_owned(),
    });

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("unavailable".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(true))
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_class"))
            .and_then(|value| value.as_str()),
        Some("semantic")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        Some("strict_failure")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("semantic_reason"))
            .and_then(|value| value.as_str()),
        Some("provider outage")
    );
}

#[test]
fn invalid_input_maps_to_invalid_params_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::InvalidInput("bad input".to_owned()));

    assert_eq!(error.code, ErrorCode::INVALID_PARAMS);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("invalid_params".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn not_found_maps_to_resource_not_found_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::NotFound("missing".to_owned()));

    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("resource_not_found".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn access_denied_maps_to_access_denied_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::AccessDenied("blocked".to_owned()));

    assert_eq!(error.code, ErrorCode::INVALID_REQUEST);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("access_denied".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(error.message, "blocked");
}

#[test]
fn io_error_maps_to_internal_error_class() {
    use std::io::Error as IoError;

    let error = FriggMcpServer::map_frigg_error(FriggError::Io(IoError::new(
        std::io::ErrorKind::PermissionDenied,
        "denied",
    )));

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("internal".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_class"))
            .and_then(|value| value.as_str()),
        Some("io")
    );
}

#[test]
fn internal_error_maps_to_internal_error_class() {
    let error = FriggMcpServer::map_frigg_error(FriggError::Internal("boom".to_owned()));

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code")),
        Some(&serde_json::Value::String("internal".to_owned()))
    );
    assert_eq!(
        error.data.as_ref().and_then(|value| value.get("retryable")),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(error.message, "boom");
}

#[test]
fn search_hybrid_warning_surfaces_semantic_ok_empty_channel() {
    let warning = FriggMcpServer::search_hybrid_warning(
        Some(crate::domain::ChannelHealthStatus::Ok),
        None,
        Some(0),
        Some(0),
    );

    assert_eq!(
        warning.as_deref(),
        Some(
            "semantic retrieval completed successfully but retained no query-relevant semantic hits; results are ranked from lexical and graph signals only"
        )
    );
}

#[test]
fn search_hybrid_warning_surfaces_semantic_ok_noncontributing_hits() {
    let warning = FriggMcpServer::search_hybrid_warning(
        Some(crate::domain::ChannelHealthStatus::Ok),
        None,
        Some(3),
        Some(0),
    );

    assert_eq!(
        warning.as_deref(),
        Some(
            "semantic retrieval retained semantic hits, but none contributed to the returned top results; ranking is effectively lexical and graph for this result set"
        )
    );
}

#[test]
fn precise_artifact_discovery_is_scoped_to_runtime_scip_directory() {
    let workspace_root = PathBuf::from("/tmp/frigg-runtime-scip-scope");
    let directories = FriggMcpServer::scip_candidate_directories(&workspace_root);

    assert_eq!(directories, [workspace_root.join(".frigg/scip")]);
}

#[test]
fn precise_artifact_discovery_includes_json_and_scip_files() {
    let workspace_root = temp_workspace_root("scip-discovery-extensions");
    let scip_root = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&scip_root).expect("failed to create scip fixture directory");
    fs::write(scip_root.join("a.json"), "{}").expect("failed to write json fixture");
    fs::write(scip_root.join("b.scip"), [0_u8, 1_u8, 2_u8])
        .expect("failed to write protobuf fixture");
    fs::write(scip_root.join("ignored.txt"), "x").expect("failed to write ignored fixture");

    let discovery = FriggMcpServer::collect_scip_artifact_digests(&workspace_root);
    assert_eq!(discovery.artifact_digests.len(), 2);
    assert_eq!(
        discovery
            .artifact_digests
            .iter()
            .map(|digest| digest.path.file_name().and_then(|name| name.to_str()))
            .collect::<Vec<_>>(),
        vec![Some("a.json"), Some("b.scip")]
    );
    assert_eq!(
        discovery
            .artifact_digests
            .iter()
            .map(|digest| digest.format.as_str())
            .collect::<Vec<_>>(),
        vec!["json", "protobuf"]
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn manifest_source_paths_filter_to_symbol_corpus_capability() {
    let digests = vec![
        FileMetadataDigest {
            path: PathBuf::from("src/lib.rs"),
            size_bytes: 10,
            mtime_ns: Some(1),
        },
        FileMetadataDigest {
            path: PathBuf::from("src/server.php"),
            size_bytes: 20,
            mtime_ns: Some(2),
        },
        FileMetadataDigest {
            path: PathBuf::from("src/app.ts"),
            size_bytes: 30,
            mtime_ns: Some(3),
        },
        FileMetadataDigest {
            path: PathBuf::from("README.md"),
            size_bytes: 40,
            mtime_ns: Some(4),
        },
    ];

    let source_paths = FriggMcpServer::manifest_source_paths_for_digests(&digests);

    assert_eq!(
        source_paths,
        vec![
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/server.php"),
            PathBuf::from("src/app.ts")
        ]
    );
}

#[test]
fn semantic_refresh_plan_detects_latest_snapshot_missing_active_model() {
    let workspace_root = temp_workspace_root("semantic-refresh-plan");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.semantic_runtime = semantic_runtime_enabled_openai();
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    let storage = Storage::new(&workspace.db_path);
    storage
        .replace_semantic_embeddings_for_repository(
            &workspace.repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("seed semantic embeddings should persist");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-002",
        &["src/lib.rs"],
    );

    let plan = server
        .workspace_semantic_refresh_plan(&workspace)
        .expect("latest snapshot without active-model semantic rows should trigger refresh");
    assert_eq!(plan.latest_snapshot_id, "snapshot-002");
    assert_eq!(plan.reason, "semantic_snapshot_missing_for_active_model");

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_response_cache_freshness_returns_ready_manifest_scope() {
    let workspace_root = temp_workspace_root("response-cache-freshness-ready");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new_with_runtime_options(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
        false,
        false,
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );

    let freshness = server
        .repository_response_cache_freshness(
            &[workspace.clone()],
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("manifest freshness should compute");

    let scopes = freshness
        .scopes
        .as_ref()
        .expect("ready manifest snapshot should be cacheable");
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].repository_id, workspace.repository_id);
    assert_eq!(scopes[0].snapshot_id, "snapshot-001");
    assert_eq!(scopes[0].semantic_state, None);
    assert_eq!(
        freshness.basis.get("cacheable").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/manifest")
            .and_then(Value::as_str),
        Some("ready")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/dirty_root")
            .and_then(Value::as_bool),
        Some(false)
    );
    let runtime_contract = freshness
        .basis
        .get("runtime_cache_contract")
        .and_then(Value::as_array)
        .expect("runtime cache contract should be present in freshness basis");
    assert!(runtime_contract.iter().any(|entry| {
        entry.get("family").and_then(Value::as_str) == Some("search_text_response")
            && entry.pointer("/budget/max_entries").and_then(Value::as_u64) == Some(32)
    }));
    assert!(runtime_contract.iter().any(|entry| {
        entry.get("family").and_then(Value::as_str) == Some("go_to_definition_response")
            && entry
                .pointer("/telemetry/invalidations")
                .and_then(Value::as_u64)
                == Some(0)
    }));

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_response_cache_freshness_marks_dirty_root_uncacheable() {
    let workspace_root = temp_workspace_root("response-cache-freshness-dirty-root");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Dirty;\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new_with_runtime_options(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
        false,
        false,
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    server
        .runtime_state
        .validated_manifest_candidate_cache
        .write()
        .expect("validated manifest candidate cache should not be poisoned")
        .mark_dirty_root(&workspace.root);

    let freshness = server
        .repository_response_cache_freshness(
            &[workspace],
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("manifest freshness should compute");

    assert!(
        freshness.scopes.is_none(),
        "dirty roots must bypass repository answer caches even when the latest snapshot is still valid"
    );
    assert_eq!(
        freshness.basis.get("cacheable").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/manifest")
            .and_then(Value::as_str),
        Some("ready")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/dirty_root")
            .and_then(Value::as_bool),
        Some(true)
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_response_cache_freshness_marks_missing_snapshot_uncacheable() {
    let workspace_root = temp_workspace_root("response-cache-freshness-missing");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Missing;\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new_with_runtime_options(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
        false,
        false,
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    let db_path = crate::storage::ensure_provenance_db_parent_dir(&workspace_root)
        .expect("provenance db parent dir should be creatable for missing-snapshot test");
    Storage::new(db_path)
        .initialize()
        .expect("storage should initialize for missing-snapshot freshness checks");

    let freshness = server
        .repository_response_cache_freshness(
            &[workspace],
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("manifest freshness should compute");

    assert!(
        freshness.scopes.is_none(),
        "missing manifest snapshots must bypass repository answer caches"
    );
    assert_eq!(
        freshness.basis.get("cacheable").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/manifest")
            .and_then(Value::as_str),
        Some("missing_snapshot")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/dirty_root")
            .and_then(Value::as_bool),
        Some(false)
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_response_cache_freshness_marks_stale_snapshot_uncacheable() {
    let workspace_root = temp_workspace_root("response-cache-freshness-stale");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    let source_path = workspace_root.join("src/lib.rs");
    fs::write(&source_path, "pub struct Stale;\n").expect("failed to write source fixture");

    let server = FriggMcpServer::new_with_runtime_options(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
        false,
        false,
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    rewrite_fixture_file_with_mtime_tick(&source_path, "pub struct StaleAfterEdit;\n");

    let freshness = server
        .repository_response_cache_freshness(
            &[workspace],
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("manifest freshness should compute");

    assert!(
        freshness.scopes.is_none(),
        "stale manifest snapshots must bypass repository answer caches"
    );
    assert_eq!(
        freshness.basis.get("cacheable").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/manifest")
            .and_then(Value::as_str),
        Some("stale_snapshot")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/dirty_root")
            .and_then(Value::as_bool),
        Some(false)
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_response_cache_freshness_includes_semantic_scope_metadata() {
    let workspace_root = temp_workspace_root("response-cache-freshness-semantic-aware");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Semantic;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.semantic_runtime = semantic_runtime_enabled_openai();
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    Storage::new(&workspace.db_path)
        .replace_semantic_embeddings_for_repository(
            &workspace.repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("semantic embeddings should persist");

    let freshness = server
        .repository_response_cache_freshness(
            &[workspace.clone()],
            RepositoryResponseCacheFreshnessMode::SemanticAware,
        )
        .expect("semantic-aware freshness should compute");

    let scopes = freshness
        .scopes
        .as_ref()
        .expect("ready semantic snapshot should remain cacheable");
    assert_eq!(scopes.len(), 1);
    assert_eq!(scopes[0].repository_id, workspace.repository_id);
    assert_eq!(scopes[0].snapshot_id, "snapshot-001");
    assert_eq!(scopes[0].semantic_state.as_deref(), Some("ready"));
    assert_eq!(scopes[0].semantic_provider.as_deref(), Some("openai"));
    assert_eq!(
        scopes[0].semantic_model.as_deref(),
        Some("text-embedding-3-small")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/semantic")
            .and_then(Value::as_str),
        Some("ready")
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_lexical_summary_stays_ready_when_semantic_config_is_invalid() {
    let workspace_root = temp_workspace_root("workspace-lexical-invalid-semantic-config");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.semantic_runtime.enabled = true;
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );

    let storage = FriggMcpServer::workspace_storage_summary(&workspace);
    let lexical = server.workspace_lexical_index_summary(&workspace, &storage);
    let semantic = server.workspace_semantic_index_summary(&workspace, &storage);

    assert_eq!(lexical.state, WorkspaceIndexComponentState::Ready);
    assert_eq!(lexical.reason, None);
    assert_eq!(lexical.snapshot_id.as_deref(), Some("snapshot-001"));
    assert_eq!(lexical.artifact_count, Some(1));

    assert_eq!(semantic.state, WorkspaceIndexComponentState::Error);
    assert_eq!(
        semantic.reason.as_deref(),
        Some("semantic_runtime_invalid_config")
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_summary_bypasses_cached_ready_lexical_health_for_dirty_roots() {
    let workspace_root = temp_workspace_root("repository-summary-dirty-root-bypass");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct DirtySummary;\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new_with_runtime_options(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
        false,
        false,
    );
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );

    let initial = server.repository_summary(&workspace);
    let initial_lexical = initial
        .health
        .as_ref()
        .expect("repository summary should expose health")
        .lexical
        .clone();
    assert_eq!(initial_lexical.state, WorkspaceIndexComponentState::Ready);
    assert_eq!(initial_lexical.reason, None);
    assert_eq!(initial_lexical.snapshot_id.as_deref(), Some("snapshot-001"));

    server
        .runtime_state
        .validated_manifest_candidate_cache
        .write()
        .expect("validated manifest candidate cache should not be poisoned")
        .mark_dirty_root(&workspace.root);

    let refreshed = server.repository_summary(&workspace);
    let refreshed_lexical = refreshed
        .health
        .as_ref()
        .expect("repository summary should expose health")
        .lexical
        .clone();
    assert_eq!(refreshed_lexical.state, WorkspaceIndexComponentState::Stale);
    assert_eq!(refreshed_lexical.reason.as_deref(), Some("dirty_root"));
    assert_eq!(
        refreshed_lexical.snapshot_id.as_deref(),
        Some("snapshot-001")
    );
    assert_eq!(refreshed_lexical.artifact_count, Some(1));

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_current_runtime_tasks_surface_class_aware_watch_phases() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), true, false);

    let manifest_task_id = server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .start_task(
            RuntimeTaskKind::ChangedReindex,
            "repo-001",
            "watch_manifest_fast",
            Some("watch root /tmp/repo-001 class manifest_fast".to_owned()),
        );
    server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .finish_task(
            &manifest_task_id,
            RuntimeTaskStatus::Succeeded,
            Some("watch root /tmp/repo-001 class manifest_fast".to_owned()),
        );
    server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .start_task(
            RuntimeTaskKind::SemanticRefresh,
            "repo-001",
            "watch_semantic_followup",
            Some("watch root /tmp/repo-001 class semantic_followup".to_owned()),
        );

    let runtime = server.runtime_status_summary();

    assert!(
        runtime.recent_tasks.iter().any(|task| {
            task.kind == RuntimeTaskKind::ChangedReindex
                && task.phase == "watch_manifest_fast"
                && task.detail.as_deref() == Some("watch root /tmp/repo-001 class manifest_fast")
        }),
        "recent tasks should surface manifest-fast watch work distinctly"
    );
    assert!(
        runtime.active_tasks.iter().any(|task| {
            task.kind == RuntimeTaskKind::SemanticRefresh
                && task.phase == "watch_semantic_followup"
                && task.detail.as_deref()
                    == Some("watch root /tmp/repo-001 class semantic_followup")
        }),
        "active tasks should surface semantic-followup watch work distinctly"
    );
}

#[test]
fn precise_graph_prewarm_populates_latest_precise_cache() {
    let workspace_root = temp_workspace_root("precise-prewarm");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct User;\n\npub fn current_user() -> User { User }\n",
    )
    .expect("failed to write source fixture");
    write_scip_protobuf_fixture(&workspace_root, "fixture.scip");

    let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

    let _ = server.prewarm_precise_graph_for_workspace(&workspace);

    let cached = server
        .cache_state
        .latest_precise_graph_cache
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .get(&workspace.repository_id)
        .cloned()
        .expect("precise prewarm should populate the latest precise graph cache");
    assert_eq!(cached.ingest_stats.artifacts_ingested, 1);
    assert_eq!(cached.ingest_stats.artifacts_failed, 0);
    assert_eq!(
        cached.coverage_mode,
        crate::mcp::server_state::PreciseCoverageMode::Full
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn precise_definition_fast_path_resolves_location_without_symbol_corpus_rebuild() {
    let workspace_root = temp_workspace_root("precise-fast-path");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct User;\n\npub fn current_user() -> User { User }\n",
    )
    .expect("failed to write source fixture");
    write_scip_protobuf_fixture(&workspace_root, "fixture.scip");

    let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    let workspace = server
        .runtime_state
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

    let response = server
        .try_precise_definition_fast_path(
            Some(&workspace.repository_id),
            "src/lib.rs",
            1,
            Some(13),
            10,
        )
        .expect("cached precise fast path should not error")
        .expect("cached precise fast path should resolve a definition");
    assert_eq!(response.1, workspace.repository_id);
    assert_eq!(response.2, "scip-rust pkg repo#User");
    assert_eq!(response.3, "precise");
    assert_eq!(response.0.0.matches.len(), 1);
    assert_eq!(response.0.0.matches[0].path, "src/lib.rs");
    assert_eq!(response.0.0.matches[0].line, 1);

    let _ = fs::remove_dir_all(workspace_root);
}
