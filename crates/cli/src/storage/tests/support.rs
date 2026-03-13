pub(super) use std::path::{Path, PathBuf};
pub(super) use std::{env, fs};

pub(super) use super::super::{
    DEFAULT_VECTOR_DIMENSIONS, MIGRATIONS, ManifestEntry, PROVENANCE_STORAGE_DB_FILE,
    PROVENANCE_STORAGE_DIR, PathWitnessProjectionRecord, SQLITE_VEC_REQUIRED_VERSION,
    SemanticChunkEmbeddingRecord, Storage, VECTOR_TABLE_NAME, encode_f32_vector,
    ensure_provenance_db_parent_dir, ensure_sqlite_vec_pinned_version,
    initialize_vector_store_on_connection_with_detected_capability, open_connection,
    resolve_provenance_db_path, set_schema_version, table_exists,
    verify_vector_store_on_connection_with_detected_capability,
};
pub(super) use crate::domain::{FriggError, FriggResult};
pub(super) use rusqlite::Connection;
pub(super) use serde_json::json;
pub(super) use uuid::Uuid;

pub(super) fn temp_db_path(test_name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "frigg-storage-{test_name}-{}.sqlite3",
        Uuid::now_v7()
    ))
}

pub(super) fn temp_workspace_root(test_name: &str) -> PathBuf {
    env::temp_dir().join(format!(
        "frigg-storage-workspace-{test_name}-{}",
        Uuid::now_v7()
    ))
}

pub(super) fn open_test_connection(path: &Path) -> FriggResult<Connection> {
    Connection::open(path).map_err(|err| {
        FriggError::Internal(format!(
            "failed to open sqlite db for test assertions: {err}"
        ))
    })
}

pub(super) fn initialize_v3_storage_schema(path: &Path) -> FriggResult<()> {
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
            "failed to create schema_version table for v3 migration test: {err}"
        ))
    })?;

    let tx = conn.transaction().map_err(|err| {
        FriggError::Internal(format!(
            "failed to start v3 migration seed transaction for tests: {err}"
        ))
    })?;
    for migration in MIGRATIONS
        .iter()
        .take_while(|migration| migration.version <= 3)
    {
        tx.execute_batch(migration.sql).map_err(|err| {
            FriggError::Internal(format!(
                "failed to seed migration v{} for v3 migration test: {err}",
                migration.version
            ))
        })?;
    }
    set_schema_version(&tx, 3)?;
    tx.commit().map_err(|err| {
        FriggError::Internal(format!(
            "failed to commit v3 schema seed transaction for tests: {err}"
        ))
    })?;

    Ok(())
}

pub(super) fn count_rows(conn: &Connection, table_name: &str) -> FriggResult<i64> {
    let query = format!("SELECT COUNT(*) FROM {table_name}");
    conn.query_row(&query, [], |row| row.get(0)).map_err(|err| {
        FriggError::Internal(format!(
            "failed to count rows in sqlite table '{table_name}': {err}"
        ))
    })
}

pub(super) fn index_exists(conn: &Connection, index_name: &str) -> FriggResult<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'index' AND name = ?1)",
        [index_name],
        |row| row.get::<_, i64>(0),
    )
    .map(|exists| exists != 0)
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to inspect sqlite index '{index_name}': {err}"
        ))
    })
}

pub(super) fn explain_query_plan(conn: &Connection, query: &str) -> FriggResult<Vec<String>> {
    let explain_sql = format!("EXPLAIN QUERY PLAN {query}");
    let mut statement = conn.prepare(&explain_sql).map_err(|err| {
        FriggError::Internal(format!(
            "failed to prepare explain query plan statement: {err}"
        ))
    })?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(3))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to execute explain query plan statement: {err}"
            ))
        })?;
    rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
        FriggError::Internal(format!(
            "failed to decode explain query plan details: {err}"
        ))
    })
}

pub(super) fn cleanup_db(path: &Path) {
    let _ = fs::remove_file(path);
}

