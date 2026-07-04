//! Integration tests for workspace MCP handlers (attach, prepare, current status, and repository listing).

use super::*;
use frigg::mcp::types::{
    WorkspaceAttachIndexMode, WorkspaceIndexLifecyclePhase, WorkspacePreciseLifecyclePhase,
};

#[tokio::test]
async fn core_list_repositories_is_deterministic() {
    let server = server_for_fixture().await;

    let first = server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed")
        .0;
    let second = server
        .list_repositories(Parameters(ListRepositoriesParams {}))
        .await
        .expect("list_repositories should succeed")
        .0;

    assert_eq!(first.repositories.len(), second.repositories.len());
    assert_eq!(first.repositories.len(), 1);
    assert_eq!(
        first.repositories[0].repository_id,
        stable_public_repository_id_for_root(Path::new(&first.repositories[0].root_path))
    );
    assert_eq!(
        first.repositories[0].repository_id,
        second.repositories[0].repository_id
    );
    assert_eq!(
        first.repositories[0].display_name,
        second.repositories[0].display_name
    );
    assert_eq!(
        first.repositories[0].root_path,
        second.repositories[0].root_path
    );
    assert!(first.repositories[0].storage.is_some());
    assert!(first.repositories[0].health.is_none());
    assert!(second.repositories[0].health.is_none());
}

