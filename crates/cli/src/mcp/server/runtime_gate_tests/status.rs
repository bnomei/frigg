#![allow(clippy::panic)]

use super::*;
use crate::mcp::types::{
    WorkspacePreciseCoverageMode, WorkspacePreciseIngestState, WorkspacePreciseState,
};

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
    assert!(instructions.contains("Use workspace_current for repository health"));
    assert!(instructions.contains("Prefer shell tools for cheap local reads"));
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
    let server = FriggMcpServer::new_with_runtime_options(config, false, false);
    assert!(server.attached_workspaces().is_empty());
    assert!(server.current_repository_id().is_none());

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
