use super::*;

impl Storage {
    pub fn upsert_repository(
        &self,
        repository_id: &str,
        root_path: &std::path::Path,
        display_name: &str,
    ) -> FriggResult<()> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }

        let display_name = display_name.trim();
        if display_name.is_empty() {
            return Err(FriggError::InvalidInput(
                "display_name must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        conn.execute(
            r#"
            INSERT INTO repository (repository_id, root_path, display_name, created_at)
            VALUES (?1, ?2, ?3, STRFTIME('%Y-%m-%dT%H:%M:%fZ', 'now'))
            ON CONFLICT(repository_id) DO UPDATE SET
                root_path = excluded.root_path,
                display_name = excluded.display_name
            "#,
            (
                repository_id,
                root_path.to_string_lossy().into_owned(),
                display_name,
            ),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to upsert repository metadata for '{repository_id}': {err}"
            ))
        })?;

        Ok(())
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
            INSERT OR IGNORE INTO repository (repository_id, root_path, display_name, created_at)
            VALUES (?1, '/legacy-import', ?1, STRFTIME('%Y-%m-%dT%H:%M:%fZ', 'now'))
            "#,
            [repository_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to seed repository stub row for snapshot '{snapshot_id}' in repository '{repository_id}': {err}"
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
            "DELETE FROM test_subject_projection WHERE snapshot_id = ?1",
            [snapshot_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to delete test subject projection rows for snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM entrypoint_surface_projection WHERE snapshot_id = ?1",
            [snapshot_id],
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to delete entrypoint surface projection rows for snapshot '{snapshot_id}': {err}"
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
}
