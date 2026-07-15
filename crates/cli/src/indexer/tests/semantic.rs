//! Regression tests for semantic chunking, embedding persistence across index, and runtime executor integration.

use crate::indexer::index_repository_with_runtime_config_and_dirty_paths_and_plan_callback;

use super::support::*;

#[test]
fn semantic_indexing_index_persists_deterministic_embeddings_when_enabled() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-enabled-roundtrip");
    let workspace_root = temp_workspace_root("semantic-enabled-roundtrip");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "pub fn main_api() { println!(\"main\"); }\n"),
            (
                "src/lib.rs",
                "pub struct User;\nimpl User { pub fn id(&self) -> u64 { 7 } }\n",
            ),
            ("README.md", "# Frigg\nsemantic runtime indexed\n"),
        ],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;
    let storage = Storage::new(&db_path);
    let first_semantic =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-001", &first.snapshot_id)?;
    assert!(
        !first_semantic.is_empty(),
        "expected semantic embeddings for supported source and markdown files"
    );
    assert!(
        first_semantic
            .iter()
            .all(|record| record.path.starts_with("src/") || record.path == "README.md"),
        "semantic indexing should use repository-relative canonical source paths"
    );
    assert!(
        first_semantic
            .iter()
            .any(|record| record.path == "README.md"),
        "README.md should participate in semantic indexing"
    );
    assert!(
        first_semantic
            .windows(2)
            .all(|window| window[0].path <= window[1].path),
        "semantic records should be deterministically ordered by path"
    );

    let second = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;
    let second_semantic = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?;
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert_eq!(first_semantic, second_semantic);

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_emits_batched_file_progress() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-file-progress");
    let workspace_root = temp_workspace_root("semantic-file-progress");
    let first_file = format!(
        "{}\n",
        "x".repeat(SEMANTIC_CHUNK_MAX_CHARS.saturating_sub(1))
    )
    .repeat(24);
    prepare_workspace(
        &workspace_root,
        &[
            ("src/first.rs", first_file.as_str()),
            ("src/second.rs", "pub fn second() {}\n"),
        ],
    )?;

    let events = Mutex::new(Vec::new());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    runtime.block_on(async {
        index_repository_with_semantic_executor_and_progress(
            "repo-001",
            &workspace_root,
            &db_path,
            IndexMode::Full,
            &semantic_runtime_enabled_openai(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
                openai_compat_api_key: None,
            },
            &FixtureSemanticEmbeddingExecutor,
            |event| {
                events
                    .lock()
                    .expect("semantic progress events lock should not be poisoned")
                    .push(event);
            },
        )
    })?;

    let progress = events
        .into_inner()
        .expect("semantic progress events lock should not be poisoned")
        .into_iter()
        .filter_map(|event| Some((event.semantic_files_completed?, event.semantic_files_total?)))
        .collect::<Vec<_>>();
    assert_eq!(progress, vec![(0, 2), (1, 2), (2, 2)]);

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_local_provider_persists_local_model_rows_and_projects_short_vectors()
-> FriggResult<()> {
    let db_path = temp_db_path("semantic-local-provider-short-vector");
    let workspace_root = temp_workspace_root("semantic-local-provider-short-vector");
    prepare_workspace(
        &workspace_root,
        &[("src/local.rs", "pub fn local_semantic_index() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_local();
    let summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &SemanticRuntimeCredentials::default(),
        &ShortSemanticEmbeddingExecutor,
    )?;

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
    assert!(!semantic_rows.is_empty());
    assert!(semantic_rows.iter().all(|record| {
        record.provider == "local"
            && record.model == "all-MiniLM-L6-v2"
            && record.embedding.len() == 2
    }));

    let health =
        storage.audit_semantic_embedding_partition("repo-001", "local", "all-MiniLM-L6-v2")?;
    assert!(health.vector_consistent);
    assert_eq!(
        health.covered_snapshot_id.as_deref(),
        Some(summary.snapshot_id.as_str())
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_local_model_alias_indexes_with_canonical_partition() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-local-model-alias");
    let workspace_root = temp_workspace_root("semantic-local-model-alias");
    prepare_workspace(
        &workspace_root,
        &[("src/alias.rs", "pub fn local_model_alias_index() {}\n")],
    )?;

    let semantic_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::Local),
        model: Some("AllMiniLML6V2".to_owned()),
        strict_mode: false,
        openai_compat_endpoint: None,
    };
    let summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &SemanticRuntimeCredentials::default(),
        &ShortSemanticEmbeddingExecutor,
    )?;

    assert_eq!(summary.semantic_model.as_deref(), Some("all-MiniLM-L6-v2"));

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
    assert!(!semantic_rows.is_empty());
    assert!(
        semantic_rows
            .iter()
            .all(|record| { record.provider == "local" && record.model == "all-MiniLM-L6-v2" })
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_full_index_skips_stale_deleted_absolute_paths_outside_workspace() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-full-stale-deleted-outside-root");
    let workspace_root = temp_workspace_root("semantic-full-stale-deleted-outside-root");
    prepare_workspace(
        &workspace_root,
        &[
            ("AGENTS.md", "# Workspace instructions\n"),
            ("src/lib.rs", "pub fn current_workspace() {}\n"),
        ],
    )?;

    let stale_root = temp_workspace_root("semantic-full-stale-old-root");
    let stale_agents_path = stale_root.join("AGENTS.md");
    assert!(
        !stale_agents_path.exists(),
        "stale manifest path must not exist: {}",
        stale_agents_path.display()
    );

    let manifest_store = ManifestStore::new(&db_path);
    manifest_store.initialize()?;
    manifest_store.persist_snapshot_manifest(
        "repo-001",
        "snapshot-stale-root",
        &[FileDigest {
            path: stale_agents_path,
            size_bytes: 21,
            mtime_ns: Some(1),
            hash_blake3_hex: "stale-agents-hash".to_owned(),
        }],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &FixtureSemanticEmbeddingExecutor,
    )?;

    assert_eq!(summary.deleted_paths, Vec::<String>::new());
    assert_eq!(
        summary.changed_paths,
        vec!["AGENTS.md".to_owned(), "src/lib.rs".to_owned()]
    );

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
    assert!(
        semantic_rows
            .iter()
            .any(|record| record.path == "AGENTS.md"),
        "current AGENTS.md should be indexed after skipping the stale deleted path"
    );
    assert!(
        semantic_rows
            .iter()
            .all(|record| !record.path.contains("semantic-full-stale-old-root")),
        "stale absolute paths must not leak into semantic records"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_changed_only_full_rebuilds_when_deleted_path_cannot_be_mapped() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-changed-stale-deleted-outside-root");
    let old_workspace_root = temp_workspace_root("semantic-changed-stale-old-root");
    prepare_workspace(
        &old_workspace_root,
        &[("legacy.rs", "pub fn legacy_workspace() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let old_summary = index_repository_with_semantic_executor(
        "repo-001",
        &old_workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &FixtureSemanticEmbeddingExecutor,
    )?;
    let storage = Storage::new(&db_path);
    let old_semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &old_summary.snapshot_id)?;
    assert!(
        old_semantic_rows
            .iter()
            .any(|record| record.path == "legacy.rs"),
        "old semantic snapshot should contain the legacy path before simulating a moved workspace"
    );
    cleanup_workspace(&old_workspace_root);

    let new_workspace_root = temp_workspace_root("semantic-changed-stale-new-root");
    prepare_workspace(
        &new_workspace_root,
        &[("src/lib.rs", "pub fn current_workspace() {}\n")],
    )?;

    let plan = build_index_plan_for_tests(
        "repo-001",
        &new_workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &[],
    )?;
    assert_eq!(
        plan.semantic_refresh.mode,
        SemanticRefreshMode::FullRebuildFromChangedOnly
    );
    assert!(plan.semantic_refresh.deleted_paths.is_empty());

    let new_summary = index_repository_with_semantic_executor(
        "repo-001",
        &new_workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &credentials,
        &FixtureSemanticEmbeddingExecutor,
    )?;

    assert_eq!(new_summary.files_deleted, 1);
    assert!(new_summary.deleted_paths.is_empty());
    let new_semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &new_summary.snapshot_id)?;
    assert!(
        new_semantic_rows
            .iter()
            .any(|record| record.path == "src/lib.rs"),
        "new semantic snapshot should contain the current workspace path"
    );
    assert!(
        new_semantic_rows
            .iter()
            .all(|record| record.path != "legacy.rs"),
        "unmappable stale deleted paths should force a full rebuild instead of carrying old semantic rows forward"
    );

    cleanup_workspace(&new_workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_enabled_succeeds_inside_existing_tokio_runtime() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-enabled-inside-runtime");
    let workspace_root = temp_workspace_root("semantic-enabled-inside-runtime");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn inside_runtime() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    let summary = runtime.block_on(async {
        index_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            IndexMode::Full,
            &semantic_runtime,
            &credentials,
            &executor,
        )
    })?;

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
    assert!(
        !semantic_rows.is_empty(),
        "expected semantic embeddings when index runs inside a tokio runtime"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_disabled_preserves_index_behavior() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-disabled-preserves");
    let workspace_root = temp_workspace_root("semantic-disabled-preserves");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
    )?;

    let summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
        &RuntimeSemanticEmbeddingExecutor::with_endpoint(
            SemanticRuntimeCredentials::default(),
            None,
        ),
    )?;
    assert_eq!(summary.files_scanned, 2);
    assert_eq!(summary.files_changed, 2);
    assert_eq!(summary.files_deleted, 0);

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
    assert!(semantic_rows.is_empty());

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_validation_failure_keeps_existing_semantic_state() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-invalid-does-not-mutate");
    let workspace_root = temp_workspace_root("semantic-invalid-does-not-mutate");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "pub fn stable() {}\n"),
            ("README.md", "hello\n"),
        ],
    )?;

    let executor = FixtureSemanticEmbeddingExecutor;
    let valid_runtime = semantic_runtime_enabled_openai();
    let valid_credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let valid_summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &valid_runtime,
        &valid_credentials,
        &executor,
    )?;

    let storage = Storage::new(&db_path);
    let before = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &valid_summary.snapshot_id)?;
    assert!(
        !before.is_empty(),
        "expected seeded semantic records before invalid index attempt"
    );

    let invalid_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
        openai_compat_endpoint: None,
    };
    let invalid_credentials = SemanticRuntimeCredentials::default();
    let error = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &invalid_runtime,
        &invalid_credentials,
        &executor,
    )
    .expect_err("missing provider credentials should fail semantic indexing");
    assert!(
        matches!(error, FriggError::InvalidInput(_)),
        "expected invalid input from semantic startup validation, got {error}"
    );
    assert!(
        error
            .to_string()
            .contains("semantic runtime validation failed code=invalid_params"),
        "unexpected semantic validation error: {error}"
    );

    let after = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &valid_summary.snapshot_id)?;
    assert_eq!(before, after);

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_failure_rolls_back_new_manifest_snapshot() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-failure-rolls-back-manifest");
    let workspace_root = temp_workspace_root("semantic-failure-rolls-back-manifest");
    prepare_workspace(&workspace_root, &[("src/main.rs", "pub fn stable() {}\n")])?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    fs::write(
        workspace_root.join("src/main.rs"),
        "pub fn changed_after_failure() {}\n",
    )
    .map_err(FriggError::Io)?;

    let plan = build_index_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &[],
    )?;
    assert_eq!(
        plan.previous_snapshot_id.as_deref(),
        Some(first.snapshot_id.as_str())
    );
    assert!(matches!(
        &plan.snapshot_plan,
        super::super::ManifestSnapshotPlan::PersistNew {
            rollback_on_semantic_failure: true,
            ..
        }
    ));
    assert_eq!(plan.semantic_refresh.mode, SemanticRefreshMode::FullRebuild);
    assert_eq!(plan.semantic_refresh.records_manifest.len(), 1);

    let error = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &FailingSemanticEmbeddingExecutor,
    )
    .expect_err("failing semantic executor should abort index");
    assert!(
        error
            .to_string()
            .contains("semantic embedding batch failed batch_index=0 total_batches=1"),
        "unexpected semantic failure: {error}"
    );
    assert!(
        error.to_string().contains("phase=semantic_refresh"),
        "semantic execution failures should surface the planned phase: {error}"
    );

    let storage = Storage::new(&db_path);
    let latest = storage
        .load_latest_manifest_for_repository("repo-001")?
        .expect("expected previous manifest snapshot to remain active");
    assert_eq!(
        latest.snapshot_id, first.snapshot_id,
        "failed semantic index must not advance the latest manifest snapshot"
    );
    let semantic_rows =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-001", &first.snapshot_id)?;
    assert!(
        !semantic_rows.is_empty(),
        "previous semantic rows should remain intact after rollback"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_changed_only_updates_only_changed_paths() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-changed-only-updates");
    let workspace_root = temp_workspace_root("semantic-changed-only-updates");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "pub fn main_api() { println!(\"main\"); }\n"),
            ("src/lib.rs", "pub fn stable_lib() -> u64 { 7 }\n"),
        ],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let first_executor = CountingSemanticEmbeddingExecutor::default();
    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &first_executor,
    )?;
    let first_inputs = first_executor.observed_inputs();
    assert!(!first.snapshot_id.is_empty());
    assert_eq!(first_inputs.len(), 2);

    fs::write(
        workspace_root.join("src/lib.rs"),
        "pub fn changed_lib() -> u64 { 9 }\n",
    )
    .map_err(FriggError::Io)?;

    let second_executor = CountingSemanticEmbeddingExecutor::default();
    let second = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &credentials,
        &second_executor,
    )?;
    let second_inputs = second_executor.observed_inputs();
    assert_eq!(second.files_changed, 1);
    assert!(second.snapshot_id != first.snapshot_id);
    assert_eq!(second_inputs.len(), 1);
    assert!(
        second_inputs[0].contains("changed_lib"),
        "changed-only semantic indexing should embed only modified path chunks"
    );

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?;
    assert!(
        semantic_rows.len() >= 2,
        "expected unchanged and changed semantic rows in the advanced snapshot"
    );
    assert!(
        semantic_rows.iter().any(|record| {
            record.path == "src/main.rs" && record.content_text.contains("main_api")
        }),
        "unchanged semantic rows should advance into the new snapshot"
    );
    assert!(
        semantic_rows.iter().any(|record| {
            record.path == "src/lib.rs" && record.content_text.contains("changed_lib")
        }),
        "changed semantic rows should be replaced in the new snapshot"
    );
    assert!(
        semantic_rows.iter().all(|record| {
            !(record.path == "src/lib.rs" && record.content_text.contains("stable_lib"))
        }),
        "stale semantic chunks for modified paths should be removed from the advanced snapshot"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_full_rebuild_reuses_existing_chunk_embeddings() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-full-reuses-existing-chunks");
    let workspace_root = temp_workspace_root("semantic-full-reuses-existing-chunks");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn reusable_chunk() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let first_executor = CountingSemanticEmbeddingExecutor::default();
    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &first_executor,
    )?;
    assert_eq!(first_executor.observed_inputs().len(), 1);

    let second_executor = CountingSemanticEmbeddingExecutor::default();
    let second = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &second_executor,
    )?;
    assert_eq!(second.snapshot_id, first.snapshot_id);
    assert!(
        second_executor.observed_inputs().is_empty(),
        "unchanged full rebuild should reuse existing chunk embeddings"
    );

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?;
    assert_eq!(semantic_rows.len(), 1);
    assert_eq!(semantic_rows[0].content_text, "pub fn reusable_chunk() {}");

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_changed_only_reuses_unchanged_chunks_inside_changed_file() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-changed-only-reuses-file-chunks");
    let workspace_root = temp_workspace_root("semantic-changed-only-reuses-file-chunks");
    let stable_chunk = "a".repeat(SEMANTIC_CHUNK_MAX_CHARS);
    let original_tail = "b".repeat(64);
    let changed_tail = "c".repeat(65);
    prepare_workspace(
        &workspace_root,
        &[(
            "src/main.rs",
            format!("{stable_chunk}{original_tail}").as_str(),
        )],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let first_executor = CountingSemanticEmbeddingExecutor::default();
    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &first_executor,
    )?;
    assert_eq!(
        first_executor.observed_inputs(),
        vec![
            format!("path: src/main.rs\nlanguage: rust\n\n{stable_chunk}"),
            format!("path: src/main.rs\nlanguage: rust\n\n{original_tail}"),
        ],
        "embed batch uses path+language envelope; stored body remains pure source"
    );

    fs::write(
        workspace_root.join("src/main.rs"),
        format!("{stable_chunk}{changed_tail}"),
    )
    .map_err(FriggError::Io)?;
    let second_executor = CountingSemanticEmbeddingExecutor::default();
    let second = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &credentials,
        &second_executor,
    )?;
    assert_ne!(second.snapshot_id, first.snapshot_id);
    assert_eq!(
        second_executor.observed_inputs(),
        vec![format!(
            "path: src/main.rs\nlanguage: rust\n\n{changed_tail}"
        )],
        "only the modified tail chunk should be embedded (with envelope)"
    );

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?;
    assert_eq!(semantic_rows.len(), 2);
    assert!(
        semantic_rows
            .iter()
            .any(|record| record.content_text == stable_chunk),
        "unchanged first chunk should advance into the new snapshot"
    );
    assert!(
        semantic_rows
            .iter()
            .any(|record| record.content_text == changed_tail),
        "changed tail chunk should be embedded into the new snapshot"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_changed_only_deletes_rows_for_changed_file_that_fails_semantic_read() -> FriggResult<()>
{
    let db_path = temp_db_path("semantic-changed-only-unreadable-deletes");
    let workspace_root = temp_workspace_root("semantic-changed-only-unreadable-deletes");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "pub fn main_api() { println!(\"main\"); }\n"),
            ("src/lib.rs", "pub fn stable_lib() -> u64 { 7 }\n"),
        ],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };

    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &CountingSemanticEmbeddingExecutor::default(),
    )?;
    assert!(!first.snapshot_id.is_empty());

    fs::write(
        workspace_root.join("src/lib.rs"),
        b"pub fn changed() {} // \xff\xfe invalid utf8 \xff".as_slice(),
    )
    .map_err(FriggError::Io)?;

    let second = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &credentials,
        &CountingSemanticEmbeddingExecutor::default(),
    )?;
    assert_eq!(
        second.files_changed, 1,
        "the invalid-utf8 rewrite should be detected as a manifest change"
    );

    let storage = Storage::new(&db_path);
    let semantic_rows = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?;
    assert!(
        semantic_rows.iter().all(
            |record| record.path != "src/lib.rs" && !record.content_text.contains("stable_lib")
        ),
        "existing semantic rows for a changed file that fails to read must be deleted rather than retained as stale evidence"
    );
    assert!(
        semantic_rows
            .iter()
            .any(|record| record.path == "src/main.rs"),
        "unrelated semantic rows must remain intact"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_repeated_changed_only_cycles_keep_live_corpus_bounded() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-changed-only-bounded");
    let workspace_root = temp_workspace_root("semantic-changed-only-bounded");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "pub fn main_api() { println!(\"main\"); }\n"),
            ("src/lib.rs", "pub fn changed_lib_0() -> u64 { 0 }\n"),
        ],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };

    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &FixtureSemanticEmbeddingExecutor,
    )?;
    let storage = Storage::new(&db_path);
    let baseline_health = storage.audit_semantic_embedding_partition(
        "repo-001",
        "openai",
        "text-embedding-3-small",
    )?;
    assert!(baseline_health.vector_consistent);
    assert_eq!(
        baseline_health.covered_snapshot_id.as_deref(),
        Some(first.snapshot_id.as_str())
    );

    for idx in 1usize..=12 {
        fs::write(
            workspace_root.join("src/lib.rs"),
            format!("pub fn changed_lib_{idx}() -> u64 {{ {idx} }}\n"),
        )
        .map_err(FriggError::Io)?;

        let executor = CountingSemanticEmbeddingExecutor::default();
        let summary = index_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            IndexMode::ChangedOnly,
            &semantic_runtime,
            &credentials,
            &executor,
        )?;
        let observed_inputs = executor.observed_inputs();
        assert_eq!(observed_inputs.len(), 1);
        assert!(
            observed_inputs[0].contains(&format!("changed_lib_{idx}")),
            "changed-only semantic indexing should only re-embed the touched file"
        );

        let semantic_rows = storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
        assert!(
            semantic_rows
                .iter()
                .any(|record| record.path == "src/main.rs"
                    && record.content_text.contains("main_api")),
            "unchanged live semantic rows should remain present after cycle {idx}"
        );
        assert!(
            semantic_rows.iter().any(|record| {
                record.path == "src/lib.rs"
                    && record.content_text.contains(&format!("changed_lib_{idx}"))
            }),
            "changed live semantic row should update after cycle {idx}"
        );
        assert!(
            semantic_rows.iter().all(|record| {
                !(record.path == "src/lib.rs"
                    && record
                        .content_text
                        .contains(&format!("changed_lib_{}", idx.saturating_sub(1))))
            }),
            "stale changed-only content should not survive cycle {idx}"
        );

        let health = storage.audit_semantic_embedding_partition(
            "repo-001",
            "openai",
            "text-embedding-3-small",
        )?;
        assert!(health.vector_consistent);
        assert_eq!(
            health.covered_snapshot_id.as_deref(),
            Some(summary.snapshot_id.as_str())
        );
        assert_eq!(
            health.live_embedding_rows,
            baseline_health.live_embedding_rows
        );
        assert!(
            health.retained_manifest_snapshots <= DEFAULT_RETAINED_MANIFEST_SNAPSHOTS,
            "manifest retention should stay bounded after cycle {idx}"
        );
    }

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_index_failure_surfaces_batch_context() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-failure-batch-context");
    let workspace_root = temp_workspace_root("semantic-failure-batch-context");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn failing_semantic_case() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let error = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &FailingSemanticEmbeddingExecutor,
    )
    .expect_err("semantic indexing should surface failing batch context");
    let message = error.to_string();
    assert!(
        message.contains("semantic embedding batch failed batch_index=0 total_batches=1"),
        "semantic failure should include batch index context: {message}"
    );
    assert!(
        message.contains("batch_size=1"),
        "semantic failure should include batch size: {message}"
    );
    assert!(
        message.contains("first_chunk=src/main.rs:1-1"),
        "semantic failure should include the first chunk anchor: {message}"
    );
    assert!(
        message.contains("last_chunk=src/main.rs:1-1"),
        "semantic failure should include the last chunk anchor: {message}"
    );
    assert!(
        message.contains("request_context{model=text-embedding-3-small"),
        "semantic failure should preserve provider diagnostics: {message}"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_chunk_candidates_include_docs_and_fixture_text_sources() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("semantic-chunk-doc-sources");
    prepare_workspace(
        &workspace_root,
        &[
            ("README.md", "# Frigg\nsemantic runtime docs\n"),
            (
                "contracts/errors.md",
                "# Errors\ninvalid_params maps to -32602\n",
            ),
            (
                "fixtures/playbooks/deep-search-suite-core.playbook.json",
                "{\n  \"playbook_id\": \"suite-core\"\n}\n",
            ),
            ("src/lib.rs", "pub fn semantic_runtime() {}\n"),
        ],
    )?;

    let manifest = ManifestBuilder::default().build(&workspace_root)?;
    let chunks =
        build_semantic_chunk_candidates("repo-001", &workspace_root, "snapshot-001", &manifest)?
            .candidates;

    assert!(
        chunks.iter().any(|chunk| {
            chunk.path.as_ref() == "README.md" && chunk.language.as_ref() == "markdown"
        }),
        "README.md should participate in semantic chunking"
    );
    assert!(
        chunks.iter().any(|chunk| {
            chunk.path.as_ref() == "contracts/errors.md" && chunk.language.as_ref() == "markdown"
        }),
        "contract markdown should participate in semantic chunking"
    );
    assert!(
        chunks.iter().any(|chunk| {
            chunk.path.as_ref() == "fixtures/playbooks/deep-search-suite-core.playbook.json"
                && chunk.language.as_ref() == "json"
        }),
        "fixture json should participate in semantic chunking"
    );
    assert!(
        chunks.iter().any(|chunk| {
            chunk.path.as_ref() == "src/lib.rs" && chunk.language.as_ref() == "rust"
        }),
        "source files should remain in semantic chunking"
    );

    cleanup_workspace(&workspace_root);
    Ok(())
}

