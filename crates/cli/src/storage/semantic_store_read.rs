use std::collections::BTreeMap;

use crate::domain::{FriggError, FriggResult};
use rusqlite::types::Value as SqlValue;
use rusqlite::{OptionalExtension, params_from_iter};

use super::super::vector_store::{decode_f32_vector, encode_f32_vector};
use super::super::{
    SQLITE_VEC_MAX_KNN_LIMIT, SemanticChunkEmbeddingProjection, SemanticChunkPayload,
    SemanticChunkVectorMatch, SemanticHeadRecord, i64_to_u64, usize_to_i64,
};
use super::*;

impl Storage {
    pub fn load_semantic_head_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<Option<SemanticHeadRecord>> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        load_semantic_head_for_repository_model_on_connection(&conn, repository_id, provider, model)
    }

    pub fn load_semantic_embeddings_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<SemanticChunkEmbeddingRecord>> {
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
        let mut statement = conn
            .prepare(
                r#"
                SELECT
                    embedding.chunk_id,
                    embedding.repository_id,
                    head.covered_snapshot_id,
                    chunk.path,
                    chunk.language,
                    chunk.chunk_index,
                    chunk.start_line,
                    chunk.end_line,
                    embedding.provider,
                    embedding.model,
                    embedding.trace_id,
                    chunk.content_hash_blake3,
                    chunk.content_text,
                    embedding.embedding_blob,
                    embedding.dimensions
                FROM semantic_chunk_embedding AS embedding
                INNER JOIN semantic_chunk AS chunk
                  ON chunk.repository_id = embedding.repository_id
                 AND chunk.provider = embedding.provider
                 AND chunk.model = embedding.model
                 AND chunk.chunk_id = embedding.chunk_id
                INNER JOIN semantic_head AS head
                  ON head.repository_id = embedding.repository_id
                 AND head.provider = embedding.provider
                 AND head.model = embedding.model
                WHERE head.repository_id = ?1 AND head.covered_snapshot_id = ?2
                ORDER BY chunk.path ASC, chunk.chunk_index ASC, embedding.chunk_id ASC, embedding.provider ASC, embedding.model ASC
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic embedding load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let rows = statement
            .query_map((repository_id, snapshot_id), |row| {
                let embedding_blob: Vec<u8> = row.get(13)?;
                let dimensions_raw: i64 = row.get(14)?;
                let embedding = decode_f32_vector(&embedding_blob).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        13,
                        rusqlite::types::Type::Blob,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                    )
                })?;
                let dimensions = i64_to_u64(dimensions_raw, "dimensions")? as usize;
                if embedding.len() != dimensions {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        14,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "semantic embedding dimensions mismatch for chunk: decoded={} stored={dimensions}",
                                embedding.len()
                            ),
                        )),
                    ));
                }
                Ok(SemanticChunkEmbeddingRecord {
                    chunk_id: row.get(0)?,
                    repository_id: row.get(1)?,
                    snapshot_id: row.get(2)?,
                    path: row.get(3)?,
                    language: row.get(4)?,
                    chunk_index: i64_to_u64(row.get::<_, i64>(5)?, "chunk_index")? as usize,
                    start_line: i64_to_u64(row.get::<_, i64>(6)?, "start_line")? as usize,
                    end_line: i64_to_u64(row.get::<_, i64>(7)?, "end_line")? as usize,
                    provider: row.get(8)?,
                    model: row.get(9)?,
                    trace_id: row.get(10)?,
                    content_hash_blake3: row.get(11)?,
                    content_text: row.get(12)?,
                    embedding,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic embeddings for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic embeddings for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn load_semantic_embedding_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<SemanticChunkEmbeddingProjection>> {
        self.load_semantic_embedding_projections_for_repository_snapshot_model(
            repository_id,
            snapshot_id,
            None,
            None,
        )
    }

    pub fn load_semantic_embedding_projections_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: Option<&str>,
        model: Option<&str>,
    ) -> FriggResult<Vec<SemanticChunkEmbeddingProjection>> {
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
        let mut statement = conn
            .prepare(
                r#"
                SELECT
                    embedding.chunk_id,
                    embedding.repository_id,
                    head.covered_snapshot_id,
                    chunk.path,
                    chunk.language,
                    chunk.start_line,
                    chunk.end_line,
                    embedding.embedding_blob,
                    embedding.dimensions
                FROM semantic_chunk_embedding AS embedding
                INNER JOIN semantic_chunk AS chunk
                  ON chunk.repository_id = embedding.repository_id
                 AND chunk.provider = embedding.provider
                 AND chunk.model = embedding.model
                 AND chunk.chunk_id = embedding.chunk_id
                INNER JOIN semantic_head AS head
                  ON head.repository_id = embedding.repository_id
                 AND head.provider = embedding.provider
                 AND head.model = embedding.model
                WHERE head.repository_id = ?1
                  AND head.covered_snapshot_id = ?2
                  AND (?3 IS NULL OR embedding.provider = ?3)
                  AND (?4 IS NULL OR embedding.model = ?4)
                ORDER BY chunk.path ASC, chunk.chunk_index ASC, embedding.chunk_id ASC
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic embedding projection query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        let rows = statement
            .query_map((repository_id, snapshot_id, provider, model), |row| {
                let start_line = i64_to_u64(row.get::<_, i64>(5)?, "start_line")? as usize;
                let end_line = i64_to_u64(row.get::<_, i64>(6)?, "end_line")? as usize;
                let embedding_blob: Vec<u8> = row.get(7)?;
                let dimensions_raw: i64 = row.get(8)?;
                let embedding = decode_f32_vector(&embedding_blob).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Blob,
                        Box::new(std::io::Error::new(std::io::ErrorKind::InvalidData, err)),
                    )
                })?;
                let dimensions = i64_to_u64(dimensions_raw, "dimensions")? as usize;
                if embedding.len() != dimensions {
                    return Err(rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "semantic embedding dimensions mismatch for projection: decoded={} stored={dimensions}",
                                embedding.len()
                            ),
                        )),
                    ));
                }
                Ok(SemanticChunkEmbeddingProjection {
                    chunk_id: row.get(0)?,
                    repository_id: row.get(1)?,
                    snapshot_id: row.get(2)?,
                    path: row.get(3)?,
                    language: row.get(4)?,
                    start_line,
                    end_line,
                    embedding,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic embedding projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic embedding projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn load_semantic_vector_topk_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
        query_embedding: &[f32],
        limit: usize,
        language: Option<&str>,
    ) -> FriggResult<Vec<SemanticChunkVectorMatch>> {
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
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }
        if query_embedding.is_empty() {
            return Err(FriggError::InvalidInput(
                "query_embedding must not be empty".to_owned(),
            ));
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        let conn = open_connection(&self.db_path)?;
        ensure_semantic_vector_rows_current(&conn, repository_id, provider, model)?;

        let capped_limit = limit.min(SQLITE_VEC_MAX_KNN_LIMIT);
        let scan_limit = capped_limit.saturating_mul(4).min(SQLITE_VEC_MAX_KNN_LIMIT);
        let normalized_query =
            normalize_embedding_for_vector_projection("<query>", query_embedding.to_vec())?;
        let encoded_query = encode_f32_vector(&normalized_query);
        let vector_sql = format!(
            r#"
            SELECT chunk_id, distance
            FROM {VECTOR_TABLE_NAME}
            WHERE k = ?1
              AND repository_id = ?2
              AND provider = ?3
              AND model = ?4
              AND embedding MATCH ?5
            "#
        );

        let mut vector_statement = conn.prepare(&vector_sql).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic vector top-k query for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
        let vector_rows = vector_statement
            .query_map(
                (
                    usize_to_i64(scan_limit, "scan_limit")?,
                    repository_id,
                    provider,
                    model,
                    encoded_query,
                ),
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, f32>(1)?,
                    ))
                },
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic vector top-k matches for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        let vector_rows = vector_rows
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to decode semantic vector top-k matches for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        let mut matches = Vec::new();
        if vector_rows.is_empty() {
            return Ok(matches);
        }

        let mut membership_statement = conn
            .prepare(
                r#"
            SELECT 1
            FROM semantic_chunk
            WHERE repository_id = ?1
              AND provider = ?2
              AND model = ?3
              AND snapshot_id = ?4
              AND chunk_id = ?5
              AND (?6 IS NULL OR language = ?6)
            LIMIT 1
            "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare semantic vector top-k filter query for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        for (chunk_id, distance) in vector_rows {
            let is_in_snapshot: Option<i64> = membership_statement
                .query_row(
                    (
                        repository_id,
                        provider,
                        model,
                        snapshot_id,
                        chunk_id.as_str(),
                        language,
                    ),
                    |row| row.get(0),
                )
                .optional()
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "failed to query semantic vector top-k chunk membership for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                    ))
                })?;
            if is_in_snapshot.is_some() {
                matches.push(SemanticChunkVectorMatch {
                    chunk_id,
                    repository_id: repository_id.to_owned(),
                    snapshot_id: snapshot_id.to_owned(),
                    distance,
                });
            }
        }

        matches.sort_by(|left, right| {
            left.distance
                .total_cmp(&right.distance)
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        matches.truncate(capped_limit);

        Ok(matches)
    }

    pub fn load_semantic_chunk_payloads_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        chunk_ids: &[String],
    ) -> FriggResult<BTreeMap<String, SemanticChunkPayload>> {
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
        if chunk_ids.is_empty() {
            return Ok(BTreeMap::new());
        }

        let conn = open_connection(&self.db_path)?;
        let placeholders = (0..chunk_ids.len())
            .map(|idx| format!("?{}", idx + 3))
            .collect::<Vec<_>>()
            .join(", ");
        let sql = format!(
            r#"
            SELECT
                chunk.chunk_id,
                chunk.path,
                chunk.language,
                chunk.chunk_index,
                chunk.start_line,
                chunk.end_line,
                chunk.content_hash_blake3,
                chunk.content_text
            FROM semantic_chunk AS chunk
            INNER JOIN semantic_head AS head
              ON head.repository_id = chunk.repository_id
             AND head.provider = chunk.provider
             AND head.model = chunk.model
            WHERE head.repository_id = ?1
              AND head.covered_snapshot_id = ?2
              AND chunk.chunk_id IN ({placeholders})
            ORDER BY chunk.path ASC, chunk.chunk_index ASC, chunk.chunk_id ASC
            "#
        );
        let mut statement = conn.prepare(&sql).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic chunk payload query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;

        let mut params = Vec::with_capacity(2 + chunk_ids.len());
        params.push(SqlValue::from(repository_id.to_owned()));
        params.push(SqlValue::from(snapshot_id.to_owned()));
        for chunk_id in chunk_ids {
            params.push(SqlValue::from(chunk_id.clone()));
        }

        let rows = statement
            .query_map(params_from_iter(params.iter()), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    SemanticChunkPayload {
                        chunk_id: row.get(0)?,
                        path: row.get(1)?,
                        language: row.get(2)?,
                        start_line: i64_to_u64(row.get::<_, i64>(4)?, "start_line")? as usize,
                        end_line: i64_to_u64(row.get::<_, i64>(5)?, "end_line")? as usize,
                        content_text: row.get(7)?,
                    },
                ))
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic chunk payloads for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                ))
            })?;

        rows.collect::<Result<BTreeMap<_, _>, _>>().map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic chunk payloads for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }

    pub fn has_semantic_embeddings_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<bool> {
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
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let exists: i64 = conn
            .query_row(
                r#"
                SELECT EXISTS(
                    SELECT 1
                    FROM semantic_head
                    WHERE repository_id = ?1
                      AND covered_snapshot_id = ?2
                      AND provider = ?3
                      AND model = ?4
                      AND EXISTS(
                        SELECT 1
                        FROM semantic_chunk_embedding
                        WHERE semantic_chunk_embedding.repository_id = semantic_head.repository_id
                          AND semantic_chunk_embedding.provider = semantic_head.provider
                          AND semantic_chunk_embedding.model = semantic_head.model
                      )
                )
                "#,
                (repository_id, snapshot_id, provider, model),
                |row| row.get(0),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query semantic embedding presence for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        Ok(exists == 1)
    }

    pub fn count_semantic_embeddings_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<usize> {
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
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let count: i64 = conn
            .query_row(
                r#"
                SELECT COUNT(*)
                FROM semantic_chunk_embedding
                INNER JOIN semantic_head
                  ON semantic_head.repository_id = semantic_chunk_embedding.repository_id
                 AND semantic_head.provider = semantic_chunk_embedding.provider
                 AND semantic_head.model = semantic_chunk_embedding.model
                WHERE semantic_head.repository_id = ?1
                  AND semantic_head.covered_snapshot_id = ?2
                  AND semantic_head.provider = ?3
                  AND semantic_head.model = ?4
                "#,
                (repository_id, snapshot_id, provider, model),
                |row| row.get(0),
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to count semantic embeddings for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        usize::try_from(count).map_err(|err| {
            FriggError::Internal(format!(
                "semantic embedding count overflow for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
            ))
        })
    }

    pub fn load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<Option<String>> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let snapshot_id = conn
            .query_row(
                r#"
                SELECT semantic_head.covered_snapshot_id
                FROM semantic_head
                WHERE semantic_head.repository_id = ?1
                  AND semantic_head.provider = ?2
                  AND semantic_head.model = ?3
                  AND EXISTS(
                    SELECT 1
                    FROM semantic_chunk_embedding
                    WHERE semantic_chunk_embedding.repository_id = semantic_head.repository_id
                      AND semantic_chunk_embedding.provider = semantic_head.provider
                      AND semantic_chunk_embedding.model = semantic_head.model
                  )
                "#,
                (repository_id, provider, model),
                |row| row.get(0),
            )
            .optional()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query latest semantic-populated manifest snapshot for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        Ok(snapshot_id)
    }

    pub fn load_semantic_chunk_texts_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        chunk_ids: &[String],
    ) -> FriggResult<BTreeMap<String, String>> {
        Ok(self
            .load_semantic_chunk_payloads_for_repository_snapshot(
                repository_id,
                snapshot_id,
                chunk_ids,
            )?
            .into_iter()
            .map(|(chunk_id, payload)| (chunk_id, payload.content_text))
            .collect())
    }
}
