use std::path::Path;

use crate::domain::{FriggError, FriggResult};
use rusqlite::{Connection, OptionalExtension, Transaction};

use super::vector_store::{
    ensure_sqlite_vec_auto_extension_registered, ensure_sqlite_vec_registration_readiness,
};
use super::{
    ManifestEntry, ManifestMetadataEntry, Migration, RepositoryManifestMetadataSnapshot,
    RepositoryManifestSnapshot,
};

pub(super) fn count_provenance_events(conn: &Connection) -> FriggResult<usize> {
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM provenance_event", [], |row| {
            row.get(0)
        })
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count provenance events for retention: {err}"
            ))
        })?;
    usize::try_from(count).map_err(|err| {
        FriggError::Internal(format!(
            "provenance event count overflow for retention: {err}"
        ))
    })
}

pub(super) fn prune_provenance_events_on_connection(
    conn: &Connection,
    keep_latest: usize,
) -> FriggResult<()> {
    let total = count_provenance_events(conn)?;
    if total <= keep_latest {
        return Ok(());
    }

    conn.execute(
        r#"
        DELETE FROM provenance_event
        WHERE rowid NOT IN (
          SELECT rowid
          FROM provenance_event
          ORDER BY created_at DESC, rowid DESC
          LIMIT ?1
        )
        "#,
        [usize_to_i64(keep_latest, "keep_latest")?],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to prune provenance events on the live connection: {err}"
        ))
    })?;

    Ok(())
}

pub(super) fn load_semantic_head_snapshot_ids_for_repository(
    conn: &Connection,
    repository_id: &str,
) -> FriggResult<Vec<String>> {
    let mut statement = conn
        .prepare(
            "SELECT covered_snapshot_id FROM semantic_head WHERE repository_id = ?1 ORDER BY covered_snapshot_id ASC",
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic head snapshot lookup for repository '{repository_id}': {err}"
            ))
        })?;
    statement
        .query_map([repository_id], |row| row.get::<_, String>(0))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query semantic head snapshot ids for repository '{repository_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic head snapshot ids for repository '{repository_id}': {err}"
            ))
        })
}

pub(super) fn open_connection(path: &Path) -> FriggResult<Connection> {
    ensure_sqlite_vec_auto_extension_registered()?;
    let conn = Connection::open(path)
        .map_err(|err| FriggError::Internal(format!("failed to open sqlite db: {err}")))?;
    ensure_sqlite_vec_registration_readiness(&conn)?;
    Ok(conn)
}

pub(super) fn load_manifest_entries_for_snapshot(
    conn: &Connection,
    snapshot_id: &str,
) -> FriggResult<Vec<ManifestEntry>> {
    let mut statement = conn
        .prepare(
            r#"
            SELECT path, sha256, size_bytes, mtime_ns
            FROM file_manifest
            WHERE snapshot_id = ?1
            ORDER BY path ASC
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare manifest load query for snapshot '{snapshot_id}': {err}"
            ))
        })?;

    let rows = statement
        .query_map([snapshot_id], |row| {
            let size_bytes_raw: i64 = row.get(2)?;
            let mtime_ns_raw: Option<i64> = row.get(3)?;
            Ok(ManifestEntry {
                path: row.get(0)?,
                sha256: row.get(1)?,
                size_bytes: i64_to_u64(size_bytes_raw, "size_bytes")?,
                mtime_ns: option_i64_to_option_u64(mtime_ns_raw, "mtime_ns")?,
            })
        })
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query manifest rows for snapshot '{snapshot_id}': {err}"
            ))
        })?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
        FriggError::Internal(format!(
            "failed to decode manifest rows for snapshot '{snapshot_id}': {err}"
        ))
    })
}

