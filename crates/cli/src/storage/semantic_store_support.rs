use crate::domain::{FriggError, FriggResult};
use rusqlite::types::Value as SqlValue;
use rusqlite::{Connection, OptionalExtension, Transaction, params_from_iter};

use super::super::vector_store::{decode_f32_vector, encode_f32_vector};
use super::super::{
    DEFAULT_VECTOR_DIMENSIONS, SemanticChunkEmbeddingRecord, SemanticHeadRecord, VECTOR_TABLE_NAME,
    usize_to_i64,
};

pub(super) fn validate_semantic_target(
    record: &SemanticChunkEmbeddingRecord,
    expected_provider: &str,
    expected_model: &str,
) -> FriggResult<()> {
    if record.provider != expected_provider {
        return Err(FriggError::InvalidInput(format!(
            "semantic chunk record provider mismatch: expected '{expected_provider}' found '{}'",
            record.provider
        )));
    }
    if record.model != expected_model {
        return Err(FriggError::InvalidInput(format!(
            "semantic chunk record model mismatch: expected '{expected_model}' found '{}'",
            record.model
        )));
    }

    Ok(())
}

pub(super) fn load_semantic_head_for_repository_model_on_connection(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<Option<SemanticHeadRecord>> {
    conn.query_row(
        r#"
        SELECT repository_id, provider, model, covered_snapshot_id, live_chunk_count, last_refresh_reason
        FROM semantic_head
        WHERE repository_id = ?1 AND provider = ?2 AND model = ?3
        "#,
        (repository_id, provider, model),
        |row| {
            let live_chunk_count = row.get::<_, i64>(4).and_then(|value| {
                usize::try_from(value).map_err(|_| {
                    rusqlite::Error::FromSqlConversionFailure(
                        4,
                        rusqlite::types::Type::Integer,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            format!(
                                "semantic head live_chunk_count is negative for repository '{repository_id}' provider '{provider}' model '{model}': {value}"
                            ),
                        )),
                    )
                })
            })?;
            Ok(SemanticHeadRecord {
                repository_id: row.get(0)?,
                provider: row.get(1)?,
                model: row.get(2)?,
                covered_snapshot_id: row.get(3)?,
                live_chunk_count,
                last_refresh_reason: row.get(5)?,
            })
        },
    )
    .optional()
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to query semantic head for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })
}

