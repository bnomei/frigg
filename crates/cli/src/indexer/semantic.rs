//! Semantic chunk construction and embedding provider bridge.
//!
//! Turns manifest entries into bounded text chunks, assigns deterministic chunk ids, and batches
//! embedding requests through the configured semantic runtime.

use std::fs::File;
use std::future::Future;
use std::io::Read;
use std::pin::Pin;
use std::sync::Arc;

use rayon::prelude::*;

use super::*;
use crate::embeddings::{
    LocalArtifactPolicy, SemanticEmbeddingProviderFactoryConfig,
    cached_semantic_embedding_provider, provider_factory::canonical_provider_model,
};
use crate::indexer::manifest::normalize_repository_relative_path;
use crate::settings::{
    OPENAI_COMPAT_ENDPOINT_ENV_VAR, SemanticRuntimeConfig, SemanticRuntimeCredentials,
    SemanticRuntimeProvider,
};
use crate::storage::{
    DEFAULT_VECTOR_DIMENSIONS, ManifestEntry, SemanticChunkEmbeddingRecord, SemanticHeadRecord,
    Storage, StorageSession,
};

/// One embeddable semantic chunk before vector write: identity, envelope text, and content hash.
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
    #[allow(clippy::type_complexity)]
    fn embed_documents<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        input: Vec<String>,
        trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>>;
}

pub(super) trait SemanticIndexStorage {
    fn load_semantic_head_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<Option<SemanticHeadRecord>>;

    fn load_manifest_for_snapshot(&self, snapshot_id: &str) -> FriggResult<Vec<ManifestEntry>>;

    fn load_semantic_embeddings_for_repository_model_chunk_ids(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
        chunk_ids: &[String],
    ) -> FriggResult<std::collections::BTreeMap<String, SemanticChunkEmbeddingRecord>>;
}

impl SemanticIndexStorage for Storage {
    fn load_semantic_head_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<Option<SemanticHeadRecord>> {
        Storage::load_semantic_head_for_repository_model(self, repository_id, provider, model)
    }

    fn load_manifest_for_snapshot(&self, snapshot_id: &str) -> FriggResult<Vec<ManifestEntry>> {
        Storage::load_manifest_for_snapshot(self, snapshot_id)
    }

    fn load_semantic_embeddings_for_repository_model_chunk_ids(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
        chunk_ids: &[String],
    ) -> FriggResult<std::collections::BTreeMap<String, SemanticChunkEmbeddingRecord>> {
        Storage::load_semantic_embeddings_for_repository_model_chunk_ids(
            self,
            repository_id,
            provider,
            model,
            chunk_ids,
        )
    }
}

impl SemanticIndexStorage for StorageSession {
    fn load_semantic_head_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<Option<SemanticHeadRecord>> {
        StorageSession::load_semantic_head_for_repository_model(
            self,
            repository_id,
            provider,
            model,
        )
    }

    fn load_manifest_for_snapshot(&self, snapshot_id: &str) -> FriggResult<Vec<ManifestEntry>> {
        StorageSession::load_manifest_for_snapshot(self, snapshot_id)
    }

    fn load_semantic_embeddings_for_repository_model_chunk_ids(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
        chunk_ids: &[String],
    ) -> FriggResult<std::collections::BTreeMap<String, SemanticChunkEmbeddingRecord>> {
        StorageSession::load_semantic_embeddings_for_repository_model_chunk_ids(
            self,
            repository_id,
            provider,
            model,
            chunk_ids,
        )
    }
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
    runtime: &tokio::runtime::Runtime,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    provider: SemanticRuntimeProvider,
    model: &str,
    input: Vec<String>,
    trace_id: Option<String>,
) -> FriggResult<Vec<Vec<f32>>> {
    runtime.block_on(executor.embed_documents(provider, model, input, trace_id))
}

#[derive(Debug, Default)]
pub(super) struct RuntimeSemanticEmbeddingExecutor {
    credentials: SemanticRuntimeCredentials,
    endpoint: Option<String>,
}

