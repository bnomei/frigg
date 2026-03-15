use super::*;

impl Storage {
    pub fn replace_path_witness_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[PathWitnessProjection],
    ) -> FriggResult<()> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start path witness projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM path_witness_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id, snapshot_id),
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
                  path_terms_json,
                  flags_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
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
                    repository_id,
                    snapshot_id,
                    record.path,
                    record.path_class.as_str(),
                    record.source_class.as_str(),
                    serde_json::to_string(&record.path_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode path witness projection terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    record.flags_json,
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
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT repository_id, snapshot_id, path, path_class, source_class, path_terms_json, flags_json
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
            .query_map((repository_id, snapshot_id), |row| {
                Ok((
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
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
                |(path, path_class, source_class, path_terms_json, flags_json)| {
                    let path_class = PathClass::from_str(&path_class).ok_or_else(|| {
                        FriggError::Internal(format!(
                            "invalid path witness projection path_class '{path_class}' for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}'"
                        ))
                    })?;
                    let source_class = SourceClass::from_str(&source_class).ok_or_else(|| {
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
                        path_terms,
                        flags_json,
                    })
                },
            )
            .collect::<FriggResult<Vec<_>>>()?;

        Ok(rows)
    }

    pub fn replace_test_subject_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[TestSubjectProjection],
    ) -> FriggResult<()> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start test subject projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM test_subject_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id, snapshot_id),
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
                    repository_id,
                    snapshot_id,
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
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

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
            .query_map((repository_id, snapshot_id), |row| {
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

    pub fn replace_entrypoint_surface_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        records: &[EntrypointSurfaceProjection],
    ) -> FriggResult<()> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start entrypoint surface projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        tx.execute(
            "DELETE FROM entrypoint_surface_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id, snapshot_id),
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
                    repository_id,
                    snapshot_id,
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
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let snapshot_id = snapshot_id.trim();
        if snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "snapshot_id must not be empty".to_owned(),
            ));
        }

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
            .query_map((repository_id, snapshot_id), |row| {
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
                    let path_class = PathClass::from_str(&path_class).ok_or_else(|| {
                        FriggError::Internal(format!(
                            "invalid entrypoint surface path_class '{path_class}' for repository '{repository_id}' snapshot '{snapshot_id}' path '{path}'"
                        ))
                    })?;
                    let source_class = SourceClass::from_str(&source_class).ok_or_else(|| {
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
