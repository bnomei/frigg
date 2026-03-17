use std::collections::BTreeMap;
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::domain::{FriggError, FriggResult, PathClass, SourceClass};
use rusqlite::{Connection, ErrorCode};
use serde_json::Value;
#[allow(unused_imports)]
use sqlite_vec as _;

mod db_runtime;
mod manifest_store;
mod projection_store;
mod provenance_path;
mod provenance_store;
mod semantic_store;
mod vector_store;
#[cfg(test)]
use db_runtime::set_schema_version;
use db_runtime::{
    apply_migration, count_provenance_events, count_snapshots_for_repository_and_kind, i64_to_u64,
    latest_schema_version, load_latest_manifest_metadata_snapshot_for_repository,
    load_latest_manifest_snapshot_for_repository, load_manifest_entries_for_snapshot,
    load_semantic_head_snapshot_ids_for_repository, load_snapshot_ids_for_repository_and_kind,
    open_connection, option_u64_to_option_i64, prune_provenance_events_on_connection,
    read_schema_version, run_repository_roundtrip_probe, table_exists, u64_to_i64, usize_to_i64,
};

pub(super) const SNAPSHOT_KIND_MANIFEST: &str = "manifest";
pub use provenance_path::{ensure_provenance_db_parent_dir, resolve_provenance_db_path};
#[cfg(test)]
pub(crate) use vector_store::{
    encode_f32_vector, ensure_sqlite_vec_pinned_version,
    initialize_vector_store_on_connection_with_detected_capability,
    verify_vector_store_on_connection_with_detected_capability,
};
use vector_store::{initialize_vector_store_on_connection, verify_vector_store_on_connection};

const INVARIANT_MANIFEST_ROWS_REQUIRE_MANIFEST_SNAPSHOTS: &str =
    "manifest_rows_require_manifest_snapshots";
const INVARIANT_SEMANTIC_HEAD_REQUIRES_MANIFEST_SNAPSHOT: &str =
    "semantic_head_requires_manifest_snapshot";
const INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC: &str = "semantic_vector_partition_in_sync";

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
    Migration {
        version: 7,
        sql: r#"
            CREATE TABLE IF NOT EXISTS test_subject_projection (
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              test_path TEXT NOT NULL,
              subject_path TEXT NOT NULL,
              shared_terms_json TEXT NOT NULL,
              score_hint INTEGER NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, test_path, subject_path)
            );

            CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_test
            ON test_subject_projection (repository_id, snapshot_id, test_path, subject_path);

            CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_subject
            ON test_subject_projection (repository_id, snapshot_id, subject_path, test_path);

            CREATE TABLE IF NOT EXISTS entrypoint_surface_projection (
              repository_id TEXT NOT NULL,
              snapshot_id TEXT NOT NULL,
              path TEXT NOT NULL,
              path_class TEXT NOT NULL,
              source_class TEXT NOT NULL,
              path_terms_json TEXT NOT NULL,
              surface_terms_json TEXT NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            CREATE INDEX IF NOT EXISTS idx_entrypoint_surface_projection_repo_snapshot_path
            ON entrypoint_surface_projection (repository_id, snapshot_id, path);
        "#,
    },
    Migration {
        version: 8,
        sql: r#"
            ALTER TABLE snapshot RENAME TO snapshot_v8;

            INSERT INTO repository (repository_id, root_path, display_name, created_at)
            SELECT DISTINCT
                snapshot_v8.repository_id,
                '/legacy-import',
                snapshot_v8.repository_id,
                CURRENT_TIMESTAMP
            FROM snapshot_v8
            WHERE NOT EXISTS (
                SELECT 1
                FROM repository
                WHERE repository.repository_id = snapshot_v8.repository_id
            );

            CREATE TABLE snapshot (
              snapshot_id TEXT PRIMARY KEY,
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              kind TEXT NOT NULL,
              revision TEXT,
              created_at TEXT NOT NULL
            );

            INSERT INTO snapshot (snapshot_id, repository_id, kind, revision, created_at)
            SELECT snapshot_id, repository_id, kind, revision, created_at
            FROM snapshot_v8;

            DROP TABLE snapshot_v8;

            CREATE INDEX IF NOT EXISTS idx_snapshot_repository_created_snapshot
            ON snapshot (repository_id, created_at DESC, snapshot_id DESC);

            ALTER TABLE file_manifest RENAME TO file_manifest_v8;

            CREATE TABLE IF NOT EXISTS file_manifest (
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              sha256 TEXT NOT NULL,
              size_bytes INTEGER NOT NULL,
              mtime_ns INTEGER,
              PRIMARY KEY (snapshot_id, path)
            );

            INSERT INTO file_manifest (snapshot_id, path, sha256, size_bytes, mtime_ns)
            SELECT snapshot_id, path, sha256, size_bytes, mtime_ns
            FROM file_manifest_v8;

            DROP TABLE file_manifest_v8;

            ALTER TABLE path_witness_projection RENAME TO path_witness_projection_v8;

            CREATE TABLE IF NOT EXISTS path_witness_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              path_class TEXT NOT NULL,
              source_class TEXT NOT NULL,
              path_terms_json TEXT NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            INSERT INTO path_witness_projection (
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                flags_json,
                created_at
            )
            SELECT
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                flags_json,
                created_at
            FROM path_witness_projection_v8;

            DROP TABLE path_witness_projection_v8;

            CREATE INDEX IF NOT EXISTS idx_path_witness_projection_repo_snapshot_path
            ON path_witness_projection (repository_id, snapshot_id, path);

            ALTER TABLE test_subject_projection RENAME TO test_subject_projection_v8;

            CREATE TABLE IF NOT EXISTS test_subject_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              test_path TEXT NOT NULL,
              subject_path TEXT NOT NULL,
              shared_terms_json TEXT NOT NULL,
              score_hint INTEGER NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, test_path, subject_path)
            );

            INSERT INTO test_subject_projection (
                repository_id,
                snapshot_id,
                test_path,
                subject_path,
                shared_terms_json,
                score_hint,
                flags_json,
                created_at
            )
            SELECT
                repository_id,
                snapshot_id,
                test_path,
                subject_path,
                shared_terms_json,
                score_hint,
                flags_json,
                created_at
            FROM test_subject_projection_v8;

            DROP TABLE test_subject_projection_v8;

            CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_test
            ON test_subject_projection (repository_id, snapshot_id, test_path, subject_path);

            CREATE INDEX IF NOT EXISTS idx_test_subject_projection_repo_snapshot_subject
            ON test_subject_projection (repository_id, snapshot_id, subject_path, test_path);

            ALTER TABLE entrypoint_surface_projection RENAME TO entrypoint_surface_projection_v8;

            CREATE TABLE IF NOT EXISTS entrypoint_surface_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              path_class TEXT NOT NULL,
              source_class TEXT NOT NULL,
              path_terms_json TEXT NOT NULL,
              surface_terms_json TEXT NOT NULL,
              flags_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            INSERT INTO entrypoint_surface_projection (
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                surface_terms_json,
                flags_json,
                created_at
            )
            SELECT
                repository_id,
                snapshot_id,
                path,
                path_class,
                source_class,
                path_terms_json,
                surface_terms_json,
                flags_json,
                created_at
            FROM entrypoint_surface_projection_v8;

            DROP TABLE entrypoint_surface_projection_v8;

            CREATE INDEX IF NOT EXISTS idx_entrypoint_surface_projection_repo_snapshot_path
            ON entrypoint_surface_projection (repository_id, snapshot_id, path);
        "#,
    },
    Migration {
        version: 9,
        sql: r#"
            ALTER TABLE path_witness_projection
            ADD COLUMN file_stem TEXT NOT NULL DEFAULT '';

            ALTER TABLE path_witness_projection
            ADD COLUMN subtree_root TEXT;

            ALTER TABLE path_witness_projection
            ADD COLUMN family_bits INTEGER NOT NULL DEFAULT 0;

            ALTER TABLE path_witness_projection
            ADD COLUMN heuristic_version INTEGER NOT NULL DEFAULT 0;

            CREATE TABLE IF NOT EXISTS retrieval_projection_head (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              family TEXT NOT NULL,
              heuristic_version INTEGER NOT NULL,
              input_modes_json TEXT NOT NULL,
              row_count INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, family)
            );

            CREATE INDEX IF NOT EXISTS idx_retrieval_projection_head_repo_snapshot_family
            ON retrieval_projection_head (repository_id, snapshot_id, family);

            CREATE TABLE IF NOT EXISTS path_relation_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
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

            CREATE INDEX IF NOT EXISTS idx_path_relation_projection_repo_snapshot_src
            ON path_relation_projection (repository_id, snapshot_id, src_path, relation_kind, dst_path);

            CREATE INDEX IF NOT EXISTS idx_path_relation_projection_repo_snapshot_dst
            ON path_relation_projection (repository_id, snapshot_id, dst_path, relation_kind, src_path);

            CREATE TABLE IF NOT EXISTS subtree_coverage_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              subtree_root TEXT NOT NULL,
              family TEXT NOT NULL,
              path_count INTEGER NOT NULL,
              exemplar_path TEXT NOT NULL,
              exemplar_score_hint INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, subtree_root, family)
            );

            CREATE INDEX IF NOT EXISTS idx_subtree_coverage_projection_repo_snapshot_subtree
            ON subtree_coverage_projection (repository_id, snapshot_id, subtree_root, family);

            CREATE TABLE IF NOT EXISTS path_surface_term_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
              path TEXT NOT NULL,
              term_weights_json TEXT NOT NULL,
              exact_terms_json TEXT NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
              PRIMARY KEY (repository_id, snapshot_id, path)
            );

            CREATE INDEX IF NOT EXISTS idx_path_surface_term_projection_repo_snapshot_path
            ON path_surface_term_projection (repository_id, snapshot_id, path);

            CREATE TABLE IF NOT EXISTS path_anchor_sketch_projection (
              repository_id TEXT NOT NULL REFERENCES repository(repository_id) ON DELETE CASCADE,
              snapshot_id TEXT NOT NULL REFERENCES snapshot(snapshot_id) ON DELETE CASCADE,
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

            CREATE INDEX IF NOT EXISTS idx_path_anchor_sketch_projection_repo_snapshot_path
            ON path_anchor_sketch_projection (repository_id, snapshot_id, path, anchor_rank);
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
    "test_subject_projection",
    "entrypoint_surface_projection",
    "retrieval_projection_head",
    "path_relation_projection",
    "subtree_coverage_projection",
    "path_surface_term_projection",
    "path_anchor_sketch_projection",
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
pub struct PathWitnessProjection {
    pub path: String,
    pub path_class: PathClass,
    pub source_class: SourceClass,
    pub file_stem: String,
    pub path_terms: Vec<String>,
    pub subtree_root: Option<String>,
    pub family_bits: u64,
    pub flags_json: String,
    pub heuristic_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSubjectProjection {
    pub test_path: String,
    pub subject_path: String,
    pub shared_terms: Vec<String>,
    pub score_hint: usize,
    pub flags_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrypointSurfaceProjection {
    pub path: String,
    pub path_class: PathClass,
    pub source_class: SourceClass,
    pub path_terms: Vec<String>,
    pub surface_terms: Vec<String>,
    pub flags_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalProjectionHeadRecord {
    pub family: String,
    pub heuristic_version: i64,
    pub input_modes: Vec<String>,
    pub row_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRelationProjection {
    pub src_path: String,
    pub dst_path: String,
    pub relation_kind: String,
    pub evidence_source: String,
    pub src_symbol_id: Option<String>,
    pub dst_symbol_id: Option<String>,
    pub src_family_bits: u64,
    pub dst_family_bits: u64,
    pub shared_terms: Vec<String>,
    pub score_hint: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtreeCoverageProjection {
    pub subtree_root: String,
    pub family: String,
    pub path_count: usize,
    pub exemplar_path: String,
    pub exemplar_score_hint: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSurfaceTermProjection {
    pub path: String,
    pub term_weights: BTreeMap<String, u16>,
    pub exact_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAnchorSketchProjection {
    pub path: String,
    pub anchor_rank: usize,
    pub line: usize,
    pub anchor_kind: String,
    pub excerpt: String,
    pub terms: Vec<String>,
    pub score_hint: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetrievalProjectionBundle {
    pub heads: Vec<RetrievalProjectionHeadRecord>,
    pub path_witness: Vec<PathWitnessProjection>,
    pub test_subject: Vec<TestSubjectProjection>,
    pub entrypoint_surface: Vec<EntrypointSurfaceProjection>,
    pub path_relations: Vec<PathRelationProjection>,
    pub subtree_coverage: Vec<SubtreeCoverageProjection>,
    pub path_surface_terms: Vec<PathSurfaceTermProjection>,
    pub path_anchor_sketches: Vec<PathAnchorSketchProjection>,
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
        self.verify_storage_invariants_with_connection(&conn)?;

        Ok(())
    }

    pub fn repair_storage_invariants(&self) -> FriggResult<StorageInvariantRepairSummary> {
        let conn = open_connection(&self.db_path)?;
        let mut repaired_categories = Vec::new();

        let inconsistent_partitions = self.semantic_vector_partition_violations(&conn)?;
        if !inconsistent_partitions.is_empty() {
            self.repair_semantic_vector_store()?;
            repaired_categories.push(INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC.to_string());
        }

        Ok(StorageInvariantRepairSummary {
            repaired_categories,
        })
    }

    fn verify_storage_invariants_with_connection(&self, conn: &Connection) -> FriggResult<()> {
        let invalid_manifest_rows: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM file_manifest AS manifest
                INNER JOIN snapshot ON snapshot.snapshot_id = manifest.snapshot_id
                WHERE snapshot.kind != ?1
                "#,
                [SNAPSHOT_KIND_MANIFEST],
                |row| row.get(0),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "storage verification failed: invariant={} error=failed to count invalid manifest rows: {err}",
                    INVARIANT_MANIFEST_ROWS_REQUIRE_MANIFEST_SNAPSHOTS
                ))
            })?;
        if invalid_manifest_rows > 0 {
            return Err(FriggError::Internal(format!(
                "storage verification failed: invariant={} count={invalid_manifest_rows}",
                INVARIANT_MANIFEST_ROWS_REQUIRE_MANIFEST_SNAPSHOTS
            )));
        }

        let invalid_semantic_heads: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM semantic_head
                LEFT JOIN snapshot
                  ON snapshot.snapshot_id = semantic_head.covered_snapshot_id
                 AND snapshot.repository_id = semantic_head.repository_id
                WHERE snapshot.snapshot_id IS NULL OR snapshot.kind != ?1
                "#,
                [SNAPSHOT_KIND_MANIFEST],
                |row| row.get(0),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "storage verification failed: invariant={} error=failed to count invalid semantic heads: {err}",
                    INVARIANT_SEMANTIC_HEAD_REQUIRES_MANIFEST_SNAPSHOT
                ))
            })?;
        if invalid_semantic_heads > 0 {
            return Err(FriggError::Internal(format!(
                "storage verification failed: invariant={} count={invalid_semantic_heads}",
                INVARIANT_SEMANTIC_HEAD_REQUIRES_MANIFEST_SNAPSHOT
            )));
        }

        let inconsistent_partitions = self.semantic_vector_partition_violations(conn)?;
        if !inconsistent_partitions.is_empty() {
            return Err(FriggError::Internal(format!(
                "storage verification failed: invariant={} count={} partitions={}",
                INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC,
                inconsistent_partitions.len(),
                inconsistent_partitions.join(",")
            )));
        }

        Ok(())
    }

    fn semantic_vector_partition_violations(&self, conn: &Connection) -> FriggResult<Vec<String>> {
        let mut stmt = conn
            .prepare(
                r#"
                SELECT repository_id, provider, model
                FROM semantic_head
                ORDER BY repository_id, provider, model
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "storage verification failed: invariant={} error=failed to prepare semantic partition scan: {err}",
                    INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC
                ))
            })?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "storage verification failed: invariant={} error=failed to iterate semantic partitions: {err}",
                    INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC
                ))
            })?;

        let mut partitions = Vec::new();
        for row in rows {
            let (repository_id, provider, model) = row.map_err(|err| {
                FriggError::Internal(format!(
                    "storage verification failed: invariant={} error=failed to decode semantic partition row: {err}",
                    INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC
                ))
            })?;
            let health = self.collect_semantic_storage_health_for_repository_model(
                &repository_id,
                &provider,
                &model,
            )?;
            if !health.vector_consistent {
                partitions.push(format!("{repository_id}:{provider}:{model}"));
            }
        }

        Ok(partitions)
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
}

#[derive(Debug, Default, Clone)]
pub struct StorageInvariantRepairSummary {
    pub repaired_categories: Vec<String>,
}

#[cfg(test)]
mod tests;
