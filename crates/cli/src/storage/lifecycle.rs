//! Storage initialization, verification, and vector-store lifecycle entry points.

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

    pub(crate) fn initialize_without_vector_store(&self) -> FriggResult<()> {
        self.initialize_with_vector_store(false)
    }

    fn incompatible_schema_error(&self, found_version: i64) -> FriggError {
        FriggError::Internal(format!(
            "storage schema is incompatible (found {found_version}, expected {CURRENT_SCHEMA_VERSION}); automatic schema migrations are disabled for Frigg's regenerable local index; delete '{}' and run `frigg index` to rebuild it",
            self.db_path.display()
        ))
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

    /// Verifies schema version, required tables, and storage invariants.
    pub fn verify(&self) -> FriggResult<()> {
        let mut conn = open_existing_connection(&self.db_path)?;
        self.require_current_schema_on_connection(&conn)?;
        self.verify_required_tables_on_connection(&conn)?;

        run_repository_roundtrip_probe(&mut conn)?;
        verify_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        self.verify_storage_invariants_with_connection(&conn)?;

        Ok(())
    }

    pub fn verify_relational_schema(&self) -> FriggResult<()> {
        let mut conn = open_existing_connection(&self.db_path)?;
        self.require_current_schema_on_connection(&conn)?;
        self.verify_required_tables_on_connection(&conn)?;
        run_repository_roundtrip_probe(&mut conn)?;
        self.verify_storage_invariants_with_connection(&conn)?;
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

        let version = read_schema_version(&conn)?;
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
        let conn = open_existing_connection(&self.db_path)?;
        verify_vector_store_on_connection(&conn, expected_dimensions)
    }
}