#[test]
fn semantic_chunk_candidates_include_playbook_markdown_under_generic_policy() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("semantic-chunk-skip-playbooks");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "playbooks/hybrid-search-context-retrieval.md",
                "# Playbook\nquery echo\n",
            ),
            ("contracts/errors.md", "# Errors\ninvalid_params\n"),
        ],
    )?;

    let manifest = ManifestBuilder::default().build(&workspace_root)?;
    let chunks =
        build_semantic_chunk_candidates("repo-001", &workspace_root, "snapshot-001", &manifest)?
            .candidates;

    assert!(
        chunks
            .iter()
            .any(|chunk| chunk.path.as_ref() == "playbooks/hybrid-search-context-retrieval.md"),
        "playbook markdown should no longer receive a repo-specific semantic exclusion"
    );
    assert!(
        chunks
            .iter()
            .any(|chunk| chunk.path.as_ref() == "contracts/errors.md"),
        "docs markdown should still remain eligible for semantic chunking"
    );

    cleanup_workspace(&workspace_root);
    Ok(())
}

#[test]
fn semantic_chunking_flushes_markdown_headings_into_separate_chunks() {
    let source = [
        "# Hybrid Search Context Retrieval",
        "",
        "semantic runtime strict failure note metadata",
        "",
        "## Expected Return Cues",
        "",
        "semantic_status",
        "semantic_reason",
    ]
    .join("\n");

    let chunks = build_file_semantic_chunks(
        "repo-001",
        "snapshot-001",
        "contracts/hybrid-search.md",
        "markdown",
        &source,
    );

    assert_eq!(chunks.len(), 2);
    assert!(
        chunks[0]
            .content_text
            .starts_with("# Hybrid Search Context Retrieval")
    );
    assert!(
        chunks[1]
            .content_text
            .starts_with("## Expected Return Cues")
    );
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[1].start_line, 5);
}

