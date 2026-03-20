//! Durable storage for manifests, semantic state, retrieval projections, and provenance. Storage
//! is the handoff point that lets indexing, search, and MCP runtime share consistent repository
//! state across process boundaries and refresh cycles.

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
mod lifecycle;
mod manifest_store;
mod projection_store;
mod provenance_path;
mod provenance_store;
mod schema;
mod semantic_store;
mod types;
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
#[cfg(test)]
pub(crate) use db_runtime::{reset_semantic_read_trace, snapshot_semantic_read_trace};
pub use provenance_path::{ensure_provenance_db_parent_dir, resolve_provenance_db_path};
pub(crate) use schema::{MIGRATIONS, Migration, REQUIRED_TABLES};
pub use types::*;
#[cfg(test)]
pub(crate) use vector_store::{
    encode_f32_vector, ensure_sqlite_vec_pinned_version,
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

#[derive(Debug, Clone)]
/// Owns Frigg's durable SQLite state so indexing and serving can share the same repository
/// snapshots, projections, semantic artifacts, and provenance history.
pub struct Storage {
    db_path: PathBuf,
    provenance_write_connection: Arc<OnceLock<Mutex<Connection>>>,
}

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

#[cfg(test)]
mod tests;
