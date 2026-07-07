#![allow(clippy::panic)]

//! Regression tests for workspace attach/prepare/index adoption and index lifecycle gates.

use super::*;
use crate::mcp::types::{
    ListRepositoriesParams, WorkspaceAttachAction, WorkspaceAttachIndexMode, WorkspaceIndexAction,
    WorkspaceIndexLifecyclePhase, WorkspacePreciseGenerationAction,
    WorkspacePreciseGenerationStatus, WorkspacePreciseLifecyclePhase,
};
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials};

#[tokio::test]
async fn list_files_auto_adopts_single_known_repository_without_repository_id() {
    let workspace_root = temp_workspace_root("list-files-auto-adopt-single-repo");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("README.md"), "# List Files\n")
        .expect("failed to write README fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Listed;\n")
        .expect("failed to write source fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("startup roots should register globally known workspaces");

    assert!(server.attached_workspaces().is_empty());

    let response = server
        .list_files(Parameters(ListFilesParams {
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            glob: None,
            language: None,
            path_class: None,
            include_hidden: None,
            limit: Some(10),
            resume_from: None,
        }))
        .await
        .expect("list_files should auto-adopt the only known repository")
        .0;

    assert_eq!(response.total_files, 1);
    assert!(!response.truncated);
    assert_eq!(response.files[0].repository_id, workspace.repository_id);
    assert_eq!(response.files[0].path, "src/lib.rs");
    assert_eq!(
        server.current_repository_id().as_deref(),
        Some(workspace.repository_id.as_str())
    );
    assert_eq!(server.attached_workspaces().len(), 1);

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn list_files_supports_rg_aligned_filters_and_resume_from() {
    let workspace_root = temp_workspace_root("list-files-rg-filters");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::create_dir_all(workspace_root.join("tests")).expect("failed to create tests fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Lib;\n")
        .expect("failed to write lib fixture");
    fs::write(workspace_root.join("src/main.rs"), "fn main() {}\n")
        .expect("failed to write main fixture");
    fs::write(
        workspace_root.join("src/.hidden.rs"),
        "pub struct Hidden;\n",
    )
    .expect("failed to write hidden fixture");
    fs::write(
        workspace_root.join("tests/lib_test.rs"),
        "#[test] fn test_it() {}\n",
    )
    .expect("failed to write test fixture");
    fs::write(workspace_root.join("README.md"), "# fixture\n")
        .expect("failed to write readme fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );

    let first_page = server
        .list_files(Parameters(ListFilesParams {
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            glob: Some("*.rs".to_owned()),
            language: Some("rust".to_owned()),
            path_class: Some(crate::mcp::types::SearchSymbolPathClass::Runtime),
            include_hidden: Some(false),
            limit: Some(1),
            resume_from: None,
        }))
        .await
        .expect("list_files should support rg-shaped filters")
        .0;

    assert_eq!(first_page.total_files, 2);
    assert!(first_page.truncated);
    assert_eq!(first_page.files.len(), 1);
    assert_eq!(first_page.files[0].path, "src/lib.rs");
    assert_eq!(first_page.resume_from.as_deref(), Some("1"));

    let second_page = server
        .list_files(Parameters(ListFilesParams {
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            glob: Some("*.rs".to_owned()),
            language: Some("rust".to_owned()),
            path_class: Some(crate::mcp::types::SearchSymbolPathClass::Runtime),
            include_hidden: Some(false),
            limit: Some(10),
            resume_from: first_page.resume_from,
        }))
        .await
        .expect("list_files should resume from returned cursor")
        .0;

    assert_eq!(second_page.total_files, 2);
    assert!(!second_page.truncated);
    assert_eq!(second_page.resume_from, None);
    assert_eq!(second_page.files.len(), 1);
    assert_eq!(second_page.files[0].path, "src/main.rs");

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn search_text_supports_rg_aligned_options() {
    let workspace_root = temp_workspace_root("search-text-rg-options");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "Alpha beta\nalpha_beta\nALPHA beta\n",
    )
    .expect("failed to write lib fixture");
    fs::write(workspace_root.join("src/other.rs"), "Alpha beta\n")
        .expect("failed to write other fixture");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("workspace root must produce valid config"),
    );
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("startup roots should register globally known workspaces");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");

    let files_with_matches = server
        .search_text_impl(crate::mcp::types::SearchTextParams {
            query: "alpha".to_owned(),
            pattern_type: None,
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(10),
            context_lines: None,
            case_sensitive: None,
            ignore_case: Some(true),
            word: Some(true),
            files_with_matches: Some(true),
            count_only: None,
            glob: Some("*.rs".to_owned()),
            exclude_glob: Some("*other.rs".to_owned()),
            include_hidden: Some(false),
            max_count_per_file: None,
            collapse_by_file: None,
            response_mode: None,
            include_context_efficiency: None,
        })
        .await
        .expect("search_text should support rg-shaped flags")
        .0;

    assert_eq!(files_with_matches.total_matches, 2);
    assert_eq!(files_with_matches.matches.len(), 1);
    assert_eq!(files_with_matches.matches[0].path, "src/lib.rs");
    assert_eq!(files_with_matches.matches[0].line, 1);
    assert!(files_with_matches.result_handle.is_some());

    let count_only = server
        .search_text_impl(crate::mcp::types::SearchTextParams {
            query: "alpha".to_owned(),
            pattern_type: None,
            repository_id: None,
            path_regex: Some("^src/".to_owned()),
            limit: Some(10),
            context_lines: None,
            case_sensitive: None,
            ignore_case: Some(true),
            word: Some(true),
            files_with_matches: None,
            count_only: Some(true),
            glob: Some("*.rs".to_owned()),
            exclude_glob: Some("*other.rs".to_owned()),
            include_hidden: Some(false),
            max_count_per_file: None,
            collapse_by_file: None,
            response_mode: None,
            include_context_efficiency: None,
        })
        .await
        .expect("search_text should support count_only")
        .0;

    assert_eq!(count_only.total_matches, 2);
    assert!(count_only.matches.is_empty());
    assert!(count_only.result_handle.is_none());

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn read_file_uses_start_end_and_line_count_names() {
    let workspace_root = temp_workspace_root("read-file-rg-line-names");
    fs::create_dir_all(workspace_root.join("src")).expect("failed to create src fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "line one\nline two\nline three\nline four\n",
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
        .expect("startup roots should register globally known workspaces");
    server
        .adopt_workspace(&workspace, true)
        .expect("server should adopt known workspace");

    let response = server
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: None,
            max_bytes: None,
            start_line: Some(2),
            end_line: None,
            line_count: Some(2),
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await
        .expect("read_file should accept line_count");

    assert_eq!(response.content, "line two\nline three");

    let error = server
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: None,
            max_bytes: None,
            start_line: Some(2),
            end_line: Some(3),
            line_count: Some(2),
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await
        .expect_err("end_line and line_count must be mutually exclusive");
    assert_eq!(
        error.message,
        "end_line and line_count are mutually exclusive"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

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
async fn workspace_detach_prunes_ephemeral_known_workspace_after_last_session() {
    let workspace_root = authorized_temp_workspace_root("detach-prunes-ephemeral-workspace");
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
fn workspace_attach_default_gitroot_rejects_non_git_parent() {
    let non_git_workspace_root =
        non_git_temp_workspace_root("attach-default-gitroot-non-git-parent");
    let child_repo = non_git_workspace_root.join("rust-mcp-mongodb-railway");
    fs::create_dir_all(child_repo.join(".git")).expect("child git marker should be creatable");
    fs::create_dir_all(child_repo.join("target/debug/.fingerprint"))
        .expect("child target fixture should be creatable");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let rejected = server
        .attach_workspace_internal(&non_git_workspace_root, true, WorkspaceResolveMode::GitRoot)
        .expect_err("default GitRoot attach must reject a non-git parent");
    assert!(
        rejected.message.contains("could not resolve a Git root"),
        "unexpected GitRoot attach rejection: {rejected:?}"
    );
    assert!(
        server.known_workspaces().is_empty(),
        "rejected GitRoot attach must not leave the non-git parent registered"
    );

    let workspace_root = authorized_temp_workspace_root("attach-direct-non-git-path");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("authorized direct workspace should be creatable");
    let direct = server
        .attach_workspace_internal(&workspace_root, true, WorkspaceResolveMode::Direct)
        .expect("explicit Direct attach should still support non-git scratch workspaces");
    assert_eq!(direct.resolution, WorkspaceResolveMode::Direct);
    assert_eq!(
        direct.repository.root_path,
        workspace_root
            .canonicalize()
            .expect("workspace root should canonicalize")
            .display()
            .to_string()
    );

    let _ = fs::remove_dir_all(non_git_workspace_root);
    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn provisional_path_workspace_can_be_pruned_after_pre_adoption_failure() {
    let workspace_root = authorized_temp_workspace_root("pre-adoption-failure-prunes-workspace");
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
    let workspace_root = authorized_temp_workspace_root("adopted-path-survives-pending-drop");
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
fn workspace_target_relative_path_falls_back_to_current_workspace_before_direct_resolution() {
    let workspace_root = authorized_temp_workspace_root("relative-path-current-workspace-root");
    let nested_dir = workspace_root.join("fallback-only/src");
    fs::create_dir_all(&nested_dir).expect("failed to create nested workspace fixture");
    fs::write(nested_dir.join("lib.rs"), "pub struct RelativeAttach;\n")
        .expect("failed to write nested workspace fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    let attach_response = server
        .attach_workspace_internal(&workspace_root, true, WorkspaceResolveMode::Direct)
        .expect("absolute path attach should establish a session default");

    let (workspace, resolved_from, resolution, _resolution_guard) = server
        .resolve_workspace_target(
            Some("fallback-only/src/lib.rs"),
            None,
            WorkspaceResolveMode::Direct,
        )
        .expect("relative path should resolve from the current workspace root");

    assert_ne!(
        workspace.repository_id, attach_response.repository.repository_id,
        "direct resolution should attach the resolved file parent as its own workspace"
    );
    assert_eq!(resolution, Some(WorkspaceResolveMode::Direct));
    let expected_resolved_from = nested_dir
        .canonicalize()
        .expect("nested directory should canonicalize")
        .display()
        .to_string();
    assert_eq!(
        resolved_from.as_deref(),
        Some(expected_resolved_from.as_str())
    );
    assert_eq!(
        workspace.root,
        nested_dir
            .canonicalize()
            .expect("nested directory should canonicalize")
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_attach_rejects_absolute_path_outside_authorized_roots() {
    let workspace_root = temp_workspace_root("attach-outside-authorized-roots");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create outside workspace fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Outside;\n")
        .expect("failed to write outside workspace fixture");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);

    let err = match server.resolve_workspace_target(
        Some(
            workspace_root
                .to_str()
                .expect("workspace root path is utf-8"),
        ),
        None,
        WorkspaceResolveMode::Direct,
    ) {
        Ok(_) => panic!("outside absolute attach path should be rejected"),
        Err(err) => err,
    };
    assert!(
        err.message.contains("outside authorized workspace roots"),
        "unexpected error: {err:?}"
    );
    assert!(
        server.known_workspaces().is_empty(),
        "rejected paths must not enter the process registry"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_attach_rejects_relative_parent_directory_components() {
    let workspace_root = authorized_temp_workspace_root("attach-relative-parent");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Root;\n")
        .expect("failed to write workspace source");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    server
        .attach_workspace_internal(&workspace_root, true, WorkspaceResolveMode::Direct)
        .expect("absolute path attach should establish a session default");

    let err = match server.resolve_workspace_target(
        Some("../outside"),
        None,
        WorkspaceResolveMode::Direct,
    ) {
        Ok(_) => panic!("relative parent traversal should be rejected"),
        Err(err) => err,
    };
    assert!(
        err.message.contains("parent directory components"),
        "unexpected error: {err:?}"
    );
    assert_eq!(
        server.known_workspaces().len(),
        1,
        "rejected relative paths must not create provisional workspaces"
    );

    let _ = fs::remove_dir_all(workspace_root);
}

#[tokio::test]
async fn ephemeral_workspace_is_hidden_from_other_sessions_by_repository_id() {
    let workspace_root = authorized_temp_workspace_root("cross-session-ephemeral-hidden");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(workspace_root.join("src/lib.rs"), "pub struct Hidden;\n")
        .expect("failed to write workspace source");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = FriggMcpServer::new_with_runtime_options(config, false);
    let first_session = server.clone_for_new_session();
    let second_session = server.clone_for_new_session();

    let attach = first_session
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: Some(false),
        }))
        .await
        .expect("first session should attach the ad hoc workspace")
        .0;
    let repository_id = attach.repository.repository_id.clone();

    let listed = second_session
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
        .expect("list_repositories should succeed")
        .0;
    assert!(
        listed
            .repositories
            .iter()
            .all(|repository| repository.repository_id != repository_id),
        "another session must not list a peer session's ad hoc repository"
    );

    let attach_by_id = second_session
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: None,
            repository_id: Some(repository_id.clone()),
            set_default: Some(true),
            resolve_mode: None,
            wait_for_precise: Some(false),
        }))
        .await;
    assert!(
        attach_by_id.is_err(),
        "another session must not adopt a peer ad hoc workspace by repository_id"
    );

    let read_by_id = second_session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/lib.rs".to_owned(),
            repository_id: Some(repository_id),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await;
    assert!(
        read_by_id.is_err(),
        "another session must not read a peer ad hoc workspace by repository_id"
    );
    assert!(second_session.attached_workspaces().is_empty());

    let _ = fs::remove_dir_all(workspace_root);
}

#[test]
fn workspace_attach_path_rollback_guard_releases_fresh_adoption_on_cancellation() {
    let first_workspace_root = authorized_temp_workspace_root("attach-rollback-preserves-default");
    let second_workspace_root = authorized_temp_workspace_root("attach-rollback-prunes-fresh-path");
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
            WorkspaceAttachIndexMode::Defer,
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
    let workspace_root =
        authorized_temp_workspace_root("attach-rollback-preserves-completed-reuse");
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
            WorkspaceAttachIndexMode::Defer,
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
            WorkspaceAttachIndexMode::Defer,
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
    let first_workspace_root = authorized_temp_workspace_root("attach-rollback-default-first");
    let second_workspace_root = authorized_temp_workspace_root("attach-rollback-default-second");
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
            WorkspaceAttachIndexMode::Defer,
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
            WorkspaceAttachIndexMode::Defer,
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
    let first_workspace_root =
        authorized_temp_workspace_root("attach-rollback-default-first-complete-first");
    let second_workspace_root =
        authorized_temp_workspace_root("attach-rollback-default-second-complete-first");
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
            WorkspaceAttachIndexMode::Defer,
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
            WorkspaceAttachIndexMode::Defer,
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
    let workspace_root = authorized_temp_workspace_root("attach-rollback-ignores-cancelled-reuse");
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
            WorkspaceAttachIndexMode::Defer,
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
            WorkspaceAttachIndexMode::Defer,
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
    let workspace_root =
        authorized_temp_workspace_root("attach-watch-lease-failure-prunes-workspace");
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

#[test]
fn workspace_index_lifecycle_summary_ready_with_failed_reports_failed_phase() {
    let workspace_root = temp_workspace_root("index-lifecycle-ready-with-failed");
    fs::create_dir_all(workspace_root.join("src"))
        .expect("failed to create workspace root fixture");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub struct ReadyWithFailed;\n",
    )
    .expect("failed to write workspace root fixture");

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("workspace root must produce valid config");
    config.semantic_runtime.enabled = false;

    let server = FriggMcpServer::new(config);
    let workspace = server
        .known_workspaces()
        .into_iter()
        .next()
        .expect("startup roots should register globally known workspaces");
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

    let failure_summary = Some("attach index refresh task failed".to_owned());
    let summary = server.workspace_index_lifecycle_summary(
        &workspace,
        WorkspaceAttachIndexMode::Ensure,
        true,
        false,
        WorkspaceIndexAction::Failed,
        failure_summary.clone(),
    );

    assert!(
        summary.lexical_ready && summary.semantic_ready,
        "counterexample requires concurrent readiness while attach refresh failed"
    );
    assert_eq!(summary.action_taken, WorkspaceIndexAction::Failed);
    assert_eq!(summary.failure_summary, failure_summary);
    assert_eq!(
        summary.phase,
        WorkspaceIndexLifecyclePhase::Failed,
        "attach failure must not be masked as Ready when index freshness is already true"
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
async fn read_file_requires_explicit_workspace_adoption_for_repository_id() {
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

    let absolute_secret = workspace_root_b
        .join("src/secret.rs")
        .to_string_lossy()
        .into_owned();
    let absolute = session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: absolute_secret,
            repository_id: None,
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await;
    assert!(
        absolute.is_err(),
        "session scoped to repo A must not read repo B by absolute path before repo B is explicitly adopted"
    );

    let explicit_before_adoption = session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/secret.rs".to_owned(),
            repository_id: Some(workspace_b.repository_id.clone()),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await;
    assert!(
        explicit_before_adoption.is_err(),
        "repository_id reads must not auto-adopt another visible repository"
    );
    assert_eq!(
        session.current_repository_id().as_deref(),
        Some(workspace_a.repository_id.as_str()),
        "failed repository_id reads must not replace the session default"
    );

    session
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: None,
            repository_id: Some(workspace_b.repository_id.clone()),
            set_default: Some(false),
            resolve_mode: None,
            wait_for_precise: Some(false),
        }))
        .await
        .expect("workspace_attach should explicitly adopt repo B without making it default");
    assert_eq!(
        session.current_repository_id().as_deref(),
        Some(workspace_a.repository_id.as_str()),
        "set_default=false adoption must preserve repo A as the session default"
    );

    let explicit = session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/secret.rs".to_owned(),
            repository_id: Some(workspace_b.repository_id.clone()),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await
        .expect("explicitly adopted repo B should be readable by repository_id");
    assert!(explicit.content.contains("pub struct Secret"));

    let absolute_a = workspace_root_a
        .join("src/lib.rs")
        .to_string_lossy()
        .into_owned();
    let allowed = session
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: absolute_a,
            repository_id: None,
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await
        .expect("adopted repo A must remain readable");
    assert!(allowed.content.contains("pub struct A"));

    let _ = fs::remove_dir_all(workspace_root_a);
    let _ = fs::remove_dir_all(workspace_root_b);
}

#[tokio::test]
async fn read_file_rejects_ambiguous_relative_path_across_adopted_repositories() {
    let workspace_root_a = temp_workspace_root("ambiguous-read-repo-a");
    let workspace_root_b = temp_workspace_root("ambiguous-read-repo-b");
    fs::create_dir_all(workspace_root_a.join("src")).expect("failed to create repo A fixture root");
    fs::create_dir_all(workspace_root_b.join("src")).expect("failed to create repo B fixture root");
    fs::write(workspace_root_a.join("src/shared.rs"), "pub struct A;\n")
        .expect("failed to write repo A source");
    fs::write(workspace_root_b.join("src/shared.rs"), "pub struct B;\n")
        .expect("failed to write repo B source");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root_a.clone(), workspace_root_b.clone()])
            .expect("workspace roots must produce valid config"),
    );
    let canonical_b = workspace_root_b
        .canonicalize()
        .expect("repo B root should canonicalize");
    let workspaces = server.known_workspaces();
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
    for workspace in &workspaces {
        server
            .adopt_workspace(workspace, false)
            .expect("workspace should adopt");
    }
    server.set_current_repository_id(None);

    let ambiguous = server
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/shared.rs".to_owned(),
            repository_id: None,
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await;
    assert!(
        ambiguous.is_err(),
        "relative reads that match multiple adopted repositories should require repository_id"
    );

    let explicit = server
        .read_file_impl(crate::mcp::types::ReadFileParams {
            path: "src/shared.rs".to_owned(),
            repository_id: Some(workspace_b.repository_id.clone()),
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(crate::mcp::types::ReadPresentationMode::Json),
            include_context_efficiency: None,
        })
        .await
        .expect("explicit repository_id should disambiguate");
    assert!(explicit.content.contains("pub struct B"));

    let _ = fs::remove_dir_all(workspace_root_a);
    let _ = fs::remove_dir_all(workspace_root_b);
}

#[test]
fn workspace_attach_rejects_ambiguous_relative_path_across_adopted_repositories() {
    let workspace_root_a = temp_workspace_root("ambiguous-attach-repo-a");
    let workspace_root_b = temp_workspace_root("ambiguous-attach-repo-b");
    fs::create_dir_all(workspace_root_a.join("src")).expect("failed to create repo A fixture root");
    fs::create_dir_all(workspace_root_b.join("src")).expect("failed to create repo B fixture root");
    fs::write(workspace_root_a.join("src/shared.rs"), "pub struct A;\n")
        .expect("failed to write repo A source");
    fs::write(workspace_root_b.join("src/shared.rs"), "pub struct B;\n")
        .expect("failed to write repo B source");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root_a.clone(), workspace_root_b.clone()])
            .expect("workspace roots must produce valid config"),
    );
    for workspace in server.known_workspaces() {
        server
            .adopt_workspace(&workspace, false)
            .expect("workspace should adopt");
    }
    server.set_current_repository_id(None);

    let err = match server.resolve_workspace_target(
        Some("src/shared.rs"),
        None,
        WorkspaceResolveMode::Direct,
    ) {
        Ok(_) => panic!(
            "relative attach paths that match multiple adopted repositories should require repository_id"
        ),
        Err(err) => err,
    };
    assert!(
        err.message
            .contains("relative path is ambiguous across adopted repositories; pass repository_id"),
        "unexpected error: {err:?}"
    );

    let _ = fs::remove_dir_all(workspace_root_a);
    let _ = fs::remove_dir_all(workspace_root_b);
}

