#![allow(clippy::panic)]

//! Regression tests for runtime cache budgets, bypass rules, and repository invalidation.

use super::*;

#[test]
fn runtime_cache_registry_classifies_snapshot_metadata_and_request_local_families() {
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

    let process_metadata = server.runtime_cache_policy(RuntimeCacheFamily::HeuristicReference);
    assert_eq!(
        process_metadata.residency,
        RuntimeCacheResidency::ProcessWide
    );
    assert_eq!(
        process_metadata.reuse_class,
        RuntimeCacheReuseClass::ProcessMetadata
    );
    assert_eq!(
        process_metadata.freshness_contract,
        RuntimeCacheFreshnessContract::RepositoryId
    );
    assert!(process_metadata.dirty_root_bypass);
    assert_eq!(process_metadata.budget.max_entries, Some(128));

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

#[tokio::test]
async fn read_file_and_explore_read_current_file_content() {
    let workspace_root = temp_workspace_root("file-content-direct-reads");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn alpha() {}\npub fn beta() {}\n",
    )
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
        &workspace.runtime_repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");

    let read_params = crate::mcp::types::ReadFileParams {
        path: "src/lib.rs".to_owned(),
        repository_id: Some(workspace.repository_id.clone()),
        max_bytes: None,
        line_start: None,
        line_end: None,
        presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
        include_context_efficiency: None,
    };
    let first_read = server
        .read_file_impl(read_params.clone())
        .await
        .expect("first read_file call should succeed");
    assert!(first_read.content.contains("pub fn alpha"));

    let first_explore = server
        .explore_impl(crate::mcp::types::ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some(workspace.repository_id.clone()),
            operation: crate::mcp::types::ExploreOperation::Probe,
            query: Some("beta".to_owned()),
            pattern_type: Some(crate::mcp::types::SearchPatternType::Literal),
            anchor: None,
            context_lines: None,
            max_matches: Some(4),
            resume_from: None,
            presentation_mode: None,
            include_context_efficiency: None,
        })
        .await
        .expect("explore should read the source file");
    assert_eq!(first_explore.total_matches, 1);

    let second_read = server
        .read_file_impl(read_params)
        .await
        .expect("second read_file call should read the source file");
    assert_eq!(first_read.content, second_read.content);

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

    assert!(
        !runtime
            .lease_status(&workspace.runtime_repository_id)
            .active
    );

    server
        .adopt_workspace(&workspace, true)
        .expect("first session should adopt workspace");
    assert_eq!(
        runtime
            .lease_status(&workspace.runtime_repository_id)
            .lease_count,
        1
    );

    second_session
        .adopt_workspace(&workspace, true)
        .expect("second session should share the same watch lease");
    assert_eq!(
        runtime
            .lease_status(&workspace.runtime_repository_id)
            .lease_count,
        2
    );

    server
        .detach_workspace(&workspace.repository_id)
        .expect("first session should detach workspace");
    assert_eq!(
        runtime
            .lease_status(&workspace.runtime_repository_id)
            .lease_count,
        1
    );

    second_session
        .detach_workspace(&workspace.repository_id)
        .expect("second session should detach workspace");
    assert_eq!(
        runtime
            .lease_status(&workspace.runtime_repository_id)
            .lease_count,
        0
    );
    assert!(
        !runtime
            .lease_status(&workspace.runtime_repository_id)
            .active
    );

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
fn workspace_attach_invalidates_only_attached_repository_navigation_caches() {
    let workspace_root = temp_workspace_root("attach-invalidates-only-attached-navigation-caches");
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

    let attached_repository_id = crate::domain::model::stable_repository_id_for_root(&root).0;
    server.cache_heuristic_references(
        HeuristicReferenceCacheKey {
            repository_id: attached_repository_id.clone(),
            symbol_id: "symbol-001".to_owned(),
            corpus_signature: "corpus-001".to_owned(),
            scip_signature: "scip-001".to_owned(),
        },
        Vec::new(),
        0,
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
        0,
    );

    let _ = server
        .attach_workspace_internal(&root, true, WorkspaceResolveMode::GitRoot)
        .expect("workspace attach should succeed");

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
            .heuristic_reference_cache
            .read()
            .expect("heuristic reference cache should not be poisoned")
            .keys()
            .all(|key| key.repository_id == "repo-002"),
        "heuristic reference cache should retain only unaffected repository entries"
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
async fn watch_notify_invalidates_live_server_navigation_caches() {
    let workspace_root = temp_workspace_root("watch-invalidates-live-navigation-caches");
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
    crate::indexer::index_repository_with_runtime_config(
        &declared_repository.repository_id.0,
        &declared_root,
        &db_path,
        IndexMode::ChangedOnly,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
    )
    .expect("baseline changed-only index should succeed");

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
        0,
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
        wait_for_heuristic_reference_cache_eviction(&server, Duration::from_secs(5)).await,
        "watch notify should evict live repository-scoped navigation caches"
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

#[tokio::test]
async fn read_match_reads_from_cached_result_handle() {
    let workspace_root = temp_workspace_root("read-match-result-handle");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn unique_marker_alpha() {}\n",
    )
    .expect("failed to write source fixture");

    let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("runtime gate tests should build a valid FriggConfig");
    let declared_repository = config
        .repositories()
        .into_iter()
        .next()
        .expect("read_match result-handle test should define one repository");
    let declared_root = PathBuf::from(&declared_repository.root_path);
    let db_path = crate::storage::ensure_provenance_db_parent_dir(&declared_root)
        .expect("storage path should resolve");
    Storage::new(&db_path)
        .initialize()
        .expect("storage should initialize");
    crate::indexer::index_repository_with_runtime_config(
        &declared_repository.repository_id.0,
        &declared_root,
        &db_path,
        IndexMode::ChangedOnly,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
    )
    .expect("baseline changed-only index should succeed");

    let server = FriggMcpServer::new(config);
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");

    let search = server
        .search_text_impl(crate::mcp::types::SearchTextParams {
            query: "unique_marker_alpha".to_owned(),
            pattern_type: None,
            repository_id: Some(workspace.repository_id.clone()),
            path_regex: None,
            limit: None,
            context_lines: None,
            max_matches_per_file: None,
            collapse_by_file: None,
            response_mode: None,
            include_context_efficiency: None,
        })
        .await
        .expect("search_text should succeed")
        .0;
    let result_handle = search
        .result_handle
        .expect("search_text should return a result handle");
    let match_id = search
        .matches
        .first()
        .and_then(|found| found.match_id.clone())
        .expect("search_text match should carry a match id");

    server
        .read_match_impl(crate::mcp::types::ReadMatchParams {
            result_handle: result_handle.clone(),
            match_id: match_id.clone(),
            before: None,
            after: None,
            presentation_mode: None,
            include_context_efficiency: None,
        })
        .await
        .expect("read_match should succeed");

    let _ = fs::remove_dir_all(workspace_root);
}