#[test]
fn semantic_chunking_splits_oversized_single_line_inputs() {
    let source = "x".repeat(SEMANTIC_CHUNK_MAX_CHARS * 2 + 17);

    let chunks = build_file_semantic_chunks(
        "repo-001",
        "snapshot-001",
        "fixtures/huge.yaml",
        "yaml",
        &source,
    );

    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 1);
    assert_eq!(chunks[1].start_line, 1);
    assert_eq!(chunks[1].end_line, 1);
    assert_eq!(chunks[2].start_line, 1);
    assert_eq!(chunks[2].end_line, 1);
    assert!(
        chunks
            .iter()
            .all(|chunk| chunk.content_text.chars().count() <= SEMANTIC_CHUNK_MAX_CHARS)
    );
    assert_eq!(
        chunks
            .iter()
            .map(|chunk| chunk.content_text.len())
            .sum::<usize>(),
        source.len()
    );
}

#[test]
fn semantic_chunking_skips_blank_oversized_segments() {
    let source = format!("{}a", " ".repeat(SEMANTIC_CHUNK_MAX_CHARS));

    let chunks = build_file_semantic_chunks(
        "repo-001",
        "snapshot-001",
        "fixtures/mostly-blank.yaml",
        "yaml",
        &source,
    );

    assert_eq!(chunks.len(), 1);
    assert_eq!(chunks[0].chunk_index, 0);
    assert_eq!(chunks[0].content_text, "a");
}