pub(super) fn cleanup_workspace(path: &Path) {
    let _ = fs::remove_dir_all(path);
}

#[cfg(unix)]
pub(super) fn create_dir_symlink(target: &Path, link: &Path) -> FriggResult<()> {
    std::os::unix::fs::symlink(target, link).map_err(FriggError::Io)?;
    Ok(())
}

pub(super) fn create_sqlite_vec_like_table(
    conn: &Connection,
    dimensions: usize,
) -> FriggResult<()> {
    conn.execute_batch(&format!(
        "CREATE TABLE {VECTOR_TABLE_NAME} (embedding float[{dimensions}] NOT NULL);"
    ))
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to seed sqlite-vec-like table for transition tests: {err}"
        ))
    })?;

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn semantic_record(
    chunk_id: &str,
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    language: &str,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    provider: &str,
    model: &str,
    trace_id: Option<&str>,
    content_hash_blake3: &str,
    content_text: &str,
    embedding: &[f32],
) -> SemanticChunkEmbeddingRecord {
    SemanticChunkEmbeddingRecord {
        chunk_id: chunk_id.to_owned(),
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        path: path.to_owned(),
        language: language.to_owned(),
        chunk_index,
        start_line,
        end_line,
        provider: provider.to_owned(),
        model: model.to_owned(),
        trace_id: trace_id.map(ToOwned::to_owned),
        content_hash_blake3: content_hash_blake3.to_owned(),
        content_text: content_text.to_owned(),
        embedding: embedding.to_vec(),
    }
}

pub(super) fn replace_semantic_records(
    storage: &Storage,
    repository_id: &str,
    snapshot_id: &str,
    records: &[SemanticChunkEmbeddingRecord],
) -> FriggResult<()> {
    if records.is_empty() {
        return storage.replace_semantic_embeddings_for_repository(
            repository_id,
            snapshot_id,
            "openai",
            "text-embedding-3-small",
            records,
        );
    }

    let mut grouped =
        std::collections::BTreeMap::<(String, String), Vec<SemanticChunkEmbeddingRecord>>::new();
    for record in records {
        grouped
            .entry((record.provider.clone(), record.model.clone()))
            .or_default()
            .push(record.clone());
    }

    for ((provider, model), group) in grouped {
        storage.replace_semantic_embeddings_for_repository(
            repository_id,
            snapshot_id,
            &provider,
            &model,
            &group,
        )?;
    }

    Ok(())
}

pub(super) fn advance_semantic_records(
    storage: &Storage,
    repository_id: &str,
    previous_snapshot_id: Option<&str>,
    snapshot_id: &str,
    changed_paths: &[String],
    deleted_paths: &[String],
    records: &[SemanticChunkEmbeddingRecord],
) -> FriggResult<()> {
    let provider = records
        .first()
        .map(|record| record.provider.as_str())
        .unwrap_or("openai");
    let model = records
        .first()
        .map(|record| record.model.as_str())
        .unwrap_or("text-embedding-3-small");
    storage.advance_semantic_embeddings_for_repository(
        repository_id,
        previous_snapshot_id,
        snapshot_id,
        provider,
        model,
        changed_paths,
        deleted_paths,
        records,
    )
}

pub(super) fn manifest_entry(
    path: &str,
    sha256: &str,
    size_bytes: u64,
    mtime_ns: Option<u64>,
) -> ManifestEntry {
    ManifestEntry {
        path: path.to_owned(),
        sha256: sha256.to_owned(),
        size_bytes,
        mtime_ns,
    }
}

pub(super) fn path_witness_projection_record(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    path_class: &str,
    source_class: &str,
    path_terms_json: &str,
    flags_json: &str,
) -> PathWitnessProjectionRecord {
    PathWitnessProjectionRecord {
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        path: path.to_owned(),
        path_class: path_class.to_owned(),
        source_class: source_class.to_owned(),
        path_terms_json: path_terms_json.to_owned(),
        flags_json: flags_json.to_owned(),
    }
}
