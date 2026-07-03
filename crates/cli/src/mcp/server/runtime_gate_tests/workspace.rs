#![allow(clippy::panic)]

//! Regression tests for workspace attach/prepare/index adoption and index lifecycle gates.

use super::*;
use crate::mcp::types::{
    WorkspaceAttachAction, WorkspaceAttachIndexMode, WorkspaceIndexAction,
    WorkspaceIndexLifecyclePhase, WorkspacePreciseGenerationAction,
    WorkspacePreciseGenerationStatus, WorkspacePreciseLifecyclePhase,
};

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
            wait_for_precise: None,
            index_mode: None,
            wait_for_index: None,
            index_timeout_ms: None,
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
            wait_for_precise: None,
            index_mode: None,
            wait_for_index: None,
            index_timeout_ms: None,
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

#[tokio::test]
async fn workspace_detach_prunes_ephemeral_known_workspace_after_last_session() {
    let workspace_root = temp_workspace_root("detach-prunes-ephemeral-workspace");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Ephemeral;\n")
        .expect("failed to write workspace root fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let attach_response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: None,
            index_mode: Some(WorkspaceAttachIndexMode::Skip),
            wait_for_index: None,
            index_timeout_ms: None,
        }))
        .await
        .expect("workspace_attach should adopt an ad hoc path")
        .0;
    assert_eq!(server.known_workspaces().len(), 1);

    let detach_response = server
        .workspace_detach(Parameters(WorkspaceDetachParams {
            repository_id: Some(attach_response.repository.repository_id.clone()),
        }))
        .await
        .expect("workspace_detach should detach the ad hoc repository")
        .0;

    assert_eq!(
        detach_response.repository_id,
        attach_response.repository.repository_id
    );
    assert!(detach_response.detached);
    assert!(server.attached_workspaces().is_empty());
    assert!(
        server.known_workspaces().is_empty(),
        "ad hoc repositories should be pruned from the process registry after the last session detaches"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn provisional_path_workspace_can_be_pruned_after_pre_adoption_failure() {
    let workspace_root = temp_workspace_root("pre-adoption-failure-prunes-workspace");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct Provisional;\n",
    )
    .expect("failed to write workspace root fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let (workspace, _, _, resolution_guard) = server
        .resolve_workspace_target(
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root path is utf-8"),
            ),
            None,
            WorkspaceResolveMode::Direct,
        )
        .expect("path resolution should create a provisional workspace");
    let resolution_guard = resolution_guard.expect("path resolution should hold a pending guard");
    assert_eq!(server.known_workspaces().len(), 1);

    server
        .runtime_state
        .workspace_registry
        .write()
        .expect("workspace registry should not be poisoned")
        .prune_inactive_ephemeral_workspace(&workspace.repository_id);
    assert_eq!(
        server.known_workspaces().len(),
        1,
        "pending path work should protect a provisional workspace from concurrent pruning"
    );

    drop(resolution_guard);

    assert!(
        server.known_workspaces().is_empty(),
        "dropping pending path work should remove provisional ad hoc workspaces"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn adopted_path_workspace_survives_pending_guard_drop() {
    let workspace_root = temp_workspace_root("adopted-path-survives-pending-drop");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Adopted;\n")
        .expect("failed to write workspace root fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let (workspace, _, _, resolution_guard) = server
        .resolve_workspace_target(
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root path is utf-8"),
            ),
            None,
            WorkspaceResolveMode::Direct,
        )
        .expect("path resolution should create a provisional workspace");
    let resolution_guard = resolution_guard.expect("path resolution should hold a pending guard");

    server
        .adopt_workspace(&workspace, true)
        .expect("adopting a provisional workspace should succeed");
    drop(resolution_guard);

    assert_eq!(server.known_workspaces().len(), 1);
    assert_eq!(server.attached_workspaces().len(), 1);

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_attach_path_rollback_guard_releases_fresh_adoption_on_cancellation() {
    let first_workspace_root = temp_workspace_root("attach-rollback-preserves-default");
    let second_workspace_root = temp_workspace_root("attach-rollback-prunes-fresh-path");
    for workspace_root in [&first_workspace_root, &second_workspace_root] {
        fs::create_dir_all(workspace_root.join("src"))
            .expect("failed to create workspace root fixture");
        fs::write(workspace_root.join("src/lib.rs"), "pub struct Rollback;\n")
            .expect("failed to write workspace root fixture");
    }

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let first_response = server
        .attach_workspace_internal(&first_workspace_root, true, WorkspaceResolveMode::Direct)
        .expect("first path attach should establish a session default");
    let first_repository_id = first_response.repository.repository_id.clone();
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(first_repository_id.as_str())
    );

    let second_path = second_workspace_root
        .to_str()
        .expect("workspace root path is utf-8");
    let mut second_outcome = server
        .attach_workspace_target_internal(
            Some(second_path),
            None,
            true,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("second path attach should create a fresh adoption");
    let rollback_guard = second_outcome
        .rollback_guard
        .take()
        .expect("fresh path attach should install a rollback guard");
    let second_response = second_outcome.response;
    let second_repository_id = second_response.repository.repository_id.clone();
    assert_eq!(second_response.action, WorkspaceAttachAction::AttachedFresh);
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(second_repository_id.as_str())
    );

    assert_eq!(server.attached_workspaces().len(), 2);
    assert_eq!(server.known_workspaces().len(), 2);

    drop(rollback_guard);

    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(first_repository_id.as_str()),
        "cancelled attach rollback should restore the previous session default"
    );
    assert!(
        !server
            .attached_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == second_repository_id),
        "cancelled fresh path attach should not remain adopted"
    );
    assert!(
        !server
            .known_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == second_repository_id),
        "cancelled fresh path attach should be pruned from the process registry"
    );

    let _ = fs::remove_dir_all(first_workspace_root);
    let _ = fs::remove_dir_all(second_workspace_root);
}

