//! Durable storage for manifests, semantic state, and retrieval projections. Storage is the
//! handoff point that lets indexing, search, and MCP runtime share consistent repository state
//! across process boundaries and refresh cycles.

use std::collections::BTreeMap;
use std::fs;
use std::os::raw::{c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::domain::{FriggError, FriggResult, PathClass, SourceClass};
use rusqlite::Connection;

mod db_runtime;
mod lifecycle;
mod manifest_store;
mod projection_store;
mod provenance_path;
mod schema;
mod semantic_store;
mod types;
mod vector_store;
#[cfg(test)]
pub(crate) use db_runtime::{
    DEFAULT_SQLITE_BUSY_TIMEOUT_MS, reset_semantic_read_trace, snapshot_semantic_read_trace,
    sqlite_busy_timeout_ms_from_raw,
};
use db_runtime::{
    count_snapshots_for_repository_and_kind, database_has_user_tables, i64_to_u64,
    load_latest_manifest_metadata_snapshot_for_repository,
    load_latest_manifest_snapshot_for_repository, load_latest_manifest_snapshot_id_for_repository,
    load_manifest_entries_for_snapshot, load_manifest_metadata_entries_for_snapshot,
    load_semantic_head_snapshot_ids_for_repository, load_snapshot_ids_for_repository_and_kind,
    open_connection, open_existing_connection, option_u64_to_option_i64, read_schema_version,
    run_repository_roundtrip_probe, set_schema_version, table_exists, u64_to_i64, usize_to_i64,
};
pub use provenance_path::{
    ensure_provenance_db_parent_dir, resolve_provenance_db_path,
    resolve_workspace_relative_write_path,
};
pub(crate) use schema::{CURRENT_SCHEMA_SQL, CURRENT_SCHEMA_VERSION, REQUIRED_TABLES};
pub use types::*;
#[cfg(test)]
pub(crate) use vector_store::{
    ensure_sqlite_vec_pinned_version,
    initialize_vector_store_on_connection_with_detected_capability,
    verify_vector_store_on_connection_with_detected_capability,
};
use vector_store::{initialize_vector_store_on_connection, verify_vector_store_on_connection};

pub(super) const SNAPSHOT_KIND_MANIFEST: &str = "manifest";

const INVARIANT_MANIFEST_ROWS_REQUIRE_MANIFEST_SNAPSHOTS: &str =
    "manifest_rows_require_manifest_snapshots";
const INVARIANT_SEMANTIC_HEAD_REQUIRES_MANIFEST_SNAPSHOT: &str =
    "semantic_head_requires_manifest_snapshot";
const INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC: &str = "semantic_vector_partition_in_sync";

/// Owns Frigg's durable SQLite state so indexing and serving can share the same repository
/// snapshots, projections, and semantic artifacts.
#[derive(Debug, Clone)]
pub struct Storage {
    db_path: PathBuf,
}

/// SQLite connection reused across one index pass to avoid per-phase open/schema probes.
#[derive(Debug)]
pub(crate) struct StorageSession {
    db_path: PathBuf,
    conn: Connection,
}

/// Default embedding width expected by semantic vector storage.
pub const DEFAULT_VECTOR_DIMENSIONS: usize = 1_536;
/// sqlite-vec table holding live semantic embedding vectors.
pub const VECTOR_TABLE_NAME: &str = "embedding_vectors";
/// Manifest snapshot epochs retained per repository after pruning.
pub const DEFAULT_RETAINED_MANIFEST_SNAPSHOTS: usize = 8;
const SQLITE_VEC_MAX_KNN_LIMIT: usize = 4_096;
const SQLITE_VEC_REQUIRED_VERSION: &str = "0.1.7-alpha.10";
/// Hidden workspace directory containing durable Frigg storage artifacts.
pub const PROVENANCE_STORAGE_DIR: &str = ".frigg";
/// SQLite database filename under [`PROVENANCE_STORAGE_DIR`].
pub const PROVENANCE_STORAGE_DB_FILE: &str = "storage.sqlite3";
static SQLITE_VEC_AUTO_EXTENSION_REGISTRATION: OnceLock<Result<(), String>> = OnceLock::new();
static SQLITE_VEC_CONNECTION_READINESS: OnceLock<Result<String, String>> = OnceLock::new();

/// Latest durable storage schema version expected by this build.
pub fn latest_storage_schema_version() -> i64 {
    CURRENT_SCHEMA_VERSION
}

#[allow(unsafe_code)]
unsafe extern "C" {
    fn sqlite3_vec_init(
        db: *mut rusqlite::ffi::sqlite3,
        pz_err_msg: *mut *mut c_char,
        api: *const rusqlite::ffi::sqlite3_api_routines,
    ) -> c_int;
}

/// Vector index backend backing semantic KNN queries.
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

/// Observed vector-store initialization state after extension registration.
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

#[cfg(test)]
mod tests;
