use super::*;

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
