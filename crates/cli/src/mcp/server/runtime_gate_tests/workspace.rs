#![allow(clippy::panic)]

//! Regression tests for workspace attach/prepare/index adoption and index lifecycle gates.

use super::*;
use crate::mcp::types::{
    WorkspaceAttachIndexMode, WorkspaceIndexAction, WorkspaceIndexLifecyclePhase,
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
