use super::support::*;

#[test]
fn manifest_diff_classifies_added_modified_deleted_in_path_order() {
    let old = vec![
        digest("repo/zeta.rs", 10, Some(10), "hash-z"),
        digest("repo/alpha.rs", 1, Some(1), "hash-a"),
        digest("repo/charlie.rs", 3, Some(3), "hash-c-old"),
    ];
    let new = vec![
        digest("repo/bravo.rs", 2, Some(2), "hash-b"),
        digest("repo/charlie.rs", 4, Some(4), "hash-c-new"),
        digest("repo/zeta.rs", 10, Some(10), "hash-z"),
    ];

    let manifest_diff = diff(&old, &new);

    assert_eq!(
        manifest_diff.added,
        vec![digest("repo/bravo.rs", 2, Some(2), "hash-b")]
    );
    assert_eq!(
        manifest_diff.modified,
        vec![digest("repo/charlie.rs", 4, Some(4), "hash-c-new")]
    );
    assert_eq!(
        manifest_diff.deleted,
        vec![digest("repo/alpha.rs", 1, Some(1), "hash-a")]
    );
}

#[test]
fn navigation_symbol_target_rank_is_stable_and_precedence_ordered() {
    let symbol = SymbolDefinition {
        stable_id: "sym-user-001".to_owned(),
        language: SymbolLanguage::Rust,
        kind: SymbolKind::Struct,
        name: "User".to_owned(),
        path: PathBuf::from("src/lib.rs"),
        line: 1,
        span: SourceSpan {
            start_byte: 0,
            end_byte: 10,
            start_line: 1,
            start_column: 1,
            end_line: 1,
            end_column: 10,
        },
    };

    assert_eq!(
        navigation_symbol_target_rank(&symbol, "sym-user-001"),
        Some(0)
    );
    assert_eq!(navigation_symbol_target_rank(&symbol, "User"), Some(1));
    assert_eq!(navigation_symbol_target_rank(&symbol, "user"), Some(2));
    assert_eq!(navigation_symbol_target_rank(&symbol, "Account"), None);
}

#[test]
fn manifest_diff_detects_mtime_only_change_as_modified() {
    let old = vec![digest("repo/file.rs", 10, Some(100), "same-hash")];
    let new = vec![digest("repo/file.rs", 10, Some(200), "same-hash")];

    let manifest_diff = diff(&old, &new);

    assert!(manifest_diff.added.is_empty());
    assert_eq!(
        manifest_diff.modified,
        vec![digest("repo/file.rs", 10, Some(200), "same-hash")]
    );
    assert!(manifest_diff.deleted.is_empty());
}

#[test]
fn manifest_diff_is_empty_for_identical_records_with_different_input_order() {
    let old = vec![
        digest("repo/b.rs", 2, Some(2), "hash-b"),
        digest("repo/a.rs", 1, Some(1), "hash-a"),
        digest("repo/c.rs", 3, Some(3), "hash-c"),
    ];
    let new = vec![
        digest("repo/c.rs", 3, Some(3), "hash-c"),
        digest("repo/a.rs", 1, Some(1), "hash-a"),
        digest("repo/b.rs", 2, Some(2), "hash-b"),
    ];

    let manifest_diff = diff(&old, &new);

    assert!(manifest_diff.added.is_empty());
    assert!(manifest_diff.modified.is_empty());
    assert!(manifest_diff.deleted.is_empty());
}

#[test]
fn determinism_manifest_builder_repeated_runs_match_exactly() -> FriggResult<()> {
    let fixture_root = fixture_repo_root();
    let builder = ManifestBuilder::default();

    let first = builder.build(&fixture_root)?;
    let second = builder.build(&fixture_root)?;
    let third = builder.build(&fixture_root)?;

    assert_eq!(first, second);
    assert_eq!(second, third);
    Ok(())
}

#[test]
fn determinism_manifest_builder_uses_fixture_only_expected_paths() -> FriggResult<()> {
    let fixture_root = fixture_repo_root();
    let builder = ManifestBuilder::default();

    let manifest = builder.build(&fixture_root)?;
    let relative_paths = manifest_relative_paths(&manifest, &fixture_root)?;

    assert_eq!(
        relative_paths,
        vec![
            PathBuf::from("README.md"),
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/nested/data.txt"),
        ]
    );
    Ok(())
}

#[test]
fn manifest_builder_respects_gitignored_contract_artifacts() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("manifest-builder-gitignored-contracts");
    prepare_workspace(
        &workspace_root,
        &[
            ("contracts/errors.md", "invalid_params\n"),
            ("src/main.rs", "fn main() {}\n"),
        ],
    )?;
    fs::write(workspace_root.join(".gitignore"), "contracts\n").map_err(FriggError::Io)?;

    let manifest = ManifestBuilder::default().build(&workspace_root)?;
    let relative_paths = manifest_relative_paths(&manifest, &workspace_root)?;

    assert!(
        !relative_paths.contains(&PathBuf::from("contracts/errors.md")),
        "manifest discovery should respect gitignored contract artifacts"
    );

    Ok(())
}

