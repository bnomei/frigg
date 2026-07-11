#![allow(clippy::panic)]

//! Regression tests for workspace status and repository summary precise/index status fields.

use super::*;
use crate::agent_directive::FRIGG_FIRST_DIRECTIVE;
use crate::mcp::types::{
    WorkspacePreciseCoverageMode, WorkspacePreciseIngestState, WorkspacePreciseState,
    WorkspaceStorageIndexState,
};
use crate::storage::{Storage, VECTOR_TABLE_NAME};

#[test]
fn extended_only_tools_are_hidden_by_default_runtime_options() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false);
    let names = to_set(server.runtime_registered_tool_names());

    for tool_name in extended_only_tool_names() {
        assert!(
            !names.contains(&tool_name),
            "extended-only tool should not be registered by default: {tool_name}"
        );
    }
    assert!(
        names.contains("workspace"),
        "core tools should remain registered when extended-only tools are disabled"
    );
    assert!(
        names.contains("explore"),
        "explore is product tooling and must remain registered on core"
    );
}

#[test]
fn runtime_status_watch_status_mode_off_when_watch_disabled() {
    use crate::mcp::types::WatchStatusReason;

    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false);
    let status = server.runtime_status_summary();
    let watch_status = status
        .watch_status
        .expect("watch_status should always be present on runtime summary");
    assert_eq!(watch_status.reason, WatchStatusReason::ModeOff);
    assert_eq!(watch_status.lease_count, 0);
}

#[test]
fn watch_status_refreshing_matches_runtime_repository_id_alias() {
    use crate::mcp::types::{RuntimeTaskKind, RuntimeTaskStatus, WatchStatusReason};
    use crate::mcp::workspace_registry::AttachedWorkspace;
    use std::path::PathBuf;

    let runtime_task_registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
    let server = FriggMcpServer::new_with_runtime(
        fixture_config(),
        RuntimeProfile::StdioAttached,
        true,
        Arc::clone(&runtime_task_registry),
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
    );

    let workspace = AttachedWorkspace {
        repository_id: "myrepo-deadbeef".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: "myrepo".to_owned(),
        root: PathBuf::from("/tmp/frigg-watch-status-dual-id"),
        db_path: PathBuf::from("/tmp/frigg-watch-status-dual-id/.frigg/frigg.db"),
    };

    runtime_task_registry
        .write()
        .expect("task registry lock")
        .start_task(RuntimeTaskKind::ChangedIndex, "repo-001", "refresh", None);

    let active = server
        .runtime_state
        .runtime_task_registry
        .read()
        .expect("read tasks")
        .active_tasks();
    assert!(
        active
            .iter()
            .any(|task| task.status == RuntimeTaskStatus::Running
                && task.repository_id == "repo-001")
    );

    let status = server.watch_status_summary(Some(&workspace), &active);
    assert_eq!(
        status.reason,
        WatchStatusReason::Refreshing,
        "refresh tasks keyed by runtime_repository_id must count for the session workspace"
    );
    assert_eq!(status.repository_id.as_deref(), Some("myrepo-deadbeef"));
}

#[test]
fn watch_status_no_lease_when_watch_on_without_runtime_or_lease() {
    use crate::mcp::types::WatchStatusReason;
    use crate::mcp::workspace_registry::AttachedWorkspace;
    use std::path::PathBuf;

    let server = FriggMcpServer::new_with_runtime(
        fixture_config(),
        RuntimeProfile::StdioAttached,
        true,
        Arc::new(RwLock::new(RuntimeTaskRegistry::new())),
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
    );
    let workspace = AttachedWorkspace {
        repository_id: "stable-id".to_owned(),
        runtime_repository_id: "repo-009".to_owned(),
        display_name: "ws".to_owned(),
        root: PathBuf::from("/tmp/frigg-watch-status-no-lease"),
        db_path: PathBuf::from("/tmp/frigg-watch-status-no-lease/.frigg/frigg.db"),
    };
    let status = server.watch_status_summary(Some(&workspace), &[]);
    assert_eq!(status.reason, WatchStatusReason::NoLease);
    assert_eq!(status.lease_count, 0);
}