#[test]
fn workspace_attach_path_rollback_guard_preserves_later_completed_same_session_reuse() {
    let workspace_root = temp_workspace_root("attach-rollback-preserves-completed-reuse");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Reused;\n")
        .expect("failed to write workspace root fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    let workspace_path = workspace_root
        .to_str()
        .expect("workspace root path is utf-8");

    let mut first_outcome = server
        .attach_workspace_target_internal(
            Some(workspace_path),
            None,
            true,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("first path attach should create a fresh adoption");
    let rollback_guard = first_outcome
        .rollback_guard
        .take()
        .expect("fresh path attach should install a rollback guard");
    let first_response = first_outcome.response;
    let repository_id = first_response.repository.repository_id.clone();
    assert_eq!(first_response.action, WorkspaceAttachAction::AttachedFresh);

    let mut reused_outcome = server
        .attach_workspace_target_internal(
            Some(workspace_path),
            None,
            true,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("same-session path attach should reuse the adoption");
    assert_eq!(
        reused_outcome.response.action,
        WorkspaceAttachAction::ReusedWorkspace
    );
    let reused_guard = reused_outcome
        .rollback_guard
        .take()
        .expect("same-session path reuse should install an in-flight guard");

    drop(rollback_guard);

    assert!(
        server
            .attached_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == repository_id),
        "an in-flight same-session reuse should defer the original rollback"
    );

    reused_guard.disarm();

    assert!(
        server
            .attached_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == repository_id),
        "an older cancelled attach must not detach a later same-session reuse"
    );
    assert!(
        server
            .known_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == repository_id),
        "an older cancelled attach must not prune a reused workspace"
    );
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(repository_id.as_str())
    );

    server
        .detach_workspace(&repository_id)
        .expect("cleanup detach should succeed");
    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_attach_completed_reuse_without_default_restores_cancelled_default_change() {
    let first_workspace_root = temp_workspace_root("attach-rollback-default-first");
    let second_workspace_root = temp_workspace_root("attach-rollback-default-second");
    for workspace_root in [&first_workspace_root, &second_workspace_root] {
        fs::create_dir_all(workspace_root.join("src"))
            .expect("failed to create workspace root fixture");
        fs::write(workspace_root.join("src/lib.rs"), "pub struct Default;\n")
            .expect("failed to write workspace root fixture");
    }

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let first_response = server
        .attach_workspace_internal(&first_workspace_root, true, WorkspaceResolveMode::Direct)
        .expect("first path attach should establish a session default");
    let first_repository_id = first_response.repository.repository_id.clone();

    let second_path = second_workspace_root
        .to_str()
        .expect("workspace root path is utf-8");
    let mut fresh_outcome = server
        .attach_workspace_target_internal(
            Some(second_path),
            None,
            true,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("second path attach should create a fresh adoption");
    let fresh_guard = fresh_outcome
        .rollback_guard
        .take()
        .expect("fresh path attach should install a rollback guard");
    let second_repository_id = fresh_outcome.response.repository.repository_id.clone();
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(second_repository_id.as_str())
    );

    let mut reused_outcome = server
        .attach_workspace_target_internal(
            Some(second_path),
            None,
            false,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("same-session path attach should reuse the adoption");
    assert_eq!(
        reused_outcome.response.action,
        WorkspaceAttachAction::ReusedWorkspace
    );
    let reused_guard = reused_outcome
        .rollback_guard
        .take()
        .expect("same-session path reuse should install an in-flight guard");

    drop(fresh_guard);
    reused_guard.disarm();

    assert!(
        server
            .attached_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == second_repository_id),
        "completed reuse should preserve the reused adoption"
    );
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(first_repository_id.as_str()),
        "completed set_default=false reuse should not confirm a cancelled default change"
    );

    let _ = server.detach_workspace(&second_repository_id);
    let _ = server.detach_workspace(&first_repository_id);
    let _ = fs::remove_dir_all(first_workspace_root);
    let _ = fs::remove_dir_all(second_workspace_root);
}

