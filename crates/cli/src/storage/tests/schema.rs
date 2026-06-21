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
        "retrieval_projection_head",
        "path_relation_projection",
        "subtree_coverage_projection",
        "path_surface_term_projection",
        "path_anchor_sketch_projection",
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

#[test]
fn migration_10_repairs_projection_foreign_keys_rewritten_to_snapshot_v8() -> FriggResult<()> {
    let db_path = temp_db_path("repair-snapshot-v8-projection-fks");
    seed_v9_schema_with_snapshot_v8_projection_references(&db_path)?;

    let storage = Storage::new(&db_path);
    storage.initialize()?;
    storage.verify()?;

    assert_eq!(
        storage.schema_version()?,
        MIGRATIONS
            .last()
            .expect("storage migrations should not be empty")
            .version
    );

    let conn = open_test_connection(&db_path)?;
    for table in [
        "retrieval_projection_head",
        "path_relation_projection",
        "subtree_coverage_projection",
        "path_surface_term_projection",
        "path_anchor_sketch_projection",
    ] {
        let foreign_key_targets = foreign_key_targets(&conn, table)?;
        assert!(
            foreign_key_targets
                .iter()
                .any(|target| target == "snapshot"),
            "expected {table} to reference snapshot, got {foreign_key_targets:?}"
        );
        assert!(
            foreign_key_targets
                .iter()
                .all(|target| target != "snapshot_v8"),
            "expected {table} to stop referencing snapshot_v8, got {foreign_key_targets:?}"
        );
        assert_eq!(
            count_rows(&conn, table)?,
            1,
            "expected migration to preserve valid rows for {table}"
        );
    }

    conn.execute_batch("PRAGMA foreign_keys = ON;")
        .map_err(|err| FriggError::Internal(format!("failed to enable FK checks: {err}")))?;
    conn.execute(
        "DELETE FROM snapshot WHERE snapshot_id = 'snapshot-manifest'",
        [],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to delete snapshot for repaired FK cascade assertion: {err}"
        ))
    })?;

    for table in [
        "retrieval_projection_head",
        "path_relation_projection",
        "subtree_coverage_projection",
        "path_surface_term_projection",
        "path_anchor_sketch_projection",
    ] {
        assert_eq!(
            count_rows(&conn, table)?,
            0,
            "expected repaired FK cascade to remove rows for {table}"
        );
    }

    cleanup_db(&db_path);
    Ok(())
}