#[tokio::test]
async fn workspace_auto_adopts_single_known_repository_and_returns_status() {
    let workspace_root = fresh_fixture_root("workspace-auto-adopts-single-known");
    let repository_id = stable_public_repository_id_for_root(&workspace_root);
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("fixture root must produce valid config");
    let server = FriggMcpServer::new(config);

    let first = server
        .workspace(Parameters(WorkspaceParams::default()))
        .await
        .expect("workspace should auto-adopt the only known repository")
        .0;
    let second = server
        .workspace(Parameters(WorkspaceParams::default()))
        .await
        .expect("workspace should return status when already adopted")
        .0;

    assert!(first.session_default);
    assert_eq!(
        first
            .repository
            .as_ref()
            .expect("workspace should expose current repository")
            .repository_id,
        repository_id
    );
    assert_eq!(first.repositories.len(), 1);
    assert_eq!(first.repositories[0].repository_id, repository_id);
    assert!(first.repositories[0].health.is_none());
    assert_eq!(
        first.repository.as_ref().map(|repo| &repo.repository_id),
        second.repository.as_ref().map(|repo| &repo.repository_id)
    );
    assert!(
        serde_json::to_value(&first)
            .expect("workspace response should serialize")
            .get("action")
            .is_none(),
        "workspace should not expose attach/reuse noise"
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn repository_scoped_tools_auto_adopt_explicit_known_repository() {
    let root_a = temp_workspace_root("workspace-auto-adopt-explicit-a");
    let root_b = temp_workspace_root("workspace-auto-adopt-explicit-b");
    fs::create_dir_all(root_a.join("src")).expect("repo a src dir should be creatable");
    fs::create_dir_all(root_b.join("src")).expect("repo b src dir should be creatable");
    fs::write(root_a.join("src/a.rs"), "pub fn a() {}\n").expect("repo a source should write");
    fs::write(root_b.join("src/b.rs"), "pub fn b() {}\n").expect("repo b source should write");
    let repo_a = stable_public_repository_id_for_root(&root_a);
    let repo_b = stable_public_repository_id_for_root(&root_b);
    let server = server_for_config(
        FriggConfig::from_workspace_roots(vec![root_a.clone(), root_b.clone()])
            .expect("workspace roots must produce valid config"),
    );

    server
        .workspace(Parameters(WorkspaceParams {
            path: None,
            repository_id: Some(repo_a.clone()),
            set_default: Some(true),
            resolve_mode: None,
        }))
        .await
        .expect("workspace should adopt repo a");
    let files = server
        .list_files(Parameters(ListFilesParams {
            repository_id: Some(repo_b.clone()),
            path_regex: Some("^src/".to_owned()),
            glob: None,
            language: None,
            path_class: None,
            include_hidden: None,
            limit: Some(10),
            resume_from: None,
        }))
        .await
        .expect("list_files should auto-adopt explicit repo b")
        .0;

    assert_eq!(files.files.len(), 1);
    assert_eq!(files.files[0].repository_id.as_str(), repo_b.as_str());
    let status = server
        .workspace(Parameters(WorkspaceParams::default()))
        .await
        .expect("workspace status should succeed")
        .0;
    assert_eq!(
        status
            .repository
            .as_ref()
            .map(|repo| repo.repository_id.as_str()),
        Some(repo_b.as_str())
    );
    assert_eq!(
        status
            .repositories
            .iter()
            .filter(|repo| repo.session.adopted)
            .count(),
        2
    );

    cleanup_workspace_root(&root_a);
    cleanup_workspace_root(&root_b);
}

#[tokio::test]
async fn workspace_attach_reuses_git_root_and_sets_session_default() {
    let workspace_root = fresh_fixture_root("tool-handlers-workspace-attach");
    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );
    let nested_path = workspace_root.join("src/lib.rs");

    let first = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(nested_path.display().to_string()),
            repository_id: None,
            set_default: None,
            resolve_mode: None,
            wait_for_precise: None,
        }))
        .await
        .expect("workspace_attach should succeed for fixture file path")
        .0;
    assert_eq!(
        first.repository.repository_id,
        stable_public_repository_id_for_root(&workspace_root)
    );
    assert_eq!(first.resolution, WorkspaceResolveMode::GitRoot);
    assert!(first.session_default);
    assert_eq!(first.action, WorkspaceAttachAction::AttachedFresh);
    assert_eq!(first.index_lifecycle.mode, WorkspaceAttachIndexMode::Ensure);
    assert_eq!(
        first.index_lifecycle.phase,
        WorkspaceIndexLifecyclePhase::Ready
    );
    assert!(first.index_lifecycle.waited_for_completion);
    assert!(first.index_lifecycle.lexical_ready);
    assert!(first.index_lifecycle.semantic_ready);
    assert!(matches!(
        first.precise.state,
        WorkspacePreciseState::Ok
            | WorkspacePreciseState::Unavailable
            | WorkspacePreciseState::Partial
            | WorkspacePreciseState::Failed
    ));
    assert!(
        first.precise.generation_action.is_some(),
        "workspace_attach should always expose a top-level precise generation action summary"
    );
    assert!(
        matches!(
            first.precise_lifecycle.phase,
            WorkspacePreciseLifecyclePhase::Running
                | WorkspacePreciseLifecyclePhase::Succeeded
                | WorkspacePreciseLifecyclePhase::Skipped
                | WorkspacePreciseLifecyclePhase::Unavailable
                | WorkspacePreciseLifecyclePhase::NotStarted
                | WorkspacePreciseLifecyclePhase::Failed
                | WorkspacePreciseLifecyclePhase::Timeout
        ),
        "workspace_attach should expose the precise lifecycle phase"
    );
    if first.precise.failure_tool.is_some() {
        assert!(
            first.precise.failure_summary.is_some() || first.precise.failure_class.is_some(),
            "precise failures should surface a summary or typed failure class"
        );
    }
    assert!(first.repository.storage.is_none());
    assert!(first.repository.health.is_none());
    let serialized: serde_json::Value =
        serde_json::to_value(&first).expect("workspace_attach response should serialize");
    assert!(serialized.get("storage").is_some());
    assert!(
        serialized.get("index_lifecycle").is_none(),
        "workspace_attach index_lifecycle is internal-only and intentionally omitted from the wire shape"
    );
    assert!(
        serialized
            .get("repository")
            .and_then(|value| value.get("storage"))
            .is_none(),
        "workspace_attach should keep storage only at the top level"
    );

    let second = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: Some(false),
            resolve_mode: None,
            wait_for_precise: None,
        }))
        .await
        .expect("workspace_attach should reuse existing root")
        .0;
    assert_eq!(
        second.repository.repository_id,
        first.repository.repository_id
    );
    assert_eq!(second.action, WorkspaceAttachAction::ReusedWorkspace);
    assert_eq!(
        second.index_lifecycle.phase,
        WorkspaceIndexLifecyclePhase::Ready
    );

    let current = server
        .workspace_current(Parameters(WorkspaceCurrentParams {}))
        .await
        .expect("workspace_current should succeed")
        .0;
    assert!(current.session_default);
    let current_repository = current
        .repository
        .as_ref()
        .expect("workspace_current should return attached repository");
    assert_eq!(
        current_repository.repository_id,
        first.repository.repository_id
    );
    assert!(current_repository.health.is_none());
    assert_eq!(current.repositories.len(), 1);
    assert_eq!(
        current.repositories[0].repository_id,
        first.repository.repository_id
    );
    assert!(current.precise.is_none());
    assert!(current.precise_ingest.is_none());
    let runtime = current
        .runtime
        .as_ref()
        .expect("workspace_current should expose runtime status");
    assert_eq!(runtime.profile, RuntimeProfile::StdioEphemeral);
    assert!(!runtime.persistent_state_available);
    assert!(!runtime.watch_active);
    assert_eq!(runtime.status_tool, "workspace");
}

