use std::collections::BTreeMap;
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::domain::{FriggError, FriggResult};
use rusqlite::{Connection, ErrorCode, OptionalExtension, Transaction};
use serde_json::Value;
#[allow(unused_imports)]
use sqlite_vec as _;

mod provenance_path;
mod vector_store;
pub use provenance_path::{ensure_provenance_db_parent_dir, resolve_provenance_db_path};
use vector_store::{
    decode_f32_vector, encode_f32_vector, ensure_sqlite_vec_auto_extension_registered,
    ensure_sqlite_vec_registration_readiness, initialize_vector_store_on_connection,
    semantic_chunk_embedding_record_order, validate_semantic_chunk_embedding_record,
    verify_vector_store_on_connection,
};
#[cfg(test)]
pub(crate) use vector_store::{
    ensure_sqlite_vec_pinned_version,
    initialize_vector_store_on_connection_with_detected_capability,
    verify_vector_store_on_connection_with_detected_capability,
};

#[derive(Debug, Clone)]
pub struct Storage {
    db_path: PathBuf,
}

#[derive(Debug, Clone, Copy)]
struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: r#"
            CREATE TABLE IF NOT EXISTS repository (
              repository_id TEXT PRIMARY KEY,
              root_path TEXT NOT NULL,
              display_name TEXT NOT NULL,
              created_at TEXT NOT NULL
            );

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
              mtime_ns INTEGER,
              PRIMARY KEY (snapshot_id, path)
            );

            CREATE TABLE IF NOT EXISTS provenance_event (
              trace_id TEXT NOT NULL,
              tool_name TEXT NOT NULL,
              payload_json TEXT NOT NULL,
              created_at TEXT NOT NULL,
              PRIMARY KEY (trace_id, tool_name, created_at)
            );
        "#,
    },
    Migration {
        version: 2,
        sql: r#"
            CREATE INDEX IF NOT EXISTS idx_snapshot_repository_created_snapshot
            ON snapshot (repository_id, created_at DESC, snapshot_id DESC);

            CREATE INDEX IF NOT EXISTS idx_provenance_tool_created_trace
            ON provenance_event (tool_name, created_at DESC, trace_id DESC);
        "#,
    },
    Migration {
        version: 3,
        sql: r#"
            CREATE TABLE IF NOT EXISTS semantic_chunk_embedding (
              chunk_id TEXT PRIMARY KEY,
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              language TEXT NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_line INTEGER NOT NULL,
              end_line INTEGER NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              trace_id TEXT,
              content_hash_blake3 TEXT NOT NULL,
              content_text TEXT NOT NULL,
              embedding_blob BLOB NOT NULL,
              dimensions INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE INDEX IF NOT EXISTS idx_semantic_chunk_embedding_repo_snapshot_path_chunk
            ON semantic_chunk_embedding (repository_id, snapshot_id, path, chunk_index, chunk_id);

            CREATE INDEX IF NOT EXISTS idx_semantic_chunk_embedding_repo_chunk
            ON semantic_chunk_embedding (repository_id, chunk_id);
        "#,
    },
];

const REQUIRED_TABLES: &[&str] = &[
    "schema_version",
    "repository",
    "snapshot",
    "file_manifest",
    "provenance_event",
    "semantic_chunk_embedding",
];

pub const DEFAULT_VECTOR_DIMENSIONS: usize = 1_536;
pub const VECTOR_TABLE_NAME: &str = "embedding_vectors";
const SQLITE_VEC_REQUIRED_VERSION: &str = "0.1.7-alpha.10";
pub const PROVENANCE_STORAGE_DIR: &str = ".frigg";
pub const PROVENANCE_STORAGE_DB_FILE: &str = "storage.sqlite3";
const PROVENANCE_CREATED_AT_MAX_RETRY_MS: i64 = 32;
static SQLITE_VEC_AUTO_EXTENSION_REGISTRATION: OnceLock<Result<(), String>> = OnceLock::new();

