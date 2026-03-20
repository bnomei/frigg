//! Embedding provider abstractions and readiness checks used by both indexing and runtime startup.
//! Keeping semantic transport concerns here lets the rest of the crate treat embeddings as a
//! capability boundary instead of vendor-specific HTTP code.

use crate::storage::{DEFAULT_VECTOR_DIMENSIONS, Storage};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::Duration;
use thiserror::Error;

#[allow(unused_imports)]
use sqlite_vec as _;

/// Result type shared by semantic indexing and query-time embedding calls.
pub type EmbeddingResult<T> = Result<T, EmbeddingError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingProviderKind {
    OpenAi,
    Google,
    VectorStore,
}

impl EmbeddingProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Google => "google",
            Self::VectorStore => "vector_store",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingPurpose {
    #[default]
    Document,
    Query,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmbeddingProviderLimits {
    pub max_inputs_per_request: Option<usize>,
    pub max_input_chars: Option<usize>,
    pub max_dimensions: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: usize,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(2),
        }
    }
}

impl RetryPolicy {
    fn backoff_for_retry(&self, retry_index: usize) -> Duration {
        let factor = 2_u32.pow(retry_index.min(16) as u32);
        self.initial_backoff
            .checked_mul(factor)
            .map_or(self.max_backoff, |delay| delay.min(self.max_backoff))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiEmbeddingProviderConfig {
    pub endpoint: String,
    pub timeout: Duration,
    pub retry_policy: RetryPolicy,
}

impl Default for OpenAiEmbeddingProviderConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://api.openai.com/v1/embeddings".to_string(),
            timeout: Duration::from_secs(30),
            retry_policy: RetryPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleEmbeddingProviderConfig {
    pub endpoint: String,
    pub timeout: Duration,
    pub retry_policy: RetryPolicy,
}

impl Default for GoogleEmbeddingProviderConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://generativelanguage.googleapis.com".to_string(),
            timeout: Duration::from_secs(30),
            retry_policy: RetryPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Provider-agnostic embedding request used by indexing and search code paths.
pub struct EmbeddingRequest {
    pub model: String,
    pub input: Vec<String>,
    pub purpose: EmbeddingPurpose,
    pub dimensions: Option<usize>,
    pub trace_id: Option<String>,
}

impl EmbeddingRequest {
    pub fn validate(&self) -> Result<(), ValidationFailure> {
        if self.model.trim().is_empty() {
            return Err(ValidationFailure::new("model", "model must not be empty"));
        }

        if self.input.is_empty() {
            return Err(ValidationFailure::new(
                "input",
                "input must contain at least one text segment",
            ));
        }

        if self.input.iter().any(|item| item.trim().is_empty()) {
            return Err(ValidationFailure::new(
                "input",
                "input values must not be blank",
            ));
        }

        if matches!(self.dimensions, Some(0)) {
            return Err(ValidationFailure::new(
                "dimensions",
                "dimensions must be greater than zero",
            ));
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EmbeddingVector {
    pub index: usize,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct EmbeddingUsage {
    pub prompt_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
/// Normalized embedding response so callers can consume vectors without branching on provider
/// wire formats.
pub struct EmbeddingResponse {
    pub provider: EmbeddingProviderKind,
    pub model: String,
    pub vectors: Vec<EmbeddingVector>,
    pub trace_id: Option<String>,
    pub usage: Option<EmbeddingUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Vector backend health summary used to decide whether semantic storage can participate in the
/// broader retrieval pipeline.
pub struct VectorStoreReadiness {
    pub backend: String,
    pub extension_version: String,
    pub table_name: String,
    pub expected_dimensions: usize,
}

/// Verifies that the configured SQLite/vector backend can support semantic storage before indexing
/// or search depend on it.
pub fn verify_vector_store_readiness(
    db_path: impl AsRef<Path>,
    expected_dimensions: Option<usize>,
    trace_id: Option<String>,
) -> EmbeddingResult<VectorStoreReadiness> {
    let storage = Storage::new(db_path.as_ref());
    let status = storage
        .verify_vector_store(expected_dimensions.unwrap_or(DEFAULT_VECTOR_DIMENSIONS))
        .map_err(|error| {
            EmbeddingError::Transport(TransportFailure::non_retryable(
                EmbeddingProviderKind::VectorStore,
                "vector_store_verify",
                error.to_string(),
                trace_id,
            ))
        })?;

    Ok(VectorStoreReadiness {
        backend: status.backend.as_str().to_string(),
        extension_version: status.extension_version,
        table_name: status.table_name,
        expected_dimensions: status.expected_dimensions,
    })
}

pub fn verify_sqlite_vec_readiness(
    db_path: impl AsRef<Path>,
    expected_dimensions: Option<usize>,
    trace_id: Option<String>,
) -> EmbeddingResult<VectorStoreReadiness> {
    let readiness = verify_vector_store_readiness(db_path, expected_dimensions, trace_id.clone())?;

    if readiness.backend != "sqlite_vec" {
        return Err(EmbeddingError::Transport(TransportFailure::non_retryable(
            EmbeddingProviderKind::VectorStore,
            "vector_store_verify_sqlite_vec",
            format!(
                "vector subsystem not ready: sqlite-vec backend unavailable (active backend: {})",
                readiness.backend
            ),
            trace_id,
        )));
    }

    Ok(readiness)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Retryability {
    Retryable,
    NonRetryable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingErrorCategory {
    Validation,
    Provider,
    Transport,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationFailure {
    pub field: String,
    pub message: String,
}

impl ValidationFailure {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ValidationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "validation failure on '{}': {}",
            self.field, self.message
        )
    }
}

impl std::error::Error for ValidationFailure {}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderFailure {
    pub provider: EmbeddingProviderKind,
    pub message: String,
    pub code: Option<String>,
    pub status_code: Option<u16>,
    pub retryability: Retryability,
    pub trace_id: Option<String>,
}

impl ProviderFailure {
    pub fn retryable(
        provider: EmbeddingProviderKind,
        message: impl Into<String>,
        code: Option<String>,
        status_code: Option<u16>,
        trace_id: Option<String>,
    ) -> Self {
        Self {
            provider,
            message: message.into(),
            code,
            status_code,
            retryability: Retryability::Retryable,
            trace_id,
        }
    }

    pub fn non_retryable(
        provider: EmbeddingProviderKind,
        message: impl Into<String>,
        code: Option<String>,
        status_code: Option<u16>,
        trace_id: Option<String>,
    ) -> Self {
        Self {
            provider,
            message: message.into(),
            code,
            status_code,
            retryability: Retryability::NonRetryable,
            trace_id,
        }
    }
}

impl std::fmt::Display for ProviderFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} provider failure{}{}: {}",
            self.provider.as_str(),
            self.status_code
                .map(|status| format!(" (status {})", status))
                .unwrap_or_default(),
            self.code
                .as_ref()
                .map(|code| format!(" [code={}]", code))
                .unwrap_or_default(),
            self.message,
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportFailure {
    pub provider: EmbeddingProviderKind,
    pub operation: String,
    pub message: String,
    pub retryability: Retryability,
    pub trace_id: Option<String>,
}

impl TransportFailure {
    pub fn retryable(
        provider: EmbeddingProviderKind,
        operation: impl Into<String>,
        message: impl Into<String>,
        trace_id: Option<String>,
    ) -> Self {
        Self {
            provider,
            operation: operation.into(),
            message: message.into(),
            retryability: Retryability::Retryable,
            trace_id,
        }
    }

    pub fn non_retryable(
        provider: EmbeddingProviderKind,
        operation: impl Into<String>,
        message: impl Into<String>,
        trace_id: Option<String>,
    ) -> Self {
        Self {
            provider,
            operation: operation.into(),
            message: message.into(),
            retryability: Retryability::NonRetryable,
            trace_id,
        }
    }
}

impl std::fmt::Display for TransportFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} transport failure during {}: {}",
            self.provider.as_str(),
            self.operation,
            self.message,
        )
    }
}

#[derive(Debug, Error, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EmbeddingError {
    #[error("{0}")]
    Validation(#[from] ValidationFailure),

    #[error("{0}")]
    Provider(ProviderFailure),

    #[error("{0}")]
    Transport(TransportFailure),
}

impl EmbeddingError {
    pub fn category(&self) -> EmbeddingErrorCategory {
        match self {
            Self::Validation(_) => EmbeddingErrorCategory::Validation,
            Self::Provider(_) => EmbeddingErrorCategory::Provider,
            Self::Transport(_) => EmbeddingErrorCategory::Transport,
        }
    }

    pub fn retryability(&self) -> Retryability {
        match self {
            Self::Validation(_) => Retryability::NonRetryable,
            Self::Provider(failure) => failure.retryability,
            Self::Transport(failure) => failure.retryability,
        }
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self.retryability(), Retryability::Retryable)
    }

    pub fn trace_id(&self) -> Option<&str> {
        match self {
            Self::Validation(_) => None,
            Self::Provider(failure) => failure.trace_id.as_deref(),
            Self::Transport(failure) => failure.trace_id.as_deref(),
        }
    }
}

mod transport;
use transport::*;

mod google;
mod openai;
pub use google::GoogleEmbeddingProvider;
pub use openai::OpenAiEmbeddingProvider;

#[cfg(test)]
mod tests;

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn kind(&self) -> EmbeddingProviderKind;

    fn limits(&self) -> EmbeddingProviderLimits {
        EmbeddingProviderLimits::default()
    }

    async fn embed(&self, request: EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse>;
}