#[test]
fn workspace_attach_accepts_natural_resolve_mode_aliases() {
    let git_alias: WorkspaceAttachParams = serde_json::from_value(serde_json::json!({
        "path": "/tmp/example",
        "resolve_mode": "git"
    }))
    .expect("git alias should deserialize");
    assert_eq!(git_alias.resolve_mode, Some(WorkspaceResolveMode::GitRoot));

    let directory_alias: WorkspaceAttachParams = serde_json::from_value(serde_json::json!({
        "path": "/tmp/example",
        "resolve_mode": "directory"
    }))
    .expect("directory alias should deserialize");
    assert_eq!(
        directory_alias.resolve_mode,
        Some(WorkspaceResolveMode::Direct)
    );
}

#[tokio::test]
async fn workspace_attach_ensures_missing_manifest_by_default() {
    let workspace_root = temp_workspace_root("workspace-attach-ensures-missing-manifest");
    fs::create_dir_all(workspace_root.join("src")).expect("workspace src dir should be creatable");
    fs::write(
        workspace_root.join("src/main.rs"),
        "fn main() { println!(\"indexed\"); }\n",
    )
    .expect("workspace source file should be writable");

    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: None,
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: Some(false),
        }))
        .await
        .expect("workspace_attach should index missing manifest by default")
        .0;

    assert_eq!(
        response.index_lifecycle.mode,
        WorkspaceAttachIndexMode::Ensure
    );
    assert_eq!(
        response.index_lifecycle.phase,
        WorkspaceIndexLifecyclePhase::Ready
    );
    assert!(response.index_lifecycle.waited_for_completion);
    assert!(response.index_lifecycle.lexical_ready);
    assert!(response.index_lifecycle.semantic_ready);
    assert!(
        !response.precise_lifecycle.waited_for_completion,
        "wait_for_precise=false should skip only the precise wait, not attach-time index ensure"
    );
    assert!(response.repository.health.is_none());

    fs::remove_dir_all(&workspace_root).expect("temporary workspace should clean up");
}

#[tokio::test]
async fn workspace_current_reports_default_repository_after_attach() {
    let workspace_root = temp_workspace_root("workspace-current-default-after-attach");
    fs::create_dir_all(workspace_root.join("src")).expect("workspace src dir should be creatable");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn unindexed() -> &'static str { \"fixture\" }\n",
    )
    .expect("workspace source file should be writable");

    let config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    let server = server_for_config(config);

    server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: None,
        }))
        .await
        .expect("workspace_attach should succeed");

    let current = server
        .workspace_current(Parameters(WorkspaceCurrentParams {}))
        .await
        .expect("workspace_current should succeed")
        .0;
    let repository = current
        .repository
        .as_ref()
        .expect("workspace_current should expose the default repository");
    assert_eq!(
        repository.repository_id,
        stable_public_repository_id_for_root(&workspace_root)
    );

    fs::remove_dir_all(&workspace_root).expect("temporary workspace should clean up");
}

#[tokio::test]
async fn workspace_attach_reports_schema_only_storage_as_uninitialized() {
    let workspace_root = temp_workspace_root("workspace-attach-schema-only-storage");
    fs::create_dir_all(workspace_root.join("src")).expect("workspace src dir should be creatable");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn attached_only() -> &'static str { \"fixture\" }\n",
    )
    .expect("workspace source file should be writable");

    let db_path = ensure_provenance_db_parent_dir(&workspace_root)
        .expect("workspace storage path should resolve");
    let storage = Storage::new(db_path);
    storage
        .initialize()
        .expect("schema-only workspace storage should initialize");

    let mut config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: None,
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: None,
        }))
        .await
        .expect("workspace_attach should succeed for schema-only storage")
        .0;

    assert_eq!(
        response.storage.index_state,
        WorkspaceStorageIndexState::Uninitialized
    );
    assert!(response.storage.exists);
    assert!(
        response.storage.initialized,
        "storage summary should still report that the schema exists even when no manifest snapshot has been indexed"
    );

    fs::remove_dir_all(&workspace_root).expect("temporary workspace should clean up");
}

