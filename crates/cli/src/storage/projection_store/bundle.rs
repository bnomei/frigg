use crate::domain::{FriggError, FriggResult};
use crate::storage::{
    RetrievalProjectionBundle, RetrievalProjectionHeadRecord, Storage,
    db_runtime::{i64_to_u64, open_connection, u64_to_i64, usize_to_i64},
};
use rusqlite::OptionalExtension;

use super::common::normalize_repository_snapshot_ids;

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
    pub fn replace_retrieval_projection_bundle_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        bundle: &RetrievalProjectionBundle,
    ) -> FriggResult<()> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

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
                (repository_id.as_str(), snapshot_id.as_str()),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
                    repository_id.as_str(),
                    snapshot_id.as_str(),
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
        let (repository_id, snapshot_id, family) =
            super::common::normalize_repository_snapshot_family_ids(
                repository_id,
                snapshot_id,
                family,
            )?;

        let conn = open_connection(&self.db_path)?;
        conn.query_row(
            r#"
            SELECT family, heuristic_version, input_modes_json, row_count
            FROM retrieval_projection_head
            WHERE repository_id = ?1 AND snapshot_id = ?2 AND family = ?3
            "#,
            (repository_id.as_str(), snapshot_id.as_str(), family.as_str()),
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
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

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
            .query_map((repository_id.as_str(), snapshot_id.as_str()), |row| row.get::<_, String>(0))
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
}