impl RuntimeSemanticEmbeddingExecutor {
    pub(super) fn with_endpoint(
        credentials: SemanticRuntimeCredentials,
        endpoint: Option<String>,
    ) -> Self {
        Self {
            credentials,
            endpoint,
        }
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
        Box::pin(async move {
            let request = EmbeddingRequest {
                model,
                input,
                purpose: EmbeddingPurpose::Document,
                dimensions: Some(DEFAULT_VECTOR_DIMENSIONS),
                trace_id,
            };
            let client =
                cached_semantic_embedding_provider(SemanticEmbeddingProviderFactoryConfig {
                    provider,
                    model: &request.model,
                    credentials: &self.credentials,
                    local_artifact_policy: LocalArtifactPolicy::AllowPreparation,
                    endpoint: self.endpoint.as_deref(),
                })
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "semantic embedding provider construction failed: {err}"
                    ))
                })?;
            let response = client.embed(request).await.map_err(|err| {
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
        .transpose()?
        .or(Some(SemanticRuntimeProvider::Local));
    let model = std::env::var(FRIGG_SEMANTIC_RUNTIME_MODEL_ENV)
        .ok()
        .map(|raw| raw.trim().to_owned());
    let openai_compat_endpoint = std::env::var(OPENAI_COMPAT_ENDPOINT_ENV_VAR)
        .ok()
        .map(|raw| raw.trim().to_owned())
        .filter(|value| !value.is_empty());

    Ok(SemanticRuntimeConfig {
        enabled,
        provider,
        model,
        strict_mode,
        openai_compat_endpoint,
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

#[allow(clippy::too_many_arguments)]
pub(super) fn build_semantic_embedding_records(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    storage: Option<&dyn SemanticIndexStorage>,
    on_file_progress: &mut impl FnMut(usize, usize),
) -> FriggResult<SemanticEmbeddingBuild> {
    semantic_runtime
        .validate_startup(credentials)
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
    let model = canonical_provider_model(provider, model).map_err(|err| {
        FriggError::InvalidInput(format!("semantic runtime model validation failed: {err}"))
    })?;
    let model = model.as_str();
    let SemanticChunkBuild { candidates: chunks } = build_semantic_chunk_candidates(
        repository_id,
        workspace_root,
        snapshot_id,
        current_manifest,
    )?;

    if chunks.is_empty() {
        return Ok(SemanticEmbeddingBuild {
            records: Vec::new(),
        });
    }

    let (mut records, chunks_to_embed) = reuse_existing_semantic_embedding_records(
        repository_id,
        snapshot_id,
        provider,
        model,
        &chunks,
        storage,
    )?;
    let trace_id = deterministic_semantic_trace_id(repository_id, snapshot_id, provider, model);
    if !chunks_to_embed.is_empty() {
        let files_total = semantic_chunk_file_count(&chunks_to_embed);
        on_file_progress(0, files_total);
        records.extend(execute_semantic_embedding_batches(
            provider,
            model,
            &chunks_to_embed,
            &trace_id,
            executor,
            on_file_progress,
            files_total,
        )?);
    }
    sort_semantic_embedding_records(&mut records);
    Ok(SemanticEmbeddingBuild { records })
}

fn semantic_chunk_file_count(chunks: &[SemanticChunkCandidate]) -> usize {
    chunks
        .windows(2)
        .filter(|pair| pair[0].path != pair[1].path)
        .count()
        .saturating_add(usize::from(!chunks.is_empty()))
}

fn reuse_existing_semantic_embedding_records(
    repository_id: &str,
    snapshot_id: &str,
    provider: SemanticRuntimeProvider,
    model: &str,
    chunks: &[SemanticChunkCandidate],
    storage: Option<&dyn SemanticIndexStorage>,
) -> FriggResult<(
    Vec<SemanticChunkEmbeddingRecord>,
    Vec<SemanticChunkCandidate>,
)> {
    let Some(storage) = storage else {
        return Ok((Vec::new(), chunks.to_vec()));
    };
    let chunk_ids = chunks
        .iter()
        .map(SemanticChunkCandidate::chunk_id_string)
        .collect::<Vec<_>>();
    let existing_records = storage.load_semantic_embeddings_for_repository_model_chunk_ids(
        repository_id,
        provider.as_str(),
        model,
        &chunk_ids,
    )?;
    let mut reused_records = Vec::new();
    let mut chunks_to_embed = Vec::new();
    for chunk in chunks {
        let chunk_id = chunk.chunk_id_string();
        let Some(existing_record) = existing_records.get(&chunk_id) else {
            chunks_to_embed.push(chunk.clone());
            continue;
        };
        if reusable_semantic_embedding_record(
            chunk,
            existing_record,
            repository_id,
            provider,
            model,
        ) {
            reused_records.push(rewrite_reused_semantic_embedding_record(
                chunk,
                existing_record,
                snapshot_id,
            ));
        } else {
            chunks_to_embed.push(chunk.clone());
        }
    }

    Ok((reused_records, chunks_to_embed))
}

fn reusable_semantic_embedding_record(
    chunk: &SemanticChunkCandidate,
    record: &SemanticChunkEmbeddingRecord,
    repository_id: &str,
    provider: SemanticRuntimeProvider,
    model: &str,
) -> bool {
    record.repository_id == repository_id
        && record.provider == provider.as_str()
        && record.model == model
        && record.chunk_id == chunk.chunk_id_string()
        && record.content_hash_blake3 == chunk.content_hash_blake3_string()
        && !record.embedding.is_empty()
        && record.embedding.iter().all(|value| value.is_finite())
}

fn rewrite_reused_semantic_embedding_record(
    chunk: &SemanticChunkCandidate,
    record: &SemanticChunkEmbeddingRecord,
    snapshot_id: &str,
) -> SemanticChunkEmbeddingRecord {
    SemanticChunkEmbeddingRecord {
        chunk_id: chunk.chunk_id_string(),
        repository_id: chunk.repository_id.to_string(),
        snapshot_id: snapshot_id.to_owned(),
        path: chunk.path.to_string(),
        language: chunk.language.to_string(),
        chunk_index: chunk.chunk_index,
        start_line: chunk.start_line,
        end_line: chunk.end_line,
        provider: record.provider.clone(),
        model: record.model.clone(),
        trace_id: record.trace_id.clone(),
        content_hash_blake3: chunk.content_hash_blake3_string(),
        content_text: chunk.content_text.clone(),
        embedding: record.embedding.clone(),
    }
}

fn sort_semantic_embedding_records(records: &mut [SemanticChunkEmbeddingRecord]) {
    records.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.chunk_index.cmp(&right.chunk_index))
            .then(left.chunk_id.as_bytes().cmp(right.chunk_id.as_bytes()))
    });
}

fn execute_semantic_embedding_batches(
    provider: SemanticRuntimeProvider,
    model: &str,
    chunks: &[SemanticChunkCandidate],
    trace_id: &str,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    on_file_progress: &mut impl FnMut(usize, usize),
    files_total: usize,
) -> FriggResult<Vec<SemanticChunkEmbeddingRecord>> {
    if tokio::runtime::Handle::try_current().is_ok() {
        return std::thread::scope(|scope| {
            let (progress_sender, progress_receiver) = std::sync::mpsc::channel();
            let handle = scope.spawn(move || {
                let runtime = build_semantic_embedding_runtime()?;
                let mut send_progress = |files_completed, files_total| {
                    let _ = progress_sender.send((files_completed, files_total));
                };
                build_semantic_embedding_records_with_runtime(
                    provider,
                    model,
                    chunks,
                    trace_id,
                    executor,
                    &runtime,
                    &mut send_progress,
                    files_total,
                )
            });
            for (files_completed, files_total) in progress_receiver {
                on_file_progress(files_completed, files_total);
            }
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
    build_semantic_embedding_records_with_runtime(
        provider,
        model,
        chunks,
        trace_id,
        executor,
        &runtime,
        on_file_progress,
        files_total,
    )
}

fn build_semantic_embedding_records_with_runtime(
    provider: SemanticRuntimeProvider,
    model: &str,
    chunks: &[SemanticChunkCandidate],
    trace_id: &str,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    runtime: &tokio::runtime::Runtime,
    on_file_progress: &mut impl FnMut(usize, usize),
    files_total: usize,
) -> FriggResult<Vec<SemanticChunkEmbeddingRecord>> {
    let mut output = Vec::with_capacity(chunks.len());
    let total_batches = chunks.len().div_ceil(SEMANTIC_EMBEDDING_BATCH_SIZE);
    let mut completed_files = 0usize;
    let mut previous_path: Option<&Arc<str>> = None;
    for (batch_index, batch) in chunks.chunks(SEMANTIC_EMBEDDING_BATCH_SIZE).enumerate() {
        let batch_input = batch
            .iter()
            .map(|chunk| {
                semantic_embed_document_text(&chunk.path, &chunk.language, &chunk.content_text)
            })
            .collect::<Vec<_>>();
        let vectors = execute_semantic_embedding_batch(
            runtime,
            executor,
            provider,
            model,
            batch_input,
            Some(trace_id.to_owned()),
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

        for (chunk, embedding) in batch.iter().zip(vectors) {
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
                trace_id: Some(trace_id.to_owned()),
                content_hash_blake3: chunk.content_hash_blake3_string(),
                content_text: chunk.content_text.clone(),
                embedding,
            });
        }
        for chunk in batch {
            if previous_path.is_some_and(|path| *path != chunk.path) {
                completed_files = completed_files.saturating_add(1);
            }
            previous_path = Some(&chunk.path);
        }
        if batch_index + 1 == total_batches {
            completed_files = completed_files.saturating_add(1);
        }
        on_file_progress(completed_files, files_total);
    }

    sort_semantic_embedding_records(&mut output);
    Ok(output)
}

/// Embedding-stage output: durable chunk embedding records ready for vector store write.
pub(crate) struct SemanticEmbeddingBuild {
    pub(crate) records: Vec<SemanticChunkEmbeddingRecord>,
}

/// Chunking-stage output: candidates before provider embed and storage persistence.
pub(crate) struct SemanticChunkBuild {
    pub(crate) candidates: Vec<SemanticChunkCandidate>,
}

/// Build semantic chunk candidates for every language-supported path in the current manifest.
///
/// Reads each file once, skips unsupported languages, and sorts candidates by path then chunk index.
pub(crate) fn build_semantic_chunk_candidates(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
) -> FriggResult<SemanticChunkBuild> {
    let repository_id = Arc::<str>::from(repository_id);
    let snapshot_id = Arc::<str>::from(snapshot_id);
    let estimated_capacity =
        estimate_semantic_chunk_capacity(current_manifest).max(current_manifest.len());
    let mut output = current_manifest
        .par_iter()
        .map(|entry| {
            let Some(language) = semantic_chunk_language_for_path(&entry.path) else {
                return Ok::<Vec<SemanticChunkCandidate>, FriggError>(Vec::new());
            };
            let repository_relative_path =
                normalize_repository_relative_path(workspace_root, &entry.path)?;
            let mut source = String::new();
            let mut file = match File::open(&entry.path) {
                Ok(file) => file,
                Err(err) => {
                    return Err(FriggError::Io(err));
                }
            };
            if let Err(err) = file.read_to_string(&mut source) {
                return Err(FriggError::Io(err));
            }

            let mut chunks = Vec::new();
            append_file_semantic_chunks(
                &mut chunks,
                Arc::clone(&repository_id),
                Arc::clone(&snapshot_id),
                Arc::<str>::from(repository_relative_path.as_str()),
                language,
                source.as_str(),
            );
            Ok(chunks)
        })
        .try_reduce(
            || Vec::with_capacity(estimated_capacity),
            |mut left, mut right| {
                left.append(&mut right);
                Ok::<Vec<SemanticChunkCandidate>, FriggError>(left)
            },
        )?;

    output.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.chunk_index.cmp(&right.chunk_index))
            .then(left.chunk_id.as_bytes().cmp(right.chunk_id.as_bytes()))
    });
    Ok(SemanticChunkBuild { candidates: output })
}