#[tokio::test]
async fn workspace_attach_hides_repository_health_artifact_counts() {
    let workspace_root = temp_workspace_root("workspace-attach-artifact-counts");
    fs::create_dir_all(workspace_root.join("src")).expect("workspace src dir should be creatable");
    fs::write(
        workspace_root.join("src/main.rs"),
        "fn main() { println!(\"hello\"); }\n",
    )
    .expect("workspace source file should be writable");
    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn helper() -> &'static str { \"fixture\" }\n",
    )
    .expect("workspace source file should be writable");

    let repository_id = stable_public_repository_id_for_root(&workspace_root);
    seed_manifest_snapshot(
        &workspace_root,
        &repository_id,
        "snapshot-001",
        &["src/main.rs", "src/lib.rs"],
    );
    seed_semantic_embeddings(
        &workspace_root,
        &repository_id,
        "snapshot-001",
        &[
            semantic_record(
                &repository_id,
                "snapshot-001",
                "src/main.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                &repository_id,
                "snapshot-001",
                "src/lib.rs",
                0,
                vec![0.6, 0.0],
            ),
        ],
    );

    let mut config = FriggConfig::from_optional_workspace_roots(Vec::new())
        .expect("empty serving config should be valid");
    config.semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let server = server_for_config(config);

    let response = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(workspace_root.display().to_string()),
            repository_id: None,
            set_default: None,
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: None,
        }))
        .await
        .expect("workspace_attach should succeed for indexed workspace")
        .0;

    assert_eq!(
        response.index_lifecycle.phase,
        WorkspaceIndexLifecyclePhase::Ready
    );
    assert!(response.repository.health.is_none());

    let serialized: serde_json::Value =
        serde_json::to_value(&response).expect("workspace_attach response should serialize");
    assert!(serialized.pointer("/repository/health").is_none());

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn workspace_session_default_scopes_search_text_without_repository_hint() {
    let root_a = temp_workspace_root("workspace-default-a");
    let root_b = temp_workspace_root("workspace-default-b");
    fs::create_dir_all(root_a.join("src")).expect("workspace a src dir should be creatable");
    fs::create_dir_all(root_b.join("src")).expect("workspace b src dir should be creatable");
    fs::write(
        root_a.join("src/lib.rs"),
        "pub fn shared_marker() { /* repo_a */ }\n",
    )
    .expect("workspace a source should write");
    fs::write(
        root_b.join("src/lib.rs"),
        "pub fn shared_marker() { /* repo_b */ }\n",
    )
    .expect("workspace b source should write");

    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );

    let attached_a = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(root_a.display().to_string()),
            repository_id: None,
            set_default: Some(false),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: None,
        }))
        .await
        .expect("workspace_attach should attach repo a")
        .0;
    let attached_b = server
        .workspace_attach(Parameters(WorkspaceAttachParams {
            path: Some(root_b.display().to_string()),
            repository_id: None,
            set_default: Some(true),
            resolve_mode: Some(WorkspaceResolveMode::Direct),
            wait_for_precise: None,
        }))
        .await
        .expect("workspace_attach should attach repo b and set default")
        .0;

    let response = server
        .search_text(Parameters(SearchTextParams {
            query: "shared_marker".to_owned(),
            pattern_type: Some(SearchPatternType::Literal),
            repository_id: None,
            path_regex: None,
            limit: Some(10),
            ..Default::default()
        }))
        .await
        .expect("search_text should honor session default")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(
        response.matches[0].repository_id,
        attached_b.repository.repository_id
    );
    assert_ne!(
        response.matches[0].repository_id,
        attached_a.repository.repository_id
    );

    cleanup_workspace_root(&root_a);
    cleanup_workspace_root(&root_b);
}

#[tokio::test]
async fn workspace_read_file_without_attached_repositories_auto_adopts_current_directory() {
    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );

    let result = server
        .read_file(Parameters(ReadFileParams {
            path: "README.md".to_owned(),
            repository_id: None,
            max_bytes: None,
            start_line: None,
            end_line: None,
            line_count: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: None,
        }))
        .await
        .expect("read_file should auto-adopt the current repository");
    let content = result
        .structured_content
        .expect("JSON read_file should return structured content");
    assert_eq!(
        content.get("path").and_then(serde_json::Value::as_str),
        Some("README.md")
    );
    let workspace = server
        .workspace(Parameters(WorkspaceParams::default()))
        .await
        .expect("workspace status should succeed after auto-adopt")
        .0;
    assert!(workspace.session_default);
    assert!(workspace.repository.is_some());
}
