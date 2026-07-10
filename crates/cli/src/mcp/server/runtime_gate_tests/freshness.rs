#![allow(clippy::panic)]

//! Regression tests for repository freshness scopes that drive response-cache eligibility.

use super::*;

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
        &workspace.runtime_repository_id,
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

    assert_eq!(context.base.tool_name, "search_text");
    assert_eq!(context.base.repository_hint, None);
    assert_eq!(context.scoped_repository_ids, vec![repository_id]);
    assert_eq!(context.scoped_workspaces.len(), 1);
    assert!(
        context.cache_freshness.scopes.is_some(),
        "scoped execution context should capture cache freshness inputs"
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
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.runtime_repository_id,
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
    let server = FriggMcpServer::new_with_runtime_options(config, false);
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
        &workspace.runtime_repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    let storage = Storage::new(&workspace.db_path);
    storage
        .replace_semantic_embeddings_for_repository(
            &workspace.runtime_repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.runtime_repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("seed semantic embeddings should persist");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.runtime_repository_id,
        "snapshot-002",
        &["src/lib.rs"],
    );

    let plan = server
        .workspace_semantic_refresh_plan(&workspace)
        .expect("latest snapshot without active-model semantic rows should trigger refresh");
    assert_eq!(plan.latest_snapshot_id, "snapshot-002");
    assert_eq!(plan.reason, "semantic_snapshot_missing_for_active_model");

    server.maybe_spawn_workspace_runtime_prewarm(&workspace);
    let runtime = server.runtime_status_summary();
    let spawned_semantic_refresh = runtime
        .active_tasks
        .iter()
        .chain(runtime.recent_tasks.iter())
        .any(|task| {
            task.kind == RuntimeTaskKind::SemanticRefresh && task.phase == "semantic_attach_refresh"
        });
    assert!(
        spawned_semantic_refresh,
        "missing-active-model plan should spawn a semantic refresh during defer prewarm"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn semantic_attach_refresh_is_not_admitted_during_active_index_work() {
    let workspace_root = temp_workspace_root("semantic-refresh-active-index-admission");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.semantic_runtime = semantic_runtime_enabled_openai();
    let server = FriggMcpServer::new_with_runtime_options(config, false);
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
        &workspace.runtime_repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    Storage::new(&workspace.db_path)
        .replace_semantic_embeddings_for_repository(
            &workspace.runtime_repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.runtime_repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("seed semantic embeddings should persist");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.runtime_repository_id,
        "snapshot-002",
        &["src/lib.rs"],
    );
    let plan = server
        .workspace_semantic_refresh_plan(&workspace)
        .expect("latest snapshot without active-model semantic rows should trigger refresh");
    assert_eq!(plan.latest_snapshot_id, "snapshot-002");

    let active_task_id = server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .start_task(
            RuntimeTaskKind::WorkspaceIndex,
            workspace.runtime_repository_id.clone(),
            "workspace_index",
            Some("active alias index".to_owned()),
        );

    server.maybe_spawn_workspace_runtime_prewarm(&workspace);
    let runtime = server.runtime_status_summary();
    let spawned_semantic_refresh = runtime
        .active_tasks
        .iter()
        .chain(runtime.recent_tasks.iter())
        .any(|task| {
            task.kind == RuntimeTaskKind::SemanticRefresh && task.phase == "semantic_attach_refresh"
        });
    assert!(
        !spawned_semantic_refresh,
        "semantic attach refresh must not be admitted while alias index work is active"
    );

    server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .finish_task(&active_task_id, RuntimeTaskStatus::Succeeded, None);

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn semantic_attach_refresh_is_admitted_after_prepare_task_finishes() {
    let workspace_root = temp_workspace_root("semantic-refresh-after-prepare-finished");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct User;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.semantic_runtime = semantic_runtime_enabled_openai();
    let server = FriggMcpServer::new_with_runtime_options(config, false);
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
        &workspace.runtime_repository_id,
        "snapshot-001",
        &["src/lib.rs"],
    );
    Storage::new(&workspace.db_path)
        .replace_semantic_embeddings_for_repository(
            &workspace.runtime_repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.runtime_repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("seed semantic embeddings should persist");
    seed_manifest_snapshot(
        &workspace_root,
        &workspace.runtime_repository_id,
        "snapshot-002",
        &["src/lib.rs"],
    );

    let mut prepare_guard = server
        .try_start_repository_runtime_task(
            &workspace,
            RuntimeTaskKind::WorkspacePrepare,
            "workspace_prepare",
            Some("prepare before prewarm".to_owned()),
        )
        .expect("prepare guard should start for regression setup");
    prepare_guard.finish(RuntimeTaskStatus::Succeeded, None);

    server.maybe_spawn_workspace_runtime_prewarm(&workspace);
    let runtime = server.runtime_status_summary();
    let spawned_semantic_refresh = runtime
        .active_tasks
        .iter()
        .chain(runtime.recent_tasks.iter())
        .any(|task| {
            task.kind == RuntimeTaskKind::SemanticRefresh && task.phase == "semantic_attach_refresh"
        });
    assert!(
        spawned_semantic_refresh,
        "workspace_prepare must finish its guard before spawning semantic attach refresh"
    );

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

    let freshness = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
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
        entry.get("family").and_then(Value::as_str) == Some("validated_manifest_candidate")
            && entry.pointer("/budget/max_entries").and_then(Value::as_u64) == Some(128)
    }));
    assert_eq!(runtime_contract.len(), 1);

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
        "dirty roots must mark response freshness uncacheable even when the latest snapshot is still valid"
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
        "missing manifest snapshots must mark response freshness uncacheable"
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
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/cacheable_reason")
            .and_then(Value::as_str),
        Some("missing_snapshot")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/candidate_source")
            .and_then(Value::as_str),
        Some("live_walk")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/using_live_walk")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/recommended_client_behavior")
            .and_then(Value::as_str),
        Some("continue_using_frigg_live_fallback")
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
    rewrite_fixture_file_with_mtime_tick(&source_path, "pub struct StaleAfterEdit;\n");

    let freshness = server
        .repository_response_cache_freshness(
            &[workspace],
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("manifest freshness should compute");

    assert!(
        freshness.scopes.is_none(),
        "stale manifest snapshots must mark response freshness uncacheable"
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
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/cacheable_reason")
            .and_then(Value::as_str),
        Some("stale_snapshot")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/candidate_source")
            .and_then(Value::as_str),
        Some("live_walk")
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/using_live_walk")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        freshness
            .basis
            .pointer("/repositories/0/recommended_client_behavior")
            .and_then(Value::as_str),
        Some("continue_using_frigg_live_fallback")
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
    let server = FriggMcpServer::new_with_runtime_options(config, false);
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
    Storage::new(&workspace.db_path)
        .replace_semantic_embeddings_for_repository(
            &workspace.runtime_repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.runtime_repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("semantic embeddings should persist");

    let freshness = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
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

#[tokio::test(flavor = "current_thread")]
async fn semantic_aware_response_freshness_recomputes_after_refresh_invalidation() {
    let workspace_root = temp_workspace_root("response-cache-freshness-semantic-invalidation");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Semantic;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 25,
        retry_ms: 100,
        ..WatchConfig::default()
    };
    config.semantic_runtime = semantic_runtime_enabled_openai();

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

    let mut watch_config = config.clone();
    watch_config.semantic_runtime = SemanticRuntimeConfig::default();
    let runtime = Arc::new(
        maybe_start_watch_runtime(
            &watch_config,
            RuntimeTransportKind::Stdio,
            Arc::clone(&runtime_task_registry),
            Arc::clone(&validated_manifest_candidate_cache),
            None,
        )
        .expect("watch runtime should start")
        .expect("watch runtime should be enabled"),
    );
    server.set_watch_runtime(Some(Arc::clone(&runtime)));
    runtime
        .acquire_lease(&workspace)
        .expect("runtime should watch the workspace");

    let missing = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::SemanticAware,
        )
        .expect("semantic-aware freshness should compute");
    assert_eq!(
        missing
            .basis
            .pointer("/repositories/0/semantic")
            .and_then(Value::as_str),
        Some("missing_for_active_model")
    );
    assert_eq!(
        server
            .cache_state
            .repository_response_freshness_cache
            .read()
            .expect("response freshness cache should not be poisoned")
            .len(),
        1,
        "watch-backed semantic freshness should cache while no refresh is active"
    );

    Storage::new(&workspace.db_path)
        .replace_semantic_embeddings_for_repository(
            &workspace.runtime_repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.runtime_repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("semantic refresh should persist embeddings");

    let mut task_guard = RuntimeTaskGuard::start(
        Arc::clone(&runtime_task_registry),
        RuntimeTaskKind::SemanticRefresh,
        workspace.repository_id.clone(),
        "semantic_attach_refresh",
        Some("test semantic refresh".to_owned()),
    );
    let active = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::SemanticAware,
        )
        .expect("active semantic freshness should compute");
    assert_eq!(
        active
            .basis
            .pointer("/repositories/0/semantic")
            .and_then(Value::as_str),
        Some("ready"),
        "active semantic refresh must bypass the stale cached missing state"
    );
    assert_eq!(
        active
            .basis
            .pointer("/repositories/0/refresh_in_progress")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        active.basis.get("cacheable").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        active
            .basis
            .pointer("/repositories/0/cacheable_reason")
            .and_then(Value::as_str),
        Some("refresh_in_progress")
    );
    assert_eq!(
        active
            .basis
            .pointer("/repositories/0/recommended_client_behavior")
            .and_then(Value::as_str),
        Some("requery_after_refresh")
    );
    assert!(
        active.scopes.is_none(),
        "freshness should not cache while semantic refresh metadata is changing"
    );

    task_guard.finish(RuntimeTaskStatus::Succeeded, None);
    server.invalidate_repository_response_freshness_cache(&workspace.repository_id);
    let ready = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::SemanticAware,
        )
        .expect("ready semantic freshness should compute after invalidation");
    assert_eq!(
        ready
            .basis
            .pointer("/repositories/0/semantic")
            .and_then(Value::as_str),
        Some("ready")
    );
    assert_eq!(
        ready
            .basis
            .pointer("/repositories/0/refresh_in_progress")
            .and_then(Value::as_bool),
        Some(false)
    );
    let scopes = ready
        .scopes
        .as_ref()
        .expect("ready semantic freshness should be cacheable after refresh completion");
    assert_eq!(scopes[0].semantic_state.as_deref(), Some("ready"));

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn semantic_refresh_function_invalidates_response_freshness_cache() {
    let workspace_root = temp_workspace_root("semantic-refresh-function-invalidates-freshness");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Semantic;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 25,
        retry_ms: 100,
        ..WatchConfig::default()
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

    let runtime = Arc::new(
        maybe_start_watch_runtime(
            &config,
            RuntimeTransportKind::Stdio,
            Arc::clone(&runtime_task_registry),
            Arc::clone(&validated_manifest_candidate_cache),
            None,
        )
        .expect("watch runtime should start")
        .expect("watch runtime should be enabled"),
    );
    server.set_watch_runtime(Some(Arc::clone(&runtime)));
    runtime
        .acquire_lease(&workspace)
        .expect("runtime should watch the workspace");

    let cached = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("manifest freshness should compute");
    assert!(
        cached.scopes.is_some(),
        "watch-backed manifest freshness should be cacheable before refresh"
    );
    assert_eq!(
        server
            .cache_state
            .repository_response_freshness_cache
            .read()
            .expect("response freshness cache should not be poisoned")
            .len(),
        1
    );

    let plan = crate::mcp::server_cache::WorkspaceSemanticRefreshPlan {
        latest_snapshot_id: "snapshot-001".to_owned(),
        reason: "test_refresh",
    };
    server
        .refresh_workspace_semantic_snapshot_with_plan(&workspace, &plan)
        .expect("disabled semantic runtime should still run lexical refresh");

    assert!(
        server
            .cache_state
            .repository_response_freshness_cache
            .read()
            .expect("response freshness cache should not be poisoned")
            .is_empty(),
        "semantic refresh production path should invalidate cached response freshness"
    );

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn notify_dropped_invalidation_recomputes_response_freshness_as_dirty_root() {
    let workspace_root = temp_workspace_root("notify-dropped-recomputes-freshness");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct NotifyDropped;\n",
    )
    .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 500,
        retry_ms: 100,
        ..WatchConfig::default()
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
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    fs::create_dir_all(
        workspace
            .db_path
            .parent()
            .expect("workspace db path should have a parent directory"),
    )
    .expect("workspace db parent should be creatable");
    crate::indexer::index_repository_with_runtime_config(
        &workspace.runtime_repository_id,
        &workspace.root,
        &workspace.db_path,
        crate::indexer::IndexMode::Full,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::from_process_env(),
    )
    .expect("fixture index should seed a ready manifest snapshot");

    let runtime = Arc::new(
        maybe_start_watch_runtime(
            &config,
            RuntimeTransportKind::Stdio,
            Arc::clone(&runtime_task_registry),
            Arc::clone(&validated_manifest_candidate_cache),
            Some(server.repository_cache_invalidation_callback()),
        )
        .expect("watch runtime should start")
        .expect("watch runtime should be enabled"),
    );
    server.set_watch_runtime(Some(Arc::clone(&runtime)));
    runtime
        .acquire_lease(&workspace)
        .expect("runtime should watch the workspace");

    let cached = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("ready manifest freshness should compute");
    assert!(
        cached.scopes.is_some(),
        "ready watch-backed freshness should cache before notify drops"
    );
    assert_eq!(
        cached
            .basis
            .pointer("/repositories/0/dirty_root")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        server
            .cache_state
            .repository_response_freshness_cache
            .read()
            .expect("response freshness cache should not be poisoned")
            .len(),
        1
    );

    runtime.inject_test_notify_dropped("test notify overflow");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let cache_empty = server
            .cache_state
            .repository_response_freshness_cache
            .read()
            .expect("response freshness cache should not be poisoned")
            .is_empty();
        let dirty_root = validated_manifest_candidate_cache
            .read()
            .expect("validated manifest candidate cache should not be poisoned")
            .is_dirty_root(&workspace.root);
        if cache_empty && dirty_root {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "dropped notify should invalidate cached freshness and mark the root dirty"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let dirty = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::ManifestOnly,
        )
        .expect("dirty manifest freshness should recompute after dropped notify");
    assert_eq!(
        dirty.basis.get("cacheable").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        dirty
            .basis
            .pointer("/repositories/0/dirty_root")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        dirty
            .basis
            .pointer("/repositories/0/cacheable_reason")
            .and_then(Value::as_str),
        Some("dirty_root")
    );
    assert_eq!(
        dirty
            .basis
            .pointer("/repositories/0/using_live_walk")
            .and_then(Value::as_bool),
        Some(true)
    );

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn failed_index_completion_invalidation_recomputes_response_freshness() {
    let workspace_root = temp_workspace_root("failed-index-completion-recomputes-freshness");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Semantic;\n")
        .expect("failed to write source fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 25,
        retry_ms: 100,
        ..WatchConfig::default()
    };
    config.semantic_runtime = semantic_runtime_enabled_openai();

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

    let mut watch_config = config.clone();
    watch_config.semantic_runtime = SemanticRuntimeConfig::default();
    let runtime = Arc::new(
        maybe_start_watch_runtime(
            &watch_config,
            RuntimeTransportKind::Stdio,
            Arc::clone(&runtime_task_registry),
            Arc::clone(&validated_manifest_candidate_cache),
            None,
        )
        .expect("watch runtime should start")
        .expect("watch runtime should be enabled"),
    );
    server.set_watch_runtime(Some(Arc::clone(&runtime)));
    runtime
        .acquire_lease(&workspace)
        .expect("runtime should watch the workspace");

    let missing = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::SemanticAware,
        )
        .expect("semantic-aware freshness should compute");
    assert_eq!(
        missing
            .basis
            .pointer("/repositories/0/semantic")
            .and_then(Value::as_str),
        Some("missing_for_active_model")
    );
    assert_eq!(
        server
            .cache_state
            .repository_response_freshness_cache
            .read()
            .expect("response freshness cache should not be poisoned")
            .len(),
        1
    );

    Storage::new(&workspace.db_path)
        .replace_semantic_embeddings_for_repository(
            &workspace.runtime_repository_id,
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
            &[semantic_record(
                &workspace.runtime_repository_id,
                "snapshot-001",
                "src/lib.rs",
            )],
        )
        .expect("failed-after-write fixture should persist semantic rows");
    let callback = server.repository_cache_invalidation_callback();
    callback(&workspace.repository_id, None);

    let ready = server
        .repository_response_cache_freshness(
            std::slice::from_ref(&workspace),
            RepositoryResponseCacheFreshnessMode::SemanticAware,
        )
        .expect("semantic-aware freshness should recompute after failed completion invalidation");
    assert_eq!(
        ready
            .basis
            .pointer("/repositories/0/semantic")
            .and_then(Value::as_str),
        Some("ready"),
        "failed index completion invalidation must prevent stale cached semantic freshness"
    );

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    let _ = fs::remove_dir_all(workspace_root);
}