#[test]
fn workspace_attach_completed_reuse_before_fresh_cancel_restores_cancelled_default_change() {
    let first_workspace_root = temp_workspace_root("attach-rollback-default-first-complete-first");
    let second_workspace_root =
        temp_workspace_root("attach-rollback-default-second-complete-first");
    for workspace_root in [&first_workspace_root, &second_workspace_root] {
        fs::create_dir_all(workspace_root.join("src"))
            .expect("failed to create workspace root fixture");
        fs::write(workspace_root.join("src/lib.rs"), "pub struct Default;\n")
            .expect("failed to write workspace root fixture");
    }

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let first_response = server
        .attach_workspace_internal(&first_workspace_root, true, WorkspaceResolveMode::Direct)
        .expect("first path attach should establish a session default");
    let first_repository_id = first_response.repository.repository_id.clone();

    let second_path = second_workspace_root
        .to_str()
        .expect("workspace root path is utf-8");
    let mut fresh_outcome = server
        .attach_workspace_target_internal(
            Some(second_path),
            None,
            true,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("second path attach should create a fresh adoption");
    let fresh_guard = fresh_outcome
        .rollback_guard
        .take()
        .expect("fresh path attach should install a rollback guard");
    let second_repository_id = fresh_outcome.response.repository.repository_id.clone();
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(second_repository_id.as_str())
    );

    let mut reused_outcome = server
        .attach_workspace_target_internal(
            Some(second_path),
            None,
            false,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("same-session path attach should reuse the adoption");
    let reused_guard = reused_outcome
        .rollback_guard
        .take()
        .expect("same-session path reuse should install an in-flight guard");

    reused_guard.disarm();
    drop(fresh_guard);

    assert!(
        server
            .attached_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == second_repository_id),
        "completed reuse should preserve the reused adoption"
    );
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(first_repository_id.as_str()),
        "set_default=false reuse completion before fresh cancellation should still restore the cancelled default change"
    );

    let _ = server.detach_workspace(&second_repository_id);
    let _ = server.detach_workspace(&first_repository_id);
    let _ = fs::remove_dir_all(first_workspace_root);
    let _ = fs::remove_dir_all(second_workspace_root);
}

