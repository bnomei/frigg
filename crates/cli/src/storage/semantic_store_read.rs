use std::collections::BTreeMap;

use crate::domain::{FriggError, FriggResult};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, params_from_iter};

#[cfg(test)]
use super::super::db_runtime::{record_semantic_payload_load, record_semantic_read_context_open};
use super::{
    load_ready_semantic_head_for_repository_snapshot_model_on_connection,
    load_semantic_head_for_repository_model_on_connection,
    normalize_embedding_for_vector_projection,
};
use crate::storage::vector_store::{decode_f32_vector, encode_f32_vector};
use crate::storage::{
    SNAPSHOT_KIND_MANIFEST, SQLITE_VEC_MAX_KNN_LIMIT, SemanticChunkEmbeddingProjection,
    SemanticChunkEmbeddingRecord, SemanticChunkPayload, SemanticChunkPreview,
    SemanticChunkVectorMatch, SemanticHeadRecord, Storage, VECTOR_TABLE_NAME, i64_to_u64,
    open_connection, usize_to_i64,
};

pub(crate) struct SemanticReadContext {
    conn: Connection,
}

fn require_trimmed_input<'a>(value: &'a str, field: &str) -> FriggResult<&'a str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(FriggError::InvalidInput(format!(
            "{field} must not be empty"
        )));
    }
    Ok(trimmed)
}

impl Storage {
    pub(crate) fn open_semantic_read_context(&self) -> FriggResult<SemanticReadContext> {
        #[cfg(test)]
        record_semantic_read_context_open();
        Ok(SemanticReadContext {
            conn: open_connection(&self.db_path)?,
        })
    }

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

    #[allow(clippy::too_many_arguments)]
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
        self.open_semantic_read_context()?
            .load_semantic_vector_topk_for_repository_snapshot_model(
                repository_id,
                snapshot_id,
                provider,
                model,
                query_embedding,
                limit,
                language,
            )
    }

    pub fn load_semantic_chunk_payloads_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        chunk_ids: &[String],
    ) -> FriggResult<BTreeMap<String, SemanticChunkPayload>> {
        self.open_semantic_read_context()?
            .load_semantic_chunk_payloads_for_repository_snapshot(
                repository_id,
                snapshot_id,
                chunk_ids,
            )
    }

    pub fn has_semantic_embeddings_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<bool> {
        self.open_semantic_read_context()?
            .has_semantic_embeddings_for_repository_snapshot_model(
                repository_id,
                snapshot_id,
                provider,
                model,
            )
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
        self.open_semantic_read_context()?
            .load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
                repository_id,
                provider,
                model,
            )
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

