#![allow(clippy::panic)]

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
