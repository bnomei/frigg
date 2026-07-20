//! Storage initialization, verification, and vector-store lifecycle entry points.
//!
//! Coordinates schema init, vector extension readiness, verification, and auto-repair for
//! `.frigg/storage.sqlite3`; callers treat failures here as bootstrap blockers rather than
//! per-query errors.

use std::time::Duration;

use super::*;

fn vector_store_error_is_repairable(message: &str) -> bool {
    message.contains("missing vector table")
        || message.contains("vector table schema mismatch")
        || message.contains("legacy non-sqlite-vec schema")
}

impl Storage {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            db_path: db_path.into(),
        }
    }

    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Opens storage, creates the current schema when empty, and initializes the vector extension.
    pub fn initialize(&self) -> FriggResult<()> {
        self.initialize_with_vector_store(true)
    }

    /// Initializes storage and repairs a missing or incompatible vector table when possible.
    ///
    /// This deliberately does not compare every semantic embedding with the sqlite-vec
    /// projection. That audit is explicit-only; see [`Self::validate_embeddings`].
    pub fn initialize_with_auto_repair(&self) -> FriggResult<Vec<String>> {
        match self.initialize() {
            Ok(()) => Ok(Vec::new()),
            Err(original_err) if vector_store_error_is_repairable(&original_err.to_string()) => {
                self.repair_semantic_vector_store()?;
                self.initialize()?;
                Ok(vec![
                    INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC.to_string(),
                ])
            }
            Err(original_err) => Err(original_err),
        }
    }

    pub(crate) fn initialize_without_vector_store(&self) -> FriggResult<()> {
        self.initialize_with_vector_store(false)
    }

    fn incompatible_schema_error(&self, found_version: i64) -> FriggError {
        FriggError::StorageSchemaIncompatible {
            found_version,
            expected_version: CURRENT_SCHEMA_VERSION,
            db_path: self.db_path.clone(),
        }
    }

    fn uninitialized_schema_error(&self) -> FriggError {
        FriggError::Internal(format!(
            "storage schema is uninitialized: '{}' has no Frigg schema_version row; run `frigg init` or `frigg index` to create current storage, or delete the file first if it is not a Frigg database",
            self.db_path.display()
        ))
    }

    pub fn require_current_schema(&self) -> FriggResult<()> {
        let conn = open_existing_connection(&self.db_path)?;
        self.require_current_schema_on_connection(&conn)
    }

    pub(crate) fn open_current_schema_connection(&self) -> FriggResult<Connection> {
        let conn = open_existing_connection(&self.db_path)?;
        self.require_current_schema_on_connection(&conn)?;
        Ok(conn)
    }

    pub(crate) fn open_session(&self) -> FriggResult<StorageSession> {
        let conn = self.open_current_schema_connection()?;
        Ok(StorageSession {
            db_path: self.db_path.clone(),
            conn,
        })
    }

    pub(crate) fn require_current_schema_on_connection(
        &self,
        conn: &Connection,
    ) -> FriggResult<()> {
        if !table_exists(conn, "schema_version")? {
            if database_has_user_tables(conn)? {
                return Err(self.incompatible_schema_error(0));
            }
            return Err(self.uninitialized_schema_error());
        }

        let current_version = read_schema_version(conn)?;
        if current_version != CURRENT_SCHEMA_VERSION {
            return Err(self.incompatible_schema_error(current_version));
        }

        Ok(())
    }

    fn create_current_schema(&self, conn: &mut Connection) -> FriggResult<()> {
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start current schema initialization transaction: {err}"
            ))
        })?;
        tx.execute_batch(CURRENT_SCHEMA_SQL).map_err(|err| {
            FriggError::Internal(format!(
                "failed to initialize current storage schema: {err}"
            ))
        })?;
        set_schema_version(&tx, CURRENT_SCHEMA_VERSION)?;
        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit current schema initialization transaction: {err}"
            ))
        })?;
        Ok(())
    }

    fn ensure_current_schema(&self, conn: &mut Connection) -> FriggResult<()> {
        if !table_exists(conn, "schema_version")? {
            if database_has_user_tables(conn)? {
                return Err(self.incompatible_schema_error(0));
            }
            return self.create_current_schema(conn);
        }

        let current_version = read_schema_version(conn)?;
        if current_version != CURRENT_SCHEMA_VERSION {
            return Err(self.incompatible_schema_error(current_version));
        }

        Ok(())
    }

    fn initialize_with_vector_store(&self, initialize_vector_store: bool) -> FriggResult<()> {
        let mut conn = open_connection(&self.db_path)?;

        conn.execute_batch(
            r#"
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!("failed to configure sqlite pragmas: {err}"))
        })?;

        self.ensure_current_schema(&mut conn)?;

        if initialize_vector_store {
            initialize_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        }

        Ok(())
    }

    pub fn schema_version(&self) -> FriggResult<i64> {
        let conn = open_existing_connection(&self.db_path)?;
        if !table_exists(&conn, "schema_version")? {
            return Ok(0);
        }

        read_schema_version(&conn)
    }

    /// Inspects schema and manifest readiness through a strictly read-only SQLite connection.
    pub fn inspect_repository_read_only(
        &self,
        repository_id: &str,
    ) -> FriggResult<StorageRepositoryInspection> {
        let conn = open_existing_read_only_connection(&self.db_path)?;
        if !table_exists(&conn, "schema_version")? {
            return Ok(StorageRepositoryInspection {
                schema_version: 0,
                has_manifest: false,
            });
        }

        let schema_version = read_schema_version(&conn)?;
        let has_manifest = if schema_version == CURRENT_SCHEMA_VERSION {
            load_latest_manifest_snapshot_for_repository(&conn, repository_id)?.is_some()
        } else {
            false
        };
        Ok(StorageRepositoryInspection {
            schema_version,
            has_manifest,
        })
    }

    /// Verifies cheap schema and sqlite-vec readiness required by normal runtime paths.
    ///
    /// This never scans semantic row membership. Use [`Self::validate_embeddings`] only for
    /// the explicit `frigg index --validate-embeddings` audit.
    pub fn verify_runtime_readiness(&self) -> FriggResult<()> {
        let mut conn = open_existing_connection(&self.db_path)?;
        self.require_current_schema_on_connection(&conn)?;
        self.verify_required_tables_on_connection(&conn)?;

        run_repository_roundtrip_probe(&mut conn)?;
        verify_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        self.verify_relational_invariants_with_connection(&conn)?;
        Ok(())
    }

    /// Audits every semantic embedding/vector membership pair.
    ///
    /// This may scan a large sqlite-vec table and is intentionally reserved for
    /// `frigg index --validate-embeddings`.
    pub fn validate_embeddings(&self) -> FriggResult<()> {
        self.verify_runtime_readiness()?;
        let conn = self.open_current_schema_connection()?;
        self.verify_embedding_membership_with_connection(&conn)
    }

    /// Verifies cheap relational schema readiness for workspace/status responses.
    pub fn verify_relational_schema(&self) -> FriggResult<()> {
        let mut conn = open_existing_connection(&self.db_path)?;
        self.require_current_schema_on_connection(&conn)?;
        self.verify_required_tables_on_connection(&conn)?;
        run_repository_roundtrip_probe(&mut conn)?;
        self.verify_relational_invariants_with_connection(&conn)?;
        Ok(())
    }

    fn verify_required_tables_on_connection(&self, conn: &Connection) -> FriggResult<()> {
        for table in REQUIRED_TABLES {
            if !table_exists(conn, table)? {
                return Err(FriggError::Internal(format!(
                    "storage verification failed: missing required table '{table}' in '{}'; delete the storage DB and run `frigg index` to rebuild current storage",
                    self.db_path.display()
                )));
            }
        }

        let version = read_schema_version(conn)?;
        if version != CURRENT_SCHEMA_VERSION {
            return Err(FriggError::Internal(format!(
                "storage verification failed: schema version mismatch (found {version}, expected {CURRENT_SCHEMA_VERSION}); automatic schema migrations are disabled; delete '{}' and run `frigg index` to rebuild current storage",
                self.db_path.display()
            )));
        }
        Ok(())
    }

    pub fn repair_storage_invariants(&self) -> FriggResult<StorageInvariantRepairSummary> {
        let conn = self.open_current_schema_connection()?;
        let mut repaired_categories = Vec::new();

        match verify_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS) {
            Ok(_) => {}
            Err(err) if vector_store_error_is_repairable(&err.to_string()) => {
                drop(conn);
                self.repair_semantic_vector_store()?;
                repaired_categories.push(INVARIANT_SEMANTIC_VECTOR_PARTITION_IN_SYNC.to_string());
                return Ok(StorageInvariantRepairSummary {
                    repaired_categories,
                });
            }
            Err(err) => return Err(err),
        }

        Ok(StorageInvariantRepairSummary {
            repaired_categories,
        })
    }

    /// Validates relational invariants with indexed count queries only.
    ///
    /// Keep this separate from embedding membership validation: normal runtime readiness needs
    /// these integrity checks, but must not enumerate sqlite-vec row membership.
    fn verify_relational_invariants_with_connection(&self, conn: &Connection) -> FriggResult<()> {
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

        Ok(())
    }

    /// Performs the potentially expensive exact sqlite-vec membership audit.
    ///
    /// This is reached only by [`Self::validate_embeddings`], which is wired exclusively to
    /// `frigg index --validate-embeddings`.
    fn verify_embedding_membership_with_connection(&self, conn: &Connection) -> FriggResult<()> {
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
            let health =
                self.audit_semantic_embedding_partition(&repository_id, &provider, &model)?;
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
        let conn = open_existing_connection(&self.db_path)?;
        verify_vector_store_on_connection(&conn, expected_dimensions)
    }
}

