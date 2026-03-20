#![allow(clippy::panic)]

use super::*;

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
