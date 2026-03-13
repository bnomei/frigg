use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::domain::{FriggError, FriggResult};
use rusqlite::{Connection, ErrorCode};
use serde_json::Value;
#[allow(unused_imports)]
use sqlite_vec as _;

mod db_runtime;
mod provenance_path;
mod semantic_store;
mod vector_store;
#[cfg(test)]
use db_runtime::set_schema_version;
use db_runtime::{
    apply_migration, count_provenance_events, i64_to_u64, latest_schema_version,
    load_latest_manifest_metadata_snapshot_for_repository,
    load_latest_manifest_snapshot_for_repository, load_manifest_entries_for_snapshot,
    load_semantic_head_snapshot_ids_for_repository, open_connection, option_u64_to_option_i64,
    prune_provenance_events_on_connection, read_schema_version, run_repository_roundtrip_probe,
    table_exists, u64_to_i64, usize_to_i64,
};
pub use provenance_path::{ensure_provenance_db_parent_dir, resolve_provenance_db_path};
#[cfg(test)]
pub(crate) use vector_store::{
    encode_f32_vector, ensure_sqlite_vec_pinned_version,
    initialize_vector_store_on_connection_with_detected_capability,
    verify_vector_store_on_connection_with_detected_capability,
};
use vector_store::{initialize_vector_store_on_connection, verify_vector_store_on_connection};

#[derive(Debug, Clone)]
pub struct Storage {
    db_path: PathBuf,
    provenance_write_connection: Arc<OnceLock<Mutex<Connection>>>,
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
    Migration {
        version: 4,
        sql: r#"
            ALTER TABLE semantic_chunk_embedding RENAME TO semantic_chunk_embedding_v3_legacy;

            CREATE TABLE semantic_chunk (
              chunk_id TEXT NOT NULL,
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              language TEXT NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_line INTEGER NOT NULL,
              end_line INTEGER NOT NULL,
              content_text TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, chunk_id)
            );

            CREATE INDEX idx_semantic_chunk_repo_snapshot_path_chunk
            ON semantic_chunk (repository_id, snapshot_id, path, chunk_index, chunk_id);

            CREATE TABLE semantic_chunk_embedding (
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              chunk_id TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              trace_id TEXT,
              content_hash_blake3 TEXT NOT NULL,
              embedding_blob BLOB NOT NULL,
              dimensions INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, chunk_id, provider, model)
            );

            CREATE INDEX idx_semantic_chunk_embedding_repo_snapshot_model_chunk
            ON semantic_chunk_embedding (repository_id, snapshot_id, provider, model, chunk_id);

            CREATE INDEX idx_semantic_chunk_embedding_repo_model_snapshot_chunk
            ON semantic_chunk_embedding (repository_id, provider, model, snapshot_id, chunk_id);

            INSERT INTO semantic_chunk (
              chunk_id,
              repository_id,
              snapshot_id,
              path,
              language,
              chunk_index,
              start_line,
              end_line,
              content_text,
              created_at
            )
            SELECT DISTINCT
              chunk_id,
              repository_id,
              snapshot_id,
              path,
              language,
              chunk_index,
              start_line,
              end_line,
              content_text,
              created_at
            FROM semantic_chunk_embedding_v3_legacy;

            INSERT INTO semantic_chunk_embedding (
              repository_id,
              snapshot_id,
              chunk_id,
              provider,
              model,
              trace_id,
              content_hash_blake3,
              embedding_blob,
              dimensions,
              created_at
            )
            SELECT
              repository_id,
              snapshot_id,
              chunk_id,
              provider,
              model,
              trace_id,
              content_hash_blake3,
              embedding_blob,
              dimensions,
              created_at
            FROM semantic_chunk_embedding_v3_legacy;

            DROP TABLE semantic_chunk_embedding_v3_legacy;
        "#,
    },
    Migration {
        version: 5,
        sql: r#"
            CREATE TABLE IF NOT EXISTS path_witness_projection (
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              path_class TEXT NOT NULL,
              source_class TEXT NOT NULL,
              path_terms_json TEXT NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            CREATE INDEX IF NOT EXISTS idx_path_witness_projection_repo_snapshot_path
            ON path_witness_projection (repository_id, snapshot_id, path);
        "#,
    },
    Migration {
        version: 6,
        sql: r#"
            DROP TABLE IF EXISTS semantic_chunk_embedding;
            DROP TABLE IF EXISTS semantic_chunk;
            DROP TABLE IF EXISTS semantic_head;

            CREATE TABLE semantic_head (
              repository_id TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              covered_snapshot_id TEXT NOT NULL,
              live_chunk_count INTEGER NOT NULL DEFAULT 0,
              last_refresh_reason TEXT,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, provider, model)
            );

            CREATE INDEX idx_semantic_head_repo_snapshot
            ON semantic_head (repository_id, covered_snapshot_id, provider, model);

            CREATE TABLE semantic_chunk (
              repository_id TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              chunk_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              language TEXT NOT NULL,
              chunk_index INTEGER NOT NULL,
              start_line INTEGER NOT NULL,
              end_line INTEGER NOT NULL,
              content_hash_blake3 TEXT NOT NULL,
              content_text TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, provider, model, chunk_id)
            );

            CREATE INDEX idx_semantic_chunk_repo_model_snapshot_path_chunk
            ON semantic_chunk (repository_id, provider, model, snapshot_id, path, chunk_index, chunk_id);

            CREATE INDEX idx_semantic_chunk_repo_snapshot_path_model
            ON semantic_chunk (repository_id, snapshot_id, path, provider, model, chunk_id);

            CREATE TABLE semantic_chunk_embedding (
              repository_id TEXT NOT NULL,
              provider TEXT NOT NULL,
              model TEXT NOT NULL,
              chunk_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              trace_id TEXT,
              embedding_blob BLOB NOT NULL,
              dimensions INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, provider, model, chunk_id)
            );

            CREATE INDEX idx_semantic_chunk_embedding_repo_model_snapshot_chunk
            ON semantic_chunk_embedding (repository_id, provider, model, snapshot_id, chunk_id);

            CREATE INDEX idx_semantic_chunk_embedding_repo_snapshot_model_chunk
            ON semantic_chunk_embedding (repository_id, snapshot_id, provider, model, chunk_id);
        "#,
    },
];

