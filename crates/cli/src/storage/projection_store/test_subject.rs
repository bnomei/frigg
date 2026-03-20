use crate::domain::{FriggError, FriggResult};
use crate::storage::{
    Storage, TestSubjectProjection, db_runtime::i64_to_u64, db_runtime::open_connection,
    db_runtime::usize_to_i64,
};

use super::common::normalize_repository_snapshot_ids;

impl Storage {
    pub fn replace_test_subject_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[TestSubjectProjection],
    ) -> FriggResult<()> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start test subject projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM test_subject_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id.as_str(), snapshot_id.as_str()),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to clear test subject projection rows for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by(|left, right| {
            left.test_path
                .cmp(&right.test_path)
                .then(left.subject_path.cmp(&right.subject_path))
        });
        ordered_records.dedup_by(|left, right| {
            left.test_path == right.test_path && left.subject_path == right.subject_path
        });

        let mut insert_stmt = tx
            .prepare(
                r#"
                INSERT INTO test_subject_projection (
                  repository_id,
                  snapshot_id,
                  test_path,
                  subject_path,
                  shared_terms_json,
                  score_hint,
                  flags_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare test subject projection insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        for record in ordered_records {
            insert_stmt
                .execute((
                    repository_id.as_str(),
                    snapshot_id.as_str(),
                    record.test_path,
                    record.subject_path,
                    serde_json::to_string(&record.shared_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode test subject projection terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    usize_to_i64(record.score_hint, "score_hint")?,
                    record.flags_json,
                ))
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert test subject projection row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
        }
        drop(insert_stmt);

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit test subject projection replace for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        Ok(())
    }

    pub fn load_test_subject_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<TestSubjectProjection>> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT repository_id, snapshot_id, test_path, subject_path, shared_terms_json, score_hint, flags_json
                FROM test_subject_projection
                WHERE repository_id = ?1 AND snapshot_id = ?2
                ORDER BY test_path ASC, subject_path ASC
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare test subject projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let raw_rows = stmt
            .query_map((repository_id.as_str(), snapshot_id.as_str()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, i64>(5)?,
                    row.get::<_, String>(6)?,
                ))
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query test subject projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode test subject projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let rows = raw_rows
            .into_iter()
            .map(
                |(
                    repository_id,
                    snapshot_id,
                    test_path,
                    subject_path,
                    shared_terms_json,
                    score_hint,
                    flags_json,
                )| {
                    let decoded_score_hint =
                        i64_to_u64(score_hint, "score_hint").map_err(|err| {
                            FriggError::Internal(format!(
                                "failed to decode test subject projection score_hint for repository '{repository_id}' snapshot '{snapshot_id}' path pair '{test_path}' -> '{subject_path}': {err}"
                            ))
                        })? as usize;
                    let shared_terms = serde_json::from_str(&shared_terms_json).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to decode test subject projection terms for repository '{repository_id}' snapshot '{snapshot_id}' path pair '{test_path}' -> '{subject_path}': {err}"
                        ))
                    })?;
                    Ok(TestSubjectProjection {
                        test_path,
                        subject_path,
                        shared_terms,
                        score_hint: decoded_score_hint,
                        flags_json,
                    })
                },
            )
            .collect::<FriggResult<Vec<_>>>()?;

        Ok(rows)
    }
}
