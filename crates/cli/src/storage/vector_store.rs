use super::*;

pub(super) fn ensure_sqlite_vec_auto_extension_registered() -> FriggResult<()> {
    let registration = SQLITE_VEC_AUTO_EXTENSION_REGISTRATION.get_or_init(|| {
        #[allow(unsafe_code)]
        {
            let rc = unsafe { rusqlite::ffi::sqlite3_auto_extension(Some(sqlite3_vec_init)) };
            if rc == rusqlite::ffi::SQLITE_OK {
                Ok(())
            } else {
                Err(format!(
                    "vector subsystem not ready: failed to register sqlite-vec auto extension (sqlite rc={rc})"
                ))
            }
        }
    });

    registration
        .as_ref()
        .map(|_| ())
        .map_err(|message| FriggError::Internal(message.clone()))
}

pub(super) fn ensure_sqlite_vec_registration_readiness(conn: &Connection) -> FriggResult<()> {
    cached_sqlite_vec_version(conn).map(|_| ())
}

fn cached_sqlite_vec_version(conn: &Connection) -> FriggResult<String> {
    let readiness = SQLITE_VEC_CONNECTION_READINESS
        .get_or_init(|| detect_sqlite_vec_version(conn).map_err(|err| err.to_string()));
    readiness
        .as_ref()
        .cloned()
        .map_err(|message| FriggError::Internal(message.clone()))
}

pub(super) fn validate_semantic_chunk_embedding_record(
    record: &SemanticChunkEmbeddingRecord,
    expected_repository_id: &str,
    expected_snapshot_id: &str,
) -> FriggResult<()> {
    if record.chunk_id.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record chunk_id must not be empty".to_owned(),
        ));
    }
    if record.repository_id.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record repository_id must not be empty".to_owned(),
        ));
    }
    if record.repository_id != expected_repository_id {
        return Err(FriggError::InvalidInput(format!(
            "semantic chunk record repository_id mismatch: expected '{expected_repository_id}' found '{}'",
            record.repository_id
        )));
    }
    if record.snapshot_id.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record snapshot_id must not be empty".to_owned(),
        ));
    }
    if record.snapshot_id != expected_snapshot_id {
        return Err(FriggError::InvalidInput(format!(
            "semantic chunk record snapshot_id mismatch: expected '{expected_snapshot_id}' found '{}'",
            record.snapshot_id
        )));
    }
    if record.path.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record path must not be empty".to_owned(),
        ));
    }
    if record.language.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record language must not be empty".to_owned(),
        ));
    }
    if record.start_line == 0 || record.end_line == 0 {
        return Err(FriggError::InvalidInput(
            "semantic chunk record line numbers must be greater than zero".to_owned(),
        ));
    }
    if record.start_line > record.end_line {
        return Err(FriggError::InvalidInput(
            "semantic chunk record start_line must be <= end_line".to_owned(),
        ));
    }
    if record.provider.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record provider must not be empty".to_owned(),
        ));
    }
    if record.model.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record model must not be empty".to_owned(),
        ));
    }
    if record.content_hash_blake3.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record content_hash_blake3 must not be empty".to_owned(),
        ));
    }
    if record.content_text.trim().is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record content_text must not be empty".to_owned(),
        ));
    }
    if record.embedding.is_empty() {
        return Err(FriggError::InvalidInput(
            "semantic chunk record embedding must not be empty".to_owned(),
        ));
    }
    if record.embedding.iter().any(|value| !value.is_finite()) {
        return Err(FriggError::InvalidInput(
            "semantic chunk record embedding must contain only finite values".to_owned(),
        ));
    }
    Ok(())
}

pub(super) fn semantic_chunk_embedding_record_order(
    left: &SemanticChunkEmbeddingRecord,
    right: &SemanticChunkEmbeddingRecord,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.chunk_index.cmp(&right.chunk_index))
        .then(left.start_line.cmp(&right.start_line))
        .then(left.end_line.cmp(&right.end_line))
        .then(left.chunk_id.cmp(&right.chunk_id))
}

pub(crate) fn encode_f32_vector(values: &[f32]) -> Vec<u8> {
    let mut buffer = Vec::with_capacity(values.len() * std::mem::size_of::<f32>());
    for value in values {
        buffer.extend_from_slice(&value.to_le_bytes());
    }
    buffer
}

pub(super) fn decode_f32_vector(blob: &[u8]) -> Result<Vec<f32>, String> {
    if !blob.len().is_multiple_of(std::mem::size_of::<f32>()) {
        return Err(format!(
            "semantic embedding blob length {} is not divisible by {}",
            blob.len(),
            std::mem::size_of::<f32>()
        ));
    }

    let mut out = Vec::with_capacity(blob.len() / std::mem::size_of::<f32>());
    for chunk in blob.chunks_exact(std::mem::size_of::<f32>()) {
        out.push(f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]));
    }
    Ok(out)
}

