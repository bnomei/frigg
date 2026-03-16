use super::support::*;

#[test]
fn semantic_indexing_reindex_persists_deterministic_embeddings_when_enabled() -> FriggResult<()> {
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
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let first = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
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

    let second = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
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
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("test runtime should build");
    let summary = runtime.block_on(async {
        reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
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
        "expected semantic embeddings when reindex runs inside a tokio runtime"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_indexing_disabled_preserves_reindex_behavior() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-disabled-preserves");
    let workspace_root = temp_workspace_root("semantic-disabled-preserves");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
    )?;

    let summary = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
        &RuntimeSemanticEmbeddingExecutor::new(SemanticRuntimeCredentials::default()),
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
    };
    let valid_summary = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &valid_runtime,
        &valid_credentials,
        &executor,
    )?;

    let storage = Storage::new(&db_path);
    let before = storage
        .load_semantic_embeddings_for_repository_snapshot("repo-001", &valid_summary.snapshot_id)?;
    assert!(
        !before.is_empty(),
        "expected seeded semantic records before invalid reindex attempt"
    );

    let invalid_runtime = SemanticRuntimeConfig {
        enabled: true,
        provider: Some(SemanticRuntimeProvider::OpenAi),
        model: Some("text-embedding-3-small".to_owned()),
        strict_mode: false,
    };
    let invalid_credentials = SemanticRuntimeCredentials::default();
    let error = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
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
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let first = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    fs::write(
        workspace_root.join("src/main.rs"),
        "pub fn changed_after_failure() {}\n",
    )
    .map_err(FriggError::Io)?;

    let plan = build_reindex_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
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

    let error = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
        &FailingSemanticEmbeddingExecutor,
    )
    .expect_err("failing semantic executor should abort reindex");
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
        "failed semantic reindex must not advance the latest manifest snapshot"
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
    };
    let first_executor = CountingSemanticEmbeddingExecutor::default();
    let first = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
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
    let second = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
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
    };

    let first = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
        &FixtureSemanticEmbeddingExecutor,
    )?;
    let storage = Storage::new(&db_path);
    let baseline_health = storage.collect_semantic_storage_health_for_repository_model(
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
        let summary = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
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

        let health = storage.collect_semantic_storage_health_for_repository_model(
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
fn semantic_indexing_reindex_failure_surfaces_batch_context() -> FriggResult<()> {
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
    };
    let error = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
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
        build_semantic_chunk_candidates("repo-001", &workspace_root, "snapshot-001", &manifest)?;

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
        build_semantic_chunk_candidates("repo-001", &workspace_root, "snapshot-001", &manifest)?;

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
fn semantic_chunk_language_supports_blade_paths() {
    assert_eq!(
        semantic_chunk_language_for_path(Path::new("resources/views/welcome.blade.php")),
        Some("blade")
    );
}

#[cfg(unix)]
#[test]
fn reindex_continues_with_read_diagnostics_for_unreadable_files() -> FriggResult<()> {
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

    let first = reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;
    let second = reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;

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

    let first = reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;
    let unreadable_path = workspace_root.join("src/private.rs");
    set_file_mode(&unreadable_path, 0o000)?;

    let second = reindex_repository(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
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
fn reindex_plan_full_mode_marks_semantic_full_rebuild_when_enabled() -> FriggResult<()> {
    let db_path = temp_db_path("reindex-plan-semantic-full-db");
    let workspace_root = temp_workspace_root("reindex-plan-semantic-full-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_full() {}\n")],
    )?;

    let plan = build_reindex_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
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
fn reindex_plan_changed_only_marks_incremental_semantic_refresh_for_deltas() -> FriggResult<()> {
    let db_path = temp_db_path("reindex-plan-semantic-delta-db");
    let workspace_root = temp_workspace_root("reindex-plan-semantic-delta-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_delta() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let _summary = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    fs::write(
        workspace_root.join("src/main.rs"),
        "pub fn semantic_delta() { println!(\"changed\"); }\n",
    )
    .map_err(FriggError::Io)?;

    let plan = build_reindex_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
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
fn reindex_plan_changed_only_reuses_existing_semantic_state_when_workspace_is_unchanged()
-> FriggResult<()> {
    let db_path = temp_db_path("reindex-plan-semantic-reuse-db");
    let workspace_root = temp_workspace_root("reindex-plan-semantic-reuse-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_reuse() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let summary = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    let plan = build_reindex_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
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
fn reindex_plan_changed_only_marks_full_rebuild_when_semantic_head_is_stale() -> FriggResult<()> {
    let db_path = temp_db_path("reindex-plan-semantic-stale-db");
    let workspace_root = temp_workspace_root("reindex-plan-semantic-stale-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "pub fn semantic_stale_v1() {}\n")],
    )?;

    let semantic_runtime = semantic_runtime_enabled_openai();
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: Some("test-openai-key".to_owned()),
        gemini_api_key: None,
    };
    let executor = FixtureSemanticEmbeddingExecutor;
    let semantic_summary = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
        &executor,
    )?;

    fs::write(
        workspace_root.join("src/main.rs"),
        "pub fn semantic_stale_v2() {}\n",
    )
    .map_err(FriggError::Io)?;

    let manifest_only_summary = reindex_repository_with_semantic_executor(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
        &RuntimeSemanticEmbeddingExecutor::new(SemanticRuntimeCredentials::default()),
    )?;
    assert_ne!(
        manifest_only_summary.snapshot_id,
        semantic_summary.snapshot_id
    );

    let plan = build_reindex_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
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
