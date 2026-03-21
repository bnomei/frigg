use std::collections::BTreeSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::domain::FriggError;
use crate::domain::model::TextMatch;
use crate::indexer::{FileMetadataDigest, ReindexMode, reindex_repository};
use crate::mcp::RuntimeTaskRegistry;
use crate::mcp::server_cache::{
    FileContentSnapshot, FileContentWindowCacheKey, FindDeclarationsResponseCacheKey,
    GoToDefinitionResponseCacheKey, HeuristicReferenceCacheKey, RepositoryFreshnessCacheScope,
    RuntimeCacheFamily, RuntimeCacheFreshnessContract, RuntimeCacheResidency,
    RuntimeCacheReuseClass, SearchHybridResponseCacheKey, SearchSymbolResponseCacheKey,
    SearchTextResponseCacheKey,
};
use crate::mcp::tool_surface::{ToolSurfaceProfile, manifest_for_tool_surface_profile};
use crate::mcp::types::{
    FindDeclarationsResponse, GoToDefinitionResponse, InspectSyntaxTreeParams, NavigationMode,
    RuntimeTaskKind, RuntimeTaskStatus, SearchHybridResponse, SearchStructuralParams,
    SearchSymbolResponse, SearchTextResponse, WorkspaceAttachParams, WorkspaceDetachParams,
    WorkspaceIndexComponentState, WorkspacePreciseGeneratorState, WorkspaceResolveMode,
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

use super::{
    FriggMcpServer, PreciseCoverageMode, PreciseIngestStats, ReadOnlyToolExecutionContext,
    RepositoryResponseCacheFreshnessMode,
};

static TEST_PATH_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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

fn write_fake_precise_generator_script(
    bin_dir: &Path,
    name: &str,
    version_output: &str,
    payload: &str,
) -> PathBuf {
    let path = bin_dir.join(name);
    write_fake_precise_generator_script_with_body(
        bin_dir,
        name,
        &format!(
            r#"#!/bin/sh
if [ "${{1:-}}" = "--version" ] || [ "${{1:-}}" = "version" ]; then
  printf '%s\n' "{version_output}"
  exit 0
fi
printf '%s' "{payload}"
"#
        ),
    );
    path
}

fn write_fake_precise_generator_script_with_body(
    bin_dir: &Path,
    name: &str,
    body: &str,
) -> PathBuf {
    let path = bin_dir.join(name);
    fs::write(&path, body).expect("failed to write fake precise generator script");
    let mut permissions = fs::metadata(&path)
        .expect("fake precise generator script should exist")
        .permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&path, permissions)
        .expect("fake precise generator script should be executable");
    path
}

fn with_fake_precise_generator_path<T>(bin_dir: &Path, f: impl FnOnce() -> T) -> T {
    let _guard = TEST_PATH_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    FriggMcpServer::set_test_precise_generator_bin_override(Some(bin_dir.to_path_buf()));

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    FriggMcpServer::set_test_precise_generator_bin_override(None);

    match result {
        Ok(value) => value,
        Err(payload) => std::panic::resume_unwind(payload),
    }
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

    unreachable!("fixture file mtime did not advance after rewrite");
}

fn semantic_runtime_enabled_openai() -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    }
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
        let file_content_evicted = server
            .cache_state
            .file_content_window_cache
            .read()
            .expect("file content window cache should not be poisoned")
            .is_empty();

        if summary_evicted
            && text_evicted
            && hybrid_evicted
            && symbol_evicted
            && definition_evicted
            && declarations_evicted
            && heuristic_evicted
            && file_content_evicted
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

mod cache_runtime;
mod error_mapping;
mod freshness;
mod navigation;
mod precise_generation;
mod status;
mod workspace;