#[test]
fn migration_8_enforces_snapshot_repository_and_manifest_row_references() -> FriggResult<()> {
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

fn seed_v9_schema_with_snapshot_v8_projection_references(path: &Path) -> FriggResult<()> {
    let mut conn = open_test_connection(path)?;
    conn.execute_batch(
        r#"
            CREATE TABLE schema_version (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );
            "#,
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to create schema_version table for snapshot_v8 FK fixture: {err}"
        ))
    })?;

    {
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start migration seed transaction for snapshot_v8 FK fixture: {err}"
            ))
        })?;
        for migration in MIGRATIONS
            .iter()
            .take_while(|migration| migration.version <= 8)
        {
            tx.execute_batch(migration.sql).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to seed migration v{} for snapshot_v8 FK fixture: {err}",
                    migration.version
                ))
            })?;
        }
        set_schema_version(&tx, 8)?;
        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit v8 seed transaction for snapshot_v8 FK fixture: {err}"
            ))
        })?;
    }

    conn.execute_batch("PRAGMA foreign_keys = OFF;")
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to disable FK checks for snapshot_v8 FK fixture: {err}"
            ))
        })?;

    let tx = conn.transaction().map_err(|err| {
        FriggError::Internal(format!(
            "failed to start stale v9 seed transaction for snapshot_v8 FK fixture: {err}"
        ))
    })?;
    tx.execute_batch(
        r#"
            ALTER TABLE path_witness_projection
            ADD COLUMN file_stem TEXT NOT NULL DEFAULT '';

            ALTER TABLE path_witness_projection
            ADD COLUMN subtree_root TEXT;

            ALTER TABLE path_witness_projection
            ADD COLUMN family_bits INTEGER NOT NULL DEFAULT 0;

            ALTER TABLE path_witness_projection
            ADD COLUMN heuristic_version INTEGER NOT NULL DEFAULT 0;

            CREATE TABLE retrieval_projection_head (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES "snapshot_v8"(snapshot_id) ON DELETE CASCADE,
              family TEXT NOT NULL,
              heuristic_version INTEGER NOT NULL,
              input_modes_json TEXT NOT NULL,
              row_count INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, family)
            );

            CREATE INDEX idx_retrieval_projection_head_repo_snapshot_family
            ON retrieval_projection_head (repository_id, snapshot_id, family);

            CREATE TABLE path_relation_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES "snapshot_v8"(snapshot_id) ON DELETE CASCADE,
              src_path TEXT NOT NULL,
              dst_path TEXT NOT NULL,
              relation_kind TEXT NOT NULL,
              evidence_source TEXT NOT NULL,
              src_symbol_id TEXT,
              dst_symbol_id TEXT,
              src_family_bits INTEGER NOT NULL DEFAULT 0,
              dst_family_bits INTEGER NOT NULL DEFAULT 0,
              shared_terms_json TEXT NOT NULL,
              score_hint INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, src_path, dst_path, relation_kind)
            );

            CREATE INDEX idx_path_relation_projection_repo_snapshot_src
            ON path_relation_projection (repository_id, snapshot_id, src_path, relation_kind, dst_path);

            CREATE INDEX idx_path_relation_projection_repo_snapshot_dst
            ON path_relation_projection (repository_id, snapshot_id, dst_path, relation_kind, src_path);

            CREATE TABLE subtree_coverage_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES "snapshot_v8"(snapshot_id) ON DELETE CASCADE,
              subtree_root TEXT NOT NULL,
              family TEXT NOT NULL,
              path_count INTEGER NOT NULL,
              exemplar_path TEXT NOT NULL,
              exemplar_score_hint INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, subtree_root, family)
            );

            CREATE INDEX idx_subtree_coverage_projection_repo_snapshot_subtree
            ON subtree_coverage_projection (repository_id, snapshot_id, subtree_root, family);

            CREATE TABLE path_surface_term_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES "snapshot_v8"(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              term_weights_json TEXT NOT NULL,
              exact_terms_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            CREATE INDEX idx_path_surface_term_projection_repo_snapshot_path
            ON path_surface_term_projection (repository_id, snapshot_id, path);

            CREATE TABLE path_anchor_sketch_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES "snapshot_v8"(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              anchor_rank INTEGER NOT NULL,
              line INTEGER NOT NULL,
              anchor_kind TEXT NOT NULL,
              excerpt TEXT NOT NULL,
              terms_json TEXT NOT NULL,
              score_hint INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path, anchor_rank)
            );

            CREATE INDEX idx_path_anchor_sketch_projection_repo_snapshot_path
            ON path_anchor_sketch_projection (repository_id, snapshot_id, path, anchor_rank);

            INSERT INTO repository (repository_id, root_path, display_name, created_at)
            VALUES ('repo-1', '/tmp/repo-1', 'Repo 1', '2026-03-11T00:00:00Z');

            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            VALUES ('snapshot-manifest', 'repo-1', 'manifest', NULL, '2026-03-11T00:00:00Z');

            INSERT INTO retrieval_projection_head (
                repository_id,
                snapshot_id,
                family,
                heuristic_version,
                input_modes_json,
                row_count,
                created_at,
                updated_at
            )
            VALUES (
                'repo-1',
                'snapshot-manifest',
                'path_relation',
                1,
                '["manifest"]',
                1,
                '2026-03-11T00:00:00Z',
                '2026-03-11T00:00:00Z'
            );

            INSERT INTO path_relation_projection (
                repository_id,
                snapshot_id,
                src_path,
                dst_path,
                relation_kind,
                evidence_source,
                src_symbol_id,
                dst_symbol_id,
                src_family_bits,
                dst_family_bits,
                shared_terms_json,
                score_hint,
                created_at
            )
            VALUES (
                'repo-1',
                'snapshot-manifest',
                'src/main.rs',
                'src/lib.rs',
                'imports',
                'manifest',
                NULL,
                NULL,
                1,
                2,
                '["src"]',
                42,
                '2026-03-11T00:00:00Z'
            );

            INSERT INTO subtree_coverage_projection (
                repository_id,
                snapshot_id,
                subtree_root,
                family,
                path_count,
                exemplar_path,
                exemplar_score_hint,
                created_at
            )
            VALUES (
                'repo-1',
                'snapshot-manifest',
                'src',
                'path_witness',
                2,
                'src/main.rs',
                10,
                '2026-03-11T00:00:00Z'
            );

            INSERT INTO path_surface_term_projection (
                repository_id,
                snapshot_id,
                path,
                term_weights_json,
                exact_terms_json,
                created_at
            )
            VALUES (
                'repo-1',
                'snapshot-manifest',
                'src/main.rs',
                '{"main":1}',
                '["main"]',
                '2026-03-11T00:00:00Z'
            );

            INSERT INTO path_anchor_sketch_projection (
                repository_id,
                snapshot_id,
                path,
                anchor_rank,
                line,
                anchor_kind,
                excerpt,
                terms_json,
                score_hint,
                created_at
            )
            VALUES (
                'repo-1',
                'snapshot-manifest',
                'src/main.rs',
                0,
                1,
                'symbol',
                'fn main() {}',
                '["main"]',
                20,
                '2026-03-11T00:00:00Z'
            );
            "#,
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed stale v9 projection schema for snapshot_v8 FK fixture: {err}"
        ))
    })?;
    set_schema_version(&tx, 9)?;
    tx.commit().map_err(|err| {
        FriggError::Internal(format!(
            "failed to commit stale v9 seed transaction for snapshot_v8 FK fixture: {err}"
        ))
    })?;

    Ok(())
}

fn foreign_key_targets(conn: &Connection, table: &str) -> FriggResult<Vec<String>> {
    let query = format!("PRAGMA foreign_key_list({table})");
    let mut statement = conn.prepare(&query).map_err(|err| {
        FriggError::Internal(format!(
            "failed to prepare FK target query for table '{table}': {err}"
        ))
    })?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(2))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query FK targets for table '{table}': {err}"
            ))
        })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
        FriggError::Internal(format!(
            "failed to decode FK targets for table '{table}': {err}"
        ))
    })
}