#[test]
fn semantic_chunking_anchors_end_line_to_embedded_segment_text() {
    let source = format!(
        "{}\n\n{}",
        "a".repeat(SEMANTIC_CHUNK_MAX_CHARS),
        "b".repeat(10)
    );

    let chunks = build_file_semantic_chunks(
        "repo-001",
        "snapshot-001",
        "fixtures/trailing-blank.yaml",
        "yaml",
        &source,
    );

    assert_eq!(chunks.len(), 2);
    assert_eq!(chunks[0].start_line, 1);
    assert_eq!(chunks[0].end_line, 1);
    assert_eq!(chunks[1].start_line, 2);
    assert_eq!(chunks[1].end_line, 3);
}

#[cfg(unix)]
#[test]
fn semantic_chunk_build_fails_for_unreadable_manifest_file() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("semantic-unreadable-manifest-file");
    prepare_workspace(
        &workspace_root,
        &[("src/private.rs", "pub fn hidden() {}\n")],
    )?;
    let manifest = ManifestBuilder::default().build(&workspace_root)?;
    let unreadable_path = workspace_root.join("src/private.rs");
    set_file_mode(&unreadable_path, 0o000)?;

    let result =
        build_semantic_chunk_candidates("repo-001", &workspace_root, "snapshot-001", &manifest);
    assert!(
        result.is_err(),
        "unreadable semantic manifest file should fail chunk build"
    );
    let error = result
        .err()
        .expect("checked unreadable semantic manifest file error");

    set_file_mode(&unreadable_path, 0o644)?;
    cleanup_workspace(&workspace_root);

    assert!(
        matches!(error, FriggError::Io(_)),
        "expected unreadable semantic file to surface as IO error, got {error:?}"
    );
    Ok(())
}