#[allow(unsafe_code)]
unsafe extern "C" {
    fn sqlite3_vec_init(
        db: *mut rusqlite::ffi::sqlite3,
        pz_err_msg: *mut *mut c_char,
        api: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> c_int;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VectorStoreBackend {
    SqliteVec,
}

impl VectorStoreBackend {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SqliteVec => "sqlite_vec",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VectorStoreStatus {
    pub backend: VectorStoreBackend,
    pub extension_version: String,
    pub table_name: String,
    pub expected_dimensions: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingVectorStoreBackend {
    Uninitialized,
    SqliteVec,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryManifestSnapshot {
    pub repository_id: String,
    pub snapshot_id: String,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceEventRow {
    pub trace_id: String,
    pub tool_name: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticChunkEmbeddingRecord {
    pub chunk_id: String,
    pub repository_id: String,
    pub snapshot_id: String,
    pub path: String,
    pub language: String,
    pub chunk_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub provider: String,
    pub model: String,
    pub trace_id: Option<String>,
    pub content_hash_blake3: String,
    pub content_text: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticChunkEmbeddingProjection {
    pub chunk_id: String,
    pub repository_id: String,
    pub snapshot_id: String,
    pub path: String,
    pub language: String,
    pub embedding: Vec<f32>,
}

impl Storage {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    pub fn new_provenance_trace_id(_tool_name: &str) -> String {
        uuid::Uuid::now_v7().to_string()
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn initialize(&self) -> FriggResult<()> {
        let mut conn = open_connection(&self.db_path)?;

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to configure sqlite pragmas: {err}"))
        })?;

        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_version (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to initialize schema version tracking: {err}"
            ))
        })?;

        let current_version = read_schema_version(&conn)?;
        for migration in MIGRATIONS {
            if migration.version > current_version {
                apply_migration(&mut conn, migration)?;
            }
        }

        initialize_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;

        Ok(())
    }

    pub fn schema_version(&self) -> FriggResult<i64> {
        let conn = open_connection(&self.db_path)?;
        if !table_exists(&conn, "schema_version")? {
            return Ok(0);
        }

        read_schema_version(&conn)
    }

    pub fn verify(&self) -> FriggResult<()> {
        let mut conn = open_connection(&self.db_path)?;

        for table in REQUIRED_TABLES {
            if !table_exists(&conn, table)? {
                return Err(FriggError::Internal(format!(
                    "storage verification failed: missing required table '{table}'"
                )));
            }
        }

        let version = read_schema_version(&conn)?;
        let latest = latest_schema_version();
        if version != latest {
            return Err(FriggError::Internal(format!(
                "storage verification failed: schema version mismatch (found {version}, expected {latest})"
            )));
        }

        run_repository_roundtrip_probe(&mut conn)?;
        verify_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;

        Ok(())
    }

    pub fn initialize_vector_store(
        &self,
        expected_dimensions: usize,
    ) -> FriggResult<VectorStoreStatus> {
        let conn = open_connection(&self.db_path)?;
        initialize_vector_store_on_connection(&conn, expected_dimensions)
    }

    pub fn verify_vector_store(
        &self,
        expected_dimensions: usize,
    ) -> FriggResult<VectorStoreStatus> {
        let conn = open_connection(&self.db_path)?;
        verify_vector_store_on_connection(&conn, expected_dimensions)
    }

    pub fn upsert_manifest(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        entries: &[ManifestEntry],
    ) -> FriggResult<()> {
        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start manifest upsert transaction for snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            r#"
            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            VALUES (?1, ?2, 'manifest', NULL, STRFTIME('%Y-%m-%dT%H:%M:%fZ', 'now'))
            ON CONFLICT(snapshot_id) DO UPDATE SET
                repository_id = excluded.repository_id
            "#,
            [snapshot_id, repository_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to upsert snapshot metadata for '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM file_manifest WHERE snapshot_id = ?1",
            [snapshot_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to clear existing manifest rows for snapshot '{snapshot_id}': {err}"
            ))
        })?;

        let mut ordered_entries = entries.to_vec();
        ordered_entries.sort_by(|left, right| left.path.cmp(&right.path));

        let mut insert_stmt = tx
            .prepare(
                r#"
                INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare manifest insert statement for snapshot '{snapshot_id}': {err}"
                ))
            })?;

        for entry in ordered_entries {
            insert_stmt
                .execute((
                    snapshot_id,
                    entry.path,
                    entry.sha256,
                    u64_to_i64(entry.size_bytes, "size_bytes")?,
                    option_u64_to_option_i64(entry.mtime_ns, "mtime_ns")?,
                ))
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert manifest row for snapshot '{snapshot_id}': {err}"
                    ))
                })?;
        }

        drop(insert_stmt);

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit manifest upsert for snapshot '{snapshot_id}': {err}"
            ))
        })?;

        Ok(())
    }

    pub fn load_manifest_for_snapshot(&self, snapshot_id: &str) -> FriggResult<Vec<ManifestEntry>> {
        let conn = open_connection(&self.db_path)?;
        load_manifest_entries_for_snapshot(&conn, snapshot_id)
    }

    pub fn load_latest_manifest_for_repository(
        &self,
        repository_id: &str,
    ) -> FriggResult<Option<RepositoryManifestSnapshot>> {
        let conn = open_connection(&self.db_path)?;
        let snapshot_id: Option<String> = conn
            .query_row(
                r#"
                SELECT snapshot_id
                FROM snapshot
                WHERE repository_id = ?1
                ORDER BY created_at DESC, rowid DESC
                LIMIT 1
                "#,
                [repository_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query latest snapshot for repository '{repository_id}': {err}"
                ))
            })?;

        let snapshot_id = match snapshot_id {
            Some(snapshot_id) => snapshot_id,
            None => return Ok(None),
        };

        let entries = load_manifest_entries_for_snapshot(&conn, &snapshot_id)?;
        Ok(Some(RepositoryManifestSnapshot {
            repository_id: repository_id.to_owned(),
            snapshot_id,
            entries,
        }))
    }

    pub fn delete_snapshot(&self, snapshot_id: &str) -> FriggResult<()> {
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start snapshot delete transaction for '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM semantic_chunk_embedding WHERE snapshot_id = ?1",
            [snapshot_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to delete semantic embeddings for snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM file_manifest WHERE snapshot_id = ?1",
            [snapshot_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to delete manifest rows for snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute("DELETE FROM snapshot WHERE snapshot_id = ?1", [snapshot_id])
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to delete snapshot metadata for '{snapshot_id}': {err}"
                ))
            })?;

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit snapshot delete for '{snapshot_id}': {err}"
            ))
        })?;

        Ok(())
    }

    pub fn replace_semantic_embeddings_for_repository(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[SemanticChunkEmbeddingRecord],
    ) -> FriggResult<()> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        for record in records {
            validate_semantic_chunk_embedding_record(record, repository_id, snapshot_id)?;
        }

        let mut conn = open_connection(&self.db_path)?;
        let _ = verify_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start semantic embedding replace transaction for repository '{repository_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM semantic_chunk_embedding WHERE repository_id = ?1",
            [repository_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to clear semantic embeddings for repository '{repository_id}': {err}"
            ))
        })?;

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by(semantic_chunk_embedding_record_order);
        let mut insert_stmt = tx
            .prepare(
                r#"
                INSERT INTO semantic_chunk_embedding (
                    chunk_id,
                    repository_id,
                    snapshot_id,
                    path,
                    language,
                    chunk_index,
                    start_line,
                    end_line,
                    provider,
                    model,
                    trace_id,
                    content_hash_blake3,
                    content_text,
                    embedding_blob,
                    dimensions
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic embedding insert statement for repository '{repository_id}': {err}"
                ))
            })?;

        for record in ordered_records {
            let dimensions = record.embedding.len();
            let embedding_blob = encode_f32_vector(&record.embedding);
            insert_stmt
                .execute((
                    record.chunk_id,
                    repository_id,
                    snapshot_id,
                    record.path,
                    record.language,
                    usize_to_i64(record.chunk_index, "chunk_index")?,
                    usize_to_i64(record.start_line, "start_line")?,
                    usize_to_i64(record.end_line, "end_line")?,
                    record.provider,
                    record.model,
                    record.trace_id,
                    record.content_hash_blake3,
                    record.content_text,
                    embedding_blob,
                    usize_to_i64(dimensions, "dimensions")?,
                ))
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert semantic embedding for repository '{repository_id}': {err}"
                    ))
                })?;
        }
        drop(insert_stmt);

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit semantic embedding replace for repository '{repository_id}': {err}"
            ))
        })?;
        Ok(())
    }

    pub fn advance_semantic_embeddings_for_repository(
        &self,
        repository_id: &str,
        previous_snapshot_id: Option<&str>,
        snapshot_id: &str,
        changed_paths: &[String],
        deleted_paths: &[String],
        records: &[SemanticChunkEmbeddingRecord],
    ) -> FriggResult<()> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }
        let previous_snapshot_id = previous_snapshot_id
            .map(str::trim)
            .filter(|value| !value.is_empty());

        for record in records {
            validate_semantic_chunk_embedding_record(record, repository_id, snapshot_id)?;
        }

        let mut conn = open_connection(&self.db_path)?;
        let _ = verify_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start semantic embedding advance transaction for repository '{repository_id}': {err}"
            ))
        })?;

        match previous_snapshot_id {
            Some(previous_snapshot_id) if previous_snapshot_id != snapshot_id => {
                tx.execute(
                    r#"
                    UPDATE semantic_chunk_embedding
                    SET snapshot_id = ?1
                    WHERE repository_id = ?2 AND snapshot_id = ?3
                    "#,
                    (snapshot_id, repository_id, previous_snapshot_id),
                )
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to advance unchanged semantic embeddings for repository '{repository_id}': {err}"
                    ))
                })?;
            }
            _ => {
                tx.execute(
                    "DELETE FROM semantic_chunk_embedding WHERE repository_id = ?1",
                    [repository_id],
                )
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to clear semantic embeddings for repository '{repository_id}': {err}"
                    ))
                })?;
            }
        }

        let mut removed_paths = changed_paths
            .iter()
            .chain(deleted_paths.iter())
            .map(|path| path.trim())
            .filter(|path| !path.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        removed_paths.sort();
        removed_paths.dedup();
        for path in &removed_paths {
            tx.execute(
                "DELETE FROM semantic_chunk_embedding WHERE repository_id = ?1 AND path = ?2",
                (repository_id, path),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to delete semantic embeddings for repository '{repository_id}' path '{path}': {err}"
                ))
            })?;
        }

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by(semantic_chunk_embedding_record_order);
        let mut insert_stmt = tx
            .prepare(
                r#"
                INSERT OR REPLACE INTO semantic_chunk_embedding (
                    chunk_id,
                    repository_id,
                    snapshot_id,
                    path,
                    language,
                    chunk_index,
                    start_line,
                    end_line,
                    provider,
                    model,
                    trace_id,
                    content_hash_blake3,
                    content_text,
                    embedding_blob,
                    dimensions
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic embedding advance statement for repository '{repository_id}': {err}"
                ))
            })?;

        for record in ordered_records {
            let dimensions = record.embedding.len();
            let embedding_blob = encode_f32_vector(&record.embedding);
            insert_stmt
                .execute((
                    record.chunk_id,
                    repository_id,
                    snapshot_id,
                    record.path,
                    record.language,
                    usize_to_i64(record.chunk_index, "chunk_index")?,
                    usize_to_i64(record.start_line, "start_line")?,
                    usize_to_i64(record.end_line, "end_line")?,
                    record.provider,
                    record.model,
                    record.trace_id,
                    record.content_hash_blake3,
                    record.content_text,
                    embedding_blob,
                    usize_to_i64(dimensions, "dimensions")?,
                ))
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to upsert semantic embedding for repository '{repository_id}': {err}"
                    ))
                })?;
        }
        drop(insert_stmt);

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit semantic embedding advance for repository '{repository_id}': {err}"
            ))
        })?;
        Ok(())
    }

    pub fn load_semantic_embeddings_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<SemanticChunkEmbeddingRecord>> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT
                    chunk_id,
                    repository_id,
                    snapshot_id,
                    path,
                    language,
                    chunk_index,
                    start_line,
                    end_line,
                    provider,
                    model,
                    trace_id,
                    content_hash_blake3,
                    content_text,
                    embedding_blob,
                    dimensions
                FROM semantic_chunk_embedding
                WHERE repository_id = ?1 AND snapshot_id = ?2
                ORDER BY path ASC, chunk_index ASC, chunk_id ASC
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic embedding load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let rows = statement
            .query_map((repository_id, snapshot_id), |row| {
                let embedding_blob: Vec<u8> = row.get(13)?;
                let dimensions_raw: i64 = row.get(14)?;
                let embedding = decode_f32_vector(&embedding_blob).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        13,
                        rusqlite::types::Type::Blob,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                    )
                })?;
                let dimensions = i64_to_u64(dimensions_raw, "dimensions")? as usize;
                if embedding.len() != dimensions {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        14,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "semantic embedding dimensions mismatch for chunk: decoded={} stored={dimensions}",
                                embedding.len()
                            ),
                        )),
                    ));
                }
                Ok(SemanticChunkEmbeddingRecord {
                    chunk_id: row.get(0)?,
                    repository_id: row.get(1)?,
                    snapshot_id: row.get(2)?,
                    path: row.get(3)?,
                    language: row.get(4)?,
                    chunk_index: i64_to_u64(row.get::<_, i64>(5)?, "chunk_index")? as usize,
                    start_line: i64_to_u64(row.get::<_, i64>(6)?, "start_line")? as usize,
                    end_line: i64_to_u64(row.get::<_, i64>(7)?, "end_line")? as usize,
                    provider: row.get(8)?,
                    model: row.get(9)?,
                    trace_id: row.get(10)?,
                    content_hash_blake3: row.get(11)?,
                    content_text: row.get(12)?,
                    embedding,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic embeddings for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic embeddings for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn load_semantic_embedding_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<SemanticChunkEmbeddingProjection>> {
        self.load_semantic_embedding_projections_for_repository_snapshot_model(
            repository_id,
            snapshot_id,
            None,
            None,
        )
    }

    pub fn load_semantic_embedding_projections_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> FriggResult<Vec<SemanticChunkEmbeddingProjection>> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT
                    chunk_id,
                    repository_id,
                    snapshot_id,
                    path,
                    language,
                    embedding_blob,
                    dimensions
                FROM semantic_chunk_embedding
                WHERE repository_id = ?1
                  AND snapshot_id = ?2
                  AND (?3 IS NULL OR provider = ?3)
                  AND (?4 IS NULL OR model = ?4)
                ORDER BY path ASC, chunk_index ASC, chunk_id ASC
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic embedding projection query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let rows = statement
            .query_map((repository_id, snapshot_id, provider, model), |row| {
                let embedding_blob: Vec<u8> = row.get(5)?;
                let dimensions_raw: i64 = row.get(6)?;
                let embedding = decode_f32_vector(&embedding_blob).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Blob,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                    )
                })?;
                let dimensions = i64_to_u64(dimensions_raw, "dimensions")? as usize;
                if embedding.len() != dimensions {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "semantic embedding dimensions mismatch for projection: decoded={} stored={dimensions}",
                                embedding.len()
                            ),
                        )),
                    ));
                }
                Ok(SemanticChunkEmbeddingProjection {
                    chunk_id: row.get(0)?,
                    repository_id: row.get(1)?,
                    snapshot_id: row.get(2)?,
                    path: row.get(3)?,
                    language: row.get(4)?,
                    embedding,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic embedding projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic embedding projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn has_semantic_embeddings_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<bool> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let exists: i64 = conn
            .query_row(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM semantic_chunk_embedding
                    WHERE repository_id = ?1
                      AND snapshot_id = ?2
                      AND provider = ?3
                      AND model = ?4
                )
                "#,
                (repository_id, snapshot_id, provider, model),
                |row| row.get(0),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic embedding presence for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        Ok(exists == 1)
    }

    pub fn count_semantic_embeddings_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<usize> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let count: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM semantic_chunk_embedding
                WHERE repository_id = ?1
                  AND snapshot_id = ?2
                  AND provider = ?3
                  AND model = ?4
                "#,
                (repository_id, snapshot_id, provider, model),
                |row| row.get(0),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to count semantic embeddings for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        usize::try_from(count).map_err(|err| {
            FriggError::Internal(format!(
                "semantic embedding count overflow for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
            ))
        })
    }

    pub fn load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<Option<String>> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let snapshot_id = conn
            .query_row(
                r#"
                SELECT snapshot.snapshot_id
                FROM snapshot
                WHERE snapshot.repository_id = ?1
                  AND snapshot.kind = 'manifest'
                  AND EXISTS(
                    SELECT 1
                    FROM semantic_chunk_embedding
                    WHERE semantic_chunk_embedding.repository_id = snapshot.repository_id
                      AND semantic_chunk_embedding.snapshot_id = snapshot.snapshot_id
                      AND semantic_chunk_embedding.provider = ?2
                      AND semantic_chunk_embedding.model = ?3
                  )
                ORDER BY snapshot.created_at DESC, snapshot.rowid DESC
                LIMIT 1
                "#,
                (repository_id, provider, model),
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query latest semantic-populated manifest snapshot for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        Ok(snapshot_id)
    }

    pub fn load_semantic_chunk_texts_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        chunk_ids: &[String],
    ) -> FriggResult<BTreeMap<String, String>> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }
        if chunk_ids.is_empty() {
            return Ok(BTreeMap::new());
        }

        let conn = open_connection(&self.db_path)?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT chunk_id, content_text
                FROM semantic_chunk_embedding
                WHERE repository_id = ?1 AND snapshot_id = ?2 AND chunk_id = ?3
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic chunk text lookup for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let mut ordered_chunk_ids = chunk_ids.to_vec();
        ordered_chunk_ids.sort();
        ordered_chunk_ids.dedup();

        let mut texts = BTreeMap::new();
        for chunk_id in ordered_chunk_ids {
            let maybe_text = statement
                .query_row((repository_id, snapshot_id, chunk_id.as_str()), |row| {
                    let returned_chunk_id: String = row.get(0)?;
                    let content_text: String = row.get(1)?;
                    Ok((returned_chunk_id, content_text))
                })
                .optional()
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to query semantic chunk text for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            if let Some((returned_chunk_id, content_text)) = maybe_text {
                texts.insert(returned_chunk_id, content_text);
            }
        }

        Ok(texts)
    }

    pub fn append_provenance_event(
        &self,
        trace_id: &str,
        tool_name: &str,
        payload_json: &Value,
    ) -> FriggResult<()> {
        let trace_id = trace_id.trim();
        if trace_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "trace_id must not be empty".to_owned(),
            ));
        }

        let tool_name = tool_name.trim();
        if tool_name.is_empty() {
            return Err(FriggError::InvalidInput(
                "tool_name must not be empty".to_owned(),
            ));
        }

        let payload_raw = serde_json::to_string(payload_json).map_err(|err| {
            FriggError::Internal(format!(
                "failed to serialize provenance payload for tool '{tool_name}': {err}"
            ))
        })?;

        let conn = open_connection(&self.db_path)?;
        let mut attempt_ms = 0i64;
        loop {
            let insert_result = conn.execute(
                r#"
                INSERT INTO provenance_event (trace_id, tool_name, payload_json, created_at)
                VALUES (
                    ?1,
                    ?2,
                    ?3,
                    printf(
                        '%s-%03d',
                        STRFTIME('%Y-%m-%dT%H:%M:%fZ', 'now'),
                        ?4
                    )
                )
                "#,
                (trace_id, tool_name, &payload_raw, attempt_ms),
            );

            match insert_result {
                Ok(_) => return Ok(()),
                Err(rusqlite::Error::SqliteFailure(err, _))
                    if err.code == ErrorCode::ConstraintViolation
                        && attempt_ms < PROVENANCE_CREATED_AT_MAX_RETRY_MS =>
                {
                    attempt_ms += 1;
                }
                Err(err) => {
                    return Err(FriggError::Internal(format!(
                        "failed to persist provenance event for tool '{tool_name}': {err}"
                    )));
                }
            }
        }
    }

    pub fn load_provenance_events_for_tool(
        &self,
        tool_name: &str,
        limit: usize,
    ) -> FriggResult<Vec<ProvenanceEventRow>> {
        let tool_name = tool_name.trim();
        if tool_name.is_empty() {
            return Err(FriggError::InvalidInput(
                "tool_name must not be empty".to_owned(),
            ));
        }
        if limit == 0 {
            return Err(FriggError::InvalidInput(
                "limit must be greater than zero".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT trace_id, tool_name, payload_json, created_at
                FROM provenance_event
                WHERE tool_name = ?1
                ORDER BY created_at DESC, rowid DESC
                LIMIT ?2
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare provenance query for tool '{tool_name}': {err}"
                ))
            })?;

        let rows = statement
            .query_map((tool_name, usize_to_i64(limit, "limit")?), |row| {
                Ok(ProvenanceEventRow {
                    trace_id: row.get(0)?,
                    tool_name: row.get(1)?,
                    payload_json: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query provenance events for tool '{tool_name}': {err}"
                ))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode provenance events for tool '{tool_name}': {err}"
            ))
        })
    }
}