pub(super) fn load_latest_manifest_snapshot_for_repository(
    conn: &Connection,
    repository_id: &str,
) -> FriggResult<Option<RepositoryManifestSnapshot>> {
    let mut statement = conn
        .prepare(
            r#"
            WITH latest AS (
                SELECT snapshot_id
                FROM snapshot
                WHERE repository_id = ?1
                ORDER BY created_at DESC, snapshot_id DESC
                LIMIT 1
            )
            SELECT latest.snapshot_id, file_manifest.path, file_manifest.sha256, file_manifest.size_bytes, file_manifest.mtime_ns
            FROM latest
            LEFT JOIN file_manifest ON file_manifest.snapshot_id = latest.snapshot_id
            ORDER BY file_manifest.path ASC
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare latest manifest query for repository '{repository_id}': {err}"
            ))
        })?;

    let rows = statement
        .query_map([repository_id], |row| {
            let snapshot_id: String = row.get(0)?;
            let path: Option<String> = row.get(1)?;
            let sha256: Option<String> = row.get(2)?;
            let size_bytes_raw: Option<i64> = row.get(3)?;
            let mtime_ns_raw: Option<i64> = row.get(4)?;
            Ok((snapshot_id, path, sha256, size_bytes_raw, mtime_ns_raw))
        })
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query latest manifest rows for repository '{repository_id}': {err}"
            ))
        })?;

    let mut snapshot_id = None;
    let mut entries = Vec::new();
    for row in rows {
        let (row_snapshot_id, path, sha256, size_bytes_raw, mtime_ns_raw) = row.map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode latest manifest rows for repository '{repository_id}': {err}"
            ))
        })?;
        snapshot_id.get_or_insert(row_snapshot_id);
        let Some(path) = path else {
            continue;
        };
        let size_bytes_raw = size_bytes_raw.ok_or_else(|| {
            FriggError::Internal(format!(
                "latest manifest row for repository '{repository_id}' missing size_bytes"
            ))
        })?;
        entries.push(ManifestEntry {
            path,
            sha256: sha256.unwrap_or_default(),
            size_bytes: i64_to_u64(size_bytes_raw, "size_bytes").map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode latest manifest size for repository '{repository_id}': {err}"
                ))
            })?,
            mtime_ns: option_i64_to_option_u64(mtime_ns_raw, "mtime_ns").map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode latest manifest mtime for repository '{repository_id}': {err}"
                ))
            })?,
        });
    }

    Ok(snapshot_id.map(|snapshot_id| RepositoryManifestSnapshot {
        repository_id: repository_id.to_owned(),
        snapshot_id,
        entries,
    }))
}

pub(super) fn load_latest_manifest_metadata_snapshot_for_repository(
    conn: &Connection,
    repository_id: &str,
) -> FriggResult<Option<RepositoryManifestMetadataSnapshot>> {
    let mut statement = conn
        .prepare(
            r#"
            WITH latest AS (
                SELECT snapshot_id
                FROM snapshot
                WHERE repository_id = ?1
                ORDER BY created_at DESC, snapshot_id DESC
                LIMIT 1
            )
            SELECT latest.snapshot_id, file_manifest.path, file_manifest.size_bytes, file_manifest.mtime_ns
            FROM latest
            LEFT JOIN file_manifest ON file_manifest.snapshot_id = latest.snapshot_id
            ORDER BY file_manifest.path ASC
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare latest manifest metadata query for repository '{repository_id}': {err}"
            ))
        })?;

    let rows = statement
        .query_map([repository_id], |row| {
            let snapshot_id: String = row.get(0)?;
            let path: Option<String> = row.get(1)?;
            let size_bytes_raw: Option<i64> = row.get(2)?;
            let mtime_ns_raw: Option<i64> = row.get(3)?;
            Ok((snapshot_id, path, size_bytes_raw, mtime_ns_raw))
        })
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query latest manifest metadata rows for repository '{repository_id}': {err}"
            ))
        })?;

    let mut snapshot_id = None;
    let mut entries = Vec::new();
    for row in rows {
        let (row_snapshot_id, path, size_bytes_raw, mtime_ns_raw) = row.map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode latest manifest metadata rows for repository '{repository_id}': {err}"
            ))
        })?;
        snapshot_id.get_or_insert(row_snapshot_id);
        let Some(path) = path else {
            continue;
        };
        let size_bytes_raw = size_bytes_raw.ok_or_else(|| {
            FriggError::Internal(format!(
                "latest manifest metadata row for repository '{repository_id}' missing size_bytes"
            ))
        })?;
        entries.push(ManifestMetadataEntry {
            path,
            size_bytes: i64_to_u64(size_bytes_raw, "size_bytes").map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode latest manifest metadata size for repository '{repository_id}': {err}"
                ))
            })?,
            mtime_ns: option_i64_to_option_u64(mtime_ns_raw, "mtime_ns").map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode latest manifest metadata mtime for repository '{repository_id}': {err}"
                ))
            })?,
        });
    }

    Ok(
        snapshot_id.map(|snapshot_id| RepositoryManifestMetadataSnapshot {
            repository_id: repository_id.to_owned(),
            snapshot_id,
            entries,
        }),
    )
}