pub(super) fn clear_live_semantic_corpus_for_repository_model(
    tx: &Transaction<'_>,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<()> {
    delete_vector_partition(tx, repository_id, provider, model)?;

    tx.execute(
        r#"
        DELETE FROM semantic_chunk_embedding
        WHERE repository_id = ?1 AND provider = ?2 AND model = ?3
        "#,
        (repository_id, provider, model),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to clear semantic embeddings for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;

    tx.execute(
        r#"
        DELETE FROM semantic_chunk
        WHERE repository_id = ?1 AND provider = ?2 AND model = ?3
        "#,
        (repository_id, provider, model),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to clear semantic chunks for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;

    tx.execute(
        r#"
        DELETE FROM semantic_head
        WHERE repository_id = ?1 AND provider = ?2 AND model = ?3
        "#,
        (repository_id, provider, model),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to clear semantic head for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;

    Ok(())
}

pub(super) fn insert_semantic_embeddings_for_records(
    tx: &Transaction<'_>,
    repository_id: &str,
    snapshot_id: &str,
    provider: &str,
    model: &str,
    records: &[SemanticChunkEmbeddingRecord],
) -> FriggResult<usize> {
    let mut insert_chunk_stmt = tx
        .prepare(
            r#"
            INSERT INTO semantic_chunk (
                repository_id,
                provider,
                model,
                chunk_id,
                snapshot_id,
                path,
                language,
                chunk_index,
                start_line,
                end_line,
                content_hash_blake3,
                content_text
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic chunk insert statement for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
    let mut insert_embedding_stmt = tx
        .prepare(
            r#"
            INSERT INTO semantic_chunk_embedding (
                repository_id,
                provider,
                model,
                chunk_id,
                snapshot_id,
                trace_id,
                embedding_blob,
                dimensions
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic embedding insert statement for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;

    let mut previous_chunk_record: Option<&SemanticChunkEmbeddingRecord> = None;
    let mut inserted_chunks = 0usize;
    for record in records {
        let duplicate_chunk_id = previous_chunk_record
            .map(|previous| previous.chunk_id == record.chunk_id)
            .unwrap_or(false);
        if duplicate_chunk_id {
            let previous =
                previous_chunk_record.expect("duplicate chunk rows require previous row");
            let shared_fields_match = previous.path == record.path
                && previous.language == record.language
                && previous.chunk_index == record.chunk_index
                && previous.start_line == record.start_line
                && previous.end_line == record.end_line
                && previous.content_hash_blake3 == record.content_hash_blake3
                && previous.content_text == record.content_text;
            if !shared_fields_match {
                return Err(FriggError::Internal(format!(
                    "semantic chunk record shared content mismatch for duplicate chunk_id '{}'",
                    record.chunk_id
                )));
            }
            previous_chunk_record = Some(record);
            continue;
        }

        insert_chunk_stmt
            .execute((
                repository_id,
                provider,
                model,
                record.chunk_id.as_str(),
                snapshot_id,
                record.path.as_str(),
                record.language.as_str(),
                usize_to_i64(record.chunk_index, "chunk_index")?,
                usize_to_i64(record.start_line, "start_line")?,
                usize_to_i64(record.end_line, "end_line")?,
                record.content_hash_blake3.as_str(),
                record.content_text.as_str(),
            ))
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to insert semantic chunk for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;
        let dimensions = record.embedding.len();
        let embedding_blob = encode_f32_vector(&record.embedding);
        insert_embedding_stmt
            .execute((
                repository_id,
                provider,
                model,
                record.chunk_id.as_str(),
                snapshot_id,
                record.trace_id.as_deref(),
                embedding_blob,
                usize_to_i64(dimensions, "dimensions")?,
            ))
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to insert semantic embedding for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
                ))
            })?;

        previous_chunk_record = Some(record);
        inserted_chunks = inserted_chunks.saturating_add(1);
    }

    drop(insert_embedding_stmt);
    drop(insert_chunk_stmt);
    Ok(inserted_chunks)
}

pub(super) fn upsert_semantic_head(
    tx: &Transaction<'_>,
    repository_id: &str,
    provider: &str,
    model: &str,
    covered_snapshot_id: &str,
    live_chunk_count: usize,
    last_refresh_reason: Option<&str>,
) -> FriggResult<()> {
    tx.execute(
        r#"
        INSERT INTO semantic_head (
            repository_id,
            provider,
            model,
            covered_snapshot_id,
            live_chunk_count,
            last_refresh_reason
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6)
        ON CONFLICT(repository_id, provider, model) DO UPDATE SET
            covered_snapshot_id = excluded.covered_snapshot_id,
            live_chunk_count = excluded.live_chunk_count,
            last_refresh_reason = excluded.last_refresh_reason,
            updated_at = CURRENT_TIMESTAMP
        "#,
        (
            repository_id,
            provider,
            model,
            covered_snapshot_id,
            usize_to_i64(live_chunk_count, "live_chunk_count")?,
            last_refresh_reason,
        ),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to upsert semantic head for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;

    Ok(())
}

pub(super) fn load_live_semantic_chunk_ids_for_paths(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
    paths: &[String],
) -> FriggResult<Vec<String>> {
    if paths.is_empty() {
        return Ok(Vec::new());
    }

    let placeholders = (0..paths.len())
        .map(|idx| format!("?{}", idx + 4))
        .collect::<Vec<_>>()
        .join(", ");
    let sql = format!(
        r#"
        SELECT chunk_id
        FROM semantic_chunk
        WHERE repository_id = ?1
          AND provider = ?2
          AND model = ?3
          AND path IN ({placeholders})
        ORDER BY path ASC, chunk_index ASC, chunk_id ASC
        "#
    );
    let mut statement = conn.prepare(&sql).map_err(|err| {
        FriggError::Internal(format!(
            "failed to prepare semantic chunk id lookup for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;

    let mut params = Vec::with_capacity(3 + paths.len());
    params.push(SqlValue::from(repository_id.to_owned()));
    params.push(SqlValue::from(provider.to_owned()));
    params.push(SqlValue::from(model.to_owned()));
    for path in paths {
        params.push(SqlValue::from(path.clone()));
    }

    statement
        .query_map(params_from_iter(params.iter()), |row| row.get::<_, String>(0))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query semantic chunk ids for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic chunk ids for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })
}

pub(super) fn delete_live_semantic_rows_for_paths(
    tx: &Transaction<'_>,
    repository_id: &str,
    provider: &str,
    model: &str,
    paths: &[String],
) -> FriggResult<()> {
    for path in paths {
        tx.execute(
            r#"
            DELETE FROM semantic_chunk_embedding
            WHERE repository_id = ?1
              AND provider = ?2
              AND model = ?3
              AND chunk_id IN (
                SELECT chunk_id
                FROM semantic_chunk
                WHERE repository_id = ?1
                  AND provider = ?2
                  AND model = ?3
                  AND path = ?4
              )
            "#,
            (repository_id, provider, model, path),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to delete semantic embeddings for repository '{repository_id}' provider '{provider}' model '{model}' path '{path}': {err}"
            ))
        })?;
        tx.execute(
            r#"
            DELETE FROM semantic_chunk
            WHERE repository_id = ?1
              AND provider = ?2
              AND model = ?3
              AND path = ?4
            "#,
            (repository_id, provider, model, path),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to delete semantic chunks for repository '{repository_id}' provider '{provider}' model '{model}' path '{path}': {err}"
            ))
        })?;
    }

    Ok(())
}

