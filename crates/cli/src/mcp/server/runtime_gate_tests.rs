use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::FriggError;
use crate::indexer::FileMetadataDigest;
use crate::mcp::RuntimeTaskRegistry;
use crate::mcp::types::{
    RuntimeTaskKind, RuntimeTaskStatus, WorkspaceIndexComponentState, WorkspaceResolveMode,
};
use crate::searcher::ValidatedManifestCandidateCache;
use crate::settings::{
    FriggConfig, RuntimeProfile, SemanticRuntimeConfig, SemanticRuntimeProvider,
};
use crate::storage::{
    DEFAULT_VECTOR_DIMENSIONS, ManifestEntry, SemanticChunkEmbeddingRecord, Storage,
};
use protobuf::{EnumOrUnknown, Message};
use rmcp::model::ErrorCode;
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    SymbolInformation as ScipSymbolInformationProto,
};

use super::FriggMcpServer;

fn fixture_config() -> FriggConfig {
    let workspace_root = std::env::current_dir()
        .expect("current working directory should exist for runtime gate tests");
    FriggConfig::from_workspace_roots(vec![workspace_root])
        .expect("runtime gate tests should build a valid FriggConfig")
}

fn to_set(values: Vec<String>) -> BTreeSet<String> {
    values.into_iter().collect()
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

fn semantic_runtime_enabled_openai() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
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

    for tool_name in FriggMcpServer::EXTENDED_ONLY_TOOL_NAMES {
        assert!(
            !names.contains(tool_name),
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

    for tool_name in FriggMcpServer::EXTENDED_ONLY_TOOL_NAMES {
        assert!(
            names.contains(tool_name),
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
fn strict_semantic_failure_maps_to_unavailable_error_code() {
    let error = FriggMcpServer::map_frigg_error(FriggError::Internal(
        "semantic_status=strict_failure: provider outage".to_owned(),
    ));

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
            .and_then(|value| value.get("semantic_status"))
            .and_then(|value| value.as_str()),
        Some("strict_failure")
    );
}

#[test]
fn search_hybrid_warning_surfaces_semantic_ok_empty_channel() {
    let warning = FriggMcpServer::search_hybrid_warning(Some("ok"), None, Some(0), Some(0));

    assert_eq!(
        warning.as_deref(),
        Some(
            "semantic retrieval completed successfully but retained no query-relevant semantic hits; results are ranked from lexical and graph signals only"
        )
    );
}

#[test]
fn search_hybrid_warning_surfaces_semantic_ok_noncontributing_hits() {
    let warning = FriggMcpServer::search_hybrid_warning(Some("ok"), None, Some(3), Some(0));

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
        vec![PathBuf::from("src/lib.rs"), PathBuf::from("src/server.php")]
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
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .attached_workspaces()
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
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .attached_workspaces()
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
fn workspace_current_runtime_tasks_surface_class_aware_watch_phases() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), true, false);

    let manifest_task_id = server
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
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .finish_task(
            &manifest_task_id,
            RuntimeTaskStatus::Succeeded,
            Some("watch root /tmp/repo-001 class manifest_fast".to_owned()),
        );
    server
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
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .attached_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");

    let _ = server.prewarm_precise_graph_for_workspace(&workspace);

    let cached = server
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
        .workspace_registry
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .attached_workspaces()
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
