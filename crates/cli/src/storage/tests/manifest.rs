use super::support::*;
use std::collections::BTreeMap;

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
fn manifest_load_latest_for_repository_ignores_non_manifest_rows() -> FriggResult<()> {
    let db_path = temp_db_path("manifest-load-latest-ignores-non-manifest");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-manifest-old",
        &[manifest_entry("src/alpha.rs", "hash-a1", 10, Some(100))],
    )?;
    let conn = open_test_connection(&db_path)?;
    conn.execute(
        "INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at) VALUES ('snapshot-semantic-latest', 'repo-1', 'semantic', NULL, '2099-01-01T00:00:00Z')",
        [],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed semantic snapshot for mixed-kind lookup test: {err}"
        ))
    })?;

    let latest = storage
        .load_latest_manifest_for_repository("repo-1")?
        .expect("expected latest manifest snapshot");
    assert_eq!(latest.snapshot_id, "snapshot-manifest-old");

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
            INSERT INTO repository (repository_id, root_path, display_name, created_at)
            VALUES ('repo-1', '/repo-1', 'repo-1', ?1)
            "#,
        [tied_created_at],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed repository row for tie-break test: {err}"
        ))
    })?;
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
fn manifest_upsert_creates_stub_repository_row_to_match_fk_schema() -> FriggResult<()> {
    let db_path = temp_db_path("manifest-upsert-seeds-repository");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-orphan",
        "snapshot-001",
        &[manifest_entry("src/main.rs", "hash-main", 10, Some(100))],
    )?;

    let conn = open_test_connection(&db_path)?;
    let repository_rows: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM repository WHERE repository_id = 'repo-orphan'",
            [],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count repository rows for manifest-upsert seed assertion: {err}"
            ))
        })?;
    assert_eq!(repository_rows, 1);

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn manifest_latest_load_prefers_manifest_kind_when_non_manifest_rows_are_present() -> FriggResult<()>
{
    let db_path = temp_db_path("manifest-latest-manifest-kind-only");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-manifest-old",
        &[manifest_entry("src/main.rs", "hash-old", 10, Some(100))],
    )?;

    let conn = open_test_connection(&db_path)?;
    conn.execute(
        "INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at) VALUES ('snapshot-non-manifest', 'repo-1', 'semantic', NULL, '2026-03-12T00:00:00Z')",
        [],
    )
    .map_err(|err| FriggError::Internal(format!("failed to seed non-manifest snapshot for test: {err}")))?;
    conn.execute(
        "INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns) VALUES ('snapshot-non-manifest', 'src/other.rs', 'hash-other', 20, 200)",
        [],
    )
    .map_err(|err| FriggError::Internal(format!("failed to seed non-manifest manifest rows for test: {err}")))?;

    let latest = storage
        .load_latest_manifest_for_repository("repo-1")?
        .expect("expected manifest latest snapshot");
    assert_eq!(latest.snapshot_id, "snapshot-manifest-old");

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn path_witness_projection_replace_and_load_roundtrip() -> FriggResult<()> {
    let db_path = temp_db_path("path-witness-projection-roundtrip");
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

#[test]
fn test_subject_projection_replace_and_load_roundtrip() -> FriggResult<()> {
    let db_path = temp_db_path("test-subject-projection-roundtrip");
    let storage = Storage::new(&db_path);
    storage.initialize()?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[manifest_entry(
            "src/user_service.rs",
            "hash-user-service",
            10,
            Some(100),
        )],
    )?;

    storage.replace_test_subject_projections_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &[
            test_subject_projection_record(
                "repo-1",
                "snapshot-001",
                "tests/unit/user_service_test.rs",
                "src/user_service.rs",
                r#"["user","service"]"#,
                19,
                r#"{"exact_stem_match":true}"#,
            ),
            test_subject_projection_record(
                "repo-1",
                "snapshot-001",
                "tests/integration/auth_spec.py",
                "src/auth.py",
                r#"["auth"]"#,
                12,
                r#"{"same_language":true}"#,
            ),
        ],
    )?;

    let rows =
        storage.load_test_subject_projections_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert_eq!(
        rows,
        vec![
            test_subject_projection_record(
                "repo-1",
                "snapshot-001",
                "tests/integration/auth_spec.py",
                "src/auth.py",
                r#"["auth"]"#,
                12,
                r#"{"same_language":true}"#,
            ),
            test_subject_projection_record(
                "repo-1",
                "snapshot-001",
                "tests/unit/user_service_test.rs",
                "src/user_service.rs",
                r#"["user","service"]"#,
                19,
                r#"{"exact_stem_match":true}"#,
            ),
        ]
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn entrypoint_surface_projection_replace_and_load_roundtrip() -> FriggResult<()> {
    let db_path = temp_db_path("entrypoint-surface-projection-roundtrip");
    let storage = Storage::new(&db_path);
    storage.initialize()?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[manifest_entry("src/main.rs", "hash-main", 10, Some(100))],
    )?;

    storage.replace_entrypoint_surface_projections_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &[
            entrypoint_surface_projection_record(
                "repo-1",
                "snapshot-001",
                ".github/workflows/ci.yml",
                "project",
                "project",
                r#"["ci","workflow"]"#,
                r#"["automation","workflow"]"#,
                r#"{"is_ci_workflow":true}"#,
            ),
            entrypoint_surface_projection_record(
                "repo-1",
                "snapshot-001",
                "src/main.rs",
                "runtime",
                "runtime",
                r#"["main"]"#,
                r#"["entrypoint","runtime"]"#,
                r#"{"is_runtime_entrypoint":true}"#,
            ),
        ],
    )?;

    let rows = storage
        .load_entrypoint_surface_projections_for_repository_snapshot("repo-1", "snapshot-001")?;
    assert_eq!(
        rows,
        vec![
            entrypoint_surface_projection_record(
                "repo-1",
                "snapshot-001",
                ".github/workflows/ci.yml",
                "project",
                "project",
                r#"["ci","workflow"]"#,
                r#"["automation","workflow"]"#,
                r#"{"is_ci_workflow":true}"#,
            ),
            entrypoint_surface_projection_record(
                "repo-1",
                "snapshot-001",
                "src/main.rs",
                "runtime",
                "runtime",
                r#"["main"]"#,
                r#"["entrypoint","runtime"]"#,
                r#"{"is_runtime_entrypoint":true}"#,
            ),
        ]
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn delete_snapshot_removes_overlay_projection_rows() -> FriggResult<()> {
    let db_path = temp_db_path("overlay-projection-delete-snapshot");
    let storage = Storage::new(&db_path);
    storage.initialize()?;

    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[manifest_entry("src/main.rs", "hash-main", 10, Some(100))],
    )?;
    storage.replace_test_subject_projections_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &[test_subject_projection_record(
            "repo-1",
            "snapshot-001",
            "tests/unit/user_service_test.rs",
            "src/user_service.rs",
            r#"["user","service"]"#,
            19,
            r#"{"exact_stem_match":true}"#,
        )],
    )?;
    storage.replace_entrypoint_surface_projections_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &[entrypoint_surface_projection_record(
            "repo-1",
            "snapshot-001",
            "src/main.rs",
            "runtime",
            "runtime",
            r#"["main"]"#,
            r#"["entrypoint","runtime"]"#,
            r#"{"is_runtime_entrypoint":true}"#,
        )],
    )?;

    storage.delete_snapshot("snapshot-001")?;

    assert!(
        storage
            .load_test_subject_projections_for_repository_snapshot("repo-1", "snapshot-001")?
            .is_empty()
    );
    assert!(
        storage
            .load_entrypoint_surface_projections_for_repository_snapshot("repo-1", "snapshot-001")?
            .is_empty()
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn retrieval_projection_bundle_replace_and_load_roundtrip() -> FriggResult<()> {
    let db_path = temp_db_path("retrieval-projection-bundle-roundtrip");
    let storage = Storage::new(&db_path);
    storage.initialize()?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[
            manifest_entry("src/main.rs", "hash-main", 10, Some(100)),
            manifest_entry("Cargo.toml", "hash-cargo", 10, Some(100)),
        ],
    )?;

    let bundle = RetrievalProjectionBundle {
        heads: vec![
            RetrievalProjectionHeadRecord {
                family: "path_witness".to_owned(),
                heuristic_version: 1,
                input_modes: vec!["path".to_owned()],
                row_count: 1,
            },
            RetrievalProjectionHeadRecord {
                family: "path_relation".to_owned(),
                heuristic_version: 1,
                input_modes: vec!["path".to_owned()],
                row_count: 1,
            },
            RetrievalProjectionHeadRecord {
                family: "subtree_coverage".to_owned(),
                heuristic_version: 1,
                input_modes: vec!["path".to_owned()],
                row_count: 1,
            },
            RetrievalProjectionHeadRecord {
                family: "path_surface_term".to_owned(),
                heuristic_version: 1,
                input_modes: vec!["path".to_owned()],
                row_count: 1,
            },
            RetrievalProjectionHeadRecord {
                family: "path_anchor_sketch".to_owned(),
                heuristic_version: 1,
                input_modes: vec!["path".to_owned()],
                row_count: 1,
            },
        ],
        path_witness: vec![path_witness_projection_record(
            "repo-1",
            "snapshot-001",
            "src/main.rs",
            "runtime",
            "runtime",
            r#"["src","main","rs"]"#,
            r#"{"is_entrypoint_runtime":true}"#,
        )],
        test_subject: Vec::new(),
        entrypoint_surface: Vec::new(),
        path_relations: vec![PathRelationProjection {
            src_path: "src/main.rs".to_owned(),
            dst_path: "Cargo.toml".to_owned(),
            relation_kind: "entrypoint_package".to_owned(),
            evidence_source: "path".to_owned(),
            src_symbol_id: None,
            dst_symbol_id: None,
            src_family_bits: 1,
            dst_family_bits: 4,
            shared_terms: vec!["main".to_owned()],
            score_hint: 110,
        }],
        subtree_coverage: vec![SubtreeCoverageProjection {
            subtree_root: "src".to_owned(),
            family: "runtime".to_owned(),
            path_count: 1,
            exemplar_path: "src/main.rs".to_owned(),
            exemplar_score_hint: 24,
        }],
        path_surface_terms: vec![PathSurfaceTermProjection {
            path: "src/main.rs".to_owned(),
            term_weights: BTreeMap::from([("main".to_owned(), 4), ("entrypoint".to_owned(), 2)]),
            exact_terms: vec!["main".to_owned(), "entrypoint".to_owned()],
        }],
        path_anchor_sketches: vec![PathAnchorSketchProjection {
            path: "src/main.rs".to_owned(),
            anchor_rank: 0,
            line: 1,
            anchor_kind: "line_excerpt".to_owned(),
            excerpt: "fn main() {}".to_owned(),
            terms: vec!["main".to_owned()],
            score_hint: 18,
        }],
    };

    storage.replace_retrieval_projection_bundle_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &bundle,
    )?;

    let head = storage
        .load_retrieval_projection_head_for_repository_snapshot_family(
            "repo-1",
            "snapshot-001",
            "path_relation",
        )?
        .expect("expected path relation head");
    assert_eq!(head.row_count, 1);
    assert_eq!(head.input_modes, vec!["path".to_owned()]);
    assert_eq!(
        storage
            .load_path_relation_projections_for_repository_snapshot("repo-1", "snapshot-001")?
            .len(),
        1
    );
    assert_eq!(
        storage
            .load_subtree_coverage_projections_for_repository_snapshot("repo-1", "snapshot-001")?
            .len(),
        1
    );
    assert_eq!(
        storage
            .load_path_surface_term_projections_for_repository_snapshot("repo-1", "snapshot-001",)?
            .len(),
        1
    );
    assert_eq!(
        storage
            .load_path_anchor_sketch_projections_for_repository_snapshot("repo-1", "snapshot-001",)?
            .len(),
        1
    );

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn delete_snapshot_removes_retrieval_projection_bundle_rows() -> FriggResult<()> {
    let db_path = temp_db_path("retrieval-projection-bundle-delete-snapshot");
    let storage = Storage::new(&db_path);
    storage.initialize()?;
    storage.upsert_manifest(
        "repo-1",
        "snapshot-001",
        &[manifest_entry("src/main.rs", "hash-main", 10, Some(100))],
    )?;
    storage.replace_retrieval_projection_bundle_for_repository_snapshot(
        "repo-1",
        "snapshot-001",
        &RetrievalProjectionBundle {
            heads: vec![RetrievalProjectionHeadRecord {
                family: "path_anchor_sketch".to_owned(),
                heuristic_version: 1,
                input_modes: vec!["path".to_owned()],
                row_count: 1,
            }],
            path_witness: Vec::new(),
            test_subject: Vec::new(),
            entrypoint_surface: Vec::new(),
            path_relations: vec![PathRelationProjection {
                src_path: "src/main.rs".to_owned(),
                dst_path: "src/main.rs".to_owned(),
                relation_kind: "companion_surface".to_owned(),
                evidence_source: "path".to_owned(),
                src_symbol_id: None,
                dst_symbol_id: None,
                src_family_bits: 1,
                dst_family_bits: 1,
                shared_terms: vec!["main".to_owned()],
                score_hint: 80,
            }],
            subtree_coverage: vec![SubtreeCoverageProjection {
                subtree_root: "src".to_owned(),
                family: "runtime".to_owned(),
                path_count: 1,
                exemplar_path: "src/main.rs".to_owned(),
                exemplar_score_hint: 10,
            }],
            path_surface_terms: vec![PathSurfaceTermProjection {
                path: "src/main.rs".to_owned(),
                term_weights: BTreeMap::from([("main".to_owned(), 4)]),
                exact_terms: vec!["main".to_owned()],
            }],
            path_anchor_sketches: vec![PathAnchorSketchProjection {
                path: "src/main.rs".to_owned(),
                anchor_rank: 0,
                line: 1,
                anchor_kind: "line_excerpt".to_owned(),
                excerpt: "fn main() {}".to_owned(),
                terms: vec!["main".to_owned()],
                score_hint: 18,
            }],
        },
    )?;

    storage.delete_snapshot("snapshot-001")?;

    assert!(
        storage
            .load_retrieval_projection_head_for_repository_snapshot_family(
                "repo-1",
                "snapshot-001",
                "path_anchor_sketch",
            )?
            .is_none()
    );
    assert!(
        storage
            .load_path_relation_projections_for_repository_snapshot("repo-1", "snapshot-001")?
            .is_empty()
    );
    assert!(
        storage
            .load_subtree_coverage_projections_for_repository_snapshot("repo-1", "snapshot-001")?
            .is_empty()
    );
    assert!(
        storage
            .load_path_surface_term_projections_for_repository_snapshot("repo-1", "snapshot-001",)?
            .is_empty()
    );
    assert!(
        storage
            .load_path_anchor_sketch_projections_for_repository_snapshot("repo-1", "snapshot-001",)?
            .is_empty()
    );

    cleanup_db(&db_path);
    Ok(())
}