pub(super) fn delete_vector_partition(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<()> {
    conn.execute(
        &format!(
            "DELETE FROM {VECTOR_TABLE_NAME} WHERE repository_id = ?1 AND provider = ?2 AND model = ?3"
        ),
        (repository_id, provider, model),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "failed to clear semantic vector partition for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })?;

    Ok(())
}

pub(super) fn delete_vector_rows_for_chunk_ids(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
    chunk_ids: &[String],
) -> FriggResult<()> {
    if chunk_ids.is_empty() {
        return Ok(());
    }

    let mut delete_statement = conn
        .prepare(&format!(
            "DELETE FROM {VECTOR_TABLE_NAME} WHERE repository_id = ?1 AND provider = ?2 AND model = ?3 AND chunk_id = ?4"
        ))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic vector delete statement for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
    for chunk_id in chunk_ids {
        delete_statement
            .execute((repository_id, provider, model, chunk_id.as_str()))
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to delete semantic vector row for repository '{repository_id}' provider '{provider}' model '{model}' chunk '{chunk_id}': {err}"
                ))
            })?;
    }
    drop(delete_statement);

    Ok(())
}

pub(super) fn sync_vector_partition_replace(
    tx: &Transaction<'_>,
    repository_id: &str,
    provider: &str,
    model: &str,
    records: &[SemanticChunkEmbeddingRecord],
) -> FriggResult<()> {
    delete_vector_partition(tx, repository_id, provider, model)?;
    sync_vector_rows_insert(tx, repository_id, provider, model, records)
}

