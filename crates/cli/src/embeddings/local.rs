//! Local fastembed-backed embedding provider.
//!
//! Runs prepared on-disk models through fastembed batching and rejects unsafe cache overrides so
//! offline semantic indexing and recall stay reproducible across hosts.

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use fastembed::{TextEmbedding, TextInitOptions};

use super::local_model::{
    DEFAULT_LOCAL_EMBEDDING_MODEL, FRIGG_SEMANTIC_MODEL_CACHE_ENV, LocalModelArtifact,
    LocalModelError, require_prepared_local_model, resolve_model_alias,
};
use super::*;

const FASTEMBED_BATCH_SIZE: usize = 256;
const HF_HOME_ENV: &str = "HF_HOME";

pub(super) trait LocalEmbeddingBackend: Send {
    fn embed(&mut self, input: &[String]) -> Result<Vec<Vec<f32>>, String>;
}

struct FastembedLocalEmbeddingBackend {
    inner: TextEmbedding,
}

impl LocalEmbeddingBackend for FastembedLocalEmbeddingBackend {
    fn embed(&mut self, input: &[String]) -> Result<Vec<Vec<f32>>, String> {
        self.inner
            .embed(input, None)
            .map_err(|error| error.to_string())
    }
}

/// fastembed local embeddings client implementing the shared [`EmbeddingProvider`] contract.
pub struct LocalEmbeddingProvider {
    model: String,
    output_dimensions: usize,
    backend: Mutex<Box<dyn LocalEmbeddingBackend>>,
}

impl LocalEmbeddingProvider {
    pub fn new(model: impl Into<String>) -> EmbeddingResult<Self> {
        let model = normalize_local_model(model.into());
        let alias = resolve_model_alias(&model).map_err(local_model_error_to_embedding_error)?;
        let artifact =
            require_prepared_local_model(&model).map_err(local_model_error_to_embedding_error)?;
        reject_hf_home_override_for_provider(&artifact, hf_home_from_process_env())?;
        let options = TextInitOptions::new(alias.fastembed_model.clone())
            .with_cache_dir(artifact.cache_root)
            .with_show_download_progress(false);
        let backend = TextEmbedding::try_new(options)
            .map(|inner| Box::new(FastembedLocalEmbeddingBackend { inner }) as _)
            .map_err(|error| {
                EmbeddingError::Provider(ProviderFailure::non_retryable(
                    EmbeddingProviderKind::Local,
                    format!(
                        "failed to load prepared local semantic model '{}': {error}",
                        model
                    ),
                    Some("local_model_load_failed".to_owned()),
                    None,
                    None,
                ))
            })?;

        Ok(Self {
            model,
            output_dimensions: alias.dimensions,
            backend: Mutex::new(backend),
        })
    }

    #[cfg(test)]
    pub(super) fn with_backend_for_test(
        model: impl Into<String>,
        output_dimensions: usize,
        backend: Box<dyn LocalEmbeddingBackend>,
    ) -> Self {
        Self {
            model: normalize_local_model(model.into()),
            output_dimensions,
            backend: Mutex::new(backend),
        }
    }

    fn validate_local_request(&self, request: &EmbeddingRequest) -> EmbeddingResult<()> {
        request.validate()?;

        let request_model = normalize_local_model(request.model.clone());
        if request_model != self.model {
            return Err(EmbeddingError::Validation(ValidationFailure::new(
                "model",
                format!(
                    "local embedding provider was initialized for model '{}' but request selected '{}'",
                    self.model, request_model
                ),
            )));
        }

        if let Some(dimensions) = request.dimensions {
            if dimensions < self.output_dimensions {
                return Err(EmbeddingError::Validation(ValidationFailure::new(
                    "dimensions",
                    format!(
                        "local embedding model '{}' outputs {} dimensions; requested dimensions must be at least {}",
                        self.model, self.output_dimensions, self.output_dimensions
                    ),
                )));
            }
            if dimensions > DEFAULT_VECTOR_DIMENSIONS {
                return Err(EmbeddingError::Validation(ValidationFailure::new(
                    "dimensions",
                    format!(
                        "local embedding request dimensions must not exceed {DEFAULT_VECTOR_DIMENSIONS}"
                    ),
                )));
            }
        }

        Ok(())
    }

    fn map_backend_vectors(
        &self,
        embeddings: Vec<Vec<f32>>,
        request: &EmbeddingRequest,
    ) -> EmbeddingResult<Vec<EmbeddingVector>> {
        if embeddings.len() != request.input.len() {
            return Err(local_invalid_response(
                format!(
                    "local embedding response length mismatch: expected {} vectors, received {}",
                    request.input.len(),
                    embeddings.len()
                ),
                request.trace_id.clone(),
            ));
        }

        embeddings
            .into_iter()
            .enumerate()
            .map(|(index, values)| {
                if values.is_empty() {
                    return Err(local_invalid_response(
                        format!("local embedding response vector {index} was empty"),
                        request.trace_id.clone(),
                    ));
                }
                if values.len() > DEFAULT_VECTOR_DIMENSIONS {
                    return Err(local_invalid_response(
                        format!(
                            "local embedding response vector {index} has {} dimensions, exceeding {DEFAULT_VECTOR_DIMENSIONS}",
                            values.len()
                        ),
                        request.trace_id.clone(),
                    ));
                }
                if values.iter().any(|value| !value.is_finite()) {
                    return Err(local_invalid_response(
                        format!("local embedding response vector {index} contained non-finite values"),
                        request.trace_id.clone(),
                    ));
                }

                Ok(EmbeddingVector { index, values })
            })
            .collect()
    }
}