pub(super) fn u64_to_i64(value: u64, field_name: &str) -> FriggResult<i64> {
    i64::try_from(value).map_err(|_| {
        FriggError::Internal(format!(
            "failed to persist manifest field '{field_name}': value {value} exceeds sqlite INTEGER range"
        ))
    })
}

pub(super) fn usize_to_i64(value: usize, field_name: &str) -> FriggResult<i64> {
    i64::try_from(value).map_err(|_| {
        FriggError::Internal(format!(
            "failed to persist field '{field_name}': value {value} exceeds sqlite INTEGER range"
        ))
    })
}

pub(super) fn option_u64_to_option_i64(
    value: Option<u64>,
    field_name: &str,
) -> FriggResult<Option<i64>> {
    value
        .map(|current| u64_to_i64(current, field_name))
        .transpose()
}

pub(super) fn i64_to_u64(value: i64, field_name: &str) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| {
        rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Integer,
            Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("manifest field '{field_name}' contains negative sqlite INTEGER: {value}"),
            )),
        )
    })
}

pub(super) fn option_i64_to_option_u64(
    value: Option<i64>,
    field_name: &str,
) -> rusqlite::Result<Option<u64>> {
    value
        .map(|current| i64_to_u64(current, field_name))
        .transpose()
}

pub(super) fn read_schema_version(conn: &Connection) -> FriggResult<i64> {
    conn.query_row(
        "SELECT version FROM schema_version WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| FriggError::Internal(format!("failed to query schema version: {err}")))?
    .map_or(Ok(0), Ok)
}

pub(super) fn apply_migration(conn: &mut Connection, migration: &Migration) -> FriggResult<()> {
    let tx = conn.transaction().map_err(|err| {
        FriggError::Internal(format!(
            "failed to start migration transaction v{}: {err}",
            migration.version
        ))
    })?;

    tx.execute_batch(migration.sql).map_err(|err| {
        FriggError::Internal(format!(
            "failed to apply schema migration v{}: {err}",
            migration.version
        ))
    })?;

    set_schema_version(&tx, migration.version)?;

    tx.commit().map_err(|err| {
        FriggError::Internal(format!(
            "failed to commit migration transaction v{}: {err}",
            migration.version
        ))
    })?;

    Ok(())
}

pub(super) fn set_schema_version(tx: &Transaction<'_>, version: i64) -> FriggResult<()> {
    tx.execute(
        r#"
        INSERT INTO schema_version (id, version, updated_at)
        VALUES (1, ?1, CURRENT_TIMESTAMP)
        ON CONFLICT(id) DO UPDATE SET
            version = excluded.version,
            updated_at = excluded.updated_at
        "#,
        [version],
    )
    .map_err(|err| FriggError::Internal(format!("failed to update schema version: {err}")))?;

    Ok(())
}

pub(super) fn table_exists(conn: &Connection, table_name: &str) -> FriggResult<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
        [table_name],
        |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to query sqlite table existence for '{table_name}': {err}"
        ))
    })
}

pub(super) fn latest_schema_version(migrations: &[Migration]) -> i64 {
    migrations.last().map_or(0, |migration| migration.version)
}