#[tokio::test]
async fn watch_status_debouncing_when_lease_and_dual_class_pending() {
    use crate::mcp::types::WatchStatusReason;
    use crate::mcp::workspace_registry::AttachedWorkspace;
    use crate::settings::{RuntimeTransportKind, WatchConfig, WatchMode};
    use crate::watch::{WatchRepositoryQueueSnapshot, maybe_start_watch_runtime};
    use std::path::PathBuf;

    let mut config = fixture_config();
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 25,
        retry_ms: 100,
        ..WatchConfig::default()
    };
    let runtime_task_registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
    let validated = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    let server = FriggMcpServer::new_with_runtime(
        config.clone(),
        RuntimeProfile::StdioAttached,
        true,
        Arc::clone(&runtime_task_registry),
        Arc::clone(&validated),
    );
    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        runtime_task_registry,
        validated,
        None,
    )
    .expect("watch runtime start")
    .expect("watch enabled");
    server.set_watch_runtime(Some(Arc::new(runtime)));

    let runtime = server
        .runtime_state
        .watch_runtime
        .read()
        .expect("watch lock")
        .as_ref()
        .expect("runtime")
        .clone();
    runtime.test_set_lease_count("repo-deb", 1);
    runtime.test_set_queue_snapshot(
        "repo-deb",
        WatchRepositoryQueueSnapshot {
            manifest_fast_pending: true,
            semantic_followup_pending: false,
            manifest_fast_in_flight: false,
            semantic_followup_in_flight: false,
            dirty_path_hint_count: 3,
            ..Default::default()
        },
    );

    let workspace = AttachedWorkspace {
        repository_id: "stable-deb".to_owned(),
        runtime_repository_id: "repo-deb".to_owned(),
        display_name: "ws".to_owned(),
        root: PathBuf::from("/tmp/frigg-watch-status-debouncing"),
        db_path: PathBuf::from("/tmp/frigg-watch-status-debouncing/.frigg/frigg.db"),
    };
    let status = server.watch_status_summary(Some(&workspace), &[]);
    assert_eq!(status.reason, WatchStatusReason::Debouncing);
    assert_eq!(status.lease_count, 1);
    assert_eq!(status.refresh_queue_depth, Some(1));
    assert_eq!(status.pending_dirty_path_count, Some(3));
}

#[test]
fn watch_status_includes_gate_dirty_path_count_when_pending() {
    use crate::mcp::types::WatchStatusReason;
    use crate::mcp::workspace_registry::AttachedWorkspace;
    use std::path::PathBuf;

    let server = FriggMcpServer::new_with_runtime(
        fixture_config(),
        RuntimeProfile::StdioAttached,
        true,
        Arc::new(RwLock::new(RuntimeTaskRegistry::new())),
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
    );
    let workspace = AttachedWorkspace {
        repository_id: "stable-dirty".to_owned(),
        runtime_repository_id: "repo-dirty".to_owned(),
        display_name: "ws".to_owned(),
        root: PathBuf::from("/tmp/frigg-watch-status-dirty"),
        db_path: PathBuf::from("/tmp/frigg-watch-status-dirty/.frigg/frigg.db"),
    };
    server.test_record_gate_dirty_paths(
        "stable-dirty",
        &["src/a.rs".to_owned(), "src/b.rs".to_owned()],
        &[],
    );
    let status = server.watch_status_summary(Some(&workspace), &[]);
    assert_eq!(status.reason, WatchStatusReason::NoLease);
    assert_eq!(status.pending_dirty_path_count, Some(2));
}

#[test]
fn runtime_status_tools_exposed_matches_filtered_router() {
    use crate::mcp::tool_surface::{ToolSurfaceProfile, manifest_for_tool_surface_profile};
    use crate::mcp::types::PUBLIC_TOOL_NAMES;

    let core = FriggMcpServer::new_with_runtime_options(fixture_config(), false);
    let extended = FriggMcpServer::new_with_runtime_options(fixture_config(), true);

    let core_status = core.runtime_status_summary();
    let extended_status = extended.runtime_status_summary();

    let mut core_registered = core.runtime_registered_tool_names();
    core_registered.sort();
    core_registered.dedup();
    assert_eq!(
        core_status.tools_exposed, core_registered,
        "tools_exposed must mirror the live filtered router (core)"
    );
    assert_eq!(
        core_status.tools_exposed,
        manifest_for_tool_surface_profile(ToolSurfaceProfile::Core).tool_names,
        "tools_exposed must match the core profile manifest"
    );
    assert_eq!(core_status.tool_surface_profile, "core");
    assert!(
        core_status.tools_exposed.contains(&"workspace".to_owned()),
        "core tools_exposed should include workspace"
    );
    assert!(
        core_status.tools_exposed.contains(&"explore".to_owned()),
        "explore is product core tooling (not extended-only)"
    );
    #[cfg(feature = "playbook")]
    {
        for playbook in ["playbook_start", "playbook_step", "playbook_finish"] {
            assert!(
                !core_status
                    .tools_exposed
                    .iter()
                    .any(|name| name == playbook),
                "core tools_exposed must omit playbook tool {playbook}"
            );
        }
    }

    for phantom in [
        "workspace_index",
        "workspace_attach",
        "workspace_detach",
        "workspace_prepare",
        "workspace_current",
        "workspace_reindex",
        "list_repositories",
        "deep_search",
    ] {
        assert!(
            !core_status.tools_exposed.iter().any(|name| name == phantom),
            "tools_exposed must not list non-public/phantom tool {phantom}"
        );
        assert!(
            !PUBLIC_TOOL_NAMES.contains(&phantom),
            "phantom sample {phantom} should remain outside PUBLIC_TOOL_NAMES"
        );
    }

    let mut extended_registered = extended.runtime_registered_tool_names();
    extended_registered.sort();
    extended_registered.dedup();
    assert_eq!(extended_status.tools_exposed, extended_registered);
    assert_eq!(
        extended_status.tools_exposed,
        manifest_for_tool_surface_profile(ToolSurfaceProfile::Extended).tool_names,
        "tools_exposed must match the extended profile manifest"
    );
    assert_eq!(extended_status.tool_surface_profile, "extended");
    assert!(
        extended_status
            .tools_exposed
            .contains(&"explore".to_owned()),
        "extended tools_exposed should include explore"
    );

    let value = serde_json::to_value(&core_status).expect("runtime status should serialize");
    assert!(
        value
            .get("tools_exposed")
            .and_then(|v| v.as_array())
            .is_some(),
        "tools_exposed must always be present in JSON (not skip_serializing_if empty)"
    );
}

