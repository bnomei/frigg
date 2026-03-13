use super::support::*;

#[test]
fn manifest_upsert_and_load_for_snapshot_roundtrip() -> FriggResult<()> {
    let db_path = temp_db_path("manifest-upsert-load-snapshot");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[
            manifest_entry("src/zeta.rs", "hash-z", 40, Some(400)),
            manifest_entry("src/alpha.rs", "hash-a", 10, Some(100)),
            manifest_entry("src/beta.rs", "hash-b", 20, Some(200)),
        ],
    )?;

    let entries = storage.load_manifest_for_snapshot("snapshot-001")?;
    assert_eq!(
        entries,
        vec![
            manifest_entry("src/alpha.rs", "hash-a", 10, Some(100)),
            manifest_entry("src/beta.rs", "hash-b", 20, Some(200)),
            manifest_entry("src/zeta.rs", "hash-z", 40, Some(400)),
        ]
    );

    let latest = storage
        .load_latest_manifest_for_repository("repo-1")?
        .expect("expected manifest snapshot for repository");
    assert_eq!(latest.repository_id, "repo-1");
    assert_eq!(latest.snapshot_id, "snapshot-001");
    assert_eq!(latest.entries, entries);

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn manifest_upsert_replaces_existing_snapshot_rows() -> FriggResult<()> {
    let db_path = temp_db_path("manifest-upsert-replace");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[
            manifest_entry("src/alpha.rs", "hash-a1", 10, Some(100)),
            manifest_entry("src/beta.rs", "hash-b1", 20, Some(200)),
        ],
    )?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[
            manifest_entry("src/beta.rs", "hash-b2", 22, Some(220)),
            manifest_entry("src/gamma.rs", "hash-g2", 30, Some(300)),
        ],
    )?;

    let entries = storage.load_manifest_for_snapshot("snapshot-001")?;
    assert_eq!(
        entries,
        vec![
            manifest_entry("src/beta.rs", "hash-b2", 22, Some(220)),
            manifest_entry("src/gamma.rs", "hash-g2", 30, Some(300)),
        ]
    );

    let conn = open_test_connection(&db_path)?;
    let row_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM file_manifest WHERE snapshot_id = 'snapshot-001'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count manifest rows for replacement assertion: {err}"
            ))
        })?;
    assert_eq!(row_count, 2);

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn manifest_load_latest_for_repository_prefers_newer_snapshot() -> FriggResult<()> {
    let db_path = temp_db_path("manifest-load-latest");
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

    let latest = storage
        .load_latest_manifest_for_repository("repo-1")?
        .expect("expected latest manifest snapshot");
    assert_eq!(latest.snapshot_id, "snapshot-002");
    assert_eq!(
        latest.entries,
        vec![manifest_entry("src/alpha.rs", "hash-a2", 11, Some(110))]
    );
    assert!(
        storage
            .load_latest_manifest_for_repository("repo-missing")?
            .is_none(),
        "expected missing repository lookup to return None"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn manifest_load_latest_for_repository_breaks_timestamp_ties_by_insertion_order() -> FriggResult<()>
{
    let db_path = temp_db_path("manifest-load-latest-tie-break");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    let conn = open_test_connection(&db_path)?;
    let tied_created_at = "2026-03-05T00:00:00.000Z";
    conn.execute(
        r#"
            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            VALUES ('snapshot-001', 'repo-1', 'manifest', NULL, ?1)
            "#,
        [tied_created_at],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed first tied snapshot row for tie-break test: {err}"
        ))
    })?;
    conn.execute(
        r#"
            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            VALUES ('snapshot-002', 'repo-1', 'manifest', NULL, ?1)
            "#,
        [tied_created_at],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed second tied snapshot row for tie-break test: {err}"
        ))
    })?;
    conn.execute(
        r#"
            INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
            VALUES ('snapshot-001', 'src/alpha.rs', 'hash-a1', 10, 100)
            "#,
        [],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed first tied snapshot manifest row for tie-break test: {err}"
        ))
    })?;
    conn.execute(
        r#"
            INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
            VALUES ('snapshot-002', 'src/alpha.rs', 'hash-a2', 11, 110)
            "#,
        [],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed second tied snapshot manifest row for tie-break test: {err}"
        ))
    })?;

    let latest = storage
        .load_latest_manifest_for_repository("repo-1")?
        .expect("expected latest manifest snapshot");
    assert_eq!(latest.snapshot_id, "snapshot-002");
    assert_eq!(
        latest.entries,
        vec![manifest_entry("src/alpha.rs", "hash-a2", 11, Some(110))]
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn path_witness_projection_replace_and_load_roundtrip() -> FriggResult<()> {
    let db_path = temp_db_path("path-witness-projection-roundtrip");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.replace_path_witness_projections_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &[
            path_witness_projection_record(
                "repo-1",
                "snapshot-001",
                "src/main.rs",
                "runtime",
                "runtime",
                r#"["src","main","rs"]"#,
                r#"{"is_entrypoint":true}"#,
            ),
            path_witness_projection_record(
                "repo-1",
                "snapshot-001",
                "tests/cli.rs",
                "support",
                "tests",
                r#"["tests","cli","rs"]"#,
                r#"{"is_cli_test":true}"#,
            ),
        ],
    )?;

    let rows =
        storage.load_path_witness_projections_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert_eq!(
        rows,
        vec![
            path_witness_projection_record(
                "repo-1",
                "snapshot-001",
                "src/main.rs",
                "runtime",
                "runtime",
                r#"["src","main","rs"]"#,
                r#"{"is_entrypoint":true}"#,
            ),
            path_witness_projection_record(
                "repo-1",
                "snapshot-001",
                "tests/cli.rs",
                "support",
                "tests",
                r#"["tests","cli","rs"]"#,
                r#"{"is_cli_test":true}"#,
            ),
        ]
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn delete_snapshot_removes_path_witness_projection_rows() -> FriggResult<()> {
    let db_path = temp_db_path("path-witness-projection-delete-snapshot");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[manifest_entry("src/main.rs", "hash-main", 10, Some(100))],
    )?;
    storage.replace_path_witness_projections_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &[path_witness_projection_record(
            "repo-1",
            "snapshot-001",
            "src/main.rs",
            "runtime",
            "runtime",
            r#"["src","main","rs"]"#,
            r#"{"is_entrypoint":true}"#,
        )],
    )?;

    storage.delete_snapshot("snapshot-001")?;
    let rows =
        storage.load_path_witness_projections_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert!(rows.is_empty());

    cleanup_db(&db_path);
    Ok(())
}
