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
