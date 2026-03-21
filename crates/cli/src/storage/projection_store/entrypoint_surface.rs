use crate::domain::{FriggError, FriggResult, PathClass, SourceClass};
use crate::storage::{EntrypointSurfaceProjection, Storage, db_runtime::open_connection};

use super::common::normalize_repository_snapshot_ids;

impl Storage {
    pub fn replace_entrypoint_surface_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[EntrypointSurfaceProjection],
    ) -> FriggResult<()> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start entrypoint surface projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM entrypoint_surface_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id.as_str(), snapshot_id.as_str()),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to clear entrypoint surface projection rows for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by(|left, right| left.path.cmp(&right.path));
        ordered_records.dedup_by(|left, right| left.path == right.path);

        let mut insert_stmt = tx
            .prepare(
                r#"
                INSERT INTO entrypoint_surface_projection (
                  repository_id,
                  snapshot_id,
                  path,
                  path_class,
                  source_class,
                  path_terms_json,
                  surface_terms_json,
                  flags_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare entrypoint surface projection insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        for record in ordered_records {
            insert_stmt
                .execute((
                    repository_id.as_str(),
                    snapshot_id.as_str(),
                    record.path,
                    record.path_class.as_str(),
                    record.source_class.as_str(),
                    serde_json::to_string(&record.path_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode entrypoint surface path terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    serde_json::to_string(&record.surface_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode entrypoint surface terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    record.flags_json,
                ))
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert entrypoint surface projection row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
        }
        drop(insert_stmt);

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit entrypoint surface projection replace for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        Ok(())
    }

    pub fn load_entrypoint_surface_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<EntrypointSurfaceProjection>> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT repository_id, snapshot_id, path, path_class, source_class, path_terms_json, surface_terms_json, flags_json
                FROM entrypoint_surface_projection
                WHERE repository_id = ?1 AND snapshot_id = ?2
                ORDER BY path ASC
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare entrypoint surface projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let rows = stmt
            .query_map((repository_id.as_str(), snapshot_id.as_str()), |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                ))
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query entrypoint surface projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode entrypoint surface projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?
            .into_iter()
            .map(
                |(
                    path,
                    path_class,
                    source_class,
                    path_terms_json,
                    surface_terms_json,
                    flags_json,
                )| {
                    let path_class = PathClass::from_label(&path_class).ok_or_else(|| {
                        FriggError::Internal(format!(
                            "invalid entrypoint surface path_class '{path_class}' for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}'"
                        ))
                    })?;
                    let source_class = SourceClass::from_label(&source_class).ok_or_else(|| {
                        FriggError::Internal(format!(
                            "invalid entrypoint surface source_class '{source_class}' for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}'"
                        ))
                    })?;
                    let path_terms = serde_json::from_str(&path_terms_json).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to decode entrypoint surface path terms for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}': {err}"
                        ))
                    })?;
                    let surface_terms =
                        serde_json::from_str(&surface_terms_json).map_err(|err| {
                            FriggError::Internal(format!(
                                "failed to decode entrypoint surface terms for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}': {err}"
                            ))
                        })?;
                    Ok(EntrypointSurfaceProjection {
                        path,
                        path_class,
                        source_class,
                        path_terms,
                        surface_terms,
                        flags_json,
                    })
                },
            )
            .collect::<FriggResult<Vec<_>>>()?;

        Ok(rows)
    }
}