impl SemanticReadContext {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn load_semantic_vector_topk_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
        query_embedding: &[f32],
        limit: usize,
        language: Option<&str>,
    ) -> FriggResult<Vec<SemanticChunkVectorMatch>> {
        let repository_id = require_trimmed_input(repository_id, "repository_id")?;
        let snapshot_id = require_trimmed_input(snapshot_id, "snapshot_id")?;
        let provider = require_trimmed_input(provider, "provider")?;
        let model = require_trimmed_input(model, "model")?;
        if query_embedding.is_empty() {
            return Err(FriggError::InvalidInput(
                "query_embedding must not be empty".to_owned(),
            ));
        }
        if limit == 0 {
            return Ok(Vec::new());
        }

        load_semantic_vector_topk_for_repository_snapshot_model_on_connection(
            &self.conn,
            repository_id,
            snapshot_id,
            provider,
            model,
            query_embedding,
            limit,
            language,
        )
    }

    pub(crate) fn load_semantic_chunk_payloads_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        chunk_ids: &[String],
    ) -> FriggResult<BTreeMap<String, SemanticChunkPayload>> {
        let repository_id = require_trimmed_input(repository_id, "repository_id")?;
        let snapshot_id = require_trimmed_input(snapshot_id, "snapshot_id")?;
        if chunk_ids.is_empty() {
            return Ok(BTreeMap::new());
        }

        load_semantic_chunk_payloads_for_repository_snapshot_on_connection(
            &self.conn,
            repository_id,
            snapshot_id,
            chunk_ids,
        )
    }

    pub(crate) fn load_semantic_chunk_previews_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        chunk_ids: &[String],
    ) -> FriggResult<BTreeMap<String, SemanticChunkPreview>> {
        let repository_id = require_trimmed_input(repository_id, "repository_id")?;
        let snapshot_id = require_trimmed_input(snapshot_id, "snapshot_id")?;
        if chunk_ids.is_empty() {
            return Ok(BTreeMap::new());
        }

        load_semantic_chunk_previews_for_repository_snapshot_on_connection(
            &self.conn,
            repository_id,
            snapshot_id,
            chunk_ids,
        )
    }

    pub(crate) fn has_semantic_embeddings_for_repository_snapshot_model(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<bool> {
        let repository_id = require_trimmed_input(repository_id, "repository_id")?;
        let snapshot_id = require_trimmed_input(snapshot_id, "snapshot_id")?;
        let provider = require_trimmed_input(provider, "provider")?;
        let model = require_trimmed_input(model, "model")?;

        has_semantic_embeddings_for_repository_snapshot_model_on_connection(
            &self.conn,
            repository_id,
            snapshot_id,
            provider,
            model,
        )
    }

    pub(crate) fn load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<Option<String>> {
        let repository_id = require_trimmed_input(repository_id, "repository_id")?;
        let provider = require_trimmed_input(provider, "provider")?;
        let model = require_trimmed_input(model, "model")?;

        load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model_on_connection(
            &self.conn,
            repository_id,
            provider,
            model,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn load_semantic_vector_topk_for_repository_snapshot_model_on_connection(
    conn: &Connection,
    repository_id: &str,
    snapshot_id: &str,
    provider: &str,
    model: &str,
    query_embedding: &[f32],
    limit: usize,
    language: Option<&str>,
) -> FriggResult<Vec<SemanticChunkVectorMatch>> {
    if load_ready_semantic_head_for_repository_snapshot_model_on_connection(
        conn,
        repository_id,
        snapshot_id,
        provider,
        model,
    )?
    .is_none()
    {
        return Ok(Vec::new());
    }

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
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, f32>(1)?)),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query semantic vector top-k matches for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;

    let mut matches = vector_rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
        FriggError::Internal(format!(
            "failed to decode semantic vector top-k matches for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;
    if matches.is_empty() {
        return Ok(Vec::new());
    }
    let allowed_chunk_ids = load_allowed_semantic_chunk_ids_for_snapshot_on_connection(
        conn,
        repository_id,
        snapshot_id,
        provider,
        model,
        matches.iter().map(|(chunk_id, _)| chunk_id.as_str()),
        language,
    )?;
    matches.retain(|(chunk_id, _)| allowed_chunk_ids.contains(chunk_id));

    matches.sort_by(|left, right| {
        left.1
            .total_cmp(&right.1)
            .then_with(|| left.0.cmp(&right.0))
    });
    matches.truncate(capped_limit);
    Ok(matches
        .into_iter()
        .map(|(chunk_id, distance)| SemanticChunkVectorMatch {
            chunk_id,
            repository_id: repository_id.to_owned(),
            snapshot_id: snapshot_id.to_owned(),
            distance,
        })
        .collect())
}

fn load_allowed_semantic_chunk_ids_for_snapshot_on_connection<'a>(
    conn: &Connection,
    repository_id: &str,
    snapshot_id: &str,
    provider: &str,
    model: &str,
    chunk_ids: impl Iterator<Item = &'a str>,
    language: Option<&str>,
) -> FriggResult<std::collections::BTreeSet<String>> {
    let chunk_ids = chunk_ids.map(ToOwned::to_owned).collect::<Vec<_>>();
    if chunk_ids.is_empty() {
        return Ok(std::collections::BTreeSet::new());
    }

    let placeholders = (0..chunk_ids.len())
        .map(|idx| format!("?{}", idx + 6))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT chunk_id
        FROM semantic_chunk
        WHERE repository_id = ?1
          AND snapshot_id = ?2
          AND provider = ?3
          AND model = ?4
          AND (?5 IS NULL OR language = ?5)
          AND chunk_id IN ({placeholders})
        "#
    );
    let mut statement = conn.prepare(&sql).map_err(|err| {
        FriggError::Internal(format!(
            "failed to prepare semantic vector top-k membership set query for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;

    let mut params = Vec::with_capacity(5 + chunk_ids.len());
    params.push(SqlValue::from(repository_id.to_owned()));
    params.push(SqlValue::from(snapshot_id.to_owned()));
    params.push(SqlValue::from(provider.to_owned()));
    params.push(SqlValue::from(model.to_owned()));
    params.push(match language {
        Some(language) => SqlValue::from(language.to_owned()),
        None => SqlValue::Null,
    });
    for chunk_id in &chunk_ids {
        params.push(SqlValue::from(chunk_id.clone()));
    }

    statement
        .query_map(params_from_iter(params.iter()), |row| row.get::<_, String>(0))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query semantic vector top-k membership set for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic vector top-k membership set for repository '{repository_id}' snapshot '{snapshot_id}' provider '{provider}' model '{model}': {err}"
            ))
        })
}

fn load_semantic_chunk_payloads_for_repository_snapshot_on_connection(
    conn: &Connection,
    repository_id: &str,
    snapshot_id: &str,
    chunk_ids: &[String],
) -> FriggResult<BTreeMap<String, SemanticChunkPayload>> {
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

    let payloads = rows.collect::<Result<BTreeMap<_, _>, _>>().map_err(|err| {
        FriggError::Internal(format!(
            "failed to decode semantic chunk payloads for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
        ))
    })?;
    #[cfg(test)]
    record_semantic_payload_load(
        payloads.len(),
        payloads
            .values()
            .map(|payload| payload.content_text.len())
            .sum(),
    );
    Ok(payloads)
}

fn load_semantic_chunk_previews_for_repository_snapshot_on_connection(
    conn: &Connection,
    repository_id: &str,
    snapshot_id: &str,
    chunk_ids: &[String],
) -> FriggResult<BTreeMap<String, SemanticChunkPreview>> {
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
            chunk.start_line,
            chunk.end_line,
            substr(chunk.content_text, 1, 512) AS preview_text
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
            "failed to prepare semantic chunk preview query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
        ))
    })?;

    let mut params = Vec::with_capacity(2 + chunk_ids.len());
    params.push(SqlValue::from(repository_id.to_owned()));
    params.push(SqlValue::from(snapshot_id.to_owned()));
    for chunk_id in chunk_ids {
        params.push(SqlValue::from(chunk_id.clone()));
    }

    statement
        .query_map(params_from_iter(params.iter()), |row| {
            Ok((
                row.get::<_, String>(0)?,
                SemanticChunkPreview {
                    chunk_id: row.get(0)?,
                    path: row.get(1)?,
                    language: row.get(2)?,
                    start_line: i64_to_u64(row.get::<_, i64>(3)?, "start_line")? as usize,
                    end_line: i64_to_u64(row.get::<_, i64>(4)?, "end_line")? as usize,
                    preview_text: row.get(5)?,
                },
            ))
        })
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query semantic chunk previews for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<BTreeMap<_, _>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic chunk previews for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
}

