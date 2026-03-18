use super::*;
use rusqlite::OptionalExtension;

const REQUIRED_RETRIEVAL_PROJECTION_FAMILIES: &[&str] = &[
    "path_witness",
    "test_subject",
    "entrypoint_surface",
    "path_relation",
    "subtree_coverage",
    "path_surface_term",
    "path_anchor_sketch",
];

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
                    repository_id,
                    snapshot_id,
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
            .query_map((repository_id, snapshot_id), |row| {
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

    pub fn replace_retrieval_projection_bundle_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        bundle: &RetrievalProjectionBundle,
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
                "failed to start retrieval projection replace transaction for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        for table in [
            "retrieval_projection_head",
            "path_relation_projection",
            "subtree_coverage_projection",
            "path_surface_term_projection",
            "path_anchor_sketch_projection",
            "path_witness_projection",
            "test_subject_projection",
            "entrypoint_surface_projection",
        ] {
            tx.execute(
                &format!("DELETE FROM {table} WHERE repository_id = ?1 AND snapshot_id = ?2"),
                (repository_id, snapshot_id),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to clear {table} rows for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
        }

        {
            let mut rows = bundle.path_witness.clone();
            rows.sort_by(|left, right| left.path.cmp(&right.path));
            rows.dedup_by(|left, right| left.path == right.path);
            let mut insert_stmt = tx.prepare(
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
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare path witness projection bundle insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in rows {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.path,
                    record.path_class.as_str(),
                    record.source_class.as_str(),
                    record.file_stem,
                    serde_json::to_string(&record.path_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode path witness bundle terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    record.subtree_root,
                    u64_to_i64(record.family_bits, "family_bits")?,
                    record.flags_json,
                    record.heuristic_version,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert path witness bundle row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        {
            let mut rows = bundle.test_subject.clone();
            rows.sort_by(|left, right| {
                left.test_path
                    .cmp(&right.test_path)
                    .then(left.subject_path.cmp(&right.subject_path))
            });
            rows.dedup_by(|left, right| {
                left.test_path == right.test_path && left.subject_path == right.subject_path
            });
            let mut insert_stmt = tx.prepare(
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
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare test subject bundle insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in rows {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.test_path,
                    record.subject_path,
                    serde_json::to_string(&record.shared_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode test subject bundle terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    usize_to_i64(record.score_hint, "score_hint")?,
                    record.flags_json,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert test subject bundle row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        {
            let mut rows = bundle.entrypoint_surface.clone();
            rows.sort_by(|left, right| left.path.cmp(&right.path));
            rows.dedup_by(|left, right| left.path == right.path);
            let mut insert_stmt = tx.prepare(
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
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare entrypoint surface bundle insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in rows {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.path,
                    record.path_class.as_str(),
                    record.source_class.as_str(),
                    serde_json::to_string(&record.path_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode entrypoint surface bundle path terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    serde_json::to_string(&record.surface_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode entrypoint surface bundle terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    record.flags_json,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert entrypoint surface bundle row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        {
            let mut rows = bundle.path_relations.clone();
            rows.sort_by(|left, right| {
                left.src_path
                    .cmp(&right.src_path)
                    .then(left.dst_path.cmp(&right.dst_path))
                    .then(left.relation_kind.cmp(&right.relation_kind))
            });
            rows.dedup_by(|left, right| {
                left.src_path == right.src_path
                    && left.dst_path == right.dst_path
                    && left.relation_kind == right.relation_kind
            });
            let mut insert_stmt = tx.prepare(
                r#"
                INSERT INTO path_relation_projection (
                  repository_id,
                  snapshot_id,
                  src_path,
                  dst_path,
                  relation_kind,
                  evidence_source,
                  src_symbol_id,
                  dst_symbol_id,
                  src_family_bits,
                  dst_family_bits,
                  shared_terms_json,
                  score_hint
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                "#,
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare path relation bundle insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in rows {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.src_path,
                    record.dst_path,
                    record.relation_kind,
                    record.evidence_source,
                    record.src_symbol_id,
                    record.dst_symbol_id,
                    u64_to_i64(record.src_family_bits, "src_family_bits")?,
                    u64_to_i64(record.dst_family_bits, "dst_family_bits")?,
                    serde_json::to_string(&record.shared_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode path relation bundle terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    usize_to_i64(record.score_hint, "score_hint")?,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert path relation bundle row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        {
            let mut rows = bundle.subtree_coverage.clone();
            rows.sort_by(|left, right| {
                left.subtree_root
                    .cmp(&right.subtree_root)
                    .then(left.family.cmp(&right.family))
                    .then(left.exemplar_path.cmp(&right.exemplar_path))
            });
            rows.dedup_by(|left, right| {
                left.subtree_root == right.subtree_root && left.family == right.family
            });
            let mut insert_stmt = tx.prepare(
                r#"
                INSERT INTO subtree_coverage_projection (
                  repository_id,
                  snapshot_id,
                  subtree_root,
                  family,
                  path_count,
                  exemplar_path,
                  exemplar_score_hint
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare subtree coverage bundle insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in rows {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.subtree_root,
                    record.family,
                    usize_to_i64(record.path_count, "path_count")?,
                    record.exemplar_path,
                    usize_to_i64(record.exemplar_score_hint, "exemplar_score_hint")?,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert subtree coverage bundle row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        {
            let mut rows = bundle.path_surface_terms.clone();
            rows.sort_by(|left, right| left.path.cmp(&right.path));
            rows.dedup_by(|left, right| left.path == right.path);
            let mut insert_stmt = tx.prepare(
                r#"
                INSERT INTO path_surface_term_projection (
                  repository_id,
                  snapshot_id,
                  path,
                  term_weights_json,
                  exact_terms_json
                )
                VALUES (?1, ?2, ?3, ?4, ?5)
                "#,
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare path surface-term bundle insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in rows {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.path,
                    serde_json::to_string(&record.term_weights).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode path surface-term weights for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    serde_json::to_string(&record.exact_terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode path surface-term exact terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert path surface-term bundle row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        {
            let mut rows = bundle.path_anchor_sketches.clone();
            rows.sort_by(|left, right| {
                left.path
                    .cmp(&right.path)
                    .then(left.anchor_rank.cmp(&right.anchor_rank))
            });
            rows.dedup_by(|left, right| {
                left.path == right.path && left.anchor_rank == right.anchor_rank
            });
            let mut insert_stmt = tx.prepare(
                r#"
                INSERT INTO path_anchor_sketch_projection (
                  repository_id,
                  snapshot_id,
                  path,
                  anchor_rank,
                  line,
                  anchor_kind,
                  excerpt,
                  terms_json,
                  score_hint
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare path anchor sketch bundle insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in rows {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.path,
                    usize_to_i64(record.anchor_rank, "anchor_rank")?,
                    usize_to_i64(record.line, "line")?,
                    record.anchor_kind,
                    record.excerpt,
                    serde_json::to_string(&record.terms).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode path anchor sketch terms for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    usize_to_i64(record.score_hint, "score_hint")?,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert path anchor sketch bundle row for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        {
            let mut heads = bundle.heads.clone();
            heads.sort_by(|left, right| left.family.cmp(&right.family));
            heads.dedup_by(|left, right| left.family == right.family);
            let mut insert_stmt = tx.prepare(
                r#"
                INSERT INTO retrieval_projection_head (
                  repository_id,
                  snapshot_id,
                  family,
                  heuristic_version,
                  input_modes_json,
                  row_count
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                "#,
            ).map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare retrieval projection head insert for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
            for record in heads {
                insert_stmt.execute((
                    repository_id,
                    snapshot_id,
                    record.family,
                    record.heuristic_version,
                    serde_json::to_string(&record.input_modes).map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to encode retrieval projection head input modes for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                        ))
                    })?,
                    usize_to_i64(record.row_count, "row_count")?,
                )).map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to insert retrieval projection head for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            }
        }

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit retrieval projection bundle replace for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        Ok(())
    }

    pub fn load_retrieval_projection_head_for_repository_snapshot_family(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        family: &str,
    ) -> FriggResult<Option<RetrievalProjectionHeadRecord>> {
        let repository_id = repository_id.trim();
        let snapshot_id = snapshot_id.trim();
        let family = family.trim();
        if repository_id.is_empty() || snapshot_id.is_empty() || family.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id, snapshot_id, and family must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        conn.query_row(
            r#"
            SELECT family, heuristic_version, input_modes_json, row_count
            FROM retrieval_projection_head
            WHERE repository_id = ?1 AND snapshot_id = ?2 AND family = ?3
            "#,
            (repository_id, snapshot_id, family),
            |row| {
                let input_modes_json = row.get::<_, String>(2)?;
                let row_count = row.get::<_, i64>(3)?;
                Ok(RetrievalProjectionHeadRecord {
                    family: row.get(0)?,
                    heuristic_version: row.get(1)?,
                    input_modes: serde_json::from_str(&input_modes_json).map_err(|err| {
                        rusqlite::Error::FromSqlConversionFailure(
                            2,
                            rusqlite::types::Type::Text,
                            Box::new(err),
                        )
                    })?,
                    row_count: i64_to_u64(row_count, "row_count")? as usize,
                })
            },
        )
        .optional()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to load retrieval projection head for repository '{repository_id}' snapshot '{snapshot_id}' family '{family}': {err}"
            ))
        })
    }

    pub fn missing_retrieval_projection_families_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<String>> {
        let repository_id = repository_id.trim();
        let snapshot_id = snapshot_id.trim();
        if repository_id.is_empty() || snapshot_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id and snapshot_id must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn
            .prepare(
                r#"
                SELECT family
                FROM retrieval_projection_head
                WHERE repository_id = ?1 AND snapshot_id = ?2
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare retrieval projection family presence query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;
        let present_families = stmt
            .query_map((repository_id, snapshot_id), |row| row.get::<_, String>(0))
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query retrieval projection family presence for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode retrieval projection family presence for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        Ok(REQUIRED_RETRIEVAL_PROJECTION_FAMILIES
            .iter()
            .filter(|family| !present_families.contains(**family))
            .map(|family| (*family).to_owned())
            .collect())
    }

    pub fn load_path_relation_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<PathRelationProjection>> {
        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT src_path, dst_path, relation_kind, evidence_source, src_symbol_id, dst_symbol_id,
                   src_family_bits, dst_family_bits, shared_terms_json, score_hint
            FROM path_relation_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY src_path ASC, dst_path ASC, relation_kind ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare path relation projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id, snapshot_id), |row| {
            let shared_terms_json = row.get::<_, String>(8)?;
            Ok(PathRelationProjection {
                src_path: row.get(0)?,
                dst_path: row.get(1)?,
                relation_kind: row.get(2)?,
                evidence_source: row.get(3)?,
                src_symbol_id: row.get(4)?,
                dst_symbol_id: row.get(5)?,
                src_family_bits: i64_to_u64(row.get::<_, i64>(6)?, "src_family_bits")?,
                dst_family_bits: i64_to_u64(row.get::<_, i64>(7)?, "dst_family_bits")?,
                shared_terms: serde_json::from_str(&shared_terms_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                score_hint: i64_to_u64(row.get::<_, i64>(9)?, "score_hint")? as usize,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query path relation projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode path relation projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn load_subtree_coverage_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<SubtreeCoverageProjection>> {
        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT subtree_root, family, path_count, exemplar_path, exemplar_score_hint
            FROM subtree_coverage_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY subtree_root ASC, family ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare subtree coverage projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id, snapshot_id), |row| {
            Ok(SubtreeCoverageProjection {
                subtree_root: row.get(0)?,
                family: row.get(1)?,
                path_count: i64_to_u64(row.get::<_, i64>(2)?, "path_count")? as usize,
                exemplar_path: row.get(3)?,
                exemplar_score_hint: i64_to_u64(row.get::<_, i64>(4)?, "exemplar_score_hint")?
                    as usize,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query subtree coverage projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode subtree coverage projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn load_path_surface_term_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<PathSurfaceTermProjection>> {
        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT path, term_weights_json, exact_terms_json
            FROM path_surface_term_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY path ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare path surface-term projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id, snapshot_id), |row| {
            let term_weights_json = row.get::<_, String>(1)?;
            let exact_terms_json = row.get::<_, String>(2)?;
            Ok(PathSurfaceTermProjection {
                path: row.get(0)?,
                term_weights: serde_json::from_str(&term_weights_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                exact_terms: serde_json::from_str(&exact_terms_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query path surface-term projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode path surface-term projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn load_path_anchor_sketch_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<PathAnchorSketchProjection>> {
        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT path, anchor_rank, line, anchor_kind, excerpt, terms_json, score_hint
            FROM path_anchor_sketch_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY path ASC, anchor_rank ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare path anchor sketch projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id, snapshot_id), |row| {
            let terms_json = row.get::<_, String>(5)?;
            Ok(PathAnchorSketchProjection {
                path: row.get(0)?,
                anchor_rank: i64_to_u64(row.get::<_, i64>(1)?, "anchor_rank")? as usize,
                line: i64_to_u64(row.get::<_, i64>(2)?, "line")? as usize,
                anchor_kind: row.get(3)?,
                excerpt: row.get(4)?,
                terms: serde_json::from_str(&terms_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                score_hint: i64_to_u64(row.get::<_, i64>(6)?, "score_hint")? as usize,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query path anchor sketch projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode path anchor sketch projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }
}