/// Chunk a single source buffer into semantic candidates with path/language embed envelope fields.
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
                file_context,
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
        file_context,
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
        let mut segment_offset = 0usize;
        while segment_start < content_text.len() {
            let segment_end = (segment_start + SEMANTIC_CHUNK_MAX_CHARS).min(content_text.len());
            let segment_start_line =
                semantic_segment_start_line(start_line, &content_text[..segment_start]);
            push_nonblank_semantic_chunk_candidate(
                output,
                file_context,
                chunk_index + segment_offset,
                segment_start_line,
                semantic_chunk_end_line(
                    segment_start_line,
                    &content_text[segment_start..segment_end],
                )
                .min(end_line),
                &content_text[segment_start..segment_end],
            );
            segment_start = segment_end;
            segment_offset = output.len().saturating_sub(output_start);
        }
        return output_start..output.len();
    }

    let unicode_char_count = content_text.chars().count();
    if unicode_char_count <= SEMANTIC_CHUNK_MAX_CHARS {
        output.push(build_semantic_chunk_candidate(
            file_context,
            chunk_index,
            start_line,
            semantic_chunk_end_line(start_line, content_text).min(end_line),
            content_text.to_owned(),
        ));
        return output_start..output.len();
    }

    let mut segment_start = 0usize;
    let mut chars_in_segment = 0usize;
    let mut segment_offset = 0usize;
    for (byte_index, _) in content_text.char_indices() {
        if chars_in_segment == SEMANTIC_CHUNK_MAX_CHARS {
            let segment_start_line =
                semantic_segment_start_line(start_line, &content_text[..segment_start]);
            push_nonblank_semantic_chunk_candidate(
                output,
                file_context,
                chunk_index + segment_offset,
                segment_start_line,
                semantic_chunk_end_line(
                    segment_start_line,
                    &content_text[segment_start..byte_index],
                )
                .min(end_line),
                &content_text[segment_start..byte_index],
            );
            segment_start = byte_index;
            chars_in_segment = 0;
            segment_offset = output.len().saturating_sub(output_start);
        }
        chars_in_segment += 1;
    }
    if segment_start < content_text.len() {
        let segment_start_line =
            semantic_segment_start_line(start_line, &content_text[..segment_start]);
        push_nonblank_semantic_chunk_candidate(
            output,
            file_context,
            chunk_index + segment_offset,
            segment_start_line,
            semantic_chunk_end_line(segment_start_line, &content_text[segment_start..])
                .min(end_line),
            &content_text[segment_start..],
        );
    }

    output_start..output.len()
}