fn ensure_valid_dimensions(expected_dimensions: usize) -> FriggResult<()> {
    if expected_dimensions == 0 {
        return Err(FriggError::InvalidInput(
            "expected_dimensions must be greater than zero".to_string(),
        ));
    }

    Ok(())
}

fn detect_sqlite_vec_version(conn: &Connection) -> FriggResult<String> {
    match conn.query_row("SELECT vec_version()", [], |row| row.get::<_, String>(0)) {
        Ok(version) => {
            ensure_sqlite_vec_pinned_version(&version)?;
            Ok(version)
        }
        Err(err) => {
            let err_message = err.to_string();
            if err_message.contains("no such function: vec_version") {
                Err(FriggError::Internal(
                    "vector subsystem not ready: sqlite-vec extension function vec_version() is unavailable; ensure sqlite-vec FFI auto-extension registration is active"
                        .to_string(),
                ))
            } else {
                Err(FriggError::Internal(format!(
                    "vector subsystem not ready: sqlite-vec extension self-check failed: {err_message}"
                )))
            }
        }
    }
}

fn normalize_sqlite_vec_version(version: &str) -> &str {
    let trimmed = version.trim();
    trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed)
}

pub(crate) fn ensure_sqlite_vec_pinned_version(runtime_version: &str) -> FriggResult<()> {
    let normalized_runtime_version = normalize_sqlite_vec_version(runtime_version);
    let normalized_required_version = normalize_sqlite_vec_version(SQLITE_VEC_REQUIRED_VERSION);

    if normalized_runtime_version != normalized_required_version {
        return Err(FriggError::Internal(format!(
            "vector subsystem not ready: sqlite-vec extension version mismatch (found '{runtime_version}', normalized '{normalized_runtime_version}', required '{SQLITE_VEC_REQUIRED_VERSION}')"
        )));
    }

    Ok(())
}

fn create_sqlite_vec_table(conn: &Connection, expected_dimensions: usize) -> FriggResult<()> {
    let statement = format!(
        "CREATE VIRTUAL TABLE IF NOT EXISTS {VECTOR_TABLE_NAME} USING vec0(embedding float[{expected_dimensions}] distance_metric=cosine, repository_id text partition key, provider text partition key, model text partition key, language text, +chunk_id text);"
    );

    conn.execute_batch(&statement).map_err(|err| {
        FriggError::Internal(format!(
            "vector subsystem not ready: failed to initialize vector table '{VECTOR_TABLE_NAME}': {err}"
        ))
    })?;

    Ok(())
}

fn drop_sqlite_vec_table(conn: &Connection) -> FriggResult<()> {
    conn.execute_batch(&format!("DROP TABLE IF EXISTS {VECTOR_TABLE_NAME}"))
        .map_err(|err| {
            FriggError::Internal(format!(
                "vector subsystem not ready: failed to drop vector table '{VECTOR_TABLE_NAME}': {err}"
            ))
        })?;
    Ok(())
}

fn sqlite_vec_table_has_expected_projection_schema(
    conn: &Connection,
    expected_dimensions: usize,
) -> FriggResult<bool> {
    if !table_exists(conn, VECTOR_TABLE_NAME)? {
        return Ok(false);
    }

    let schema_sql = read_vector_table_schema_sql(conn)?;
    let normalized = schema_sql.to_ascii_lowercase();
    let expected_dimensions_fragment =
        format!("embedding float[{expected_dimensions}] distance_metric=cosine");
    let required_fragments = [
        expected_dimensions_fragment.as_str(),
        "repository_id text partition key",
        "provider text partition key",
        "model text partition key",
        "language text",
        "+chunk_id text",
    ];

    Ok(required_fragments
        .into_iter()
        .all(|fragment| normalized.contains(fragment)))
}

fn verify_sqlite_vec_table_schema(
    conn: &Connection,
    expected_dimensions: usize,
) -> FriggResult<()> {
    if !table_exists(conn, VECTOR_TABLE_NAME)? {
        return Err(FriggError::Internal(format!(
            "vector subsystem not ready: missing vector table '{VECTOR_TABLE_NAME}'"
        )));
    }

    let schema_sql = read_vector_table_schema_sql(conn)?;
    if !sqlite_vec_table_has_expected_projection_schema(conn, expected_dimensions)? {
        return Err(FriggError::Internal(format!(
            "vector subsystem not ready: vector table schema mismatch (found schema '{schema_sql}', expected embedding float[{expected_dimensions}] distance_metric=cosine plus repository/provider/model partition keys and language/chunk_id metadata)"
        )));
    }

    conn.query_row(
        &format!("SELECT COUNT(*) FROM {VECTOR_TABLE_NAME}"),
        [],
        |row| row.get::<_, i64>(0),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "vector subsystem not ready: vector table probe query failed: {err}"
        ))
    })?;

    Ok(())
}

