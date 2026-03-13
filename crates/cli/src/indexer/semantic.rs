use std::fs::File;
use std::future::Future;
use std::io::Read;
use std::pin::Pin;
use std::sync::Arc;

use super::*;
use crate::indexer::manifest::normalize_repository_relative_path;
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider};
use crate::storage::{DEFAULT_VECTOR_DIMENSIONS, SemanticChunkEmbeddingRecord};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticChunkCandidate {
    pub(crate) chunk_id: blake3::Hash,
    pub(crate) repository_id: Arc<str>,
    pub(crate) snapshot_id: Arc<str>,
    pub(crate) path: Arc<str>,
    pub(crate) language: Arc<str>,
    pub(crate) chunk_index: usize,
    pub(crate) start_line: usize,
    pub(crate) end_line: usize,
    pub(crate) content_hash_blake3: blake3::Hash,
    pub(crate) content_text: String,
}

impl SemanticChunkCandidate {
    fn chunk_id_string(&self) -> String {
        semantic_chunk_id_string(&self.chunk_id)
    }

    fn content_hash_blake3_string(&self) -> String {
        self.content_hash_blake3.to_hex().to_string()
    }
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
                dimensions: Some(DEFAULT_VECTOR_DIMENSIONS),
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
                    chunk.chunk_id_string()
                )));
            }
            if embedding.iter().any(|value| !value.is_finite()) {
                return Err(FriggError::Internal(format!(
                    "semantic embedding provider returned non-finite vector values for chunk_id={}",
                    chunk.chunk_id_string()
                )));
            }

            output.push(SemanticChunkEmbeddingRecord {
                chunk_id: chunk.chunk_id_string(),
                repository_id: chunk.repository_id.to_string(),
                snapshot_id: chunk.snapshot_id.to_string(),
                path: chunk.path.to_string(),
                language: chunk.language.to_string(),
                chunk_index: chunk.chunk_index,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                provider: provider.as_str().to_owned(),
                model: model.to_owned(),
                trace_id: Some(trace_id.clone()),
                content_hash_blake3: chunk.content_hash_blake3_string(),
                content_text: chunk.content_text.clone(),
                embedding,
            });
        }
    }

    output.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.chunk_index.cmp(&right.chunk_index))
            .then(left.chunk_id.as_bytes().cmp(right.chunk_id.as_bytes()))
    });
    Ok(output)
}

pub(crate) fn build_semantic_chunk_candidates(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
) -> FriggResult<Vec<SemanticChunkCandidate>> {
    let repository_id = Arc::<str>::from(repository_id);
    let snapshot_id = Arc::<str>::from(snapshot_id);
    let mut output = Vec::with_capacity(
        estimate_semantic_chunk_capacity(current_manifest).max(current_manifest.len()),
    );
    let mut last_repository_relative_path: Option<String> = None;
    let mut needs_sort = false;
    let mut source = String::new();

    for entry in current_manifest {
        let Some(language) = semantic_chunk_language_for_path(&entry.path) else {
            continue;
        };
        let repository_relative_path =
            normalize_repository_relative_path(workspace_root, &entry.path)?;
        if last_repository_relative_path
            .as_ref()
            .is_some_and(|previous| previous > &repository_relative_path)
        {
            needs_sort = true;
        }
        source.clear();
        let mut file = match File::open(&entry.path) {
            Ok(file) => file,
            Err(_) => continue,
        };
        if file.read_to_string(&mut source).is_err() {
            continue;
        }
        append_file_semantic_chunks(
            &mut output,
            Arc::clone(&repository_id),
            Arc::clone(&snapshot_id),
            Arc::<str>::from(repository_relative_path.as_str()),
            language,
            source.as_str(),
        );
        last_repository_relative_path = Some(repository_relative_path);
    }

    if needs_sort {
        output.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.chunk_index.cmp(&right.chunk_index))
                .then(left.chunk_id.as_bytes().cmp(right.chunk_id.as_bytes()))
        });
    }
    Ok(output)
}