fn push_nonblank_semantic_chunk_candidate(
    output: &mut Vec<SemanticChunkCandidate>,
    file_context: &SemanticChunkFileContext,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    content_text: &str,
) {
    if content_text.trim().is_empty() {
        return;
    }
    output.push(build_semantic_chunk_candidate(
        file_context,
        chunk_index,
        start_line,
        semantic_chunk_end_line(start_line, content_text).min(end_line),
        content_text.to_owned(),
    ));
}

fn semantic_segment_start_line(start_line: usize, preceding_text: &str) -> usize {
    start_line.saturating_add(
        preceding_text
            .as_bytes()
            .iter()
            .filter(|byte| **byte == b'\n')
            .count(),
    )
}

fn semantic_chunk_end_line(start_line: usize, content_text: &str) -> usize {
    start_line.saturating_add(
        content_text
            .as_bytes()
            .iter()
            .filter(|byte| **byte == b'\n')
            .count(),
    )
}

fn build_semantic_chunk_candidate(
    file_context: &SemanticChunkFileContext,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    content_text: String,
) -> SemanticChunkCandidate {
    let content_hash = semantic_chunk_content_hash(
        &file_context.path,
        &file_context.language,
        &content_text,
    );

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

/// Path-centered document text sent to embedding providers (EXP-minilm-quality E).
///
/// Stored `content_text` stays pure source for excerpts; only the embed batch uses this envelope
/// so MiniLM (and other models) see relative path + language with the body.
///
/// `SEMANTIC_CHUNK_MAX_CHARS` still caps the **source body** only; the short header may push
/// the embed string slightly over that body budget.
///
/// Content hashes are taken over this exact string so any template edit invalidates reuse
/// without a separate version constant.
pub(crate) fn semantic_embed_document_text(path: &str, language: &str, content_text: &str) -> String {
    let mut out = String::with_capacity(path.len() + language.len() + content_text.len() + 32);
    out.push_str("path: ");
    out.push_str(path);
    out.push('\n');
    out.push_str("language: ");
    out.push_str(language);
    out.push_str("\n\n");
    out.push_str(content_text);
    out
}

/// Content identity for embed reindex: hashes the path/language envelope text, not body alone.
fn semantic_chunk_content_hash(path: &str, language: &str, content_text: &str) -> blake3::Hash {
    blake3::hash(semantic_embed_document_text(path, language, content_text).as_bytes())
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

#[cfg(test)]
mod embed_envelope_tests {
    use super::{semantic_chunk_content_hash, semantic_embed_document_text};

    #[test]
    fn semantic_embed_document_text_prefixes_path_and_language() {
        let text = semantic_embed_document_text(
            "crates/cli/src/foo.rs",
            "rust",
            "pub fn open_window() {}",
        );
        assert_eq!(
            text,
            "path: crates/cli/src/foo.rs\nlanguage: rust\n\npub fn open_window() {}"
        );
    }

    #[test]
    fn semantic_chunk_content_hash_tracks_envelope_fields() {
        let base = semantic_chunk_content_hash("a.rs", "rust", "fn a() {}");
        let other_path = semantic_chunk_content_hash("b.rs", "rust", "fn a() {}");
        let other_lang = semantic_chunk_content_hash("a.rs", "python", "fn a() {}");
        let other_body = semantic_chunk_content_hash("a.rs", "rust", "fn b() {}");
        assert_ne!(base, other_path);
        assert_ne!(base, other_lang);
        assert_ne!(base, other_body);
        assert_eq!(base, semantic_chunk_content_hash("a.rs", "rust", "fn a() {}"));
        assert_eq!(
            base,
            blake3::hash(semantic_embed_document_text("a.rs", "rust", "fn a() {}").as_bytes())
        );
    }
}
