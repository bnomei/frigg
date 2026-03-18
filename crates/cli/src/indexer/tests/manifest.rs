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
fn reindex_materializes_authoritative_retrieval_projection_heads() -> FriggResult<()> {
    let db_path = temp_db_path("reindex-materializes-retrieval-projections");
    let workspace_root = temp_workspace_root("reindex-materializes-retrieval-projections");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.rs", "fn main() {}\n"),
            ("tests/main_test.rs", "#[test]\nfn main_test() {}\n"),
        ],
    )?;

    let semantic_runtime = SemanticRuntimeConfig {
        enabled: false,
        provider: None,
        model: None,
        strict_mode: false,
    };
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: None,
        gemini_api_key: None,
    };
    let summary = reindex_repository_with_runtime_config(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
    )?;

    let storage = Storage::new(&db_path);
    for family in [
        "path_witness",
        "test_subject",
        "entrypoint_surface",
        "subtree_coverage",
    ] {
        let head = storage
            .load_retrieval_projection_head_for_repository_snapshot_family(
                "repo-001",
                &summary.snapshot_id,
                family,
            )?
            .unwrap_or_else(|| panic!("expected retrieval projection head for family '{family}'"));
        assert_eq!(head.heuristic_version, 1);
        assert_eq!(head.input_modes, vec!["path".to_owned()]);
    }
    let path_relation_head = storage
        .load_retrieval_projection_head_for_repository_snapshot_family(
            "repo-001",
            &summary.snapshot_id,
            "path_relation",
        )?
        .expect("expected path relation head");
    assert_eq!(path_relation_head.heuristic_version, 1);
    assert_eq!(path_relation_head.input_modes, vec!["path".to_owned()]);

    let path_surface_term_head = storage
        .load_retrieval_projection_head_for_repository_snapshot_family(
            "repo-001",
            &summary.snapshot_id,
            "path_surface_term",
        )?
        .expect("expected path surface term head");
    assert_eq!(path_surface_term_head.heuristic_version, 1);
    assert_eq!(
        path_surface_term_head.input_modes,
        vec!["ast".to_owned(), "path".to_owned()]
    );

    let path_anchor_sketch_head = storage
        .load_retrieval_projection_head_for_repository_snapshot_family(
            "repo-001",
            &summary.snapshot_id,
            "path_anchor_sketch",
        )?
        .expect("expected path anchor sketch head");
    assert_eq!(path_anchor_sketch_head.heuristic_version, 1);
    assert_eq!(
        path_anchor_sketch_head.input_modes,
        vec!["ast".to_owned(), "path".to_owned()]
    );
    assert!(
        !storage
            .load_path_witness_projections_for_repository_snapshot(
                "repo-001",
                &summary.snapshot_id
            )?
            .is_empty()
    );
    assert!(
        !storage
            .load_path_relation_projections_for_repository_snapshot(
                "repo-001",
                &summary.snapshot_id
            )?
            .is_empty()
    );
    assert!(
        !storage
            .load_path_surface_term_projections_for_repository_snapshot(
                "repo-001",
                &summary.snapshot_id,
            )?
            .is_empty()
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn reindex_materializes_retrieval_projection_heads_with_scip_inputs() -> FriggResult<()> {
    let db_path = temp_db_path("reindex-materializes-retrieval-projections-scip");
    let workspace_root = temp_workspace_root("reindex-materializes-retrieval-projections-scip");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/a.rs",
                "pub struct User;\nimpl User { pub fn helper() {} }\n",
            ),
            ("src/base.rs", "pub struct Entity;\n"),
            (
                ".frigg/scip/fixture.json",
                "{\n  \"documents\": [\n    {\n      \"relative_path\": \"src/a.rs\",\n      \"occurrences\": [\n        { \"symbol\": \"scip-rust pkg repo#User\", \"range\": [0, 11, 15], \"symbol_roles\": 1 }\n      ],\n      \"symbols\": [\n        {\n          \"symbol\": \"scip-rust pkg repo#User\",\n          \"display_name\": \"User\",\n          \"kind\": \"struct\",\n          \"relationships\": [\n            { \"symbol\": \"scip-rust pkg repo#Entity\", \"is_reference\": true },\n            { \"symbol\": \"scip-rust pkg repo#Entity\", \"is_implementation\": true }\n          ]\n        }\n      ]\n    },\n    {\n      \"relative_path\": \"src/base.rs\",\n      \"occurrences\": [\n        { \"symbol\": \"scip-rust pkg repo#Entity\", \"range\": [0, 11, 17], \"symbol_roles\": 1 }\n      ],\n      \"symbols\": [\n        { \"symbol\": \"scip-rust pkg repo#Entity\", \"display_name\": \"Entity\", \"kind\": \"struct\", \"relationships\": [] }\n      ]\n    }\n  ]\n}\n",
            ),
        ],
    )?;

    let semantic_runtime = SemanticRuntimeConfig {
        enabled: false,
        provider: None,
        model: None,
        strict_mode: false,
    };
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: None,
        gemini_api_key: None,
    };
    let summary = reindex_repository_with_runtime_config(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
    )?;

    let storage = Storage::new(&db_path);
    let path_relation_head = storage
        .load_retrieval_projection_head_for_repository_snapshot_family(
            "repo-001",
            &summary.snapshot_id,
            "path_relation",
        )?
        .expect("expected path relation head");
    assert_eq!(
        path_relation_head.input_modes,
        vec!["path".to_owned(), "scip".to_owned()]
    );

    let path_surface_term_head = storage
        .load_retrieval_projection_head_for_repository_snapshot_family(
            "repo-001",
            &summary.snapshot_id,
            "path_surface_term",
        )?
        .expect("expected path surface term head");
    assert_eq!(
        path_surface_term_head.input_modes,
        vec!["ast".to_owned(), "path".to_owned(), "scip".to_owned()]
    );

    let path_anchor_sketch_head = storage
        .load_retrieval_projection_head_for_repository_snapshot_family(
            "repo-001",
            &summary.snapshot_id,
            "path_anchor_sketch",
        )?
        .expect("expected path anchor sketch head");
    assert_eq!(
        path_anchor_sketch_head.input_modes,
        vec!["ast".to_owned(), "path".to_owned(), "scip".to_owned()]
    );

    let path_relations = storage
        .load_path_relation_projections_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
    assert!(
        path_relations
            .iter()
            .any(|relation| relation.evidence_source == "scip"),
        "expected at least one SCIP-backed relation row: {path_relations:?}"
    );

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn reindex_changed_only_repairs_missing_retrieval_projection_family_on_reused_snapshot()
-> FriggResult<()> {
    let db_path = temp_db_path("reindex-repairs-missing-retrieval-projection-family");
    let workspace_root = temp_workspace_root("reindex-repairs-missing-retrieval-projection-family");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
            ),
            ("src/main.rs", "fn main() {}\n"),
            ("tests/main_test.rs", "#[test]\nfn main_test() {}\n"),
        ],
    )?;

    let semantic_runtime = SemanticRuntimeConfig {
        enabled: false,
        provider: None,
        model: None,
        strict_mode: false,
    };
    let credentials = SemanticRuntimeCredentials {
        openai_api_key: None,
        gemini_api_key: None,
    };
    let initial_summary = reindex_repository_with_runtime_config(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &semantic_runtime,
        &credentials,
    )?;

    let storage = Storage::new(&db_path);
    assert!(
        storage
            .missing_retrieval_projection_families_for_repository_snapshot(
                "repo-001",
                &initial_summary.snapshot_id,
            )?
            .is_empty()
    );
    assert!(
        !storage
            .load_path_relation_projections_for_repository_snapshot(
                "repo-001",
                &initial_summary.snapshot_id,
            )?
            .is_empty()
    );

    let conn = rusqlite::Connection::open(&db_path).map_err(|err| {
        FriggError::Internal(format!(
            "failed to open test db for retrieval projection family deletion: {err}"
        ))
    })?;
    conn.execute(
        "DELETE FROM retrieval_projection_head WHERE repository_id = ?1 AND snapshot_id = ?2 AND family = 'path_relation'",
        ("repo-001", initial_summary.snapshot_id.as_str()),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to delete path_relation head for reused snapshot repair test: {err}"
        ))
    })?;
    conn.execute(
        "DELETE FROM path_relation_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
        ("repo-001", initial_summary.snapshot_id.as_str()),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to delete path_relation rows for reused snapshot repair test: {err}"
        ))
    })?;

    assert_eq!(
        storage.missing_retrieval_projection_families_for_repository_snapshot(
            "repo-001",
            &initial_summary.snapshot_id,
        )?,
        vec!["path_relation".to_owned()]
    );
    assert!(
        storage
            .load_path_relation_projections_for_repository_snapshot(
                "repo-001",
                &initial_summary.snapshot_id,
            )?
            .is_empty()
    );

    let changed_summary = reindex_repository_with_runtime_config(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
        &semantic_runtime,
        &credentials,
    )?;
    assert_eq!(changed_summary.snapshot_id, initial_summary.snapshot_id);
    assert_eq!(changed_summary.files_changed, 0);
    assert!(
        storage
            .missing_retrieval_projection_families_for_repository_snapshot(
                "repo-001",
                &changed_summary.snapshot_id,
            )?
            .is_empty()
    );
    assert!(
        !storage
            .load_path_relation_projections_for_repository_snapshot(
                "repo-001",
                &changed_summary.snapshot_id,
            )?
            .is_empty()
    );

    cleanup_workspace(&workspace_root);
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

#[test]
fn reindex_plan_changed_only_unchanged_workspace_reuses_existing_snapshot() -> FriggResult<()> {
    let db_path = temp_db_path("reindex-plan-unchanged-db");
    let workspace_root = temp_workspace_root("reindex-plan-unchanged-workspace");
    prepare_workspace(
        &workspace_root,
        &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
    )?;

    let full_summary =
        reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;
    let plan = build_reindex_plan_for_tests(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::ChangedOnly,
        &SemanticRuntimeConfig::default(),
        &[],
    )?;

    assert_eq!(
        plan.previous_snapshot_id.as_deref(),
        Some(full_summary.snapshot_id.as_str())
    );
    assert_eq!(plan.files_changed, 0);
    assert_eq!(plan.files_deleted, 0);
    assert!(matches!(
        &plan.snapshot_plan,
        super::super::ManifestSnapshotPlan::ReuseExisting { snapshot_id }
            if snapshot_id == &full_summary.snapshot_id
    ));
    assert_eq!(plan.semantic_refresh.mode, SemanticRefreshMode::Disabled);

    cleanup_workspace(&workspace_root);
    cleanup_db(&db_path);
    Ok(())
}