#[test]
fn manifest_builder_excludes_target_artifacts_without_gitignore() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("manifest-builder-target-exclusion");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "fn main() {}\n"),
            ("target/debug/app", "binary\n"),
        ],
    )?;

    let manifest = ManifestBuilder::default().build(&workspace_root)?;
    let relative_paths = manifest_relative_paths(&manifest, &workspace_root)?;

    assert!(
        !relative_paths
            .iter()
            .any(|path| path.starts_with(Path::new("target"))),
        "target artifacts must stay excluded from manifest discovery: {relative_paths:?}"
    );

    Ok(())
}

#[test]
fn manifest_builder_respects_root_ignore_file_for_auxiliary_trees() -> FriggResult<()> {
    let workspace_root = temp_workspace_root("manifest-builder-root-ignore");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/main.rs", "fn main() {}\n"),
            ("auxiliary/embedded-repo/src/lib.rs", "pub fn leaked() {}\n"),
        ],
    )?;
    fs::write(workspace_root.join(".ignore"), "auxiliary/\n").map_err(FriggError::Io)?;

    let manifest = ManifestBuilder::default().build(&workspace_root)?;
    let relative_paths = manifest_relative_paths(&manifest, &workspace_root)?;

    assert!(
        !relative_paths
            .iter()
            .any(|path| path.starts_with(Path::new("auxiliary"))),
        "root ignore files must exclude auxiliary trees from manifest discovery: {relative_paths:?}"
    );

    Ok(())
}

#[test]
fn incremental_roundtrip_persist_load_and_diff() -> FriggResult<()> {
    let db_path = temp_db_path("incremental-roundtrip");
    let fixture_root = fixture_repo_root();
    let manifest_store = ManifestStore::new(&db_path);
    manifest_store.initialize()?;

    let repository_id = "repo-001";
    let snapshot_old = "snapshot-001";
    let snapshot_new = "snapshot-002";
    let builder = ManifestBuilder::default();
    let old_manifest = builder.build(&fixture_root)?;

    manifest_store.persist_snapshot_manifest(repository_id, snapshot_old, &old_manifest)?;
    let loaded_old = manifest_store.load_snapshot_manifest(snapshot_old)?;
    assert_eq!(loaded_old, old_manifest);

    let latest_before = manifest_store
        .load_latest_manifest_for_repository(repository_id)?
        .expect("expected latest repository manifest");
    assert_eq!(latest_before.snapshot_id, snapshot_old);
    assert_eq!(latest_before.entries, old_manifest);

    let new_manifest = mutate_manifest_for_incremental_roundtrip(&old_manifest, &fixture_root)?;
    manifest_store.persist_snapshot_manifest(repository_id, snapshot_new, &new_manifest)?;

    let loaded_new = manifest_store.load_snapshot_manifest(snapshot_new)?;
    assert_eq!(loaded_new, new_manifest);

    let latest_after = manifest_store
        .load_latest_manifest_for_repository(repository_id)?
        .expect("expected latest repository manifest after second snapshot");
    assert_eq!(latest_after.snapshot_id, snapshot_new);
    assert_eq!(latest_after.entries, new_manifest);

    let manifest_diff = diff(&latest_before.entries, &latest_after.entries);
    assert_eq!(manifest_diff.added.len(), 1);
    assert_eq!(manifest_diff.modified.len(), 1);
    assert_eq!(manifest_diff.deleted.len(), 1);
    assert_eq!(
        manifest_diff.added[0].path,
        fixture_root.join("src/incremental-new.rs")
    );
    assert_eq!(
        manifest_diff.modified[0].path,
        fixture_root.join("README.md")
    );
    assert_eq!(
        manifest_diff.deleted[0].path,
        fixture_root.join("src/nested/data.txt")
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn incremental_roundtrip_changed_only_reports_zero_for_unchanged_workspace() -> FriggResult<()> {
    let db_path = temp_db_path("incremental-unchanged-db");
    let workspace_root = temp_workspace_root("incremental-unchanged-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
    )?;

    let full_summary =
        reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;
    let changed_summary = reindex_repository(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
    )?;

    assert_eq!(full_summary.files_scanned, 2);
    assert_eq!(full_summary.files_changed, 2);
    assert_eq!(full_summary.files_deleted, 0);
    assert_eq!(full_summary.diagnostics.total_count(), 0);
    assert_eq!(changed_summary.files_scanned, 2);
    assert_eq!(changed_summary.files_changed, 0);
    assert_eq!(changed_summary.files_deleted, 0);
    assert_eq!(changed_summary.diagnostics.total_count(), 0);
    assert_eq!(changed_summary.snapshot_id, full_summary.snapshot_id);

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn incremental_roundtrip_changed_only_detects_modified_added_and_deleted_files() -> FriggResult<()>
{
    let db_path = temp_db_path("incremental-changed-db");
    let workspace_root = temp_workspace_root("incremental-changed-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
    )?;

    let full_summary =
        reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;

    fs::write(workspace_root.join("README.md"), "hello changed\n").map_err(FriggError::Io)?;
    fs::remove_file(workspace_root.join("src/main.rs")).map_err(FriggError::Io)?;
    fs::write(workspace_root.join("src/new.rs"), "pub fn added() {}\n").map_err(FriggError::Io)?;

    let changed_summary = reindex_repository(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
    )?;

    assert_eq!(changed_summary.files_scanned, 2);
    assert_eq!(changed_summary.files_changed, 2);
    assert_eq!(changed_summary.files_deleted, 1);
    assert_eq!(changed_summary.diagnostics.total_count(), 0);
    assert_ne!(changed_summary.snapshot_id, full_summary.snapshot_id);

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}
