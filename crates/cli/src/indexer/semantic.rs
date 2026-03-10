use std::future::Future;
use std::pin::Pin;

use super::*;
use crate::indexer::manifest::normalize_repository_relative_path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticChunkCandidate {
    pub(crate) chunk_id: String,
    pub(crate) repository_id: String,
    pub(crate) snapshot_id: String,
    pub(crate) path: String,
    pub(crate) language: String,
    pub(crate) chunk_index: usize,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) content_hash_blake3: String,
    pub(crate) content_text: String,
}

pub(super) trait SemanticRuntimeEmbeddingExecutor: Sync {
    fn embed_documents<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        input: Vec<String>,
        trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>>;
}

fn build_semantic_embedding_runtime() -> FriggResult<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to build tokio runtime for semantic embedding requests: {err}"
            ))
        })
}

fn execute_semantic_embedding_batch(
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    provider: SemanticRuntimeProvider,
    model: &str,
    input: Vec<String>,
    trace_id: Option<String>,
) -> FriggResult<Vec<Vec<f32>>> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let model = model.to_owned();
        return std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let runtime = build_semantic_embedding_runtime()?;
                runtime.block_on(executor.embed_documents(provider, &model, input, trace_id))
            });
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(FriggError::Internal(
                    "semantic embedding provider thread panicked under an active tokio runtime"
                        .to_owned(),
                )),
            }
        });
    }

    let runtime = build_semantic_embedding_runtime()?;
    runtime.block_on(executor.embed_documents(provider, model, input, trace_id))
}

#[derive(Debug, Default)]
pub(super) struct RuntimeSemanticEmbeddingExecutor {
    credentials: SemanticRuntimeCredentials,
}

impl RuntimeSemanticEmbeddingExecutor {
    pub(super) fn new(credentials: SemanticRuntimeCredentials) -> Self {
        Self { credentials }
    }
}

impl SemanticRuntimeEmbeddingExecutor for RuntimeSemanticEmbeddingExecutor {
    fn embed_documents<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        input: Vec<String>,
        trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
        let model = model.trim().to_owned();
        let api_key = self
            .credentials
            .api_key_for(provider)
            .map(str::to_owned)
            .unwrap_or_default();
        Box::pin(async move {
            let request = EmbeddingRequest {
                model,
                input,
                purpose: EmbeddingPurpose::Document,
                dimensions: None,
                trace_id,
            };
            let response = match provider {
                SemanticRuntimeProvider::OpenAi => {
                    let client = OpenAiEmbeddingProvider::new(api_key);
                    client.embed(request).await
                }
                SemanticRuntimeProvider::Google => {
                    let client = GoogleEmbeddingProvider::new(api_key);
                    client.embed(request).await
                }
            }
            .map_err(|err| {
                FriggError::Internal(format!("semantic embedding provider call failed: {err}"))
            })?;

            Ok(response
                .vectors
                .into_iter()
                .map(|vector| vector.values)
                .collect::<Vec<_>>())
        })
    }
}

pub(super) fn resolve_semantic_runtime_config_from_env() -> FriggResult<SemanticRuntimeConfig> {
    let enabled = parse_optional_bool_env(FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV)?.unwrap_or(false);
    if !enabled {
        return Ok(SemanticRuntimeConfig::default());
    }
    let strict_mode =
        parse_optional_bool_env(FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV)?.unwrap_or(false);
    let provider = std::env::var(FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV)
        .ok()
        .map(|raw| {
            SemanticRuntimeProvider::from_str(raw.trim()).map_err(|message| {
                FriggError::InvalidInput(format!(
                    "invalid {} value: {message}",
                    FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV
                ))
            })
        })
        .transpose()?;
    let model = std::env::var(FRIGG_SEMANTIC_RUNTIME_MODEL_ENV)
        .ok()
        .map(|raw| raw.trim().to_owned());

    Ok(SemanticRuntimeConfig {
        enabled,
        provider,
        model,
        strict_mode,
    })
}

fn parse_optional_bool_env(name: &str) -> FriggResult<Option<bool>> {
    let Some(raw) = std::env::var(name).ok() else {
        return Ok(None);
    };
    let normalized = raw.trim().to_ascii_lowercase();
    let value = match normalized.as_str() {
        "1" | "true" => true,
        "0" | "false" => false,
        _ => {
            return Err(FriggError::InvalidInput(format!(
                "{name} must be one of: true,false,1,0 (received: {normalized})"
            )));
        }
    };
    Ok(Some(value))
}

