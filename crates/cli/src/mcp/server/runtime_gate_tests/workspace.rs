#![allow(clippy::panic)]

use super::*;
use crate::mcp::types::{
    WorkspacePreciseGenerationAction, WorkspacePreciseGenerationStatus,
    WorkspacePreciseLifecyclePhase,
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

#[test]
fn repository_active_runtime_work_ignores_precise_generation_but_still_blocks_reindex() {
    let workspace_root = temp_workspace_root("reindex-allows-active-precise-generation");
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
        "background precise generation should not block workspace_prepare/workspace_reindex"
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
            RuntimeTaskKind::WorkspaceReindex,
            workspace.repository_id.clone(),
            "workspace_reindex",
            Some("active lexical reindex".to_owned()),
        );
    assert!(
        server.repository_has_active_runtime_work(&workspace.repository_id),
        "workspace_reindex should continue to block overlapping workspace writes"
    );
    server
        .runtime_state
        .runtime_task_registry
        .write()
        .expect("runtime task registry should not be poisoned")
        .finish_task(&blocking_task_id, RuntimeTaskStatus::Succeeded, None);

    let _ = fs::remove_dir_all(workspace_root);
}