pub(crate) fn build_file_semantic_chunks(
    repository_id: impl Into<Arc<str>>,
    snapshot_id: impl Into<Arc<str>>,
    path: impl Into<Arc<str>>,
    language: impl Into<Arc<str>>,
    source: &str,
) -> Vec<SemanticChunkCandidate> {
    let file_context = SemanticChunkFileContext::new(
        repository_id.into(),
        snapshot_id.into(),
        path.into(),
        language.into(),
    );
    let estimated_chunks = source
        .len()
        .max(1)
        .div_ceil(SEMANTIC_CHUNK_MAX_CHARS.max(1));
    let mut chunks = Vec::with_capacity(estimated_chunks);
    append_file_semantic_chunks_with_context(&mut chunks, &file_context, source);
    chunks
}

fn append_file_semantic_chunks(
    output: &mut Vec<SemanticChunkCandidate>,
    repository_id: impl Into<Arc<str>>,
    snapshot_id: impl Into<Arc<str>>,
    path: impl Into<Arc<str>>,
    language: impl Into<Arc<str>>,
    source: &str,
) {
    let file_context = SemanticChunkFileContext::new(
        repository_id.into(),
        snapshot_id.into(),
        path.into(),
        language.into(),
    );
    append_file_semantic_chunks_with_context(output, &file_context, source);
}

fn append_file_semantic_chunks_with_context(
    output: &mut Vec<SemanticChunkCandidate>,
    file_context: &SemanticChunkFileContext,
    source: &str,
) {
    let markdown_chunking = file_context.language.as_ref() == "markdown";
    if let Some(single_chunk) =
        build_single_semantic_chunk_candidate_if_small(file_context, markdown_chunking, source)
    {
        output.extend(single_chunk);
        return;
    }

    let mut current_chunk_start = 0usize;
    let mut current_chars = 0usize;
    let mut start_line = 1usize;
    let mut chunk_index = 0usize;
    let mut current_line = 0usize;

    for (line_idx, raw_line) in source.split_inclusive('\n').enumerate() {
        let line = raw_line.trim_end_matches(['\n', '\r']);
        let line_number = line_idx + 1;
        current_line = line_number;
        let markdown_heading_boundary =
            markdown_chunking && current_chars > 0 && is_markdown_heading(line);
        let projected_chars = current_chars + line.len() + usize::from(current_chars > 0);
        let should_flush = markdown_heading_boundary
            || (current_chars > 0
                && (line_number.saturating_sub(start_line) >= SEMANTIC_CHUNK_MAX_LINES
                    || projected_chars > SEMANTIC_CHUNK_MAX_CHARS));

        if should_flush {
            let created = append_semantic_chunk_candidates(
                output,
                &file_context,
                chunk_index,
                start_line,
                line_number.saturating_sub(1),
                semantic_chunk_text_from_source(
                    source,
                    current_chunk_start,
                    raw_line.as_ptr() as usize - source.as_ptr() as usize,
                ),
            );
            chunk_index += created.len();
            current_chars = 0;
            start_line = line_number;
            current_chunk_start = raw_line.as_ptr() as usize - source.as_ptr() as usize;
        }

        current_chars += line.len() + usize::from(current_chars > 0);
    }

    append_semantic_chunk_candidates(
        output,
        &file_context,
        chunk_index,
        start_line,
        current_line.max(start_line),
        semantic_chunk_text_from_source(source, current_chunk_start, source.len()),
    );
}

fn build_single_semantic_chunk_candidate_if_small(
    file_context: &SemanticChunkFileContext,
    markdown_chunking: bool,
    source: &str,
) -> Option<Vec<SemanticChunkCandidate>> {
    if markdown_chunking || !source.is_ascii() || source.len() > SEMANTIC_CHUNK_MAX_CHARS {
        return None;
    }

    let content_text = semantic_chunk_text_from_source(source, 0, source.len());
    if content_text.trim().is_empty() {
        return Some(Vec::new());
    }

    let line_count = content_text
        .as_bytes()
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1;
    if line_count > SEMANTIC_CHUNK_MAX_LINES {
        return None;
    }

    Some(vec![build_semantic_chunk_candidate(
        file_context,
        0,
        1,
        line_count,
        content_text.to_owned(),
    )])
}