pub(super) fn run_repository_roundtrip_probe(conn: &mut Connection) -> FriggResult<()> {
    let tx = conn.transaction().map_err(|err| {
        FriggError::Internal(format!(
            "storage verification failed: unable to open probe transaction: {err}"
        ))
    })?;
    let probe_repository_id = format!("verify-probe-{}", uuid::Uuid::now_v7());

    tx.execute(
        r#"
        INSERT INTO repository (repository_id, root_path, display_name, created_at)
        VALUES (?1, '/verify/probe', 'verify-probe', CURRENT_TIMESTAMP)
        "#,
        [&probe_repository_id],
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "storage verification failed: repository write probe failed: {err}"
        ))
    })?;

    let exists: i64 = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM repository WHERE repository_id = ?1)",
            [&probe_repository_id],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "storage verification failed: repository read probe failed: {err}"
            ))
        })?;

    if exists != 1 {
        return Err(FriggError::Internal(
            "storage verification failed: repository probe row not readable after insert"
                .to_owned(),
        ));
    }

    tx.rollback().map_err(|err| {
        FriggError::Internal(format!(
            "storage verification failed: probe rollback failed: {err}"
        ))
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::FriggError;
    use rusqlite::Connection;
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::env;
    use std::path::PathBuf;

    fn temp_db_path(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!("frigg-db-runtime-{test_name}-{nonce}.sqlite3"))
    }

    fn ensure_core_storage_tables(conn: &Connection) -> FriggResult<()> {
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS snapshot (
              snapshot_id TEXT PRIMARY KEY,
              repository_id TEXT NOT NULL,
              kind TEXT NOT NULL,
              revision TEXT,
              created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS file_manifest (
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              sha256 TEXT NOT NULL,
              size_bytes INTEGER NOT NULL,
              mtime_ns INTEGER
            );

            CREATE TABLE IF NOT EXISTS provenance_event (
              trace_id TEXT NOT NULL,
              tool_name TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL,
              PRIMARY KEY (trace_id, tool_name, created_at)
            );
            "#,
        )
        .map_err(|err| FriggError::Internal(format!("failed to create db-runtime test tables: {err}")))
    }

    #[test]
    fn count_provenance_events_counts_rows() -> FriggResult<()> {
        let db_path = temp_db_path("count-events");
        let conn = Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!("failed to open db for provenance count test: {err}"))
        })?;
        ensure_core_storage_tables(&conn)?;

        assert_eq!(count_provenance_events(&conn)?, 0);

        conn.execute(
            "INSERT INTO provenance_event (trace_id, tool_name, payload_json, created_at) VALUES (?1, ?2, ?3, '2026-01-01T00:00:00Z')",
            ("trace-1", "read_file", "{}"),
        )
        .map_err(|err| FriggError::Internal(format!("failed to seed provenance row: {err}")))?;

        assert_eq!(count_provenance_events(&conn)?, 1);
        let _ = std::fs::remove_file(&db_path);
        Ok(())
    }

    #[test]
    fn prune_provenance_events_on_connection_keeps_requested_retention() -> FriggResult<()> {
        let db_path = temp_db_path("prune-events");
        let conn = Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!("failed to open db for provenance prune test: {err}"))
        })?;
        ensure_core_storage_tables(&conn)?;

        for idx in 1..=3 {
            conn.execute(
                "INSERT INTO provenance_event (trace_id, tool_name, payload_json, created_at) VALUES (?1, ?2, ?3, ?4)",
                (format!("trace-{idx}"), "read_file", "{}", format!("2026-01-0{idx}T00:00:00Z")),
            )
            .map_err(|err| FriggError::Internal(format!("failed to seed provenance row {idx}: {err}")))?;
        }

        prune_provenance_events_on_connection(&conn, 2)?;
        let before = count_provenance_events(&conn)?;
        assert_eq!(before, 2);

        prune_provenance_events_on_connection(&conn, 10)?;
        assert_eq!(count_provenance_events(&conn)?, 2);

        let _ = std::fs::remove_file(&db_path);
        Ok(())
    }

    #[test]
    fn load_latest_manifest_snapshot_prefers_latest_timestamp_and_handles_empty_rows()
    -> FriggResult<()> {
        let db_path = temp_db_path("latest-manifest");
        let conn = Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!(
                "failed to open db for latest manifest helper test: {err}"
            ))
        })?;
        ensure_core_storage_tables(&conn)?;

        conn.execute(
            "INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at) VALUES ('snapshot-old', 'repo-1', 'manifest', NULL, '2026-01-01T00:00:00Z')",
            [],
        )
        .map_err(|err| FriggError::Internal(format!("failed to seed old snapshot: {err}")))?;
        conn.execute(
            "INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at) VALUES ('snapshot-new', 'repo-1', 'manifest', NULL, '2026-01-02T00:00:00Z')",
            [],
        )
        .map_err(|err| FriggError::Internal(format!("failed to seed new snapshot: {err}")))?;

        let no_match = load_latest_manifest_snapshot_for_repository(&conn, "repo-missing");
        assert!(no_match.is_ok());
        assert!(no_match?.is_none());

        let loaded = load_latest_manifest_snapshot_for_repository(&conn, "repo-1")?.expect(
            "latest manifest snapshot should be materialized even when file rows are absent",
        );
        assert_eq!(loaded.repository_id, "repo-1");
        assert_eq!(loaded.snapshot_id, "snapshot-new");
        assert!(loaded.entries.is_empty());

        let _ = std::fs::remove_file(&db_path);
        Ok(())
    }

    #[test]
    fn load_latest_manifest_metadata_uses_empty_manifest_rows_as_empty_metadata() -> FriggResult<()> {
        let db_path = temp_db_path("latest-manifest-metadata");
        let conn = Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!(
                "failed to open db for latest metadata helper test: {err}"
            ))
        })?;
        ensure_core_storage_tables(&conn)?;

        conn.execute(
            "INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at) VALUES ('snapshot-empty', 'repo-1', 'manifest', NULL, '2026-01-01T00:00:00Z')",
            [],
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to seed metadata empty snapshot: {err}"))
        })?;
        // no manifest rows for snapshot-empty

        let loaded = load_latest_manifest_metadata_snapshot_for_repository(&conn, "repo-1")?
            .expect("expected metadata snapshot helper to preserve repository snapshot id");
        assert_eq!(loaded.snapshot_id, "snapshot-empty");
        assert!(loaded.entries.is_empty());

        let _ = std::fs::remove_file(&db_path);
        Ok(())
    }

    #[test]
    fn number_conversion_helpers_cover_bounds_and_negatives() {
        assert_eq!(u64_to_i64(10, "test").expect("u64_to_i64 should support small values"), 10);
        let overflow = u64_to_i64(u64::MAX, "test-overflow");
        assert!(
            overflow.is_err(),
            "u64_to_i64 should fail when value cannot fit i64"
        );
        assert_eq!(
            usize_to_i64(12, "test").expect("usize_to_i64 should support small values"),
            12
        );
        let opt = option_u64_to_option_i64(Some(99), "test-option");
        assert_eq!(opt.expect("option conversion should work"), Some(99));
        assert_eq!(
            option_u64_to_option_i64(None, "test-option-none").expect("none option conversion"),
            None
        );

        let positive = i64_to_u64(17, "size_bytes").expect("positive conversion should work");
        assert_eq!(positive, 17);
        assert!(i64_to_u64(-5, "size_bytes").is_err());
        let optional = option_i64_to_option_u64(Some(33), "size_bytes")
            .expect("option conversion for some value should work");
        assert_eq!(optional, Some(33));
        assert_eq!(
            option_i64_to_option_u64(None, "size_bytes").expect("option conversion none"),
            None
        );
    }

    #[test]
    fn read_schema_version_errors_when_table_is_missing() -> FriggResult<()> {
        let db_path = temp_db_path("schema-version-missing");
        let conn = Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!(
                "failed to open db for schema version missing test: {err}"
            ))
        })?;

        let err = read_schema_version(&conn).expect_err("schema version query should fail without table");
        let message = match err {
            FriggError::Internal(message) => message,
            other => {
                panic!("expected internal error when schema_version table is missing, got: {other}")
            }
        };
        assert!(
            message.contains("no such table: schema_version"),
            "unexpected schema version error: {message}"
        );

        let _ = std::fs::remove_file(&db_path);
        Ok(())
    }

    #[test]
    fn read_schema_version_returns_last_version_when_seeded() -> FriggResult<()> {
        let db_path = temp_db_path("schema-version-seeded");
        let conn = Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!(
                "failed to open db for schema version seeded test: {err}"
            ))
        })?;
        conn.execute_batch(
            "CREATE TABLE schema_version (id INTEGER PRIMARY KEY CHECK (id = 1), version INTEGER NOT NULL, updated_at TEXT NOT NULL);",
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to create schema_version table for test: {err}"))
        })?;
        conn.execute(
            "INSERT INTO schema_version (id, version, updated_at) VALUES (1, 3, CURRENT_TIMESTAMP)",
            [],
        )
        .map_err(|err| FriggError::Internal(format!("failed to seed schema version: {err}")))?;

        assert_eq!(read_schema_version(&conn)?, 3);

        let _ = std::fs::remove_file(&db_path);
        Ok(())
    }

    #[test]
    fn table_exists_reflects_current_schema_tables() -> FriggResult<()> {
        let db_path = temp_db_path("table-exists");
        let conn = Connection::open(&db_path).map_err(|err| {
            FriggError::Internal(format!("failed to open db for table_exists test: {err}"))
        })?;

        assert!(!table_exists(&conn, "repository").expect("table_exists should return false"));
        conn.execute("CREATE TABLE repository (repository_id TEXT)", [])
            .map_err(|err| FriggError::Internal(format!("failed to create test table: {err}")))?;
        assert!(table_exists(&conn, "repository").expect("table_exists should return true"));

        let _ = std::fs::remove_file(&db_path);
        Ok(())
    }
}