#[test]
fn workspace_attach_ignores_nonexistent_relative_matches_across_adopted_repositories() {
    let workspace_root_a = temp_workspace_root("single-attach-repo-a");
    let workspace_root_b = temp_workspace_root("single-attach-repo-b");
    fs::create_dir_all(workspace_root_a.join("src")).expect("failed to create repo A fixture root");
    fs::create_dir_all(workspace_root_b.join("src")).expect("failed to create repo B fixture root");
    fs::write(workspace_root_b.join("src/only_b.rs"), "pub struct B;\n")
        .expect("failed to write repo B source");

    let server = FriggMcpServer::new(
        FriggConfig::from_workspace_roots(vec![workspace_root_a.clone(), workspace_root_b.clone()])
            .expect("workspace roots must produce valid config"),
    );
    for workspace in server.known_workspaces() {
        server
            .adopt_workspace(&workspace, false)
            .expect("workspace should adopt");
    }
    server.set_current_repository_id(None);

    let (_, resolved_from, resolution, _) = server
        .resolve_workspace_target(Some("src/only_b.rs"), None, WorkspaceResolveMode::Direct)
        .expect(
            "relative attach should resolve when only one adopted repository contains the path",
        );
    assert_eq!(resolution, Some(WorkspaceResolveMode::Direct));
    let expected_resolved_from = workspace_root_b
        .join("src")
        .canonicalize()
        .expect("repo B src should canonicalize")
        .display()
        .to_string();
    assert_eq!(
        resolved_from.as_deref(),
        Some(expected_resolved_from.as_str())
    );

    let _ = fs::remove_dir_all(workspace_root_a);
    let _ = fs::remove_dir_all(workspace_root_b);
}
