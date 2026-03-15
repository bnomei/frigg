use super::support::*;

#[test]
fn snapshot_and_provenance_pruning_keep_storage_bounded() -> FriggResult<()> {
    let db_path = temp_db_path("storage-prune-bounded");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    for idx in 1..=4 {
        storage.upsert_manifest(
            "repo-1",
            &format!("snapshot-00{idx}"),
            &[manifest_entry(
                "src/lib.rs",
                &format!("hash-{idx}"),
                10 + idx as u64,
                Some(100 + idx as u64),
            )],
        )?;
    }
    replace_semantic_records(
        &storage,
        "repo-1",
        "snapshot-002",
        &[semantic_record(
            "chunk-live",
            "repo-1",
            "snapshot-002",
            "src/lib.rs",
            "rust",
            0,
            1,
            10,
            "openai",
            "text-embedding-3-small",
            Some("trace-live"),
            "hash-live",
            "fn live() {}",
            &[0.1, 0.2],
        )],
    )?;

    for idx in 0..5 {
        storage.append_provenance_event(
            &format!("trace-{idx}"),
            "read_file",
            &json!({ "idx": idx }),
        )?;
    }
    let pruned_provenance = storage.prune_provenance_events(2)?;
    assert_eq!(pruned_provenance, 3);
    assert_eq!(storage.load_recent_provenance_events(10)?.len(), 2);

    let deleted_snapshots = storage.prune_repository_snapshots("repo-1", 1)?;
    assert_eq!(deleted_snapshots, 2);
    assert!(
        storage.load_manifest_for_snapshot("snapshot-002")?.len() == 1,
        "semantic-head snapshot should be protected from pruning"
    );
    assert!(
        storage.load_manifest_for_snapshot("snapshot-004")?.len() == 1,
        "latest retained snapshot should remain available"
    );
    assert!(
        storage
            .load_manifest_for_snapshot("snapshot-001")?
            .is_empty(),
        "oldest pruned snapshot should be removed"
    );
    assert!(
        storage
            .load_manifest_for_snapshot("snapshot-003")?
            .is_empty(),
        "non-protected stale snapshot should be removed"
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn snapshot_prune_keeps_non_manifest_rows_out_of_manifest_retention() -> FriggResult<()> {
    let db_path = temp_db_path("storage-prune-manifest-vs-non-manifest");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-manifest-old",
        &[manifest_entry(
            "src/lib.rs",
            "hash-manifest-old",
            101,
            Some(1001),
        )],
    )?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-manifest-new",
        &[manifest_entry(
            "src/lib.rs",
            "hash-manifest-new",
            102,
            Some(1002),
        )],
    )?;

    let conn = open_test_connection(&db_path)?;
    conn.execute(
        "INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at) VALUES (?1, ?2, 'semantic', NULL, ?3)",
        ("snapshot-semantic-only", "repo-1", "2026-01-03T00:00:00Z"),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed non-manifest snapshot for retention test: {err}"
        ))
    })?;

    let deleted = storage.prune_repository_snapshots("repo-1", 1)?;
    assert_eq!(deleted, 1);

    let manifest_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM snapshot WHERE repository_id = ?1 AND kind = 'manifest'",
            ["repo-1"],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count manifest snapshots after prune in test: {err}"
            ))
        })?;
    let non_manifest_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM snapshot WHERE repository_id = ?1 AND kind = 'semantic'",
            ["repo-1"],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count non-manifest snapshots after prune in test: {err}"
            ))
        })?;

    assert_eq!(manifest_count, 1);
    assert_eq!(non_manifest_count, 1);
    assert!(
        storage.load_manifest_for_snapshot("snapshot-manifest-old")?.is_empty(),
        "oldest manifest snapshot should be pruned by manifest retention"
    );
    assert!(
        storage
            .load_manifest_for_snapshot("snapshot-manifest-new")?
            .len()
            == 1,
        "latest manifest snapshot should remain after manifest retention"
    );
    assert_eq!(
        conn.query_row(
            "SELECT COUNT(*) FROM snapshot WHERE snapshot_id = ?1",
            ["snapshot-semantic-only"],
            |row| row.get::<_, i64>(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to assert non-manifest snapshot retained during manifest prune: {err}"
            ))
        })?,
        1
    );

    cleanup_db(&db_path);
    Ok(())
}
