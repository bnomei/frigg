#![allow(clippy::panic)]

use super::*;

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
        metadata: None,
    };
    let empty_navigation_response = GoToDefinitionResponse {
        matches: Vec::new(),
        mode: NavigationMode::UnavailableNoPrecise,
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
                include_follow_up_structural: false,
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

#[tokio::test]
async fn read_file_and_explore_share_the_file_content_window_cache() {
    let workspace_root = temp_workspace_root("file-content-window-cache-share");
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
        &workspace.repository_id,
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
    };
    let first_read = server
        .read_file_impl(read_params.clone())
        .await
        .expect("first read_file call should succeed");
    assert!(first_read.0.content.contains("pub fn alpha"));
    assert_eq!(
        server
            .cache_state
            .file_content_window_cache
            .read()
            .expect("file content cache should not be poisoned")
            .len(),
        1
    );
    assert_eq!(
        server.runtime_cache_telemetry(RuntimeCacheFamily::FileContentWindow),
        crate::mcp::server_cache::RuntimeCacheTelemetry {
            hits: 0,
            misses: 1,
            bypasses: 0,
            inserts: 1,
            evictions: 0,
            invalidations: 0,
        }
    );

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
        })
        .await
        .expect("explore should reuse the shared file content cache");
    assert_eq!(first_explore.0.total_matches, 1);
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::FileContentWindow)
            .hits
            >= 1
    );
    assert_eq!(
        server
            .cache_state
            .file_content_window_cache
            .read()
            .expect("file content cache should not be poisoned")
            .len(),
        1
    );

    let second_read = server
        .read_file_impl(read_params)
        .await
        .expect("second read_file call should reuse the shared file content cache");
    assert_eq!(first_read.0.content, second_read.0.content);
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::FileContentWindow)
            .hits
            >= 2
    );

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
        metadata: None,
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
        mode: NavigationMode::UnavailableNoPrecise,
        metadata: None,
        note: None,
    };
    let empty_declarations_response = FindDeclarationsResponse {
        matches: Vec::new(),
        mode: NavigationMode::UnavailableNoPrecise,
        metadata: None,
        note: None,
    };
    server.cache_file_content_window(
        FileContentWindowCacheKey {
            scoped_repository_ids: vec!["repo-001".to_owned()],
            freshness_scopes: vec![repo_001_scope.clone()],
            canonical_path: PathBuf::from("/tmp/repo-001/file.rs"),
        },
        Arc::new(FileContentSnapshot::from_bytes(
            b"fn repo_001() {}\n".to_vec(),
        )),
    );
    server.cache_file_content_window(
        FileContentWindowCacheKey {
            scoped_repository_ids: vec!["repo-002".to_owned()],
            freshness_scopes: vec![repo_002_scope.clone()],
            canonical_path: PathBuf::from("/tmp/repo-002/file.rs"),
        },
        Arc::new(FileContentSnapshot::from_bytes(
            b"fn repo_002() {}\n".to_vec(),
        )),
    );

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
            include_follow_up_structural: false,
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
            include_follow_up_structural: false,
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
            include_follow_up_structural: false,
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
            include_follow_up_structural: false,
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
    assert_eq!(
        server
            .cache_state
            .file_content_window_cache
            .read()
            .expect("file content cache should not be poisoned")
            .len(),
        1
    );
    assert!(
        server
            .runtime_cache_telemetry(RuntimeCacheFamily::FileContentWindow)
            .invalidations
            >= 1
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
        metadata: None,
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
        mode: NavigationMode::UnavailableNoPrecise,
        metadata: None,
        note: None,
    };
    let empty_declarations_response = FindDeclarationsResponse {
        matches: Vec::new(),
        mode: NavigationMode::UnavailableNoPrecise,
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
        std::slice::from_ref(&workspace.repository_id),
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
            include_follow_up_structural: false,
            limit: 5,
        },
        &empty_navigation_response,
        std::slice::from_ref(&workspace.repository_id),
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
            include_follow_up_structural: false,
            limit: 5,
        },
        &empty_declarations_response,
        std::slice::from_ref(&workspace.repository_id),
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
