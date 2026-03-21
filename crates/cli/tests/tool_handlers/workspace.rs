use super::*;

#[tokio::test]
async fn core_list_repositories_is_deterministic() {
    let server = server_for_fixture();

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
    assert!(first.repositories[0].health.is_some());
    assert_eq!(
        first.repositories[0]
            .health
            .as_ref()
            .map(|health| health.lexical.state),
        second.repositories[0]
            .health
            .as_ref()
            .map(|health| health.lexical.state)
    );
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
    assert_ne!(first.storage.index_state, WorkspaceStorageIndexState::Error);
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
    if first.precise.failure_tool.is_some() {
        assert!(
            first.precise.failure_summary.is_some() || first.precise.failure_class.is_some(),
            "precise failures should surface a summary or typed failure class"
        );
    }
    assert!(first.repository.storage.is_none());
    assert!(first.repository.health.is_some());
    let serialized: serde_json::Value =
        serde_json::to_value(&first).expect("workspace_attach response should serialize");
    assert!(serialized.get("storage").is_some());
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
        }))
        .await
        .expect("workspace_attach should reuse existing root")
        .0;
    assert_eq!(
        second.repository.repository_id,
        first.repository.repository_id
    );
    assert_eq!(second.action, WorkspaceAttachAction::ReusedWorkspace);

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
    assert!(current_repository.health.is_some());
    assert_eq!(current.repositories.len(), 1);
    assert_eq!(
        current.repositories[0].repository_id,
        first.repository.repository_id
    );
    assert!(current.precise.is_some());
    let runtime = current
        .runtime
        .as_ref()
        .expect("workspace_current should expose runtime status");
    assert_eq!(runtime.profile, RuntimeProfile::StdioEphemeral);
    assert!(!runtime.persistent_state_available);
    assert!(!runtime.watch_active);
    assert_eq!(runtime.status_tool, "workspace_current");
    assert!(
        runtime
            .recent_provenance
            .iter()
            .any(|event| event.tool_name == "workspace_attach"),
        "workspace_current should surface recent provenance from prior attach"
    );
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
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .map(|health| health.lexical.state),
        Some(WorkspaceIndexComponentState::Missing)
    );
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .and_then(|health| health.lexical.reason.as_deref()),
        Some("missing_manifest_snapshot")
    );
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .map(|health| health.semantic.state),
        Some(WorkspaceIndexComponentState::Missing)
    );
    assert_eq!(
        response
            .repository
            .health
            .as_ref()
            .and_then(|health| health.semantic.reason.as_deref()),
        Some("missing_manifest_snapshot")
    );
    let serialized: serde_json::Value =
        serde_json::to_value(&response).expect("workspace_attach response should serialize");
    assert!(
        serialized
            .pointer("/repository/health/lexical/artifact_count")
            .is_none(),
        "unknown lexical artifact counts should be omitted instead of serialized as null"
    );
    assert!(
        serialized
            .pointer("/repository/health/semantic/artifact_count")
            .is_none(),
        "unknown semantic artifact counts should be omitted instead of serialized as null"
    );

    fs::remove_dir_all(&workspace_root).expect("temporary workspace should clean up");
}

#[tokio::test]
async fn workspace_attach_reports_known_lexical_and_semantic_artifact_counts() {
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
        }))
        .await
        .expect("workspace_attach should succeed for indexed workspace")
        .0;

    let health = response
        .repository
        .health
        .as_ref()
        .expect("workspace_attach should include health");
    assert_eq!(health.lexical.state, WorkspaceIndexComponentState::Ready);
    assert_eq!(health.lexical.artifact_count, Some(2));
    assert_eq!(health.semantic.state, WorkspaceIndexComponentState::Ready);
    assert_eq!(health.semantic.artifact_count, Some(2));

    let serialized: serde_json::Value =
        serde_json::to_value(&response).expect("workspace_attach response should serialize");
    assert_eq!(
        serialized.pointer("/repository/health/lexical/artifact_count"),
        Some(&serde_json::Value::from(2))
    );
    assert_eq!(
        serialized.pointer("/repository/health/semantic/artifact_count"),
        Some(&serde_json::Value::from(2))
    );

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
async fn workspace_read_file_without_attached_repositories_returns_remediation() {
    let server = server_for_config(
        FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid"),
    );

    let error = match server
        .read_file(Parameters(ReadFileParams {
            path: "README.md".to_owned(),
            repository_id: None,
            max_bytes: None,
            line_start: None,
            line_end: None,
        }))
        .await
    {
        Ok(_) => panic!("read_file should fail without attached repositories"),
        Err(error) => error,
    };
    assert_eq!(error.code, ErrorCode::RESOURCE_NOT_FOUND);
    assert_eq!(error_code_tag(&error), Some("resource_not_found"));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("action"))
            .and_then(|value| value.as_str()),
        Some("workspace_attach")
    );
}

#[tokio::test]
async fn core_list_repositories_fails_with_typed_error_when_provenance_persistence_fails_by_default()
 {
    let workspace_root = temp_workspace_root("list-repositories-provenance-strict");
    fs::create_dir_all(&workspace_root).expect("failed to create temporary workspace root");
    fs::write(workspace_root.join(".frigg"), "blocked")
        .expect("failed to seed blocking provenance path fixture");
    let server = server_for_workspace_root(&workspace_root);

    let error = match server
        .list_repositories(Parameters(ListRepositoriesParams::default()))
        .await
    {
        Ok(_) => panic!("strict mode should fail when provenance persistence fails"),
        Err(error) => error,
    };

    assert_eq!(error.code, ErrorCode::INTERNAL_ERROR);
    assert_eq!(
        error_code_tag(&error),
        Some("provenance_persistence_failed")
    );
    assert_eq!(retryable_tag(&error), Some(false));
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("provenance_stage"))
            .and_then(|value| value.as_str()),
        Some("resolve_storage_path")
    );
    assert_eq!(
        error
            .data
            .as_ref()
            .and_then(|value| value.get("tool_name"))
            .and_then(|value| value.as_str()),
        Some("list_repositories")
    );

    cleanup_workspace_root(&workspace_root);
}