pub(super) fn reject_hf_home_override_for_provider(
    artifact: &LocalModelArtifact,
    hf_home: Option<PathBuf>,
) -> EmbeddingResult<()> {
    let Some(hf_home) = hf_home else {
        return Ok(());
    };

    Err(EmbeddingError::Provider(ProviderFailure::non_retryable(
        EmbeddingProviderKind::Local,
        format!(
            "{HF_HOME_ENV} is set to {}; unset it so {FRIGG_SEMANTIC_MODEL_CACHE_ENV} or Frigg's platform cache root controls prepared local model loading from {}",
            hf_home.display(),
            artifact.cache_root.display()
        ),
        Some("local_model_cache_override".to_owned()),
        None,
        None,
    )))
}

#[async_trait]
impl EmbeddingProvider for LocalEmbeddingProvider {
    fn kind(&self) -> EmbeddingProviderKind {
        EmbeddingProviderKind::Local
    }

    fn limits(&self) -> EmbeddingProviderLimits {
        EmbeddingProviderLimits {
            max_inputs_per_request: Some(FASTEMBED_BATCH_SIZE),
            max_input_chars: None,
            max_dimensions: Some(DEFAULT_VECTOR_DIMENSIONS),
        }
    }

    async fn embed(&self, request: EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse> {
        self.validate_local_request(&request)?;

        let embeddings = self
            .backend
            .lock()
            .map_err(|_| {
                EmbeddingError::Provider(ProviderFailure::non_retryable(
                    self.kind(),
                    "local embedding backend lock was poisoned",
                    Some("local_backend_unavailable".to_owned()),
                    None,
                    request.trace_id.clone(),
                ))
            })?
            .embed(&request.input)
            .map_err(|error| {
                EmbeddingError::Provider(ProviderFailure::non_retryable(
                    self.kind(),
                    format!("local embedding provider call failed: {error}"),
                    Some("local_embedding_failed".to_owned()),
                    None,
                    request.trace_id.clone(),
                ))
            })?;
        let vectors = self.map_backend_vectors(embeddings, &request)?;

        Ok(EmbeddingResponse {
            provider: self.kind(),
            model: self.model.clone(),
            vectors,
            trace_id: request.trace_id,
            usage: None,
        })
    }
}

pub(super) fn local_model_error_to_embedding_error(error: LocalModelError) -> EmbeddingError {
    match error {
        LocalModelError::Unsupported { model, supported } => {
            EmbeddingError::Validation(ValidationFailure::new(
                "model",
                format!(
                    "local semantic model '{model}' is not supported; supported v1 model: {supported}"
                ),
            ))
        }
        LocalModelError::Missing { model, cache_root } => {
            EmbeddingError::Provider(ProviderFailure::non_retryable(
                EmbeddingProviderKind::Local,
                format!(
                    "local semantic model artifact is missing for model '{model}' in {}; automatic local preparation should run before local embedding provider construction",
                    cache_root.display()
                ),
                Some("local_model_missing".to_owned()),
                None,
                None,
            ))
        }
        LocalModelError::DownloadRequired { model, cache_root } => {
            EmbeddingError::Provider(ProviderFailure::non_retryable(
                EmbeddingProviderKind::Local,
                format!(
                    "local semantic model artifact for model '{model}' requires download into {}; automatic local preparation should run before local embedding provider construction",
                    cache_root.display()
                ),
                Some("local_model_download_required".to_owned()),
                None,
                None,
            ))
        }
        LocalModelError::Corrupt {
            model,
            cache_root,
            message,
        } => EmbeddingError::Provider(ProviderFailure::non_retryable(
            EmbeddingProviderKind::Local,
            format!(
                "local semantic model artifact for model '{model}' is corrupt in {}: {message}",
                cache_root.display()
            ),
            Some("local_model_corrupt".to_owned()),
            None,
            None,
        )),
        LocalModelError::UnsupportedCacheRoot { message } => {
            EmbeddingError::Provider(ProviderFailure::non_retryable(
                EmbeddingProviderKind::Local,
                format!("local semantic model cache root could not be resolved: {message}"),
                Some("local_model_cache_unsupported".to_owned()),
                None,
                None,
            ))
        }
        LocalModelError::PreparationFailed {
            model,
            cache_root,
            message,
        } => EmbeddingError::Provider(ProviderFailure::non_retryable(
            EmbeddingProviderKind::Local,
            format!(
                "failed to prepare local semantic model '{model}' in {}: {message}",
                cache_root.display()
            ),
            Some("local_model_preparation_failed".to_owned()),
            None,
            None,
        )),
    }
}

fn local_invalid_response(message: String, trace_id: Option<String>) -> EmbeddingError {
    EmbeddingError::Provider(ProviderFailure::non_retryable(
        EmbeddingProviderKind::Local,
        message,
        Some("invalid_response".to_owned()),
        None,
        trace_id,
    ))
}

fn normalize_local_model(model: String) -> String {
    let trimmed = model.trim();
    if trimmed.is_empty() {
        DEFAULT_LOCAL_EMBEDDING_MODEL.to_owned()
    } else {
        resolve_model_alias(trimmed)
            .map(|alias| alias.semantic_model.to_owned())
            .unwrap_or_else(|_| trimmed.to_owned())
    }
}

fn hf_home_from_process_env() -> Option<PathBuf> {
    std::env::var_os(HF_HOME_ENV)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}