pub(super) fn build_semantic_embedding_records(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<Vec<SemanticChunkEmbeddingRecord>> {
    semantic_runtime
        .validate_startup(&credentials)
        .map_err(|err| {
            FriggError::InvalidInput(format!(
                "semantic runtime validation failed code={}: {err}",
                err.code()
            ))
        })?;

    let provider = semantic_runtime.provider.ok_or_else(|| {
        FriggError::Internal("semantic runtime provider missing after validation".to_owned())
    })?;
    let model = semantic_runtime.normalized_model().ok_or_else(|| {
        FriggError::Internal("semantic runtime model missing after validation".to_owned())
    })?;
    let chunks = build_semantic_chunk_candidates(
        repository_id,
        workspace_root,
        snapshot_id,
        current_manifest,
    )?;

    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let trace_id = deterministic_semantic_trace_id(repository_id, snapshot_id, provider, model);
    let mut output = Vec::with_capacity(chunks.len());
    let total_batches = chunks.len().div_ceil(SEMANTIC_EMBEDDING_BATCH_SIZE);
    for (batch_index, batch) in chunks.chunks(SEMANTIC_EMBEDDING_BATCH_SIZE).enumerate() {
        let batch_input = batch
            .iter()
            .map(|chunk| chunk.content_text.clone())
            .collect::<Vec<_>>();
        let vectors = execute_semantic_embedding_batch(
            executor,
            provider,
            model,
            batch_input,
            Some(trace_id.clone()),
        )
        .map_err(|error| {
            let first_anchor = batch
                .first()
                .map(|chunk| format!("{}:{}-{}", chunk.path, chunk.start_line, chunk.end_line))
                .unwrap_or_else(|| "-".to_owned());
            let last_anchor = batch
                .last()
                .map(|chunk| format!("{}:{}-{}", chunk.path, chunk.start_line, chunk.end_line))
                .unwrap_or_else(|| "-".to_owned());
            FriggError::Internal(format!(
                "semantic embedding batch failed batch_index={} total_batches={} batch_size={} first_chunk={} last_chunk={}: {}",
                batch_index,
                total_batches,
                batch.len(),
                first_anchor,
                last_anchor,
                error
            ))
        })?;
        if vectors.len() != batch.len() {
            return Err(FriggError::Internal(format!(
                "semantic embedding provider response length mismatch: expected {} vectors, received {}",
                batch.len(),
                vectors.len()
            )));
        }

        for (chunk, embedding) in batch.iter().zip(vectors.into_iter()) {
            if embedding.is_empty() {
                return Err(FriggError::Internal(format!(
                    "semantic embedding provider returned an empty vector for chunk_id={}",
                    chunk.chunk_id
                )));
            }
            if embedding.iter().any(|value| !value.is_finite()) {
                return Err(FriggError::Internal(format!(
                    "semantic embedding provider returned non-finite vector values for chunk_id={}",
                    chunk.chunk_id
                )));
            }

            output.push(SemanticChunkEmbeddingRecord {
                chunk_id: chunk.chunk_id.clone(),
                repository_id: chunk.repository_id.clone(),
                snapshot_id: chunk.snapshot_id.clone(),
                path: chunk.path.clone(),
                language: chunk.language.clone(),
                chunk_index: chunk.chunk_index,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                provider: provider.as_str().to_owned(),
                model: model.to_owned(),
                trace_id: Some(trace_id.clone()),
                content_hash_blake3: chunk.content_hash_blake3.clone(),
                content_text: chunk.content_text.clone(),
                embedding,
            });
        }
    }

    output.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.chunk_index.cmp(&right.chunk_index))
            .then(left.chunk_id.cmp(&right.chunk_id))
    });
    Ok(output)
}

pub(crate) fn build_semantic_chunk_candidates(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
) -> FriggResult<Vec<SemanticChunkCandidate>> {
    let mut output = Vec::new();

    for entry in current_manifest {
        let Some(language) = semantic_chunk_language_for_path(&entry.path) else {
            continue;
        };
        let source = match fs::read_to_string(&entry.path) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let repository_relative_path =
            normalize_repository_relative_path(workspace_root, &entry.path)?;
        if repository_relative_path.starts_with("playbooks/") {
            continue;
        }
        let source = scrub_playbook_metadata_header(&source);
        output.extend(build_file_semantic_chunks(
            repository_id,
            snapshot_id,
            &repository_relative_path,
            language,
            source.as_ref(),
        ));
    }

    output.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.chunk_index.cmp(&right.chunk_index))
            .then(left.chunk_id.cmp(&right.chunk_id))
    });
    Ok(output)
}