fn append_semantic_chunk_candidates(
    output: &mut Vec<SemanticChunkCandidate>,
    file_context: &SemanticChunkFileContext,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    content_text: &str,
) -> std::ops::Range<usize> {
    if content_text.trim().is_empty() {
        return 0..0;
    }

    let output_start = output.len();
    if content_text.is_ascii() {
        let mut segment_start = 0usize;
        let mut offset = 0usize;
        while segment_start < content_text.len() {
            let segment_end = (segment_start + SEMANTIC_CHUNK_MAX_CHARS).min(content_text.len());
            output.push(build_semantic_chunk_candidate(
                file_context,
                chunk_index + offset,
                start_line,
                end_line,
                content_text[segment_start..segment_end].to_owned(),
            ));
            segment_start = segment_end;
            offset += 1;
        }
        return output_start..output.len();
    }

    let unicode_char_count = content_text.chars().count();
    if unicode_char_count <= SEMANTIC_CHUNK_MAX_CHARS {
        output.push(build_semantic_chunk_candidate(
            file_context,
            chunk_index,
            start_line,
            end_line,
            content_text.to_owned(),
        ));
        return output_start..output.len();
    }

    let mut segment_start = 0usize;
    let mut chars_in_segment = 0usize;
    let mut offset = 0usize;
    for (byte_index, _) in content_text.char_indices() {
        if chars_in_segment == SEMANTIC_CHUNK_MAX_CHARS {
            output.push(build_semantic_chunk_candidate(
                file_context,
                chunk_index + offset,
                start_line,
                end_line,
                content_text[segment_start..byte_index].to_owned(),
            ));
            segment_start = byte_index;
            chars_in_segment = 0;
            offset += 1;
        }
        chars_in_segment += 1;
    }
    if segment_start < content_text.len() {
        output.push(build_semantic_chunk_candidate(
            file_context,
            chunk_index + offset,
            start_line,
            end_line,
            content_text[segment_start..].to_owned(),
        ));
    }

    output_start..output.len()
}

fn build_semantic_chunk_candidate(
    file_context: &SemanticChunkFileContext,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    content_text: String,
) -> SemanticChunkCandidate {
    let content_hash = blake3::hash(content_text.as_bytes());

    let mut chunk_id_hasher = file_context.chunk_id_prefix.clone();
    chunk_id_hasher.update(&chunk_index.to_le_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(&start_line.to_le_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(&end_line.to_le_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(content_hash.as_bytes());
    let chunk_id = chunk_id_hasher.finalize();

    SemanticChunkCandidate {
        chunk_id,
        repository_id: Arc::clone(&file_context.repository_id),
        snapshot_id: Arc::clone(&file_context.snapshot_id),
        path: Arc::clone(&file_context.path),
        language: Arc::clone(&file_context.language),
        chunk_index,
        start_line,
        end_line,
        content_hash_blake3: content_hash,
        content_text,
    }
}

fn semantic_chunk_text_from_source(source: &str, start: usize, end: usize) -> &str {
    source[start..end].trim_end_matches(['\n', '\r'])
}

fn semantic_chunk_id_string(chunk_id: &blake3::Hash) -> String {
    let chunk_id_hex = chunk_id.to_hex();
    let mut value = String::with_capacity("chunk-".len() + chunk_id_hex.as_str().len());
    value.push_str("chunk-");
    value.push_str(chunk_id_hex.as_str());
    value
}

fn estimate_semantic_chunk_capacity(current_manifest: &[FileDigest]) -> usize {
    current_manifest
        .iter()
        .filter(|entry| semantic_chunk_language_for_path(&entry.path).is_some())
        .map(|entry| {
            usize::try_from(entry.size_bytes)
                .unwrap_or(usize::MAX)
                .max(1)
                .div_ceil(SEMANTIC_CHUNK_MAX_CHARS.max(1))
        })
        .sum()
}

struct SemanticChunkFileContext {
    repository_id: Arc<str>,
    snapshot_id: Arc<str>,
    path: Arc<str>,
    language: Arc<str>,
    chunk_id_prefix: Hasher,
}

impl SemanticChunkFileContext {
    fn new(
        repository_id: Arc<str>,
        snapshot_id: Arc<str>,
        path: Arc<str>,
        language: Arc<str>,
    ) -> Self {
        let mut chunk_id_prefix = Hasher::new();
        chunk_id_prefix.update(repository_id.as_bytes());
        chunk_id_prefix.update(&[0]);
        chunk_id_prefix.update(path.as_bytes());
        chunk_id_prefix.update(&[0]);
        Self {
            repository_id,
            snapshot_id,
            path,
            language,
            chunk_id_prefix,
        }
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
