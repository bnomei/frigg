//! Regression tests for current storage schema initialization and sqlite-vec capability detection.

use super::support::*;

#[test]
fn initialize_applies_base_schema_and_version() -> FriggResult<()> {
    let db_path = temp_db_path("init-base-schema");
    let storage = Storage::new(&db_path);

    storage.initialize()?;

    assert_eq!(storage.schema_version()?, CURRENT_SCHEMA_VERSION);

    let conn = open_test_connection(&db_path)?;
    for table in REQUIRED_TABLES {
        assert!(
            table_exists(&conn, table)?,
            "expected table '{table}' to exist"
        );
    }

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn storage_connections_install_busy_timeout() -> FriggResult<()> {
    let db_path = temp_db_path("connection-busy-timeout");
    let storage = Storage::new(&db_path);

    storage.initialize()?;

    let conn = open_connection(&db_path)?;
    let busy_timeout_ms: i64 = conn
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .map_err(|err| {
            FriggError::Internal(format!("failed to read sqlite busy timeout: {err}"))
        })?;
    assert_eq!(busy_timeout_ms as u64, DEFAULT_SQLITE_BUSY_TIMEOUT_MS);

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn storage_connections_use_wal_normal_synchronous() -> FriggResult<()> {
    let db_path = temp_db_path("connection-synchronous-normal");
    let storage = Storage::new(&db_path);

    storage.initialize()?;

    let conn = open_connection(&db_path)?;
    let synchronous: i64 = conn
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .map_err(|err| {
            FriggError::Internal(format!("failed to read sqlite synchronous pragma: {err}"))
        })?;
    assert_eq!(synchronous, 1, "NORMAL synchronous mode should be active");

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn wal_checkpoint_truncate_does_not_wait_on_active_reader() -> FriggResult<()> {
    let db_path = temp_db_path("wal-checkpoint-active-reader");
    let storage = Storage::new(&db_path);

    storage.initialize()?;
    storage.upsert_repository(
        "repo-before-reader",
        Path::new("/tmp/repo-before"),
        "Before",
    )?;

    let mut reader_conn = open_connection(&db_path)?;
    let reader_tx = reader_conn.transaction().map_err(|err| {
        FriggError::Internal(format!("failed to start reader transaction: {err}"))
    })?;
    let _count: i64 = reader_tx
        .query_row("SELECT COUNT(*) FROM repository", [], |row| row.get(0))
        .map_err(|err| {
            FriggError::Internal(format!("failed to pin reader transaction snapshot: {err}"))
        })?;

    storage.upsert_repository("repo-after-reader", Path::new("/tmp/repo-after"), "After")?;

    let session = storage.open_session()?;
    let started_at = std::time::Instant::now();
    let err = session
        .checkpoint_wal_truncate()
        .expect_err("active reader should make truncate checkpoint skip");
    assert!(
        started_at.elapsed() < std::time::Duration::from_millis(500),
        "nonblocking checkpoint should not wait on the normal busy timeout"
    );
    assert!(
        err.to_string().contains("busy"),
        "unexpected checkpoint error: {err}"
    );

    drop(reader_tx);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn sqlite_busy_timeout_override_parses_positive_milliseconds() -> FriggResult<()> {
    assert_eq!(
        sqlite_busy_timeout_ms_from_raw(None)?,
        DEFAULT_SQLITE_BUSY_TIMEOUT_MS
    );
    assert_eq!(sqlite_busy_timeout_ms_from_raw(Some(" 45000 "))?, 45_000);

    let zero = sqlite_busy_timeout_ms_from_raw(Some("0"))
        .expect_err("zero busy timeout should be rejected");
    assert!(
        zero.to_string().contains("must be greater than 0"),
        "unexpected zero-timeout error: {zero}"
    );

    let invalid = sqlite_busy_timeout_ms_from_raw(Some("later"))
        .expect_err("non-numeric busy timeout should be rejected");
    assert!(
        invalid.to_string().contains("positive integer"),
        "unexpected invalid-timeout error: {invalid}"
    );

    Ok(())
}

#[test]
fn initialize_is_idempotent() -> FriggResult<()> {
    let db_path = temp_db_path("init-idempotent");
    let storage = Storage::new(&db_path);

    storage.initialize()?;
    {
        let conn = open_test_connection(&db_path)?;
        conn.execute(
            r#"
                INSERT INTO repository (repository_id, root_path, display_name, created_at)
                VALUES ('repo-1', '/tmp/repo-1', 'Repo 1', '2026-03-04T00:00:00Z')
                "#,
            [],
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to seed repository row for test: {err}"))
        })?;
    }

    storage.initialize()?;

    assert_eq!(storage.schema_version()?, CURRENT_SCHEMA_VERSION);

    let conn = open_test_connection(&db_path)?;
    let schema_version_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
        .map_err(|err| {
            FriggError::Internal(format!("failed to count schema version rows: {err}"))
        })?;
    assert_eq!(schema_version_rows, 1);

    let repository_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM repository", [], |row| row.get(0))
        .map_err(|err| FriggError::Internal(format!("failed to count repository rows: {err}")))?;
    assert_eq!(repository_rows, 1);

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn initialize_rejects_incompatible_existing_schema() -> FriggResult<()> {
    let db_path = temp_db_path("init-incompatible-schema");
    {
        let conn = open_test_connection(&db_path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );
            INSERT INTO schema_version (id, version, updated_at)
            VALUES (1, 10, CURRENT_TIMESTAMP);
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to seed incompatible schema version: {err}"))
        })?;
    }

    let storage = Storage::new(&db_path);
    let err = storage
        .initialize()
        .expect_err("initialize should reject incompatible storage");
    assert!(
        matches!(&err, FriggError::StorageSchemaIncompatible { .. }),
        "incompatible schema should use a typed error variant, got: {err}"
    );
    let message = err.to_string();
    assert!(
        message.contains("storage schema is incompatible"),
        "unexpected incompatible-schema error: {message}"
    );
    assert!(
        message.contains("delete"),
        "incompatible-schema error should tell the user to delete and regenerate: {message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn read_paths_reject_incompatible_existing_schema() -> FriggResult<()> {
    let db_path = temp_db_path("read-incompatible-schema");
    {
        let conn = open_test_connection(&db_path)?;
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );
            INSERT INTO schema_version (id, version, updated_at)
            VALUES (1, 10, CURRENT_TIMESTAMP);
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to seed incompatible schema version: {err}"))
        })?;
    }

    let storage = Storage::new(&db_path);
    let err = storage
        .load_latest_manifest_for_repository("repo-1")
        .expect_err("manifest reads should reject incompatible storage");
    assert!(
        matches!(&err, FriggError::StorageSchemaIncompatible { .. }),
        "incompatible schema reads should use a typed error variant, got: {err}"
    );
    let message = err.to_string();
    assert!(
        message.contains("storage schema is incompatible"),
        "unexpected incompatible-schema read error: {message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn verify_succeeds_after_initialize() -> FriggResult<()> {
    let db_path = temp_db_path("verify-success");
    let storage = Storage::new(&db_path);

    storage.initialize()?;
    storage.verify()?;

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn verify_missing_db_fails_without_creating_file() {
    let db_path = temp_db_path("verify-missing-db-no-create");
    let storage = Storage::new(&db_path);

    let err = storage
        .verify()
        .expect_err("verify should fail when the storage db file is missing");
    let message = err.to_string();
    assert!(
        message.contains("storage db file is missing"),
        "unexpected missing-db verify error: {message}"
    );
    assert!(
        message.contains("frigg init") || message.contains("frigg index"),
        "missing-db verify error should tell the user how to create storage: {message}"
    );
    assert!(
        !db_path.exists(),
        "verify must not create an empty sqlite file for missing storage"
    );
}

#[test]
fn verify_fails_when_required_table_missing() -> FriggResult<()> {
    let db_path = temp_db_path("verify-missing-table");
    let storage = Storage::new(&db_path);

    storage.initialize()?;
    {
        let conn = open_test_connection(&db_path)?;
        conn.execute("DROP TABLE snapshot", []).map_err(|err| {
            FriggError::Internal(format!(
                "failed to drop snapshot table for verify test: {err}"
            ))
        })?;
    }

    let err = storage
        .verify()
        .expect_err("verify should fail when schema table is missing");
    let err_message = err.to_string();
    assert!(
        err_message.contains("missing required table 'snapshot'"),
        "unexpected verify error: {err_message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn verify_fails_when_manifest_rows_reference_non_manifest_snapshots() -> FriggResult<()> {
    let db_path = temp_db_path("verify-non-manifest-manifest-row");
    let storage = Storage::new(&db_path);

    storage.initialize()?;
    {
        let conn = open_test_connection(&db_path)?;
        conn.execute(
            "INSERT INTO repository (repository_id, root_path, display_name, created_at) VALUES ('repo-1', '/tmp/repo-1', 'Repo 1', '2026-03-11T00:00:00Z')",
            [],
        )
        .map_err(|err| FriggError::Internal(format!("failed to seed repository row for manifest invariant test: {err}")))?;
        conn.execute(
            r#"
            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            VALUES ('snapshot-semantic', 'repo-1', 'semantic', NULL, '2026-03-11T00:00:00Z')
            "#,
            [],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to seed non-manifest snapshot for manifest invariant test: {err}"
            ))
        })?;
        conn.execute(
            r#"
            INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
            VALUES ('snapshot-semantic', 'src/drift.rs', 'hash-drift', 64, 12345)
            "#,
            [],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to seed drifted manifest row for manifest invariant test: {err}"
            ))
        })?;
    }

    let err = storage
        .verify()
        .expect_err("verify should fail when file_manifest rows reference non-manifest snapshots");
    let err_message = err.to_string();
    assert!(
        err_message.contains("invariant=manifest_rows_require_manifest_snapshots"),
        "unexpected invariant error: {err_message}"
    );
    assert!(
        err_message.contains("count=1"),
        "unexpected manifest invariant count: {err_message}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn initialize_creates_hotpath_indexes_for_snapshot_and_projection_queries() -> FriggResult<()> {
    let db_path = temp_db_path("hotpath-indexes");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    let conn = open_test_connection(&db_path)?;
    for index_name in [
        "idx_snapshot_repository_created_snapshot",
        "idx_test_subject_projection_repo_snapshot_test",
        "idx_test_subject_projection_repo_snapshot_subject",
        "idx_entrypoint_surface_projection_repo_snapshot_path",
        "idx_retrieval_projection_head_repo_snapshot_family",
        "idx_path_relation_projection_repo_snapshot_src",
        "idx_path_relation_projection_repo_snapshot_dst",
        "idx_subtree_coverage_projection_repo_snapshot_subtree",
        "idx_path_surface_term_projection_repo_snapshot_path",
        "idx_path_anchor_sketch_projection_repo_snapshot_path",
    ] {
        assert!(
            index_exists(&conn, index_name)?,
            "expected index '{index_name}' to exist"
        );
    }

    let snapshot_plan = explain_query_plan(
        &conn,
        r#"
            SELECT snapshot_id
            FROM snapshot
            WHERE repository_id = 'repo-1'
            ORDER BY created_at DESC, rowid DESC
            LIMIT 1
            "#,
    )?;
    assert!(
        snapshot_plan
            .iter()
            .any(|detail| detail.contains("idx_snapshot_repository_created_snapshot")),
        "expected snapshot latest lookup plan to use hotpath index, got {snapshot_plan:?}"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn current_schema_enforces_snapshot_repository_and_manifest_row_references() -> FriggResult<()> {
    let db_path = temp_db_path("fk-manifest-references");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    let conn = open_test_connection(&db_path)?;
    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|err| {
            FriggError::Internal(format!("failed to enable foreign key checks: {err}"))
        })?;

    let snapshot_repo_err = conn
        .execute(
            r#"
            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            VALUES ('snapshot-orphan', 'repo-missing', 'manifest', NULL, '2026-03-11T00:00:00Z')
            "#,
            [],
        )
        .expect_err("snapshot with missing repository should fail under FK constraint");
    assert!(snapshot_repo_err.to_string().contains("FOREIGN KEY"));

    conn.execute(
        "INSERT INTO repository (repository_id, root_path, display_name, created_at) VALUES ('repo-1', '/tmp/repo', 'repo-1', '2026-03-11T00:00:00Z')",
        [],
    )
    .map_err(|err| FriggError::Internal(format!("failed to seed manifest repository for test: {err}")))?;
    conn.execute(
        r#"
            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            VALUES ('snapshot-manifest', 'repo-1', 'manifest', NULL, '2026-03-11T00:00:00Z')
            "#,
        [],
    )
    .map_err(|err| {
        FriggError::Internal(format!("failed to seed manifest snapshot for test: {err}"))
    })?;
    conn.execute(
        r#"
            INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
            VALUES ('snapshot-manifest', 'src/main.rs', 'hash-main', 128, 12345)
            "#,
        [],
    )
    .map_err(|err| FriggError::Internal(format!("failed to seed manifest row for test: {err}")))?;

    let manifest_ref_err = conn
        .execute(
            r#"
            INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
            VALUES ('snapshot-missing', 'src/bad.rs', 'hash-bad', 10, 10)
            "#,
            [],
        )
        .expect_err("manifest rows for unknown snapshot should fail under FK constraint");
    assert!(manifest_ref_err.to_string().contains("FOREIGN KEY"));

    let manifest_row_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_manifest WHERE snapshot_id = 'snapshot-manifest'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count manifest rows before cascade assertion: {err}"
            ))
        })?;
    assert_eq!(manifest_row_count, 1);

    conn.execute(
        "DELETE FROM snapshot WHERE snapshot_id = 'snapshot-manifest'",
        [],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to delete snapshot for cascade assertion: {err}"
        ))
    })?;

    let manifest_row_count_after: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_manifest WHERE snapshot_id = 'snapshot-manifest'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count manifest rows after cascade assertion: {err}"
            ))
        })?;
    assert_eq!(manifest_row_count_after, 0);

    cleanup_db(&db_path);
    Ok(())
}