const REQUIRED_TABLES: &[&str] = &[
    "schema_version",
    "repository",
    "snapshot",
    "file_manifest",
    "provenance_event",
    "semantic_head",
    "semantic_chunk",
    "semantic_chunk_embedding",
    "path_witness_projection",
];

pub const DEFAULT_VECTOR_DIMENSIONS: usize = 1_536;
pub const VECTOR_TABLE_NAME: &str = "embedding_vectors";
pub const DEFAULT_RETAINED_MANIFEST_SNAPSHOTS: usize = 8;
pub const DEFAULT_RETAINED_PROVENANCE_EVENTS: usize = 10_000;
const SQLITE_VEC_MAX_KNN_LIMIT: usize = 4_096;
const SQLITE_VEC_REQUIRED_VERSION: &str = "0.1.7-alpha.10";
pub const PROVENANCE_STORAGE_DIR: &str = ".frigg";
pub const PROVENANCE_STORAGE_DB_FILE: &str = "storage.sqlite3";
const PROVENANCE_CREATED_AT_MAX_RETRY_MS: i64 = 32;
static SQLITE_VEC_AUTO_EXTENSION_REGISTRATION: OnceLock<Result<(), String>> = OnceLock::new();
static SQLITE_VEC_CONNECTION_READINESS: OnceLock<Result<String, String>> = OnceLock::new();

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
pub struct ManifestMetadataEntry {
    pub path: String,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryManifestMetadataSnapshot {
    pub repository_id: String,
    pub snapshot_id: String,
    pub entries: Vec<ManifestMetadataEntry>,
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
    pub start_line: usize,
    pub end_line: usize,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticChunkVectorMatch {
    pub chunk_id: String,
    pub repository_id: String,
    pub snapshot_id: String,
    pub distance: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticChunkPayload {
    pub chunk_id: String,
    pub path: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathWitnessProjectionRecord {
    pub repository_id: String,
    pub snapshot_id: String,
    pub path: String,
    pub path_class: String,
    pub source_class: String,
    pub path_terms_json: String,
    pub flags_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticHeadRecord {
    pub repository_id: String,
    pub provider: String,
    pub model: String,
    pub covered_snapshot_id: String,
    pub live_chunk_count: usize,
    pub last_refresh_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticStorageHealth {
    pub repository_id: String,
    pub provider: String,
    pub model: String,
    pub covered_snapshot_id: Option<String>,
    pub live_chunk_rows: usize,
    pub live_embedding_rows: usize,
    pub live_vector_rows: usize,
    pub retained_manifest_snapshots: usize,
    pub vector_consistent: bool,
}

impl Storage {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
            provenance_write_connection: Arc::new(OnceLock::new()),
        }
    }

    pub fn new_provenance_trace_id(_tool_name: &str) -> String {
        uuid::Uuid::now_v7().to_string()
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    pub fn initialize(&self) -> FriggResult<()> {
        self.initialize_with_vector_store(true)
    }

    pub(crate) fn initialize_without_vector_store(&self) -> FriggResult<()> {
        self.initialize_with_vector_store(false)
    }

    fn initialize_with_vector_store(&self, initialize_vector_store: bool) -> FriggResult<()> {
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

        if initialize_vector_store {
            initialize_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        }

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
        let latest = latest_schema_version(MIGRATIONS);
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
        load_latest_manifest_snapshot_for_repository(&conn, repository_id)
    }

    pub fn load_latest_manifest_metadata_for_repository(
        &self,
        repository_id: &str,
    ) -> FriggResult<Option<RepositoryManifestMetadataSnapshot>> {
        let conn = open_connection(&self.db_path)?;
        load_latest_manifest_metadata_snapshot_for_repository(&conn, repository_id)
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

        let active_semantic_heads: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM semantic_head WHERE covered_snapshot_id = ?1",
                [snapshot_id],
                |row| row.get(0),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic head coverage for snapshot '{snapshot_id}': {err}"
                ))
            })?;
        if active_semantic_heads > 0 {
            return Err(FriggError::InvalidInput(format!(
                "cannot delete snapshot '{snapshot_id}' because it is still covered by the active live semantic corpus; refresh semantics to a newer snapshot or clear the semantic head first"
            )));
        }

        tx.execute(
            "DELETE FROM path_witness_projection WHERE snapshot_id = ?1",
            [snapshot_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to delete path witness projection rows for snapshot '{snapshot_id}': {err}"
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

    pub fn replace_path_witness_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[PathWitnessProjectionRecord],
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

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start path witness projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM path_witness_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id, snapshot_id),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to clear path witness projection rows for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by(|left, right| left.path.cmp(&right.path));
        ordered_records.dedup_by(|left, right| left.path == right.path);

        let mut insert_stmt = tx
            .prepare(
                r#"
                INSERT INTO path_witness_projection (
                  repository_id,
                  snapshot_id,
                  path,
                  path_class,
                  source_class,
                  path_terms_json,
                  flags_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare path witness projection insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        for record in ordered_records {
            insert_stmt
                .execute((
                    repository_id,
                    snapshot_id,
                    record.path,
                    record.path_class,
                    record.source_class,
                    record.path_terms_json,
                    record.flags_json,
                ))
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert path witness projection row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
        }
        drop(insert_stmt);

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit path witness projection replace for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        Ok(())
    }

    pub fn load_path_witness_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<PathWitnessProjectionRecord>> {
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
        let mut stmt = conn
            .prepare(
                r#"
                SELECT repository_id, snapshot_id, path, path_class, source_class, path_terms_json, flags_json
                FROM path_witness_projection
                WHERE repository_id = ?1 AND snapshot_id = ?2
                ORDER BY path ASC
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare path witness projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let rows = stmt
            .query_map((repository_id, snapshot_id), |row| {
                Ok(PathWitnessProjectionRecord {
                    repository_id: row.get(0)?,
                    snapshot_id: row.get(1)?,
                    path: row.get(2)?,
                    path_class: row.get(3)?,
                    source_class: row.get(4)?,
                    path_terms_json: row.get(5)?,
                    flags_json: row.get(6)?,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query path witness projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode path witness projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        Ok(rows)
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

        let conn = if let Some(conn) = self.provenance_write_connection.get() {
            conn
        } else {
            let connection = Mutex::new(open_connection(&self.db_path)?);
            let _ = self.provenance_write_connection.set(connection);
            self.provenance_write_connection
                .get()
                .expect("provenance write connection should be initialized")
        };
        let conn = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
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
                Ok(_) => {
                    prune_provenance_events_on_connection(
                        &conn,
                        DEFAULT_RETAINED_PROVENANCE_EVENTS,
                    )?;
                    return Ok(());
                }
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

    pub fn load_recent_provenance_events(
        &self,
        limit: usize,
    ) -> FriggResult<Vec<ProvenanceEventRow>> {
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
                ORDER BY created_at DESC, rowid DESC
                LIMIT ?1
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!("failed to prepare recent provenance query: {err}"))
            })?;

        let rows = statement
            .query_map((usize_to_i64(limit, "limit")?,), |row| {
                Ok(ProvenanceEventRow {
                    trace_id: row.get(0)?,
                    tool_name: row.get(1)?,
                    payload_json: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!("failed to query recent provenance events: {err}"))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!("failed to decode recent provenance events: {err}"))
        })
    }

    pub fn prune_provenance_events(&self, keep_latest: usize) -> FriggResult<usize> {
        if keep_latest == 0 {
            return Err(FriggError::InvalidInput(
                "keep_latest must be greater than zero".to_owned(),
            ));
        }

        let mut conn = open_connection(&self.db_path)?;
        let before = count_provenance_events(&conn)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start provenance prune transaction: {err}"
            ))
        })?;
        tx.execute(
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
        .map_err(|err| FriggError::Internal(format!("failed to prune provenance events: {err}")))?;
        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit provenance prune transaction: {err}"
            ))
        })?;
        let conn = open_connection(&self.db_path)?;
        let after = count_provenance_events(&conn)?;

        Ok(before.saturating_sub(after))
    }
}

#[cfg(test)]
mod tests;
