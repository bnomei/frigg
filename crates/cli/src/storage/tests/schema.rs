use super::support::*;

#[test]
fn initialize_applies_base_schema_and_version() -> FriggResult<()> {
    let db_path = temp_db_path("init-base-schema");
    let storage = Storage::new(&db_path);
    let expected_schema_version = MIGRATIONS
        .last()
        .expect("storage migrations should not be empty")
        .version;

    storage.initialize()?;

    assert_eq!(storage.schema_version()?, expected_schema_version);

    let conn = open_test_connection(&db_path)?;
    for table in [
        "schema_version",
        "repository",
        "snapshot",
        "file_manifest",
        "provenance_event",
        "semantic_chunk",
        "semantic_chunk_embedding",
        "path_witness_projection",
        "test_subject_projection",
        "entrypoint_surface_projection",
    ] {
        assert!(
            table_exists(&conn, table)?,
            "expected table '{table}' to exist"
        );
    }

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn initialize_is_idempotent() -> FriggResult<()> {
    let db_path = temp_db_path("init-idempotent");
    let storage = Storage::new(&db_path);
    let expected_schema_version = MIGRATIONS
        .last()
        .expect("storage migrations should not be empty")
        .version;

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

    assert_eq!(storage.schema_version()?, expected_schema_version);

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
fn verify_succeeds_after_initialize() -> FriggResult<()> {
    let db_path = temp_db_path("verify-success");
    let storage = Storage::new(&db_path);

    storage.initialize()?;
    storage.verify()?;

    cleanup_db(&db_path);
    Ok(())
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
fn provenance_trace_ids_are_uuid_v7_and_unique() {
    let first = Storage::new_provenance_trace_id("search_symbol");
    let second = Storage::new_provenance_trace_id("search_symbol");

    assert_ne!(first, second, "trace ids must be unique");
    assert_eq!(first.len(), 36, "uuid trace ids should use canonical form");
    assert_eq!(second.len(), 36, "uuid trace ids should use canonical form");
    assert_eq!(
        first.as_bytes().get(14),
        Some(&b'7'),
        "expected UUIDv7 version nibble in first trace id"
    );
    assert_eq!(
        second.as_bytes().get(14),
        Some(&b'7'),
        "expected UUIDv7 version nibble in second trace id"
    );
}

#[test]
fn initialize_creates_hotpath_indexes_for_snapshot_and_provenance_queries() -> FriggResult<()> {
    let db_path = temp_db_path("hotpath-indexes");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    let conn = open_test_connection(&db_path)?;
    for index_name in [
        "idx_snapshot_repository_created_snapshot",
        "idx_provenance_tool_created_trace",
        "idx_test_subject_projection_repo_snapshot_test",
        "idx_test_subject_projection_repo_snapshot_subject",
        "idx_entrypoint_surface_projection_repo_snapshot_path",
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

    let provenance_plan = explain_query_plan(
        &conn,
        r#"
            SELECT trace_id, tool_name, payload_json, created_at
            FROM provenance_event
            WHERE tool_name = 'read_file'
            ORDER BY created_at DESC, rowid DESC
            LIMIT 10
            "#,
    )?;
    assert!(
        provenance_plan
            .iter()
            .any(|detail| detail.contains("idx_provenance_tool_created_trace")),
        "expected provenance tool lookup plan to use hotpath index, got {provenance_plan:?}"
    );

    cleanup_db(&db_path);
    Ok(())
}