fn has_semantic_embeddings_for_repository_snapshot_model_on_connection(
    conn: &Connection,
    repository_id: &str,
    snapshot_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<bool> {
    Ok(
        load_ready_semantic_head_for_repository_snapshot_model_on_connection(
            conn,
            repository_id,
            snapshot_id,
            provider,
            model,
        )?
        .is_some(),
    )
}

fn load_latest_manifest_snapshot_id_with_semantic_embeddings_for_repository_model_on_connection(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<Option<String>> {
    conn.query_row(
        r#"
        SELECT semantic_head.covered_snapshot_id
        FROM semantic_head
        INNER JOIN snapshot
          ON snapshot.snapshot_id = semantic_head.covered_snapshot_id
        WHERE semantic_head.repository_id = ?1
          AND snapshot.repository_id = ?1
          AND snapshot.kind = ?4
          AND semantic_head.provider = ?2
          AND semantic_head.model = ?3
          AND semantic_head.live_chunk_count > 0
        ORDER BY snapshot.created_at DESC, snapshot.rowid DESC
        LIMIT 1
        "#,
        (repository_id, provider, model, SNAPSHOT_KIND_MANIFEST),
        |row| row.get(0),
    )
    .optional()
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to query latest semantic-populated manifest snapshot for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })
}