fn read_vector_table_schema_sql(conn: &Connection) -> FriggResult<String> {
    conn.query_row(
        "SELECT sql FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [VECTOR_TABLE_NAME],
        |row| row.get(0),
    )
    .map_err(|err| {
        FriggError::Internal(format!(
            "vector subsystem not ready: failed to inspect vector table schema: {err}"
        ))
    })
}

fn infer_existing_vector_store_backend(
    conn: &Connection,
) -> FriggResult<ExistingVectorStoreBackend> {
    let vector_table_exists = table_exists(conn, VECTOR_TABLE_NAME)?;
    if !vector_table_exists {
        return Ok(ExistingVectorStoreBackend::Uninitialized);
    }

    let schema_sql = read_vector_table_schema_sql(conn)?;
    if schema_sql.to_lowercase().contains("float[") {
        return Ok(ExistingVectorStoreBackend::SqliteVec);
    }

    Err(FriggError::Internal(format!(
        "vector subsystem not ready: legacy non-sqlite-vec schema detected for table '{VECTOR_TABLE_NAME}' (schema: '{schema_sql}'); delete the storage DB and rerun `frigg init` to provision sqlite-vec"
    )))
}

fn sqlite_vec_unavailable_error() -> FriggError {
    FriggError::Internal(
        "vector subsystem not ready: sqlite-vec extension is unavailable; ensure sqlite-vec FFI auto-extension registration is active"
            .to_string(),
    )
}

fn sqlite_vec_status(extension_version: String, expected_dimensions: usize) -> VectorStoreStatus {
    VectorStoreStatus {
        backend: VectorStoreBackend::SqliteVec,
        extension_version,
        table_name: VECTOR_TABLE_NAME.to_string(),
        expected_dimensions,
    }
}

pub(crate) fn initialize_vector_store_on_connection_with_detected_capability(
    conn: &Connection,
    expected_dimensions: usize,
    sqlite_vec_version: Option<String>,
) -> FriggResult<VectorStoreStatus> {
    ensure_valid_dimensions(expected_dimensions)?;
    let extension_version = sqlite_vec_version.ok_or_else(sqlite_vec_unavailable_error)?;
    ensure_sqlite_vec_pinned_version(&extension_version)?;

    match infer_existing_vector_store_backend(conn)? {
        ExistingVectorStoreBackend::Uninitialized => {
            create_sqlite_vec_table(conn, expected_dimensions)?;
            verify_sqlite_vec_table_schema(conn, expected_dimensions)?;
            Ok(sqlite_vec_status(extension_version, expected_dimensions))
        }
        ExistingVectorStoreBackend::SqliteVec => {
            if !sqlite_vec_table_has_expected_projection_schema(conn, expected_dimensions)? {
                drop_sqlite_vec_table(conn)?;
                create_sqlite_vec_table(conn, expected_dimensions)?;
            }
            verify_sqlite_vec_table_schema(conn, expected_dimensions)?;
            Ok(sqlite_vec_status(extension_version, expected_dimensions))
        }
    }
}

pub(crate) fn verify_vector_store_on_connection_with_detected_capability(
    conn: &Connection,
    expected_dimensions: usize,
    sqlite_vec_version: Option<String>,
) -> FriggResult<VectorStoreStatus> {
    ensure_valid_dimensions(expected_dimensions)?;
    let extension_version = sqlite_vec_version.ok_or_else(sqlite_vec_unavailable_error)?;
    ensure_sqlite_vec_pinned_version(&extension_version)?;

    match infer_existing_vector_store_backend(conn)? {
        ExistingVectorStoreBackend::Uninitialized => {
            verify_sqlite_vec_table_schema(conn, expected_dimensions)?;
            Ok(sqlite_vec_status(extension_version, expected_dimensions))
        }
        ExistingVectorStoreBackend::SqliteVec => {
            verify_sqlite_vec_table_schema(conn, expected_dimensions)?;
            Ok(sqlite_vec_status(extension_version, expected_dimensions))
        }
    }
}

pub(super) fn initialize_vector_store_on_connection(
    conn: &Connection,
    expected_dimensions: usize,
) -> FriggResult<VectorStoreStatus> {
    let sqlite_vec_version = cached_sqlite_vec_version(conn)?;
    initialize_vector_store_on_connection_with_detected_capability(
        conn,
        expected_dimensions,
        Some(sqlite_vec_version),
    )
}

pub(super) fn verify_vector_store_on_connection(
    conn: &Connection,
    expected_dimensions: usize,
) -> FriggResult<VectorStoreStatus> {
    let sqlite_vec_version = cached_sqlite_vec_version(conn)?;
    verify_vector_store_on_connection_with_detected_capability(
        conn,
        expected_dimensions,
        Some(sqlite_vec_version),
    )
}