#[test]
fn semantic_chunk_language_supports_blade_paths() {
    assert_eq!(
        semantic_chunk_language_for_path(Path::new("resources/views/welcome.blade.php")),
        Some("blade")
    );
}

#[cfg(unix)]
#[test]
fn index_continues_with_read_diagnostics_for_unreadable_files() -> FriggResult<()> {
    let db_path = temp_db_path("incremental-unreadable-db");
    let workspace_root = temp_workspace_root("incremental-unreadable-workspace");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "fn main() {}\n"),
            ("src/private.rs", "pub fn hidden() {}\n"),
        ],
    )?;

    let unreadable_path = workspace_root.join("src/private.rs");
    set_file_mode(&unreadable_path, 0o000)?;

    let first =
        index_repository_without_semantic("repo-001", &workspace_root, &db_path, IndexMode::Full)?;
    let second =
        index_repository_without_semantic("repo-001", &workspace_root, &db_path, IndexMode::Full)?;

    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert_eq!(first.files_scanned, 1);
    assert_eq!(first.files_changed, 1);
    assert_eq!(first.files_deleted, 0);
    assert_eq!(first.diagnostics.entries, second.diagnostics.entries);
    assert_eq!(first.diagnostics.total_count(), 1);
    assert_eq!(
        first
            .diagnostics
            .count_by_kind(ManifestDiagnosticKind::Read),
        1
    );
    assert_eq!(
        first
            .diagnostics
            .count_by_kind(ManifestDiagnosticKind::Walk),
        0
    );
    assert_eq!(
        first.diagnostics.entries[0].path.as_deref(),
        Some(unreadable_path.as_path())
    );
    assert_eq!(
        first.diagnostics.entries[0].kind,
        ManifestDiagnosticKind::Read
    );
    assert!(
        !first.diagnostics.entries[0].message.is_empty(),
        "read diagnostics should include an error message"
    );

    set_file_mode(&unreadable_path, 0o644)?;
    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[cfg(unix)]