#[test]
fn workspace_attach_path_rollback_guard_ignores_cancelled_same_session_reuse() {
    let workspace_root = temp_workspace_root("attach-rollback-ignores-cancelled-reuse");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Cancelled;\n")
        .expect("failed to write workspace root fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    let workspace_path = workspace_root
        .to_str()
        .expect("workspace root path is utf-8");

    let mut first_outcome = server
        .attach_workspace_target_internal(
            Some(workspace_path),
            None,
            true,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("first path attach should create a fresh adoption");
    let rollback_guard = first_outcome
        .rollback_guard
        .take()
        .expect("fresh path attach should install a rollback guard");
    let first_response = first_outcome.response;
    let repository_id = first_response.repository.repository_id.clone();
    assert_eq!(first_response.action, WorkspaceAttachAction::AttachedFresh);

    let mut cancelled_reuse = server
        .attach_workspace_target_internal(
            Some(workspace_path),
            None,
            true,
            WorkspaceResolveMode::Direct,
            WorkspaceAttachIndexMode::Skip,
        )
        .expect("same-session path attach should reuse the adoption");
    assert_eq!(
        cancelled_reuse.response.action,
        WorkspaceAttachAction::ReusedWorkspace
    );
    let cancelled_reuse_guard = cancelled_reuse
        .rollback_guard
        .take()
        .expect("same-session path reuse should install an in-flight guard");

    drop(rollback_guard);

    assert!(
        server
            .attached_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == repository_id),
        "an in-flight same-session reuse should defer the original rollback"
    );

    drop(cancelled_reuse_guard);

    assert!(
        !server
            .attached_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == repository_id),
        "a cancelled same-session reuse must not suppress the original rollback"
    );
    assert!(
        !server
            .known_workspaces()
            .iter()
            .any(|workspace| workspace.repository_id == repository_id),
        "a cancelled same-session reuse must not keep an ad hoc workspace in the registry"
    );
    assert!(server.current_repository_id().is_none());

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn workspace_attach_failed_watch_lease_rolls_back_and_prunes_ephemeral_workspace() {
    let workspace_root = temp_workspace_root("attach-watch-lease-failure-prunes-workspace");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Rollback;\n")
        .expect("failed to write workspace root fixture");

    let server_config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(server_config, false);
    let mut watch_config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty watch config should be valid");
    watch_config.watch = WatchConfig {
        mode: WatchMode::On,
        ..WatchConfig::default()
    };
    let watch_runtime = maybe_start_watch_runtime(
        &watch_config,
        RuntimeTransportKind::Stdio,
        Arc::new(RwLock::new(RuntimeTaskRegistry::new())),
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
        None,
    )
    .expect("watch runtime startup should succeed")
    .expect("watch runtime should be enabled");
    server.set_watch_runtime(Some(Arc::new(watch_runtime)));

    let (workspace, _, _, resolution_guard) = server
        .resolve_workspace_target(
            Some(
                workspace_root
                    .to_str()
                    .expect("workspace root path is utf-8"),
            ),
            None,
            WorkspaceResolveMode::Direct,
        )
        .expect("path resolution should create a provisional workspace");
    let resolution_guard = resolution_guard.expect("path resolution should hold a pending guard");
    assert_eq!(server.known_workspaces().len(), 1);

    fs::remove_dir_all(&workspace_root)
        .expect("removing workspace root should force watch lease acquisition to fail");
    let adoption_error = server
        .adopt_workspace(&workspace, true)
        .expect_err("adoption should fail when the watcher cannot lease the removed root");
    assert!(
        adoption_error
            .message
            .contains("failed to register watcher for root"),
        "unexpected adoption error: {}",
        adoption_error.message
    );
    assert!(server.attached_workspaces().is_empty());

    drop(resolution_guard);

    assert!(
        server.known_workspaces().is_empty(),
        "failed watch lease adoption should rollback session state and prune the ad hoc workspace"
    );
}

#[tokio::test]
async fn workspace_attach_wait_for_precise_reports_completed_lifecycle() {
    let workspace_root = temp_workspace_root("attach-wait-for-precise");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create python src fixture");
    fs::create_dir_all(workspace_root.join("node_modules/.bin"))
        .expect("failed to create local node bin directory");
    fs::write(
        workspace_root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("failed to write pyproject fixture");
    fs::write(
        workspace_root.join("src/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    let expected_project_name = FriggMcpServer::derived_python_precise_project_name(&workspace);
    let _local_scip_python = write_fake_precise_generator_script_with_body(
        &workspace_root.join("node_modules/.bin"),
        "scip-python",
        &format!(
            r#"#!/bin/sh
if [ "${{1:-}}" = "--version" ] || [ "${{1:-}}" = "version" ]; then
  printf '%s\n' "scip-python 0.6.6"
  exit 0
fi
if [ "${{1:-}}" = "index" ] && [ "${{2:-}}" = "--help" ]; then
  printf '%s\n' "usage: scip-python index"
  exit 0
fi
if [ "${{1:-}}" != "index" ] || [ "${{2:-}}" != "--quiet" ] || [ "${{3:-}}" != "--project-name" ] || [ "${{4:-}}" != "{expected_project_name}" ] || [ "${{5:-}}" != "--output" ] || [ -z "${{6:-}}" ] || [ -n "${{7:-}}" ]; then
  printf '%s\n' "unexpected python args: $*" >&2
  exit 81
fi
printf '%s' "local-python-scip" > "${{6}}"
"#
        ),
    );

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: Some(true),
            index_mode: None,
            wait_for_index: None,
            index_timeout_ms: None,
        }))
        .await
        .expect("workspace_attach should succeed")
        .0;

    assert!(response.precise_lifecycle.waited_for_completion);
    assert_eq!(
        response.precise_lifecycle.generation_action,
        WorkspacePreciseGenerationAction::Triggered
    );
    assert_eq!(
        response.precise_lifecycle.phase,
        WorkspacePreciseLifecyclePhase::Succeeded
    );
    let last_generation = response
        .precise_lifecycle
        .last_generation
        .as_ref()
        .expect("waited attach should return the latest precise generation summary");
    assert_eq!(
        last_generation.status,
        WorkspacePreciseGenerationStatus::Succeeded
    );
    assert!(last_generation.artifact_path.is_some());

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn workspace_attach_wait_for_precise_false_still_schedules_precise_generation() {
    let workspace_root = temp_workspace_root("attach-no-wait-precise-schedules");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create python src fixture");
    fs::create_dir_all(workspace_root.join("node_modules/.bin"))
        .expect("failed to create local node bin directory");
    fs::write(
        workspace_root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("failed to write pyproject fixture");
    fs::write(
        workspace_root.join("src/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    let expected_project_name = FriggMcpServer::derived_python_precise_project_name(&workspace);
    let _local_scip_python = write_fake_precise_generator_script_with_body(
        &workspace_root.join("node_modules/.bin"),
        "scip-python",
        &format!(
            r#"#!/bin/sh
if [ "${{1:-}}" = "--version" ] || [ "${{1:-}}" = "version" ]; then
  printf '%s\n' "scip-python 0.6.6"
  exit 0
fi
if [ "${{1:-}}" = "index" ] && [ "${{2:-}}" = "--help" ]; then
  printf '%s\n' "usage: scip-python index"
  exit 0
fi
if [ "${{1:-}}" != "index" ] || [ "${{2:-}}" != "--quiet" ] || [ "${{3:-}}" != "--project-name" ] || [ "${{4:-}}" != "{expected_project_name}" ] || [ "${{5:-}}" != "--output" ] || [ -z "${{6:-}}" ] || [ -n "${{7:-}}" ]; then
  printf '%s\n' "unexpected python args: $*" >&2
  exit 81
fi
printf '%s' "nonblocking-python-scip" > "${{6}}"
"#
        ),
    );

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: Some(false),
            index_mode: None,
            wait_for_index: None,
            index_timeout_ms: None,
        }))
        .await
        .expect("workspace_attach should succeed")
        .0;

    assert!(!response.precise_lifecycle.waited_for_completion);
    assert_eq!(
        response.precise_lifecycle.generation_action,
        WorkspacePreciseGenerationAction::Triggered
    );

    let expected_artifact = workspace_root.join(".frigg/scip/python.scip");
    for _ in 0..200 {
        let ready = fs::read(&expected_artifact)
            .map(|contents| contents == b"nonblocking-python-scip")
            .unwrap_or(false);
        if ready {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read(&expected_artifact).expect("non-blocking precise generation should publish"),
        b"nonblocking-python-scip"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn workspace_attach_index_skip_still_reports_precise_generation_side_effect() {
    let workspace_root = temp_workspace_root("attach-skip-precise-side-effect");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create python src fixture");
    fs::create_dir_all(workspace_root.join("node_modules/.bin"))
        .expect("failed to create local node bin directory");
    fs::write(
        workspace_root.join("pyproject.toml"),
        "[project]\nname = \"demo\"\n",
    )
    .expect("failed to write pyproject fixture");
    fs::write(
        workspace_root.join("src/app.py"),
        "def alpha():\n    return 1\n",
    )
    .expect("failed to write python source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("server should register workspace");
    let expected_project_name = FriggMcpServer::derived_python_precise_project_name(&workspace);
    let _local_scip_python = write_fake_precise_generator_script_with_body(
        &workspace_root.join("node_modules/.bin"),
        "scip-python",
        &format!(
            r#"#!/bin/sh
if [ "${{1:-}}" = "--version" ] || [ "${{1:-}}" = "version" ]; then
  printf '%s\n' "scip-python 0.6.6"
  exit 0
fi
if [ "${{1:-}}" = "index" ] && [ "${{2:-}}" = "--help" ]; then
  printf '%s\n' "usage: scip-python index"
  exit 0
fi
if [ "${{1:-}}" != "index" ] || [ "${{2:-}}" != "--quiet" ] || [ "${{3:-}}" != "--project-name" ] || [ "${{4:-}}" != "{expected_project_name}" ] || [ "${{5:-}}" != "--output" ] || [ -z "${{6:-}}" ] || [ -n "${{7:-}}" ]; then
  printf '%s\n' "unexpected python args: $*" >&2
  exit 81
fi
printf '%s' "skip-mode-python-scip" > "${{6}}"
"#
        ),
    );

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: Some(true),
            index_mode: Some(WorkspaceAttachIndexMode::Skip),
            wait_for_index: None,
            index_timeout_ms: None,
        }))
        .await
        .expect("workspace_attach should succeed")
        .0;

    assert_eq!(
        response.index_lifecycle.mode,
        WorkspaceAttachIndexMode::Skip
    );
    assert_eq!(
        response.index_lifecycle.action_taken,
        WorkspaceIndexAction::SkippedByRequest
    );
    assert_eq!(
        response.index_lifecycle.phase,
        WorkspaceIndexLifecyclePhase::Skipped
    );
    assert!(!response.index_lifecycle.waited_for_completion);
    assert_eq!(
        response.precise_lifecycle.generation_action,
        WorkspacePreciseGenerationAction::Triggered
    );
    assert!(response.precise_lifecycle.waited_for_completion);
    assert_eq!(
        response.precise_lifecycle.phase,
        WorkspacePreciseLifecyclePhase::Succeeded
    );
    let last_generation = response
        .precise_lifecycle
        .last_generation
        .as_ref()
        .expect("skip-mode attach should report completed precise generation when waiting");
    assert_eq!(
        last_generation.status,
        WorkspacePreciseGenerationStatus::Succeeded
    );
    assert!(
        last_generation.artifact_path.is_some(),
        "skip-mode attach should preserve precise generation artifact diagnostics"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_active_runtime_work_ignores_precise_generation_but_still_blocks_index() {
    let workspace_root = temp_workspace_root("index-allows-active-precise-generation");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct WarmPrecise;\n",
    )
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

    let task_id = server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .start_task(
            RuntimeTaskKind::PreciseGenerate,
            workspace.repository_id.clone(),
            "precise_generation",
            Some("background precise generation".to_owned()),
        );

    assert!(
        !server.repository_has_active_runtime_work(&workspace.repository_id),
        "background precise generation should not block workspace_prepare/workspace_index"
    );

    server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .finish_task(&task_id, RuntimeTaskStatus::Succeeded, None);

    let blocking_task_id = server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .start_task(
            RuntimeTaskKind::WorkspaceIndex,
            workspace.repository_id.clone(),
            "workspace_index",
            Some("active lexical index".to_owned()),
        );
    assert!(
        server.repository_has_active_runtime_work(&workspace.repository_id),
        "workspace_index should continue to block overlapping workspace writes"
    );
    server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .finish_task(&blocking_task_id, RuntimeTaskStatus::Succeeded, None);

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn repository_runtime_task_atomic_start_rejects_alias_conflict() {
    let workspace_root = temp_workspace_root("atomic-runtime-task-alias-conflict");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Atomic;\n")
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

    let rejected = match server.try_start_repository_runtime_task(
        &workspace,
        RuntimeTaskKind::WorkspacePrepare,
        "workspace_prepare",
        Some("prepare during active index".to_owned()),
    ) {
        Ok(_) => panic!("atomic start should reject active alias work"),
        Err(active_tasks) => active_tasks,
    };

    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].task_id, active_task_id);
    assert_eq!(
        server
            .runtime_state
            .runtime_task_registry
            .read()
            .expect("runtime task registry should not be poisoned")
            .active_tasks()
            .len(),
        1,
        "failed atomic start must not insert a second active task"
    );

    server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .finish_task(&active_task_id, RuntimeTaskStatus::Succeeded, None);
    let mut guard = server
        .try_start_repository_runtime_task(
            &workspace,
            RuntimeTaskKind::WorkspacePrepare,
            "workspace_prepare",
            Some("prepare after active index".to_owned()),
        )
        .expect("atomic start should succeed after alias conflict finishes");
    guard.finish(RuntimeTaskStatus::Succeeded, None);

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn read_file_rejects_non_adopted_repository_for_detached_session() {
    let workspace_root_a = temp_workspace_root("adoption-gate-repo-a");
    let workspace_root_b = temp_workspace_root("adoption-gate-repo-b");
    fs::create_dir_all(workspace_root_a.join("src")).expect("failed to create repo A fixture root");
    fs::create_dir_all(workspace_root_b.join("src")).expect("failed to create repo B fixture root");
    fs::write(workspace_root_a.join("src/lib.rs"), "pub struct A;\n")
        .expect("failed to write repo A source");
    fs::write(
        workspace_root_b.join("src/secret.rs"),
        "pub struct Secret;\n",
    )
    .expect("failed to write repo B secret source");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root_a.clone(), workspace_root_b.clone()])
            .expect("workspace roots must produce valid config"),
    );
    let canonical_a = workspace_root_a
        .canonicalize()
        .expect("repo A root should canonicalize");
    let canonical_b = workspace_root_b
        .canonicalize()
        .expect("repo B root should canonicalize");
    let workspaces = server.known_workspaces();
    let workspace_a = workspaces
        .iter()
        .find(|workspace| {
            workspace
                .root
                .canonicalize()
                .map(|root| root == canonical_a)
                .unwrap_or(false)
        })
        .cloned()
        .expect("repo A should be globally known at startup");
    let workspace_b = workspaces
        .iter()
        .find(|workspace| {
            workspace
                .root
                .canonicalize()
                .map(|root| root == canonical_b)
                .unwrap_or(false)
        })
        .cloned()
        .expect("repo B should be globally known at startup");

    let session = server.clone_for_new_session();
    session
        .adopt_workspace(&workspace_a, true)
        .expect("session should adopt repo A");

    let explicit = session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/secret.rs".to_owned(),
            repository_id: Some(workspace_b.repository_id.clone()),
            max_bytes: None,
            line_start: None,
            line_end: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await;
    assert!(
        explicit.is_err(),
        "detached session must not read a non-adopted repository by explicit repository_id"
    );

    let absolute_secret = workspace_root_b
        .join("src/secret.rs")
        .to_string_lossy()
        .into_owned();
    let absolute = session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: absolute_secret,
            repository_id: None,
            max_bytes: None,
            line_start: None,
            line_end: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await;
    assert!(
        absolute.is_err(),
        "detached session must not read a non-adopted repository by absolute path"
    );

    let absolute_a = workspace_root_a
        .join("src/lib.rs")
        .to_string_lossy()
        .into_owned();
    let allowed = session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: absolute_a,
            repository_id: None,
            max_bytes: None,
            line_start: None,
            line_end: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await
        .expect("adopted repo A must remain readable");
    assert!(allowed.content.contains("pub struct A"));

    let _ = fs::remove_dir_all(workspace_root_a);
    let _ = fs::remove_dir_all(workspace_root_b);
}