pub(super) fn sync_vector_rows_insert(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
    records: &[SemanticChunkEmbeddingRecord],
) -> FriggResult<()> {
    if records.is_empty() {
        return Ok(());
    }

    let chunk_ids = records
        .iter()
        .map(|record| record.chunk_id.clone())
        .collect::<Vec<_>>();
    delete_vector_rows_for_chunk_ids(conn, repository_id, provider, model, &chunk_ids)?;

    let mut insert_statement = conn
        .prepare(&format!(
            r#"
            INSERT INTO {VECTOR_TABLE_NAME} (
                embedding,
                repository_id,
                provider,
                model,
                language,
                chunk_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#
        ))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic vector insert statement for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
    for record in records {
        let embedding =
            normalize_embedding_for_vector_projection(&record.chunk_id, record.embedding.clone())?;
        insert_statement
            .execute((
                encode_f32_vector(&embedding),
                repository_id,
                provider,
                model,
                record.language.as_str(),
                record.chunk_id.as_str(),
            ))
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to insert semantic vector row for repository '{repository_id}' provider '{provider}' model '{model}' chunk '{}': {err}",
                    record.chunk_id
                ))
            })?;
    }
    drop(insert_statement);

    Ok(())
}

pub(super) fn count_semantic_chunk_rows_for_repository_model(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<usize> {
    let count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM semantic_chunk
            WHERE repository_id = ?1 AND provider = ?2 AND model = ?3
            "#,
            (repository_id, provider, model),
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count semantic chunk rows for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
    usize::try_from(count).map_err(|err| {
        FriggError::Internal(format!(
            "semantic chunk row count overflow for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })
}

pub(super) fn count_semantic_embedding_rows_for_repository_model(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<usize> {
    let count: i64 = conn
        .query_row(
            r#"
            SELECT COUNT(*)
            FROM semantic_chunk_embedding
            WHERE repository_id = ?1 AND provider = ?2 AND model = ?3
            "#,
            (repository_id, provider, model),
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count semantic embedding rows for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
    usize::try_from(count).map_err(|err| {
        FriggError::Internal(format!(
            "semantic embedding row count overflow for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })
}

pub(super) fn count_semantic_vector_rows_for_repository_model(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<usize> {
    let count: i64 = conn
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM {VECTOR_TABLE_NAME} WHERE repository_id = ?1 AND provider = ?2 AND model = ?3"
            ),
            (repository_id, provider, model),
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count semantic vector rows for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
    usize::try_from(count).map_err(|err| {
        FriggError::Internal(format!(
            "semantic vector row count overflow for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
        ))
    })
}

pub(super) fn count_manifest_snapshots_for_repository(
    conn: &Connection,
    repository_id: &str,
) -> FriggResult<usize> {
    let count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM snapshot WHERE repository_id = ?1 AND kind = 'manifest'",
            [repository_id],
            |row| row.get(0),
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to count manifest snapshots for repository '{repository_id}': {err}"
            ))
        })?;
    usize::try_from(count).map_err(|err| {
        FriggError::Internal(format!(
            "manifest snapshot count overflow for repository '{repository_id}': {err}"
        ))
    })
}

pub(super) fn rebuild_semantic_vector_rows(conn: &Connection) -> FriggResult<()> {
    conn.execute_batch(&format!("DELETE FROM {VECTOR_TABLE_NAME}"))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to clear semantic vector rows during vector sync: {err}"
            ))
        })?;

    struct SemanticVectorProjectionSeed {
        chunk_id: String,
        repository_id: String,
        provider: String,
        model: String,
        language: String,
        embedding: Vec<f32>,
    }

    let mut select_statement = conn
        .prepare(
            r#"
            SELECT
                embedding.chunk_id,
                embedding.repository_id,
                embedding.provider,
                embedding.model,
                chunk.language,
                embedding.embedding_blob,
                embedding.dimensions
            FROM semantic_chunk_embedding AS embedding
            INNER JOIN semantic_chunk AS chunk
              ON chunk.repository_id = embedding.repository_id
             AND chunk.provider = embedding.provider
             AND chunk.model = embedding.model
             AND chunk.chunk_id = embedding.chunk_id
            ORDER BY embedding.repository_id ASC,
                     embedding.provider ASC,
                     embedding.model ASC,
                     chunk.path ASC,
                     chunk.chunk_index ASC,
                     embedding.chunk_id ASC
            "#,
        )
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic vector rebuild query: {err}"
            ))
        })?;
    let seeds = select_statement
        .query_map([], |row| {
            let chunk_id: String = row.get(0)?;
            let repository_id: String = row.get(1)?;
            let provider: String = row.get(2)?;
            let model: String = row.get(3)?;
            let language: String = row.get(4)?;
            let embedding_blob: Vec<u8> = row.get(5)?;
            let dimensions = row
                .get::<_, i64>(6)
                .and_then(|value| {
                    usize::try_from(value).map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            6,
                            rusqlite::types::Type::Integer,
                            Box::new(std::io::Error::new(
                                std::io::ErrorKind::InvalidData,
                                format!(
                                    "semantic vector sync found negative dimensions for chunk '{chunk_id}': {value}"
                                ),
                            )),
                        )
                    })
                })?;
            let embedding = decode_f32_vector(&embedding_blob).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "semantic vector sync failed to decode embedding for chunk '{chunk_id}': {err}"
                        ),
                    )),
                )
            })?;
            if embedding.len() != dimensions {
                return Err(rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "semantic vector sync found mismatched dimensions for chunk '{chunk_id}': metadata={dimensions}, decoded={}",
                            embedding.len()
                        ),
                    )),
                ));
            }
            let embedding = normalize_embedding_for_vector_projection(&chunk_id, embedding)
                .map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Blob,
                        Box::new(std::io::Error::new(
                            std::io::ErrorKind::InvalidData,
                            err.to_string(),
                        )),
                    )
                })?;

            Ok(SemanticVectorProjectionSeed {
                chunk_id,
                repository_id,
                provider,
                model,
                language,
                embedding,
            })
        })
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to query semantic embeddings for vector sync: {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode semantic embeddings for vector sync: {err}"
            ))
        })?;
    drop(select_statement);

    let mut insert_statement = conn
        .prepare(&format!(
            r#"
            INSERT INTO {VECTOR_TABLE_NAME} (
                embedding,
                repository_id,
                provider,
                model,
                language,
                chunk_id
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            "#
        ))
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare semantic vector insert statement: {err}"
            ))
        })?;
    for seed in seeds {
        insert_statement
            .execute((
                encode_f32_vector(&seed.embedding),
                seed.repository_id.as_str(),
                seed.provider.as_str(),
                seed.model.as_str(),
                seed.language.as_str(),
                seed.chunk_id.as_str(),
            ))
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to insert semantic vector row for chunk '{}': {err}",
                    seed.chunk_id
                ))
            })?;
    }

    Ok(())
}

