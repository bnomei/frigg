use super::support::*;

#[test]
fn semantic_embedding_replace_and_load_roundtrip_is_deterministic() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-roundtrip");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[
            semantic_record(
                "chunk-z",
                "repo-1",
                "snapshot-001",
                "src/zeta.rs",
                "rust",
                2,
                11,
                20,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-z",
                "fn zeta() {}",
                &[0.3, 0.4, 0.5],
            ),
            semantic_record(
                "chunk-a",
                "repo-1",
                "snapshot-001",
                "src/alpha.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-a",
                "fn alpha() {}",
                &[0.1, 0.2, 0.3],
            ),
        ],
    )?;

    let loaded =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert_eq!(loaded.len(), 2);
    assert_eq!(loaded[0].path, "src/alpha.rs");
    assert_eq!(loaded[1].path, "src/zeta.rs");
    assert_eq!(loaded[0].chunk_index, 0);
    assert_eq!(loaded[1].chunk_index, 2);
    assert_eq!(loaded[0].embedding, vec![0.1, 0.2, 0.3]);
    assert_eq!(loaded[1].embedding, vec![0.3, 0.4, 0.5]);

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_migrates_shared_chunk_rows_from_v3_schema() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-migrate-v3");
    initialize_v3_storage_schema(&db_path)?;

    let conn = open_test_connection(&db_path)?;
    conn.execute(
        r#"
            INSERT INTO semantic_chunk_embedding (
                chunk_id,
                repository_id,
                snapshot_id,
                path,
                language,
                chunk_index,
                start_line,
                end_line,
                provider,
                model,
                trace_id,
                content_hash_blake3,
                content_text,
                embedding_blob,
                dimensions,
                created_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
        (
            "chunk-legacy",
            "repo-1",
            "snapshot-001",
            "src/legacy.rs",
            "rust",
            0i64,
            1i64,
            10i64,
            "openai",
            "text-embedding-3-small",
            Some("trace-001"),
            "hash-legacy",
            "fn legacy() {}",
            encode_f32_vector(&[0.1, 0.2]),
            2i64,
            "2026-03-10T00:00:00Z",
        ),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed legacy semantic embedding row for migration test: {err}"
        ))
    })?;
    drop(conn);

    let storage = Storage::new(&db_path);
    storage.initialize()?;

    assert_eq!(storage.schema_version()?, 6);

    let migrated =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert!(
        migrated.is_empty(),
        "legacy snapshot-keyed semantic rows should be cleared during the live-corpus migration"
    );

    let conn = open_test_connection(&db_path)?;
    assert!(
        table_exists(&conn, "semantic_chunk")?,
        "expected semantic_chunk table after migration"
    );
    assert!(
        !table_exists(&conn, "semantic_chunk_embedding_v3_legacy")?,
        "legacy semantic chunk embedding table should be dropped after migration"
    );
    assert_eq!(
        count_rows(&conn, "semantic_chunk")?,
        0,
        "legacy semantic chunk rows should be cleared after migration"
    );
    assert_eq!(
        count_rows(&conn, "semantic_chunk_embedding")?,
        0,
        "legacy semantic embedding rows should be cleared after migration"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_replace_is_repository_scoped() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-replace-scoped");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[semantic_record(
            "chunk-repo1-old",
            "repo-1",
            "snapshot-001",
            "src/old.rs",
            "rust",
            0,
            1,
            10,
            "openai",
            "text-embedding-3-small",
            Some("trace-001"),
            "hash-old",
            "fn old() {}",
            &[0.1, 0.2],
        )],
    )?;
    replace_semantic_records(
        &storage,
        "repo-2",
        "snapshot-101",
        &[semantic_record(
            "chunk-repo2",
            "repo-2",
            "snapshot-101",
            "src/repo2.rs",
            "rust",
            0,
            1,
            3,
            "google",
            "gemini-embedding-001",
            Some("trace-101"),
            "hash-repo2",
            "fn repo2() {}",
            &[0.9, 0.8],
        )],
    )?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-002",
        &[semantic_record(
            "chunk-repo1-new",
            "repo-1",
            "snapshot-002",
            "src/new.rs",
            "rust",
            1,
            20,
            30,
            "openai",
            "text-embedding-3-small",
            Some("trace-002"),
            "hash-new",
            "fn new() {}",
            &[0.7, 0.6],
        )],
    )?;

    assert!(
        storage
            .load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?
            .is_empty(),
        "old repo-1 snapshot should be replaced"
    );
    let repo1_new =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-002")?;
    assert_eq!(repo1_new.len(), 1);
    assert_eq!(repo1_new[0].chunk_id, "chunk-repo1-new");

    let repo2_existing =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-2", "snapshot-101")?;
    assert_eq!(repo2_existing.len(), 1);
    assert_eq!(repo2_existing[0].chunk_id, "chunk-repo2");

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_replace_deduplicates_shared_chunk_content_across_models() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-dedupe-shared-content");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[
            semantic_record(
                "chunk-shared",
                "repo-1",
                "snapshot-001",
                "src/shared.rs",
                "rust",
                0,
                1,
                12,
                "google",
                "gemini-embedding-001",
                Some("trace-google"),
                "hash-shared",
                "fn shared() {}",
                &[0.9, 0.8],
            ),
            semantic_record(
                "chunk-shared",
                "repo-1",
                "snapshot-001",
                "src/shared.rs",
                "rust",
                0,
                1,
                12,
                "openai",
                "text-embedding-3-small",
                Some("trace-openai"),
                "hash-shared",
                "fn shared() {}",
                &[0.1, 0.2],
            ),
        ],
    )?;

    let loaded =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert_eq!(loaded.len(), 2);
    assert!(
        loaded
            .iter()
            .all(|record| record.chunk_id == "chunk-shared")
    );
    assert!(
        loaded
            .iter()
            .all(|record| record.content_text == "fn shared() {}"),
        "shared chunk text should be reconstructed for each provider/model row"
    );

    let conn = open_test_connection(&db_path)?;
    assert_eq!(
        count_rows(&conn, "semantic_chunk")?,
        2,
        "expected one live semantic chunk row per provider/model corpus"
    );
    assert_eq!(
        count_rows(&conn, "semantic_chunk_embedding")?,
        2,
        "expected one lean embedding row per provider/model variant"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_replace_rejects_invalid_records_without_mutation() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-replace-invalid");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[semantic_record(
            "chunk-valid",
            "repo-1",
            "snapshot-001",
            "src/valid.rs",
            "rust",
            0,
            1,
            4,
            "openai",
            "text-embedding-3-small",
            Some("trace-001"),
            "hash-valid",
            "fn valid() {}",
            &[0.5, 0.4],
        )],
    )?;

    let mut invalid = semantic_record(
        "chunk-invalid",
        "repo-1",
        "snapshot-002",
        "src/invalid.rs",
        "rust",
        0,
        1,
        4,
        "openai",
        "text-embedding-3-small",
        Some("trace-002"),
        "hash-invalid",
        "fn invalid() {}",
        &[0.2, 0.1],
    );
    invalid.embedding.clear();
    let error = replace_semantic_records(&storage, "repo-1", "snapshot-002", &[invalid])
        .expect_err("semantic replace should fail for empty embeddings");
    assert!(
        matches!(error, FriggError::InvalidInput(_)),
        "expected invalid input error, got {error}"
    );

    let existing =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert_eq!(existing.len(), 1);
    assert_eq!(existing[0].chunk_id, "chunk-valid");

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_projection_and_text_lookup_are_deterministic() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-projection");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[
            semantic_record(
                "chunk-b",
                "repo-1",
                "snapshot-001",
                "src/b.rs",
                "rust",
                1,
                20,
                30,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-b",
                "fn b() {}",
                &[0.3, 0.4],
            ),
            semantic_record(
                "chunk-a",
                "repo-1",
                "snapshot-001",
                "src/a.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-a",
                "fn a() {}",
                &[0.1, 0.2],
            ),
        ],
    )?;

    let projections = storage
        .load_semantic_embedding_projections_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert_eq!(projections.len(), 2);
    assert_eq!(projections[0].chunk_id, "chunk-a");
    assert_eq!(projections[0].path, "src/a.rs");
    assert_eq!(projections[0].start_line, 1);
    assert_eq!(projections[0].end_line, 10);
    assert_eq!(projections[0].embedding, vec![0.1, 0.2]);
    assert_eq!(projections[1].chunk_id, "chunk-b");
    assert!(
        storage.has_semantic_embeddings_for_repository_snapshot_model(
            "repo-1",
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
        )?
    );
    assert!(
        !storage.has_semantic_embeddings_for_repository_snapshot_model(
            "repo-1",
            "snapshot-001",
            "google",
            "gemini-embedding-001",
        )?
    );
    assert_eq!(
        storage.count_semantic_embeddings_for_repository_snapshot_model(
            "repo-1",
            "snapshot-001",
            "openai",
            "text-embedding-3-small",
        )?,
        2
    );
    assert_eq!(
        storage.count_semantic_embeddings_for_repository_snapshot_model(
            "repo-1",
            "snapshot-001",
            "google",
            "gemini-embedding-001",
        )?,
        0
    );

    let filtered = storage.load_semantic_embedding_projections_for_repository_snapshot_model(
        "repo-1",
        "snapshot-001",
        Some("openai"),
        Some("text-embedding-3-small"),
    )?;
    assert_eq!(filtered.len(), 2);
    let empty_filtered = storage
        .load_semantic_embedding_projections_for_repository_snapshot_model(
            "repo-1",
            "snapshot-001",
            Some("google"),
            Some("gemini-embedding-001"),
        )?;
    assert!(empty_filtered.is_empty());

    let texts = storage.load_semantic_chunk_texts_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &[
            "chunk-b".to_owned(),
            "chunk-a".to_owned(),
            "chunk-b".to_owned(),
        ],
    )?;
    assert_eq!(texts.len(), 2);
    assert_eq!(texts.get("chunk-a").map(String::as_str), Some("fn a() {}"));
    assert_eq!(texts.get("chunk-b").map(String::as_str), Some("fn b() {}"));

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_latest_snapshot_lookup_prefers_newest_eligible_snapshot() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-latest-snapshot");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[manifest_entry("src/alpha.rs", "hash-a1", 10, Some(100))],
    )?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-002",
        &[manifest_entry("src/alpha.rs", "hash-a2", 11, Some(110))],
    )?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-003",
        &[manifest_entry("src/alpha.rs", "hash-a3", 12, Some(120))],
    )?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[semantic_record(
            "chunk-old",
            "repo-1",
            "snapshot-001",
            "src/alpha.rs",
            "rust",
            0,
            1,
            10,
            "openai",
            "text-embedding-3-small",
            Some("trace-001"),
            "hash-old",
            "fn old() {}",
            &[0.1, 0.2],
        )],
    )?;
    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-002",
        &[semantic_record(
            "chunk-newer",
            "repo-1",
            "snapshot-002",
            "src/alpha.rs",
            "rust",
            0,
            1,
            10,
            "openai",
            "text-embedding-3-small",
            Some("trace-002"),
            "hash-newer",
            "fn newer() {}",
            &[0.3, 0.4],
        )],
    )?;

    assert_eq!(
        storage.load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
            "repo-1",
            "openai",
            "text-embedding-3-small",
        )?,
        Some("snapshot-002".to_owned())
    );
    assert_eq!(
        storage.load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
            "repo-1",
            "google",
            "gemini-embedding-001",
        )?,
        None
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_vector_topk_normalizes_short_canonical_embeddings() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-vector-topk-normalized");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[
            semantic_record(
                "chunk-a",
                "repo-1",
                "snapshot-001",
                "src/a.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-a",
                "fn a() {}",
                &[1.0, 0.0],
            ),
            semantic_record(
                "chunk-b",
                "repo-1",
                "snapshot-001",
                "src/b.rs",
                "rust",
                1,
                11,
                20,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-b",
                "fn b() {}",
                &[0.0, 1.0],
            ),
        ],
    )?;

    let mut query_embedding = vec![1.0, 0.0];
    query_embedding.resize(DEFAULT_VECTOR_DIMENSIONS, 0.0);
    let matches = storage.load_semantic_vector_topk_for_repository_snapshot_model(
        "repo-1",
        "snapshot-001",
        "openai",
        "text-embedding-3-small",
        &query_embedding,
        2,
        None,
    )?;
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].chunk_id, "chunk-a");
    assert_eq!(matches[1].chunk_id, "chunk-b");
    assert!(matches[0].distance <= matches[1].distance);

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_latest_manifest_snapshot_lookup_prefers_newest_compatible_snapshot()
-> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-latest-compatible-snapshot");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[manifest_entry("src/a.rs", "hash-a1", 10, Some(100))],
    )?;
    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[semantic_record(
            "chunk-a",
            "repo-1",
            "snapshot-001",
            "src/a.rs",
            "rust",
            0,
            1,
            10,
            "openai",
            "text-embedding-3-small",
            Some("trace-001"),
            "hash-a1",
            "fn a() {}",
            &[0.1, 0.2],
        )],
    )?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-002",
        &[manifest_entry("src/a.rs", "hash-a2", 11, Some(110))],
    )?;

    let latest_manifest = storage
        .load_latest_manifest_for_repository("repo-1")?
        .expect("expected latest manifest snapshot");
    assert_eq!(latest_manifest.snapshot_id, "snapshot-002");
    assert_eq!(
        storage.load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
            "repo-1",
            "openai",
            "text-embedding-3-small",
        )?,
        Some("snapshot-001".to_owned())
    );
    assert_eq!(
        storage.load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
            "repo-1",
            "google",
            "gemini-embedding-001",
        )?,
        None
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_embedding_advance_preserves_unchanged_rows_and_replaces_changed_paths()
-> FriggResult<()> {
    let db_path = temp_db_path("semantic-embedding-advance");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-001",
        &[
            semantic_record(
                "chunk-keep",
                "repo-1",
                "snapshot-001",
                "src/keep.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-keep-old",
                "fn keep_old() {}",
                &[0.1, 0.2],
            ),
            semantic_record(
                "chunk-change-old",
                "repo-1",
                "snapshot-001",
                "src/change.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-change-old",
                "fn change_old() {}",
                &[0.3, 0.4],
            ),
            semantic_record(
                "chunk-delete-old",
                "repo-1",
                "snapshot-001",
                "src/delete.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-delete-old",
                "fn delete_old() {}",
                &[0.5, 0.6],
            ),
        ],
    )?;

    advance_semantic_records(
        &storage,
        "repo-1",
        Some("snapshot-001"),
        "snapshot-002",
        &["src/change.rs".to_owned()],
        &["src/delete.rs".to_owned()],
        &[semantic_record(
            "chunk-change-new",
            "repo-1",
            "snapshot-002",
            "src/change.rs",
            "rust",
            0,
            11,
            20,
            "openai",
            "text-embedding-3-small",
            Some("trace-002"),
            "hash-change-new",
            "fn change_new() {}",
            &[0.7, 0.8],
        )],
    )?;

    assert!(
        storage
            .load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?
            .is_empty(),
        "old snapshot rows should be advanced or removed"
    );

    let current =
        storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-002")?;
    assert_eq!(current.len(), 2);
    assert_eq!(current[0].chunk_id, "chunk-change-new");
    assert_eq!(current[0].content_text, "fn change_new() {}");
    assert_eq!(current[1].chunk_id, "chunk-keep");
    assert_eq!(current[1].content_text, "fn keep_old() {}");
    assert!(
        current.iter().all(|record| record.path != "src/delete.rs"),
        "deleted semantic path should be removed from latest snapshot"
    );

    cleanup_db(&db_path);
    Ok(())
}