pub(crate) fn build_file_semantic_chunks(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    language: &str,
    source: &str,
) -> Vec<SemanticChunkCandidate> {
    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_chars = 0usize;
    let mut start_line = 1usize;
    let mut chunk_index = 0usize;
    let markdown_chunking = language == "markdown";

    for (line_idx, line) in source.lines().enumerate() {
        let line_number = line_idx + 1;
        let markdown_heading_boundary =
            markdown_chunking && !current_lines.is_empty() && is_markdown_heading(line);
        let projected_chars = current_chars + line.len() + usize::from(!current_lines.is_empty());
        let should_flush = markdown_heading_boundary
            || (!current_lines.is_empty()
                && (current_lines.len() >= SEMANTIC_CHUNK_MAX_LINES
                    || projected_chars > SEMANTIC_CHUNK_MAX_CHARS));

        if should_flush {
            let created = create_semantic_chunk_candidates(
                repository_id,
                snapshot_id,
                path,
                language,
                chunk_index,
                start_line,
                line_number.saturating_sub(1),
                &current_lines,
            );
            chunk_index += created.len();
            chunks.extend(created);
            current_lines.clear();
            current_chars = 0;
            start_line = line_number;
        }

        current_chars += line.len() + usize::from(!current_lines.is_empty());
        current_lines.push(line);
    }

    let created = create_semantic_chunk_candidates(
        repository_id,
        snapshot_id,
        path,
        language,
        chunk_index,
        start_line,
        source.lines().count().max(start_line),
        &current_lines,
    );
    chunks.extend(created);

    chunks
}

fn create_semantic_chunk_candidates(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    language: &str,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    lines: &[&str],
) -> Vec<SemanticChunkCandidate> {
    if lines.is_empty() {
        return Vec::new();
    }
    let content_text = lines.join("\n");
    if content_text.trim().is_empty() {
        return Vec::new();
    }

    split_text_for_semantic_chunking(&content_text, SEMANTIC_CHUNK_MAX_CHARS)
        .into_iter()
        .enumerate()
        .map(|(offset, segment_text)| {
            let mut content_hasher = Hasher::new();
            content_hasher.update(segment_text.as_bytes());
            let content_hash_blake3 = content_hasher.finalize().to_hex().to_string();

            let segment_chunk_index = chunk_index + offset;
            let mut chunk_id_hasher = Hasher::new();
            chunk_id_hasher.update(repository_id.as_bytes());
            chunk_id_hasher.update(&[0]);
            chunk_id_hasher.update(path.as_bytes());
            chunk_id_hasher.update(&[0]);
            chunk_id_hasher.update(segment_chunk_index.to_string().as_bytes());
            chunk_id_hasher.update(&[0]);
            chunk_id_hasher.update(start_line.to_string().as_bytes());
            chunk_id_hasher.update(&[0]);
            chunk_id_hasher.update(end_line.to_string().as_bytes());
            chunk_id_hasher.update(&[0]);
            chunk_id_hasher.update(content_hash_blake3.as_bytes());

            SemanticChunkCandidate {
                chunk_id: format!("chunk-{}", chunk_id_hasher.finalize().to_hex()),
                repository_id: repository_id.to_owned(),
                snapshot_id: snapshot_id.to_owned(),
                path: path.to_owned(),
                language: language.to_owned(),
                chunk_index: segment_chunk_index,
                start_line,
                end_line,
                content_hash_blake3,
                content_text: segment_text,
            }
        })
        .collect()
}

fn split_text_for_semantic_chunking(content_text: &str, max_chars: usize) -> Vec<String> {
    if content_text.is_empty() || max_chars == 0 {
        return Vec::new();
    }

    let total_chars = content_text.chars().count();
    if total_chars <= max_chars {
        return vec![content_text.to_owned()];
    }

    let mut segments = Vec::new();
    let mut segment_start = 0usize;
    let mut chars_in_segment = 0usize;

    for (byte_index, _) in content_text.char_indices() {
        if chars_in_segment == max_chars {
            segments.push(content_text[segment_start..byte_index].to_owned());
            segment_start = byte_index;
            chars_in_segment = 0;
        }
        chars_in_segment += 1;
    }

    if segment_start < content_text.len() {
        segments.push(content_text[segment_start..].to_owned());
    }

    segments
}

pub(crate) fn semantic_chunk_language_for_path(path: &Path) -> Option<&'static str> {
    if is_blade_path(path) {
        return Some("blade");
    }
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("rs") => Some("rust"),
        Some("php") => Some("php"),
        Some("md" | "markdown") => Some("markdown"),
        Some("json") => Some("json"),
        Some("toml") => Some("toml"),
        Some("txt") => Some("text"),
        Some("yaml" | "yml") => Some("yaml"),
        _ => None,
    }
}

fn is_markdown_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut heading_hashes = 0usize;
    for ch in trimmed.chars() {
        if ch == '#' {
            heading_hashes += 1;
            continue;
        }
        return heading_hashes > 0 && heading_hashes <= 6 && ch.is_ascii_whitespace();
    }
    false
}

fn deterministic_semantic_trace_id(
    repository_id: &str,
    snapshot_id: &str,
    provider: SemanticRuntimeProvider,
    model: &str,
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(repository_id.as_bytes());
    hasher.update(&[0]);
    hasher.update(snapshot_id.as_bytes());
    hasher.update(&[0]);
    hasher.update(provider.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(model.as_bytes());
    format!("trace-semantic-{}", hasher.finalize().to_hex())
}