#[test]
fn changed_only_reuses_previous_digests_for_unchanged_unreadable_files() -> FriggResult<()> {
    let db_path = temp_db_path("incremental-changed-unreadable-db");
    let workspace_root = temp_workspace_root("incremental-changed-unreadable-workspace");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "fn main() {}\n"),
            ("src/private.rs", "pub fn hidden() {}\n"),
        ],
    )?;

    let first =
        index_repository_without_semantic("repo-001", &workspace_root, &db_path, IndexMode::Full)?;
    let unreadable_path = workspace_root.join("src/private.rs");
    set_file_mode(&unreadable_path, 0o000)?;

    let second = index_repository_without_semantic(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
    )?;

    assert_eq!(second.snapshot_id, first.snapshot_id);
    assert_eq!(second.files_scanned, 2);
    assert_eq!(second.files_changed, 0);
    assert_eq!(second.files_deleted, 0);
    assert_eq!(second.diagnostics.total_count(), 0);

    set_file_mode(&unreadable_path, 0o644)?;
    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn index_plan_full_mode_marks_semantic_full_rebuild_when_enabled() -> FriggResult<()> {
    let db_path = temp_db_path("index-plan-semantic-full-db");
    let workspace_root = temp_workspace_root("index-plan-semantic-full-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_full() {}\n")],
    )?;

    let plan = build_index_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime_enabled_openai(),
        &[],
    )?;

    assert_eq!(plan.semantic_refresh.mode, SemanticRefreshMode::FullRebuild);
    assert_eq!(plan.semantic_refresh.records_manifest.len(), 1);
    assert!(plan.semantic_refresh.changed_paths.is_empty());
    assert!(plan.semantic_refresh.deleted_paths.is_empty());

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn index_plan_changed_only_marks_incremental_semantic_refresh_for_deltas() -> FriggResult<()> {
    let db_path = temp_db_path("index-plan-semantic-delta-db");
    let workspace_root = temp_workspace_root("index-plan-semantic-delta-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_delta() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let _summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    fs::write(
        workspace_root.join("src/main.rs"),
        "pub fn semantic_delta() { println!(\"changed\"); }\n",
    )
    .map_err(FriggError::Io)?;

    let plan = build_index_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &[],
    )?;

    assert_eq!(
        plan.semantic_refresh.mode,
        SemanticRefreshMode::IncrementalAdvance
    );
    assert_eq!(plan.semantic_refresh.records_manifest.len(), 1);
    assert_eq!(
        plan.semantic_refresh.changed_paths,
        vec![PathBuf::from("src/main.rs")]
    );
    assert!(plan.semantic_refresh.deleted_paths.is_empty());

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn incremental_semantic_advance_deletes_changed_unreadable_rows() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-advance-unreadable-changed-db");
    let workspace_root = temp_workspace_root("semantic-advance-unreadable-changed-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_stale_v1() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let first = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;
    let storage = Storage::new(&db_path);
    let first_rows =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-001", &first.snapshot_id)?;
    assert!(
        first_rows
            .iter()
            .any(|record| record.content_text.contains("semantic_stale_v1")),
        "full semantic pass should persist the original file content"
    );

    fs::write(workspace_root.join("src/main.rs"), [0xff, b'\n']).map_err(FriggError::Io)?;
    let second = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    assert_ne!(second.snapshot_id, first.snapshot_id);
    assert!(
        storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &first.snapshot_id)?
            .is_empty(),
        "advance must delete old chunks for changed files that cannot be re-read"
    );
    assert!(
        storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?
            .is_empty(),
        "unreadable changed file should not retain stale rows under the new snapshot"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn index_plan_changed_only_reuses_existing_semantic_state_when_workspace_is_unchanged()
-> FriggResult<()> {
    let db_path = temp_db_path("index-plan-semantic-reuse-db");
    let workspace_root = temp_workspace_root("index-plan-semantic-reuse-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_reuse() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    let plan = build_index_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &[],
    )?;

    assert_eq!(
        plan.previous_snapshot_id.as_deref(),
        Some(summary.snapshot_id.as_str())
    );
    assert_eq!(plan.files_changed, 0);
    assert_eq!(plan.files_deleted, 0);
    assert!(matches!(
        &plan.snapshot_plan,
        super::super::ManifestSnapshotPlan::ReuseExisting { snapshot_id }
            if snapshot_id == &summary.snapshot_id
    ));
    assert_eq!(
        plan.semantic_refresh.mode,
        SemanticRefreshMode::ReuseExisting
    );
    assert!(plan.semantic_refresh.records_manifest.is_empty());
    assert!(plan.semantic_refresh.changed_paths.is_empty());
    assert!(plan.semantic_refresh.deleted_paths.is_empty());

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn index_plan_changed_only_marks_full_rebuild_when_semantic_head_is_stale() -> FriggResult<()> {
    let db_path = temp_db_path("index-plan-semantic-stale-db");
    let workspace_root = temp_workspace_root("index-plan-semantic-stale-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_stale_v1() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let semantic_summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    fs::write(
        workspace_root.join("src/main.rs"),
        "pub fn semantic_stale_v2() {}\n",
    )
    .map_err(FriggError::Io)?;

    let manifest_only_summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
        &RuntimeSemanticEmbeddingExecutor::with_endpoint(
            SemanticRuntimeCredentials::default(),
            None,
        ),
    )?;
    assert_ne!(
        manifest_only_summary.snapshot_id,
        semantic_summary.snapshot_id
    );

    let plan = build_index_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &[],
    )?;

    assert_eq!(
        plan.previous_snapshot_id.as_deref(),
        Some(manifest_only_summary.snapshot_id.as_str())
    );
    assert!(matches!(
        &plan.snapshot_plan,
        super::super::ManifestSnapshotPlan::ReuseExisting { snapshot_id }
            if snapshot_id == &manifest_only_summary.snapshot_id
    ));
    assert_eq!(
        plan.semantic_refresh.mode,
        SemanticRefreshMode::FullRebuildFromChangedOnly
    );
    assert_eq!(plan.semantic_refresh.records_manifest.len(), 1);
    assert!(plan.semantic_refresh.changed_paths.is_empty());
    assert!(plan.semantic_refresh.deleted_paths.is_empty());

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn index_plan_changed_only_with_dirty_hint_advances_after_manifest_fast_pass() -> FriggResult<()> {
    let db_path = temp_db_path("index-plan-semantic-followup-db");
    let workspace_root = temp_workspace_root("index-plan-semantic-followup-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_followup_v1() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
        openai_compat_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let semantic_summary = index_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    fs::write(
        workspace_root.join("src/main.rs"),
        "pub fn semantic_followup_v2() {}\n",
    )
    .map_err(FriggError::Io)?;
    let dirty_hint = PathBuf::from("src/main.rs");

    let manifest_fast_summary =
        index_repository_with_runtime_config_and_dirty_paths_and_plan_callback(
            "repo-001",
            &workspace_root,
            &db_path,
            IndexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
            std::slice::from_ref(&dirty_hint),
            |_| Ok(()),
        )?;
    assert_ne!(
        manifest_fast_summary.snapshot_id,
        semantic_summary.snapshot_id
    );

    let plan = build_index_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        IndexMode::ChangedOnly,
        &semantic_runtime,
        &[dirty_hint],
    )?;

    assert_eq!(
        plan.previous_snapshot_id.as_deref(),
        Some(manifest_fast_summary.snapshot_id.as_str())
    );
    assert!(matches!(
        &plan.snapshot_plan,
        super::super::ManifestSnapshotPlan::ReuseExisting { snapshot_id }
            if snapshot_id == &manifest_fast_summary.snapshot_id
    ));
    assert_eq!(
        plan.semantic_refresh.mode,
        SemanticRefreshMode::IncrementalAdvance
    );
    assert_eq!(
        plan.semantic_refresh.advance_from_snapshot_id.as_deref(),
        Some(semantic_summary.snapshot_id.as_str())
    );
    assert_eq!(
        plan.semantic_refresh.changed_paths,
        vec![PathBuf::from("src/main.rs")]
    );
    assert!(plan.semantic_refresh.deleted_paths.is_empty());

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}