pub(super) fn ensure_semantic_vector_rows_current(
    conn: &Connection,
    repository_id: &str,
    provider: &str,
    model: &str,
) -> FriggResult<()> {
    let semantic_rows =
        count_semantic_embedding_rows_for_repository_model(conn, repository_id, provider, model)?;
    let vector_rows =
        count_semantic_vector_rows_for_repository_model(conn, repository_id, provider, model)?;
    if semantic_rows != vector_rows {
        return Err(FriggError::Internal(format!(
            "semantic vector partition out of sync for repository '{repository_id}' provider '{provider}' model '{model}': embeddings={semantic_rows} vectors={vector_rows}; run storage repair to rebuild sqlite-vec from the live semantic corpus"
        )));
    }
    Ok(())
}

pub(super) fn normalize_embedding_for_vector_projection(
    chunk_id: &str,
    mut embedding: Vec<f32>,
) -> FriggResult<Vec<f32>> {
    if embedding.is_empty() {
        return Err(FriggError::Internal(format!(
            "semantic vector sync found empty embedding for chunk '{chunk_id}'"
        )));
    }
    if embedding.len() > DEFAULT_VECTOR_DIMENSIONS {
        return Err(FriggError::Internal(format!(
            "semantic vector sync found {}-dimension embedding for chunk '{chunk_id}', but sqlite-vec expects at most {DEFAULT_VECTOR_DIMENSIONS}; rerun semantic reindex with the current build",
            embedding.len()
        )));
    }
    if embedding.len() < DEFAULT_VECTOR_DIMENSIONS {
        embedding.resize(DEFAULT_VECTOR_DIMENSIONS, 0.0);
    }
    Ok(embedding)
}