#[test]
fn extended_only_tools_are_registered_when_runtime_option_enabled() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), true);
    let names = to_set(server.runtime_registered_tool_names());

    for tool_name in extended_only_tool_names() {
        assert!(
            names.contains(&tool_name),
            "extended-only tool should be registered when enabled: {tool_name}"
        );
    }
}

#[test]
fn workspace_maintenance_tools_require_explicit_confirm() {
    for tool_name in ["workspace_prepare", "workspace_index"] {
        for confirm in [None, Some(false)] {
            let error = FriggMcpServer::require_confirm(tool_name, confirm)
                .expect_err("confirm must be explicit before maintenance side effects");
            assert_eq!(
                error
                    .data
                    .as_ref()
                    .and_then(|value| value.get("error_code"))
                    .and_then(|value| value.as_str()),
                Some(crate::mcp::types::WRITE_CONFIRMATION_REQUIRED_ERROR_CODE)
            );
            assert_eq!(
                error
                    .data
                    .as_ref()
                    .and_then(|value| value.get("tool_name"))
                    .and_then(|value| value.as_str()),
                Some(tool_name)
            );
        }

        FriggMcpServer::require_confirm(tool_name, Some(true))
            .expect("confirm=true should allow maintenance tool execution");
    }
}

#[test]
fn server_info_enables_resources_and_prompts() {
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false);
    let info = <FriggMcpServer as rmcp::ServerHandler>::get_info(&server);

    assert!(info.capabilities.tools.is_some());
    assert!(info.capabilities.resources.is_some());
    assert!(info.capabilities.prompts.is_some());

    let instructions = info
        .instructions
        .expect("server info should publish MCP usage instructions");
    assert!(instructions.starts_with(FRIGG_FIRST_DIRECTIVE.trim()));
    assert!(instructions.contains("Omit repository_id in normal single-repo work"));
    assert!(instructions.contains("call workspace for compact status"));
    assert!(
        instructions.contains("Before using shell `rg`, `grep`, `find`, `fd`, `cat`, or `sed`")
    );
    assert!(instructions.contains("Shell tools are fallback only"));
    assert!(instructions.contains("restricted core tool surface"));
    assert!(instructions.contains("Set `FRIGG_MCP_TOOL_SURFACE_PROFILE=extended`"));
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
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    assert!(server.attached_workspaces().is_empty());
    assert!(server.current_repository_id().is_none());

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_semantic_index_summary_reports_error_when_storage_health_probe_fails() {
    let workspace_root = temp_workspace_root("semantic-health-probe-failure");
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

    let storage = FriggMcpServer::workspace_storage_summary(&workspace);
    assert_eq!(storage.index_state, WorkspaceStorageIndexState::Ready);

    let conn = rusqlite::Connection::open(&workspace.db_path)
        .expect("workspace storage db should open for corruption fixture");
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {VECTOR_TABLE_NAME}"))
        .expect("vector table drop should corrupt semantic health probe");
    drop(conn);

    let semantic = server.workspace_semantic_index_summary(&workspace, &storage);
    assert_eq!(semantic.state, WorkspaceIndexComponentState::Error);
    assert!(
        semantic
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("failed to count semantic vector rows")),
        "semantic health probe failure should surface storage error detail, got {:?}",
        semantic.reason
    );
    assert_eq!(semantic.snapshot_id.as_deref(), Some("snapshot-001"));

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
fn public_repository_summary_keeps_health_off_the_compact_path() {
    let workspace_root = temp_workspace_root("public-repository-summary-compact");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct CompactSummary;\n",
    )
    .expect("failed to write source fixture");

    let server = FriggMcpServer::new_with_runtime_options(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
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

    let compact = server.public_repository_summary(&workspace);
    assert!(
        compact.storage.is_some(),
        "compact repository summaries should retain storage state"
    );
    assert!(
        compact.health.is_none(),
        "compact repository summaries must not expose full index health"
    );

    let full = server.repository_summary(&workspace);
    assert!(
        full.health.is_some(),
        "full repository summaries should still compute index health"
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
        &workspace.runtime_repository_id,
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
    let server = FriggMcpServer::new_with_runtime_options(fixture_config(), false);

    let manifest_task_id = server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .start_task(
            RuntimeTaskKind::ChangedIndex,
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
            task.kind == RuntimeTaskKind::ChangedIndex
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
fn repository_summary_reports_precise_ingest_failures_separately_from_scip_discovery() {
    let workspace_root = temp_workspace_root("precise-ingest-failure-summary");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct PreciseFailure;\n",
    )
    .expect("failed to write source fixture");
    let scip_dir = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&scip_dir).expect("failed to create scip dir");
    fs::write(scip_dir.join("oversized.scip"), "0123456789")
        .expect("failed to write oversized scip artifact");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.max_file_bytes = 1;
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

    let summary = server.repository_summary(&workspace);
    let health = summary
        .health
        .as_ref()
        .expect("repository summary should expose health");
    assert_eq!(health.scip.state, WorkspaceIndexComponentState::Ready);
    let precise_ingest = health
        .precise_ingest
        .as_ref()
        .expect("repository health should expose precise ingest status");
    assert_eq!(precise_ingest.state, WorkspacePreciseIngestState::Failed);
    assert_eq!(
        precise_ingest.coverage_mode,
        Some(WorkspacePreciseCoverageMode::None)
    );
    assert_eq!(precise_ingest.artifacts_discovered, 1);
    assert_eq!(precise_ingest.artifacts_failed, 1);
    assert!(
        precise_ingest
            .reason
            .as_deref()
            .is_some_and(|reason| reason.contains("scip ingest failed"))
    );

    let precise = server.workspace_precise_summary_for_workspace(&workspace, None);
    assert_eq!(precise.state, WorkspacePreciseState::Failed);
    assert!(precise.failure_summary.is_some());

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_summary_full_scip_ingest_mode_accepts_artifacts_above_default_budget() {
    let workspace_root = temp_workspace_root("precise-ingest-full-scip-mode");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace src directory");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct User;\n\npub fn current_user() -> User { User }\n",
    )
    .expect("failed to write source fixture");
    let scip_dir = workspace_root.join(".frigg/scip");
    fs::create_dir_all(&scip_dir).expect("failed to create scip dir");
    fs::write(
        scip_dir.join("oversized.json"),
        r#"{"documents":[{"relative_path":"src/lib.rs","occurrences":[{"symbol":"scip-rust pkg repo#User","range":[0,11,15],"symbol_roles":1},{"symbol":"scip-rust pkg repo#User","range":[2,31,35],"symbol_roles":8}],"symbols":[{"symbol":"scip-rust pkg repo#User","display_name":"User","kind":"class"}]}]}"#,
    )
    .expect("failed to write scip artifact");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.max_file_bytes = 1;
    config.full_scip_ingest = true;
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

    let summary = server.repository_summary(&workspace);
    let health = summary
        .health
        .as_ref()
        .expect("repository summary should expose health");
    let precise_ingest = health
        .precise_ingest
        .as_ref()
        .expect("repository health should expose precise ingest status");
    assert_eq!(precise_ingest.state, WorkspacePreciseIngestState::Ready);
    assert_eq!(
        precise_ingest.coverage_mode,
        Some(WorkspacePreciseCoverageMode::Full)
    );
    assert_eq!(precise_ingest.artifacts_discovered, 1);
    assert_eq!(precise_ingest.artifacts_ingested, 1);
    assert_eq!(precise_ingest.artifacts_failed, 0);

    let precise = server.workspace_precise_summary_for_workspace(&workspace, None);
    assert_eq!(precise.state, WorkspacePreciseState::Ok);
    assert!(precise.failure_summary.is_none());

    let _ = fs::remove_dir_all(workspace_root);
}