fn open_connection(path: &Path) -> FriggResult<Connection> {
    ensure_sqlite_vec_auto_extension_registered()?;
    let conn = Connection::open(path)
        .map_err(|err| FriggError::Internal(format!("failed to open sqlite db: {err}")))?;
    ensure_sqlite_vec_registration_readiness(&conn)?;
    Ok(conn)
}

fn load_manifest_entries_for_snapshot(
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

fn u64_to_i64(value: u64, field_name: &str) -> FriggResult<i64> {
    i64::try_from(value).map_err(|_| {
        FriggError::Internal(format!(
            "failed to persist manifest field '{field_name}': value {value} exceeds sqlite INTEGER range"
        ))
    })
}

fn usize_to_i64(value: usize, field_name: &str) -> FriggResult<i64> {
    i64::try_from(value).map_err(|_| {
        FriggError::Internal(format!(
            "failed to persist field '{field_name}': value {value} exceeds sqlite INTEGER range"
        ))
    })
}

fn option_u64_to_option_i64(value: Option<u64>, field_name: &str) -> FriggResult<Option<i64>> {
    value
        .map(|current| u64_to_i64(current, field_name))
        .transpose()
}

fn i64_to_u64(value: i64, field_name: &str) -> rusqlite::Result<u64> {
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

fn option_i64_to_option_u64(value: Option<i64>, field_name: &str) -> rusqlite::Result<Option<u64>> {
    value
        .map(|current| i64_to_u64(current, field_name))
        .transpose()
}

fn read_schema_version(conn: &Connection) -> FriggResult<i64> {
    conn.query_row(
        "SELECT version FROM schema_version WHERE id = 1",
        [],
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| FriggError::Internal(format!("failed to query schema version: {err}")))?
    .map_or(Ok(0), Ok)
}

fn apply_migration(conn: &mut Connection, migration: &Migration) -> FriggResult<()> {
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

fn set_schema_version(tx: &Transaction<'_>, version: i64) -> FriggResult<()> {
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

fn table_exists(conn: &Connection, table_name: &str) -> FriggResult<bool> {
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

fn latest_schema_version() -> i64 {
    MIGRATIONS.last().map_or(0, |migration| migration.version)
}

fn run_repository_roundtrip_probe(conn: &mut Connection) -> FriggResult<()> {
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
    use std::{env, fs};

    use super::{
        DEFAULT_VECTOR_DIMENSIONS, ManifestEntry, PROVENANCE_STORAGE_DB_FILE,
        PROVENANCE_STORAGE_DIR, SQLITE_VEC_REQUIRED_VERSION, SemanticChunkEmbeddingRecord, Storage,
        VECTOR_TABLE_NAME, ensure_provenance_db_parent_dir, ensure_sqlite_vec_pinned_version,
        initialize_vector_store_on_connection_with_detected_capability, resolve_provenance_db_path,
        table_exists, verify_vector_store_on_connection_with_detected_capability,
    };
    use crate::domain::{FriggError, FriggResult};
    use rusqlite::Connection;
    use serde_json::json;
    use uuid::Uuid;

    #[test]
    fn initialize_applies_base_schema_and_version() -> FriggResult<()> {
        let db_path = temp_db_path("init-base-schema");
        let storage = Storage::new(&db_path);

        storage.initialize()?;

        assert_eq!(storage.schema_version()?, 3);

        let conn = open_test_connection(&db_path)?;
        for table in [
            "schema_version",
            "repository",
            "snapshot",
            "file_manifest",
            "provenance_event",
            "semantic_chunk_embedding",
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

        assert_eq!(storage.schema_version()?, 3);

        let conn = open_test_connection(&db_path)?;
        let schema_version_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .map_err(|err| {
                FriggError::Internal(format!("failed to count schema version rows: {err}"))
            })?;
        assert_eq!(schema_version_rows, 1);

        let repository_rows: i64 = conn
            .query_row("SELECT COUNT(*) FROM repository", [], |row| row.get(0))
            .map_err(|err| {
                FriggError::Internal(format!("failed to count repository rows: {err}"))
            })?;
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
    fn manifest_load_latest_for_repository_breaks_timestamp_ties_by_insertion_order()
    -> FriggResult<()> {
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
    fn semantic_embedding_replace_and_load_roundtrip_is_deterministic() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-embedding-roundtrip");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-001",
            &[
                semantic_record(
                    "chunk-z",
                    "repo-1",
                    "snapshot-001",
                    "src/zeta.rs",
                    "rust",
                    2,
                    11,
                    20,
                    "openai",
                    "text-embedding-3-small",
                    Some("trace-001"),
                    "hash-z",
                    "fn zeta() {}",
                    &[0.3, 0.4, 0.5],
                ),
                semantic_record(
                    "chunk-a",
                    "repo-1",
                    "snapshot-001",
                    "src/alpha.rs",
                    "rust",
                    0,
                    1,
                    10,
                    "openai",
                    "text-embedding-3-small",
                    Some("trace-001"),
                    "hash-a",
                    "fn alpha() {}",
                    &[0.1, 0.2, 0.3],
                ),
            ],
        )?;

        let loaded =
            storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?;
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].path, "src/alpha.rs");
        assert_eq!(loaded[1].path, "src/zeta.rs");
        assert_eq!(loaded[0].chunk_index, 0);
        assert_eq!(loaded[1].chunk_index, 2);
        assert_eq!(loaded[0].embedding, vec![0.1, 0.2, 0.3]);
        assert_eq!(loaded[1].embedding, vec![0.3, 0.4, 0.5]);

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_embedding_replace_is_repository_scoped() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-embedding-replace-scoped");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-001",
            &[semantic_record(
                "chunk-repo1-old",
                "repo-1",
                "snapshot-001",
                "src/old.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-old",
                "fn old() {}",
                &[0.1, 0.2],
            )],
        )?;
        storage.replace_semantic_embeddings_for_repository(
            "repo-2",
            "snapshot-101",
            &[semantic_record(
                "chunk-repo2",
                "repo-2",
                "snapshot-101",
                "src/repo2.rs",
                "rust",
                0,
                1,
                3,
                "google",
                "gemini-embedding-001",
                Some("trace-101"),
                "hash-repo2",
                "fn repo2() {}",
                &[0.9, 0.8],
            )],
        )?;

        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-002",
            &[semantic_record(
                "chunk-repo1-new",
                "repo-1",
                "snapshot-002",
                "src/new.rs",
                "rust",
                1,
                20,
                30,
                "openai",
                "text-embedding-3-small",
                Some("trace-002"),
                "hash-new",
                "fn new() {}",
                &[0.7, 0.6],
            )],
        )?;

        assert!(
            storage
                .load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?
                .is_empty(),
            "old repo-1 snapshot should be replaced"
        );
        let repo1_new =
            storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-002")?;
        assert_eq!(repo1_new.len(), 1);
        assert_eq!(repo1_new[0].chunk_id, "chunk-repo1-new");

        let repo2_existing =
            storage.load_semantic_embeddings_for_repository_snapshot("repo-2", "snapshot-101")?;
        assert_eq!(repo2_existing.len(), 1);
        assert_eq!(repo2_existing[0].chunk_id, "chunk-repo2");

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_embedding_replace_rejects_invalid_records_without_mutation() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-embedding-replace-invalid");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-001",
            &[semantic_record(
                "chunk-valid",
                "repo-1",
                "snapshot-001",
                "src/valid.rs",
                "rust",
                0,
                1,
                4,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-valid",
                "fn valid() {}",
                &[0.5, 0.4],
            )],
        )?;

        let mut invalid = semantic_record(
            "chunk-invalid",
            "repo-1",
            "snapshot-002",
            "src/invalid.rs",
            "rust",
            0,
            1,
            4,
            "openai",
            "text-embedding-3-small",
            Some("trace-002"),
            "hash-invalid",
            "fn invalid() {}",
            &[0.2, 0.1],
        );
        invalid.embedding.clear();
        let error = storage
            .replace_semantic_embeddings_for_repository("repo-1", "snapshot-002", &[invalid])
            .expect_err("semantic replace should fail for empty embeddings");
        assert!(
            matches!(error, FriggError::InvalidInput(_)),
            "expected invalid input error, got {error}"
        );

        let existing =
            storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?;
        assert_eq!(existing.len(), 1);
        assert_eq!(existing[0].chunk_id, "chunk-valid");

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_embedding_projection_and_text_lookup_are_deterministic() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-embedding-projection");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-001",
            &[
                semantic_record(
                    "chunk-b",
                    "repo-1",
                    "snapshot-001",
                    "src/b.rs",
                    "rust",
                    1,
                    20,
                    30,
                    "openai",
                    "text-embedding-3-small",
                    Some("trace-001"),
                    "hash-b",
                    "fn b() {}",
                    &[0.3, 0.4],
                ),
                semantic_record(
                    "chunk-a",
                    "repo-1",
                    "snapshot-001",
                    "src/a.rs",
                    "rust",
                    0,
                    1,
                    10,
                    "openai",
                    "text-embedding-3-small",
                    Some("trace-001"),
                    "hash-a",
                    "fn a() {}",
                    &[0.1, 0.2],
                ),
            ],
        )?;

        let projections = storage.load_semantic_embedding_projections_for_repository_snapshot(
            "repo-1",
            "snapshot-001",
        )?;
        assert_eq!(projections.len(), 2);
        assert_eq!(projections[0].chunk_id, "chunk-a");
        assert_eq!(projections[0].path, "src/a.rs");
        assert_eq!(projections[0].embedding, vec![0.1, 0.2]);
        assert_eq!(projections[1].chunk_id, "chunk-b");
        assert!(
            storage.has_semantic_embeddings_for_repository_snapshot_model(
                "repo-1",
                "snapshot-001",
                "openai",
                "text-embedding-3-small",
            )?
        );
        assert!(
            !storage.has_semantic_embeddings_for_repository_snapshot_model(
                "repo-1",
                "snapshot-001",
                "google",
                "gemini-embedding-001",
            )?
        );
        assert_eq!(
            storage.count_semantic_embeddings_for_repository_snapshot_model(
                "repo-1",
                "snapshot-001",
                "openai",
                "text-embedding-3-small",
            )?,
            2
        );
        assert_eq!(
            storage.count_semantic_embeddings_for_repository_snapshot_model(
                "repo-1",
                "snapshot-001",
                "google",
                "gemini-embedding-001",
            )?,
            0
        );

        let filtered = storage.load_semantic_embedding_projections_for_repository_snapshot_model(
            "repo-1",
            "snapshot-001",
            Some("openai"),
            Some("text-embedding-3-small"),
        )?;
        assert_eq!(filtered.len(), 2);
        let empty_filtered = storage
            .load_semantic_embedding_projections_for_repository_snapshot_model(
                "repo-1",
                "snapshot-001",
                Some("google"),
                Some("gemini-embedding-001"),
            )?;
        assert!(empty_filtered.is_empty());

        let texts = storage.load_semantic_chunk_texts_for_repository_snapshot(
            "repo-1",
            "snapshot-001",
            &[
                "chunk-b".to_owned(),
                "chunk-a".to_owned(),
                "chunk-b".to_owned(),
            ],
        )?;
        assert_eq!(texts.len(), 2);
        assert_eq!(texts.get("chunk-a").map(String::as_str), Some("fn a() {}"));
        assert_eq!(texts.get("chunk-b").map(String::as_str), Some("fn b() {}"));

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_embedding_latest_snapshot_lookup_prefers_newest_eligible_snapshot()
    -> FriggResult<()> {
        let db_path = temp_db_path("semantic-embedding-latest-snapshot");
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
        storage.upsert_manifest(
            "repo-1",
            "snapshot-003",
            &[manifest_entry("src/alpha.rs", "hash-a3", 12, Some(120))],
        )?;

        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-001",
            &[semantic_record(
                "chunk-old",
                "repo-1",
                "snapshot-001",
                "src/alpha.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-old",
                "fn old() {}",
                &[0.1, 0.2],
            )],
        )?;
        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-002",
            &[semantic_record(
                "chunk-newer",
                "repo-1",
                "snapshot-002",
                "src/alpha.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-002"),
                "hash-newer",
                "fn newer() {}",
                &[0.3, 0.4],
            )],
        )?;

        assert_eq!(
            storage
                .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                    "repo-1",
                    "openai",
                    "text-embedding-3-small",
                )?,
            Some("snapshot-002".to_owned())
        );
        assert_eq!(
            storage
                .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                    "repo-1",
                    "google",
                    "gemini-embedding-001",
                )?,
            None
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_embedding_latest_manifest_snapshot_lookup_prefers_newest_compatible_snapshot()
    -> FriggResult<()> {
        let db_path = temp_db_path("semantic-embedding-latest-compatible-snapshot");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        storage.upsert_manifest(
            "repo-1",
            "snapshot-001",
            &[manifest_entry("src/a.rs", "hash-a1", 10, Some(100))],
        )?;
        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-001",
            &[semantic_record(
                "chunk-a",
                "repo-1",
                "snapshot-001",
                "src/a.rs",
                "rust",
                0,
                1,
                10,
                "openai",
                "text-embedding-3-small",
                Some("trace-001"),
                "hash-a1",
                "fn a() {}",
                &[0.1, 0.2],
            )],
        )?;
        storage.upsert_manifest(
            "repo-1",
            "snapshot-002",
            &[manifest_entry("src/a.rs", "hash-a2", 11, Some(110))],
        )?;

        let latest_manifest = storage
            .load_latest_manifest_for_repository("repo-1")?
            .expect("expected latest manifest snapshot");
        assert_eq!(latest_manifest.snapshot_id, "snapshot-002");
        assert_eq!(
            storage
                .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                    "repo-1",
                    "openai",
                    "text-embedding-3-small",
                )?,
            Some("snapshot-001".to_owned())
        );
        assert_eq!(
            storage
                .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                    "repo-1",
                    "google",
                    "gemini-embedding-001",
                )?,
            None
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_embedding_advance_preserves_unchanged_rows_and_replaces_changed_paths()
    -> FriggResult<()> {
        let db_path = temp_db_path("semantic-embedding-advance");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        storage.replace_semantic_embeddings_for_repository(
            "repo-1",
            "snapshot-001",
            &[
                semantic_record(
                    "chunk-keep",
                    "repo-1",
                    "snapshot-001",
                    "src/keep.rs",
                    "rust",
                    0,
                    1,
                    10,
                    "openai",
                    "text-embedding-3-small",
                    Some("trace-001"),
                    "hash-keep-old",
                    "fn keep_old() {}",
                    &[0.1, 0.2],
                ),
                semantic_record(
                    "chunk-change-old",
                    "repo-1",
                    "snapshot-001",
                    "src/change.rs",
                    "rust",
                    0,
                    1,
                    10,
                    "openai",
                    "text-embedding-3-small",
                    Some("trace-001"),
                    "hash-change-old",
                    "fn change_old() {}",
                    &[0.3, 0.4],
                ),
                semantic_record(
                    "chunk-delete-old",
                    "repo-1",
                    "snapshot-001",
                    "src/delete.rs",
                    "rust",
                    0,
                    1,
                    10,
                    "openai",
                    "text-embedding-3-small",
                    Some("trace-001"),
                    "hash-delete-old",
                    "fn delete_old() {}",
                    &[0.5, 0.6],
                ),
            ],
        )?;

        storage.advance_semantic_embeddings_for_repository(
            "repo-1",
            Some("snapshot-001"),
            "snapshot-002",
            &["src/change.rs".to_owned()],
            &["src/delete.rs".to_owned()],
            &[semantic_record(
                "chunk-change-new",
                "repo-1",
                "snapshot-002",
                "src/change.rs",
                "rust",
                0,
                11,
                20,
                "openai",
                "text-embedding-3-small",
                Some("trace-002"),
                "hash-change-new",
                "fn change_new() {}",
                &[0.7, 0.8],
            )],
        )?;

        assert!(
            storage
                .load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-001")?
                .is_empty(),
            "old snapshot rows should be advanced or removed"
        );

        let current =
            storage.load_semantic_embeddings_for_repository_snapshot("repo-1", "snapshot-002")?;
        assert_eq!(current.len(), 2);
        assert_eq!(current[0].chunk_id, "chunk-change-new");
        assert_eq!(current[0].content_text, "fn change_new() {}");
        assert_eq!(current[1].chunk_id, "chunk-keep");
        assert_eq!(current[1].content_text, "fn keep_old() {}");
        assert!(
            current.iter().all(|record| record.path != "src/delete.rs"),
            "deleted semantic path should be removed from latest snapshot"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn provenance_append_and_load_for_tool() -> FriggResult<()> {
        let db_path = temp_db_path("provenance-append-load");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        storage.append_provenance_event(
            "trace-read-file-001",
            "read_file",
            &json!({
                "tool_name": "read_file",
                "params": { "path": "src/lib.rs" },
            }),
        )?;
        storage.append_provenance_event(
            "trace-read-file-002",
            "read_file",
            &json!({
                "tool_name": "read_file",
                "params": { "path": "src/main.rs" },
            }),
        )?;
        storage.append_provenance_event(
            "trace-search-text-001",
            "search_text",
            &json!({
                "tool_name": "search_text",
                "params": { "query": "hello" },
            }),
        )?;

        let rows = storage.load_provenance_events_for_tool("read_file", 5)?;
        assert_eq!(rows.len(), 2);
        assert!(
            rows.iter().all(|row| row.tool_name == "read_file"),
            "expected only read_file provenance rows"
        );
        assert!(
            rows.iter()
                .all(|row| row.payload_json.contains("\"tool_name\":\"read_file\"")),
            "expected serialized payloads to include the tool_name marker"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn provenance_path_resolution_for_write_creates_parent_within_canonical_root() -> FriggResult<()>
    {
        let workspace_root = temp_workspace_root("provenance-path-safe");
        fs::create_dir_all(&workspace_root).map_err(FriggError::Io)?;

        let db_path = ensure_provenance_db_parent_dir(&workspace_root)?;
        let canonical_root = workspace_root.canonicalize().map_err(FriggError::Io)?;
        let expected = canonical_root
            .join(PROVENANCE_STORAGE_DIR)
            .join(PROVENANCE_STORAGE_DB_FILE);

        assert_eq!(db_path, expected);
        let parent = db_path
            .parent()
            .expect("resolved provenance db path should always have a parent");
        assert!(
            parent.is_dir(),
            "expected provenance storage parent directory to exist"
        );

        let resolved = resolve_provenance_db_path(&workspace_root)?;
        assert_eq!(resolved, expected);

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn provenance_path_resolution_rejects_symlink_escape_before_write() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("provenance-path-symlink-escape");
        let repo_root = workspace_root.join("repo");
        let escaped_root = workspace_root.join("escaped-store");
        fs::create_dir_all(&repo_root).map_err(FriggError::Io)?;
        fs::create_dir_all(&escaped_root).map_err(FriggError::Io)?;

        let provenance_link = repo_root.join(PROVENANCE_STORAGE_DIR);
        create_dir_symlink(&escaped_root, &provenance_link)?;

        let resolve_err = resolve_provenance_db_path(&repo_root)
            .expect_err("symlink escape should be rejected while resolving provenance db path");
        assert!(
            matches!(resolve_err, FriggError::AccessDenied(_)),
            "expected access denied for symlink escape, got {resolve_err}"
        );

        let prepare_err = ensure_provenance_db_parent_dir(&repo_root)
            .expect_err("symlink escape should be rejected before creating storage parent dir");
        assert!(
            matches!(
                prepare_err,
                FriggError::AccessDenied(ref message)
                    if message.contains("escapes canonical workspace root boundary")
            ),
            "expected access denied for symlink escape, got {prepare_err}"
        );

        assert!(
            !escaped_root.join(PROVENANCE_STORAGE_DB_FILE).exists(),
            "provenance db write should not escape via symlinked storage directory"
        );

        let _ = fs::remove_file(&provenance_link);
        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[test]
    fn vector_store_initialize_and_verify_roundtrip() -> FriggResult<()> {
        let db_path = temp_db_path("vector-store-roundtrip");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        let status = storage.verify_vector_store(DEFAULT_VECTOR_DIMENSIONS)?;
        assert_eq!(status.expected_dimensions, DEFAULT_VECTOR_DIMENSIONS);
        assert_eq!(status.table_name, VECTOR_TABLE_NAME);
        assert!(
            !status.extension_version.trim().is_empty(),
            "vector extension version should not be empty"
        );

        let conn = open_test_connection(&db_path)?;
        assert!(
            table_exists(&conn, VECTOR_TABLE_NAME)?,
            "expected vector table '{VECTOR_TABLE_NAME}' to exist"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn vector_store_verify_fails_on_dimension_mismatch() -> FriggResult<()> {
        let db_path = temp_db_path("vector-store-dimension-mismatch");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        let err = storage
            .verify_vector_store(DEFAULT_VECTOR_DIMENSIONS + 1)
            .expect_err("verify_vector_store should fail when expected dimensions mismatch");
        let err_message = err.to_string();
        assert!(
            err_message.contains("vector table dimensions mismatch"),
            "unexpected vector-store mismatch error: {err_message}"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn vector_store_verify_rejects_zero_dimensions_as_invalid_input() -> FriggResult<()> {
        let db_path = temp_db_path("vector-store-zero-dimensions");
        let storage = Storage::new(&db_path);
        storage.initialize()?;

        let err = storage
            .verify_vector_store(0)
            .expect_err("verify_vector_store should reject zero dimensions");
        assert!(
            matches!(
                err,
                FriggError::InvalidInput(ref message)
                    if message == "expected_dimensions must be greater than zero"
            ),
            "expected invalid_input for zero dimensions, got {err}"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn vector_store_initialize_rejects_zero_dimensions_as_invalid_input() {
        let db_path = temp_db_path("vector-store-init-zero-dimensions");
        let storage = Storage::new(&db_path);

        let err = storage
            .initialize_vector_store(0)
            .expect_err("initialize_vector_store should reject zero dimensions");
        assert!(
            matches!(
                err,
                FriggError::InvalidInput(ref message)
                    if message == "expected_dimensions must be greater than zero"
            ),
            "expected invalid_input for zero dimensions, got {err}"
        );

        cleanup_db(&db_path);
    }

    #[test]
    fn vector_store_detected_capability_rejects_unavailable_sqlite_vec() -> FriggResult<()> {
        let db_path = temp_db_path("vector-transition-sqlite-vec-missing");
        let conn = open_test_connection(&db_path)?;
        create_sqlite_vec_like_table(&conn, DEFAULT_VECTOR_DIMENSIONS)?;

        let err = verify_vector_store_on_connection_with_detected_capability(
            &conn,
            DEFAULT_VECTOR_DIMENSIONS,
            None,
        )
        .expect_err("sqlite-vec readiness should fail when extension is unavailable");
        let err_message = err.to_string();
        assert!(
            err_message.contains("sqlite-vec extension is unavailable"),
            "unexpected unavailable-extension error: {err_message}"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn vector_store_rejects_legacy_non_sqlite_vec_schema() -> FriggResult<()> {
        let db_path = temp_db_path("vector-transition-vec-blocked");
        let conn = open_test_connection(&db_path)?;
        conn.execute_batch(&format!(
            r#"
                CREATE TABLE {VECTOR_TABLE_NAME} (
                  embedding_id TEXT PRIMARY KEY,
                  embedding BLOB NOT NULL,
                  dimensions INTEGER NOT NULL,
                  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                );
                "#
        ))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to seed legacy fallback-style table for transition tests: {err}"
            ))
        })?;

        let err = verify_vector_store_on_connection_with_detected_capability(
            &conn,
            DEFAULT_VECTOR_DIMENSIONS,
            Some(format!("v{SQLITE_VEC_REQUIRED_VERSION}")),
        )
        .expect_err("legacy fallback-style schema should be rejected");
        let err_message = err.to_string();
        assert!(
            err_message.contains("legacy non-sqlite-vec schema detected"),
            "unexpected legacy schema error: {err_message}"
        );

        let init_err = initialize_vector_store_on_connection_with_detected_capability(
            &conn,
            DEFAULT_VECTOR_DIMENSIONS,
            Some(format!("v{SQLITE_VEC_REQUIRED_VERSION}")),
        )
        .expect_err("initialize should reject legacy fallback-style schema");
        let init_err_message = init_err.to_string();
        assert!(
            init_err_message.contains("legacy non-sqlite-vec schema detected"),
            "unexpected legacy schema error during initialize: {init_err_message}"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn sqlite_vec_version_pin_accepts_prefixed_and_unprefixed_versions() -> FriggResult<()> {
        ensure_sqlite_vec_pinned_version(SQLITE_VEC_REQUIRED_VERSION)?;
        ensure_sqlite_vec_pinned_version(&format!("v{SQLITE_VEC_REQUIRED_VERSION}"))?;
        ensure_sqlite_vec_pinned_version(&format!("V{SQLITE_VEC_REQUIRED_VERSION}"))?;
        Ok(())
    }

    #[test]
    fn sqlite_vec_version_pin_rejects_mismatch_deterministically() {
        let err = ensure_sqlite_vec_pinned_version("v0.0.0")
            .expect_err("mismatched sqlite-vec runtime version should be rejected");
        let message = err.to_string();
        assert!(
            message.contains("sqlite-vec extension version mismatch"),
            "unexpected version mismatch message: {message}"
        );
        assert!(
            message.contains("v0.0.0"),
            "mismatch message should include found runtime version: {message}"
        );
        assert!(
            message.contains(SQLITE_VEC_REQUIRED_VERSION),
            "mismatch message should include required pinned version: {message}"
        );
    }

    #[test]
    fn vector_store_detected_capability_rejects_mismatched_sqlite_vec_version() -> FriggResult<()> {
        let db_path = temp_db_path("vector-transition-version-mismatch");
        let conn = open_test_connection(&db_path)?;
        create_sqlite_vec_like_table(&conn, DEFAULT_VECTOR_DIMENSIONS)?;

        let err = verify_vector_store_on_connection_with_detected_capability(
            &conn,
            DEFAULT_VECTOR_DIMENSIONS,
            Some("v0.0.0".to_string()),
        )
        .expect_err("mismatched sqlite-vec version should fail readiness");
        let err_message = err.to_string();
        assert!(
            err_message.contains("sqlite-vec extension version mismatch"),
            "unexpected mismatch error: {err_message}"
        );
        assert!(
            err_message.contains(SQLITE_VEC_REQUIRED_VERSION),
            "mismatch error should include pinned version: {err_message}"
        );

        cleanup_db(&db_path);
        Ok(())
    }

    fn temp_db_path(test_name: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!(
            "frigg-storage-{test_name}-{}.sqlite3",
            Uuid::now_v7()
        ))
    }

    fn temp_workspace_root(test_name: &str) -> std::path::PathBuf {
        env::temp_dir().join(format!(
            "frigg-storage-workspace-{test_name}-{}",
            Uuid::now_v7()
        ))
    }

    fn open_test_connection(path: &std::path::Path) -> FriggResult<Connection> {
        Connection::open(path).map_err(|err| {
            FriggError::Internal(format!(
                "failed to open sqlite db for test assertions: {err}"
            ))
        })
    }

    fn index_exists(conn: &Connection, index_name: &str) -> FriggResult<bool> {
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

    fn explain_query_plan(conn: &Connection, query: &str) -> FriggResult<Vec<String>> {
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

    fn cleanup_db(path: &std::path::Path) {
        let _ = fs::remove_file(path);
    }

    fn cleanup_workspace(path: &std::path::Path) {
        let _ = fs::remove_dir_all(path);
    }

    #[cfg(unix)]
    fn create_dir_symlink(target: &std::path::Path, link: &std::path::Path) -> FriggResult<()> {
        std::os::unix::fs::symlink(target, link).map_err(FriggError::Io)?;
        Ok(())
    }

    fn create_sqlite_vec_like_table(conn: &Connection, dimensions: usize) -> FriggResult<()> {
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
    fn semantic_record(
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

    fn manifest_entry(
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
}