impl StorageSession {
    pub(crate) fn checkpoint_wal_truncate(&self) -> FriggResult<()> {
        checkpoint_wal_truncate_on_connection(&self.conn, &self.db_path)
    }
}

fn checkpoint_wal_truncate_on_connection(conn: &Connection, db_path: &Path) -> FriggResult<()> {
    let previous_busy_timeout_ms: i64 = conn
        .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to read sqlite busy timeout before WAL checkpoint for '{}': {err}",
                db_path.display()
            ))
        })?;
    let previous_busy_timeout_ms = u64::try_from(previous_busy_timeout_ms).map_err(|err| {
        FriggError::Internal(format!(
            "invalid sqlite busy timeout before WAL checkpoint for '{}': {err}",
            db_path.display()
        ))
    })?;

    conn.busy_timeout(Duration::from_millis(0)).map_err(|err| {
        FriggError::Internal(format!(
            "failed to disable sqlite busy timeout for WAL checkpoint on '{}': {err}",
            db_path.display()
        ))
    })?;
    let checkpoint_result = conn.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, i64>(2)?,
        ))
    });
    let restore_result = conn.busy_timeout(Duration::from_millis(previous_busy_timeout_ms));

    restore_result.map_err(|err| {
        FriggError::Internal(format!(
            "failed to restore sqlite busy timeout after WAL checkpoint for '{}': {err}",
            db_path.display()
        ))
    })?;

    let (busy, _log_pages, _checkpointed_pages) = checkpoint_result.map_err(|err| {
        FriggError::Internal(format!(
            "failed to checkpoint sqlite WAL for '{}': {err}",
            db_path.display()
        ))
    })?;
    if busy > 0 {
        return Err(FriggError::Internal(format!(
            "sqlite WAL checkpoint for '{}' skipped because {busy} connection(s) were busy",
            db_path.display()
        )));
    }
    Ok(())
}
