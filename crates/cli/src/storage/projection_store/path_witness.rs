use crate::domain::{FriggError, FriggResult, PathClass, SourceClass};
use crate::storage::{
    PathWitnessProjection, Storage, db_runtime::i64_to_u64, db_runtime::open_connection,
    db_runtime::u64_to_i64,
};

use super::common::normalize_repository_snapshot_ids;

impl Storage {
    pub fn replace_path_witness_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[PathWitnessProjection],
    ) -> FriggResult<()> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start path witness projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM path_witness_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id.as_str(), snapshot_id.as_str()),
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
                  file_stem,
                  path_terms_json,
                  subtree_root,
                  family_bits,
                  flags_json,
                  heuristic_version
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
                    record.path,
                    record.path_class.as_str(),
                    record.source_class.as_str(),
                    record.file_stem,
                    serde_json::to_string(&record.path_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode path witness projection terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    record.subtree_root,
                    u64_to_i64(record.family_bits, "family_bits")?,
                    record.flags_json,
                    record.heuristic_version,
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
    ) -> FriggResult<Vec<PathWitnessProjection>> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT repository_id, snapshot_id, path, path_class, source_class, file_stem, path_terms_json, subtree_root, family_bits, flags_json, heuristic_version
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
            .query_map((repository_id.as_str(), snapshot_id.as_str()), |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    i64_to_u64(row.get::<_, i64>(8)?, "family_bits")?,
                    row.get::<_, String>(9)?,
                    row.get::<_, i64>(10)?,
                ))
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
            })?
            .into_iter()
            .map(
                |(
                    path,
                    path_class,
                    source_class,
                    file_stem,
                    path_terms_json,
                    subtree_root,
                    family_bits,
                    flags_json,
                    heuristic_version,
                )| {
                    let path_class = PathClass::from_label(&path_class).ok_or_else(|| {
                        FriggError::Internal(format!(
                            "invalid path witness projection path_class '{path_class}' for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}'"
                        ))
                    })?;
                    let source_class = SourceClass::from_label(&source_class).ok_or_else(|| {
                        FriggError::Internal(format!(
                            "invalid path witness projection source_class '{source_class}' for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}'"
                        ))
                    })?;
                    let path_terms = serde_json::from_str(&path_terms_json).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to decode path witness projection terms for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}': {err}"
                        ))
                    })?;
                    Ok(PathWitnessProjection {
                        path,
                        path_class,
                        source_class,
                        file_stem,
                        path_terms,
                        subtree_root,
                        family_bits,
                        flags_json,
                        heuristic_version,
                    })
                },
            )
            .collect::<FriggResult<Vec<_>>>()?;

        Ok(rows)
    }
}
