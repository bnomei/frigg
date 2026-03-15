use super::support::*;

#[test]
fn vector_store_initialize_and_verify_roundtrip() -> FriggResult<()> {
    let db_path = temp_db_path("vector-store-roundtrip");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    let status = storage.verify_vector_store(DEFAULT_VECTOR_DIMENSIONS)?;
    assert_eq!(status.expected_dimensions, DEFAULT_VECTOR_DIMENSIONS);
    assert_eq!(status.table_name, VECTOR_TABLE_NAME);
    assert!(
        !status.extension_version.trim().is_empty(),
        "vector extension version should not be empty"
    );

    let conn = open_test_connection(&db_path)?;
    assert!(
        table_exists(&conn, VECTOR_TABLE_NAME)?,
        "expected vector table '{VECTOR_TABLE_NAME}' to exist"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn vector_store_verify_fails_on_dimension_mismatch() -> FriggResult<()> {
    let db_path = temp_db_path("vector-store-dimension-mismatch");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    let err = storage
        .verify_vector_store(DEFAULT_VECTOR_DIMENSIONS + 1)
        .expect_err("verify_vector_store should fail when expected dimensions mismatch");
    let err_message = err.to_string();
    assert!(
        err_message.contains("vector table schema mismatch"),
        "unexpected vector-store mismatch error: {err_message}"
    );
    assert!(
        err_message.contains(&format!("float[{}]", DEFAULT_VECTOR_DIMENSIONS + 1)),
        "dimension mismatch error should mention the expected vector width: {err_message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn vector_store_verify_rejects_zero_dimensions_as_invalid_input() -> FriggResult<()> {
    let db_path = temp_db_path("vector-store-zero-dimensions");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    let err = storage
        .verify_vector_store(0)
        .expect_err("verify_vector_store should reject zero dimensions");
    assert!(
        matches!(
            err,
            FriggError::InvalidInput(ref message)
                if message == "expected_dimensions must be greater than zero"
        ),
        "expected invalid_input for zero dimensions, got {err}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn vector_store_initialize_rejects_zero_dimensions_as_invalid_input() {
    let db_path = temp_db_path("vector-store-init-zero-dimensions");
    let storage = Storage::new(&db_path);

    let err = storage
        .initialize_vector_store(0)
        .expect_err("initialize_vector_store should reject zero dimensions");
    assert!(
        matches!(
            err,
            FriggError::InvalidInput(ref message)
                if message == "expected_dimensions must be greater than zero"
        ),
        "expected invalid_input for zero dimensions, got {err}"
    );

    cleanup_db(&db_path);
}

#[test]
fn vector_store_detected_capability_rejects_unavailable_sqlite_vec() -> FriggResult<()> {
    let db_path = temp_db_path("vector-transition-sqlite-vec-missing");
    let conn = open_test_connection(&db_path)?;
    create_sqlite_vec_like_table(&conn, DEFAULT_VECTOR_DIMENSIONS)?;

    let err = verify_vector_store_on_connection_with_detected_capability(
        &conn,
        DEFAULT_VECTOR_DIMENSIONS,
        None,
    )
    .expect_err("sqlite-vec readiness should fail when extension is unavailable");
    let err_message = err.to_string();
    assert!(
        err_message.contains("sqlite-vec extension is unavailable"),
        "unexpected unavailable-extension error: {err_message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn vector_store_rejects_legacy_non_sqlite_vec_schema() -> FriggResult<()> {
    let db_path = temp_db_path("vector-transition-vec-blocked");
    let conn = open_test_connection(&db_path)?;
    conn.execute_batch(&format!(
        r#"
                CREATE TABLE {VECTOR_TABLE_NAME} (
                  embedding_id TEXT PRIMARY KEY,
                  embedding BLOB NOT NULL,
                  dimensions INTEGER NOT NULL,
                  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                "#
    ))
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed legacy fallback-style table for transition tests: {err}"
        ))
    })?;

    let err = verify_vector_store_on_connection_with_detected_capability(
        &conn,
        DEFAULT_VECTOR_DIMENSIONS,
        Some(format!("v{SQLITE_VEC_REQUIRED_VERSION}")),
    )
    .expect_err("legacy fallback-style schema should be rejected");
    let err_message = err.to_string();
    assert!(
        err_message.contains("legacy non-sqlite-vec schema detected"),
        "unexpected legacy schema error: {err_message}"
    );

    let init_err = initialize_vector_store_on_connection_with_detected_capability(
        &conn,
        DEFAULT_VECTOR_DIMENSIONS,
        Some(format!("v{SQLITE_VEC_REQUIRED_VERSION}")),
    )
    .expect_err("initialize should reject legacy fallback-style schema");
    let init_err_message = init_err.to_string();
    assert!(
        init_err_message.contains("legacy non-sqlite-vec schema detected"),
        "unexpected legacy schema error during initialize: {init_err_message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn sqlite_vec_version_pin_accepts_prefixed_and_unprefixed_versions() -> FriggResult<()> {
    ensure_sqlite_vec_pinned_version(SQLITE_VEC_REQUIRED_VERSION)?;
    ensure_sqlite_vec_pinned_version(&format!("v{SQLITE_VEC_REQUIRED_VERSION}"))?;
    ensure_sqlite_vec_pinned_version(&format!("V{SQLITE_VEC_REQUIRED_VERSION}"))?;
    Ok(())
}

#[test]
fn sqlite_vec_version_pin_rejects_mismatch_deterministically() {
    let err = ensure_sqlite_vec_pinned_version("v0.0.0")
        .expect_err("mismatched sqlite-vec runtime version should be rejected");
    let message = err.to_string();
    assert!(
        message.contains("sqlite-vec extension version mismatch"),
        "unexpected version mismatch message: {message}"
    );
    assert!(
        message.contains("v0.0.0"),
        "mismatch message should include found runtime version: {message}"
    );
    assert!(
        message.contains(SQLITE_VEC_REQUIRED_VERSION),
        "mismatch message should include required pinned version: {message}"
    );
}

#[test]
fn vector_store_detected_capability_rejects_mismatched_sqlite_vec_version() -> FriggResult<()> {
    let db_path = temp_db_path("vector-transition-version-mismatch");
    let conn = open_test_connection(&db_path)?;
    create_sqlite_vec_like_table(&conn, DEFAULT_VECTOR_DIMENSIONS)?;

    let err = verify_vector_store_on_connection_with_detected_capability(
        &conn,
        DEFAULT_VECTOR_DIMENSIONS,
        Some("v0.0.0".to_string()),
    )
    .expect_err("mismatched sqlite-vec version should fail readiness");
    let err_message = err.to_string();
    assert!(
        err_message.contains("sqlite-vec extension version mismatch"),
        "unexpected mismatch error: {err_message}"
    );
    assert!(
        err_message.contains(SQLITE_VEC_REQUIRED_VERSION),
        "mismatch error should include pinned version: {err_message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn semantic_vector_repair_restores_partition_consistency() -> FriggResult<()> {
    let db_path = temp_db_path("semantic-vector-repair");
    let storage = Storage::new(&db_path);
    storage.initialize()?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[
            manifest_entry("src/a.rs", "hash-a", 10, Some(100)),
            manifest_entry("src/b.rs", "hash-b", 10, Some(100)),
        ],
    )?;

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

    let conn = open_connection(&db_path)?;
    conn.execute(
        &format!(
            "DELETE FROM {VECTOR_TABLE_NAME} WHERE repository_id = ?1 AND provider = ?2 AND model = ?3 AND chunk_id = ?4"
        ),
        ("repo-1", "openai", "text-embedding-3-small", "chunk-b"),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to corrupt semantic vector partition for repair test: {err}"
        ))
    })?;
    drop(conn);

    let broken = storage.collect_semantic_storage_health_for_repository_model(
        "repo-1",
        "openai",
        "text-embedding-3-small",
    )?;
    assert!(!broken.vector_consistent);
    assert_eq!(broken.live_embedding_rows, 2);
    assert_eq!(broken.live_vector_rows, 1);

    let err = storage
        .verify()
        .expect_err("verify should fail when semantic vector partitions drift out of sync");
    let err_message = err.to_string();
    assert!(
        err_message.contains("invariant=semantic_vector_partition_in_sync"),
        "unexpected semantic partition invariant error: {err_message}"
    );
    assert!(
        err_message.contains("count=1"),
        "unexpected semantic partition invariant count: {err_message}"
    );
    assert!(
        err_message.contains("repo-1:openai:text-embedding-3-small"),
        "unexpected semantic partition details: {err_message}"
    );

    let repair_summary = storage.repair_storage_invariants()?;
    assert_eq!(
        repair_summary.repaired_categories,
        vec!["semantic_vector_partition_in_sync".to_string()]
    );

    let repaired = storage.collect_semantic_storage_health_for_repository_model(
        "repo-1",
        "openai",
        "text-embedding-3-small",
    )?;
    assert!(repaired.vector_consistent);
    assert_eq!(repaired.live_embedding_rows, 2);
    assert_eq!(repaired.live_vector_rows, 2);

    cleanup_db(&db_path);
    Ok(())
}
