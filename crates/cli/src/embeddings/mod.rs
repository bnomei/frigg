use crate::storage::{DEFAULT_VECTOR_DIMENSIONS, Storage};
use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

#[allow(unused_imports)]
use sqlite_vec as _;

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
pub struct EmbeddingResponse {
    pub provider: EmbeddingProviderKind,
    pub model: String,
    pub vectors: Vec<EmbeddingVector>,
    pub trace_id: Option<String>,
    pub usage: Option<EmbeddingUsage>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VectorStoreReadiness {
    pub backend: String,
    pub extension_version: String,
    pub table_name: String,
    pub expected_dimensions: usize,
}

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

#[derive(Debug, Clone)]
struct HttpRequest {
    method: Method,
    url: String,
    headers: Vec<(String, String)>,
    body: serde_json::Value,
    timeout: Duration,
    diagnostics: HttpRequestDiagnostics,
}

#[derive(Debug, Clone)]
struct HttpResponse {
    status_code: u16,
    body: String,
}

#[derive(Debug, Clone)]
struct HttpTransportError {
    message: String,
    retryability: Retryability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct HttpRequestDiagnostics {
    provider: EmbeddingProviderKind,
    model: String,
    input_count: usize,
    input_chars_total: usize,
    max_input_chars: usize,
    body_bytes: usize,
    body_blake3: String,
    trace_id: Option<String>,
}

impl HttpRequestDiagnostics {
    fn from_request(
        provider: EmbeddingProviderKind,
        request: &EmbeddingRequest,
        body: &serde_json::Value,
    ) -> EmbeddingResult<Self> {
        let body_bytes = serde_json::to_vec(body).map_err(|error| {
            EmbeddingError::Provider(ProviderFailure::non_retryable(
                provider,
                format!("failed to serialize request diagnostics payload: {error}"),
                Some("request_serialization_failed".to_string()),
                None,
                request.trace_id.clone(),
            ))
        })?;
        let input_chars_total = request.input.iter().map(|item| item.chars().count()).sum();
        let max_input_chars = request
            .input
            .iter()
            .map(|item| item.chars().count())
            .max()
            .unwrap_or(0);

        Ok(Self {
            provider,
            model: request.model.clone(),
            input_count: request.input.len(),
            input_chars_total,
            max_input_chars,
            body_bytes: body_bytes.len(),
            body_blake3: blake3::hash(&body_bytes).to_hex().to_string(),
            trace_id: request.trace_id.clone(),
        })
    }
}

impl std::fmt::Display for HttpRequestDiagnostics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "request_context{{model={}, inputs={}, input_chars_total={}, max_input_chars={}, body_bytes={}, body_blake3={}",
            self.model,
            self.input_count,
            self.input_chars_total,
            self.max_input_chars,
            self.body_bytes,
            self.body_blake3,
        )?;
        if let Some(trace_id) = &self.trace_id {
            write!(f, ", trace_id={trace_id}")?;
        }
        write!(f, "}}")
    }
}

impl HttpTransportError {
    fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryability: Retryability::Retryable,
        }
    }

    fn non_retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryability: Retryability::NonRetryable,
        }
    }
}

fn append_request_diagnostics(
    message: impl AsRef<str>,
    diagnostics: &HttpRequestDiagnostics,
) -> String {
    format!("{} {}", message.as_ref(), diagnostics)
}

#[async_trait]
trait HttpExecutor: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpTransportError>;
}

#[derive(Clone)]
struct ReqwestHttpExecutor {
    client: Client,
}

impl ReqwestHttpExecutor {
    fn new(client: Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl HttpExecutor for ReqwestHttpExecutor {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpTransportError> {
        let HttpRequest {
            method,
            url,
            headers,
            body,
            timeout,
            ..
        } = request;
        let mut builder = self
            .client
            .request(method, url)
            .timeout(timeout)
            .json(&body);

        for (name, value) in headers {
            builder = builder.header(name, value);
        }

        let response = builder.send().await.map_err(map_reqwest_error)?;
        let status_code = response.status().as_u16();
        let body = response.text().await.map_err(map_reqwest_error)?;

        Ok(HttpResponse { status_code, body })
    }
}

#[async_trait]
trait BackoffSleeper: Send + Sync {
    async fn sleep(&self, duration: Duration);
}

#[derive(Clone, Default)]
struct TokioSleeper;

#[async_trait]
impl BackoffSleeper for TokioSleeper {
    async fn sleep(&self, duration: Duration) {
        tokio::time::sleep(duration).await;
    }
}

fn map_reqwest_error(error: reqwest::Error) -> HttpTransportError {
    let retryable = error.is_timeout() || error.is_connect() || error.is_body();
    if retryable {
        HttpTransportError::retryable(error.to_string())
    } else {
        HttpTransportError::non_retryable(error.to_string())
    }
}

fn status_retryability(status_code: u16) -> Retryability {
    if matches!(status_code, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504) {
        Retryability::Retryable
    } else {
        Retryability::NonRetryable
    }
}

fn usage_from_counts(
    prompt_tokens: Option<u64>,
    total_tokens: Option<u64>,
) -> Option<EmbeddingUsage> {
    if prompt_tokens.is_some() || total_tokens.is_some() {
        Some(EmbeddingUsage {
            prompt_tokens,
            total_tokens,
        })
    } else {
        None
    }
}

fn google_task_type(purpose: EmbeddingPurpose) -> &'static str {
    match purpose {
        EmbeddingPurpose::Document => "RETRIEVAL_DOCUMENT",
        EmbeddingPurpose::Query => "RETRIEVAL_QUERY",
    }
}

fn google_model_path(model: &str) -> String {
    if model.starts_with("models/") {
        model.to_string()
    } else {
        format!("models/{model}")
    }
}

#[derive(Serialize)]
struct OpenAiEmbeddingRequestPayload<'a> {
    model: &'a str,
    input: &'a [String],
    #[serde(skip_serializing_if = "Option::is_none")]
    dimensions: Option<usize>,
    encoding_format: &'static str,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponsePayload {
    #[serde(default)]
    data: Vec<OpenAiEmbeddingVectorPayload>,
    model: Option<String>,
    usage: Option<OpenAiUsagePayload>,
}

#[derive(Deserialize)]
struct OpenAiEmbeddingVectorPayload {
    index: usize,
    embedding: Vec<f32>,
}

#[derive(Deserialize)]
struct OpenAiUsagePayload {
    prompt_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct OpenAiErrorEnvelope {
    error: OpenAiErrorPayload,
}

#[derive(Deserialize)]
struct OpenAiErrorPayload {
    message: String,
    code: Option<String>,
    #[serde(rename = "type")]
    error_type: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleBatchEmbeddingRequestPayload {
    requests: Vec<GoogleBatchEmbeddingRequestItemPayload>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleBatchEmbeddingRequestItemPayload {
    model: String,
    content: GoogleContentPayload,
    task_type: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_dimensionality: Option<usize>,
}

#[derive(Serialize)]
struct GoogleContentPayload {
    parts: Vec<GooglePartPayload>,
}

#[derive(Serialize)]
struct GooglePartPayload {
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleBatchEmbeddingResponsePayload {
    #[serde(default)]
    embeddings: Vec<GoogleEmbeddingPayload>,
    #[serde(rename = "usageMetadata")]
    usage_metadata: Option<GoogleUsagePayload>,
}

#[derive(Deserialize)]
struct GoogleEmbeddingPayload {
    values: Option<Vec<f32>>,
    embedding: Option<GoogleEmbeddingValuesPayload>,
}

#[derive(Deserialize)]
struct GoogleEmbeddingValuesPayload {
    values: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GoogleUsagePayload {
    prompt_token_count: Option<u64>,
    total_token_count: Option<u64>,
}

#[derive(Deserialize)]
struct GoogleErrorEnvelope {
    error: GoogleErrorPayload,
}

#[derive(Deserialize)]
struct GoogleErrorPayload {
    code: Option<u16>,
    message: Option<String>,
    status: Option<String>,
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn kind(&self) -> EmbeddingProviderKind;

    fn limits(&self) -> EmbeddingProviderLimits {
        EmbeddingProviderLimits::default()
    }

    async fn embed(&self, request: EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse>;
}

#[derive(Clone)]
pub struct OpenAiEmbeddingProvider {
    http: Arc<dyn HttpExecutor>,
    sleeper: Arc<dyn BackoffSleeper>,
    api_key: String,
    config: OpenAiEmbeddingProviderConfig,
}

impl OpenAiEmbeddingProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_config(api_key, OpenAiEmbeddingProviderConfig::default())
    }

    pub fn with_config(api_key: impl Into<String>, config: OpenAiEmbeddingProviderConfig) -> Self {
        Self::with_runtime(
            api_key.into(),
            config,
            Arc::new(ReqwestHttpExecutor::new(Client::new())),
            Arc::new(TokioSleeper),
        )
    }

    fn with_runtime(
        api_key: String,
        config: OpenAiEmbeddingProviderConfig,
        http: Arc<dyn HttpExecutor>,
        sleeper: Arc<dyn BackoffSleeper>,
    ) -> Self {
        Self {
            http,
            sleeper,
            api_key,
            config,
        }
    }

    fn build_http_request(&self, request: &EmbeddingRequest) -> EmbeddingResult<HttpRequest> {
        let payload = OpenAiEmbeddingRequestPayload {
            model: &request.model,
            input: &request.input,
            dimensions: request.dimensions,
            encoding_format: "float",
        };

        let body = serde_json::to_value(payload).map_err(|error| {
            EmbeddingError::Provider(ProviderFailure::non_retryable(
                self.kind(),
                format!("failed to serialize OpenAI request payload: {error}"),
                Some("request_serialization_failed".to_string()),
                None,
                request.trace_id.clone(),
            ))
        })?;
        let diagnostics = HttpRequestDiagnostics::from_request(self.kind(), request, &body)?;

        Ok(HttpRequest {
            method: Method::POST,
            url: self.config.endpoint.clone(),
            headers: vec![(
                "Authorization".to_string(),
                format!("Bearer {}", self.api_key),
            )],
            body,
            timeout: self.config.timeout,
            diagnostics,
        })
    }

    fn map_transport_error(
        &self,
        operation: &str,
        trace_id: Option<String>,
        error: HttpTransportError,
        diagnostics: &HttpRequestDiagnostics,
    ) -> EmbeddingError {
        let message = append_request_diagnostics(error.message, diagnostics);
        let failure = match error.retryability {
            Retryability::Retryable => {
                TransportFailure::retryable(self.kind(), operation, message, trace_id)
            }
            Retryability::NonRetryable => {
                TransportFailure::non_retryable(self.kind(), operation, message, trace_id)
            }
        };

        EmbeddingError::Transport(failure)
    }

    fn map_provider_http_error(
        &self,
        status_code: u16,
        body: &str,
        trace_id: Option<String>,
        diagnostics: &HttpRequestDiagnostics,
    ) -> EmbeddingError {
        let mut message = format!("OpenAI request failed with status {status_code}");
        let mut code = None;

        if let Ok(envelope) = serde_json::from_str::<OpenAiErrorEnvelope>(body) {
            message = envelope.error.message;
            code = envelope.error.code.or(envelope.error.error_type);
        }
        let message = append_request_diagnostics(message, diagnostics);

        let retryability = status_retryability(status_code);
        let failure = match retryability {
            Retryability::Retryable => {
                ProviderFailure::retryable(self.kind(), message, code, Some(status_code), trace_id)
            }
            Retryability::NonRetryable => ProviderFailure::non_retryable(
                self.kind(),
                message,
                code,
                Some(status_code),
                trace_id,
            ),
        };

        EmbeddingError::Provider(failure)
    }

    fn parse_success_response(
        &self,
        body: &str,
        request: &EmbeddingRequest,
    ) -> EmbeddingResult<EmbeddingResponse> {
        let parsed =
            serde_json::from_str::<OpenAiEmbeddingResponsePayload>(body).map_err(|error| {
                EmbeddingError::Provider(ProviderFailure::non_retryable(
                    self.kind(),
                    format!("failed to parse OpenAI success response: {error}"),
                    Some("invalid_response".to_string()),
                    Some(200),
                    request.trace_id.clone(),
                ))
            })?;

        if parsed.data.is_empty() {
            return Err(EmbeddingError::Provider(ProviderFailure::non_retryable(
                self.kind(),
                "OpenAI response did not contain embeddings",
                Some("invalid_response".to_string()),
                Some(200),
                request.trace_id.clone(),
            )));
        }

        let vectors = parsed
            .data
            .into_iter()
            .map(|item| EmbeddingVector {
                index: item.index,
                values: item.embedding,
            })
            .collect();

        let usage = parsed
            .usage
            .and_then(|usage| usage_from_counts(usage.prompt_tokens, usage.total_tokens));

        Ok(EmbeddingResponse {
            provider: self.kind(),
            model: parsed.model.unwrap_or_else(|| request.model.clone()),
            vectors,
            trace_id: request.trace_id.clone(),
            usage,
        })
    }

    async fn embed_once(&self, request: &EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse> {
        let http_request = self.build_http_request(request)?;
        let diagnostics = http_request.diagnostics.clone();
        let http_response = self.http.execute(http_request).await.map_err(|error| {
            self.map_transport_error(
                "openai_embed",
                request.trace_id.clone(),
                error,
                &diagnostics,
            )
        })?;

        if (200..=299).contains(&http_response.status_code) {
            self.parse_success_response(&http_response.body, request)
        } else {
            Err(self.map_provider_http_error(
                http_response.status_code,
                &http_response.body,
                request.trace_id.clone(),
                &diagnostics,
            ))
        }
    }

    async fn embed_with_retry(
        &self,
        request: &EmbeddingRequest,
    ) -> EmbeddingResult<EmbeddingResponse> {
        let mut retries = 0usize;

        loop {
            match self.embed_once(request).await {
                Ok(response) => return Ok(response),
                Err(error)
                    if error.is_retryable() && retries < self.config.retry_policy.max_retries =>
                {
                    let backoff = self.config.retry_policy.backoff_for_retry(retries);
                    retries += 1;
                    self.sleeper.sleep(backoff).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAiEmbeddingProvider {
    fn kind(&self) -> EmbeddingProviderKind {
        EmbeddingProviderKind::OpenAi
    }

    async fn embed(&self, request: EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse> {
        request.validate()?;

        if self.api_key.trim().is_empty() {
            return Err(EmbeddingError::Validation(ValidationFailure::new(
                "api_key",
                "api_key must not be empty",
            )));
        }

        self.embed_with_retry(&request).await
    }
}

#[derive(Clone)]
pub struct GoogleEmbeddingProvider {
    http: Arc<dyn HttpExecutor>,
    sleeper: Arc<dyn BackoffSleeper>,
    api_key: String,
    config: GoogleEmbeddingProviderConfig,
}

impl GoogleEmbeddingProvider {
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::with_config(api_key, GoogleEmbeddingProviderConfig::default())
    }

    pub fn with_config(api_key: impl Into<String>, config: GoogleEmbeddingProviderConfig) -> Self {
        Self::with_runtime(
            api_key.into(),
            config,
            Arc::new(ReqwestHttpExecutor::new(Client::new())),
            Arc::new(TokioSleeper),
        )
    }

    fn with_runtime(
        api_key: String,
        config: GoogleEmbeddingProviderConfig,
        http: Arc<dyn HttpExecutor>,
        sleeper: Arc<dyn BackoffSleeper>,
    ) -> Self {
        Self {
            http,
            sleeper,
            api_key,
            config,
        }
    }

    fn build_http_request(&self, request: &EmbeddingRequest) -> EmbeddingResult<HttpRequest> {
        let model_path = google_model_path(&request.model);
        let task_type = google_task_type(request.purpose);

        let requests = request
            .input
            .iter()
            .map(|text| GoogleBatchEmbeddingRequestItemPayload {
                model: model_path.clone(),
                content: GoogleContentPayload {
                    parts: vec![GooglePartPayload { text: text.clone() }],
                },
                task_type,
                output_dimensionality: request.dimensions,
            })
            .collect();

        let payload = GoogleBatchEmbeddingRequestPayload { requests };
        let body = serde_json::to_value(payload).map_err(|error| {
            EmbeddingError::Provider(ProviderFailure::non_retryable(
                self.kind(),
                format!("failed to serialize Google request payload: {error}"),
                Some("request_serialization_failed".to_string()),
                None,
                request.trace_id.clone(),
            ))
        })?;
        let diagnostics = HttpRequestDiagnostics::from_request(self.kind(), request, &body)?;

        let endpoint = self.config.endpoint.trim_end_matches('/');
        let url = format!(
            "{endpoint}/v1beta/{model_path}:batchEmbedContents?key={}",
            self.api_key
        );

        Ok(HttpRequest {
            method: Method::POST,
            url,
            headers: Vec::new(),
            body,
            timeout: self.config.timeout,
            diagnostics,
        })
    }

    fn map_transport_error(
        &self,
        operation: &str,
        trace_id: Option<String>,
        error: HttpTransportError,
        diagnostics: &HttpRequestDiagnostics,
    ) -> EmbeddingError {
        let message = append_request_diagnostics(error.message, diagnostics);
        let failure = match error.retryability {
            Retryability::Retryable => {
                TransportFailure::retryable(self.kind(), operation, message, trace_id)
            }
            Retryability::NonRetryable => {
                TransportFailure::non_retryable(self.kind(), operation, message, trace_id)
            }
        };

        EmbeddingError::Transport(failure)
    }

    fn map_provider_http_error(
        &self,
        status_code: u16,
        body: &str,
        trace_id: Option<String>,
        diagnostics: &HttpRequestDiagnostics,
    ) -> EmbeddingError {
        let mut message = format!("Google request failed with status {status_code}");
        let mut code = None;
        let mut retryability = status_retryability(status_code);

        if let Ok(envelope) = serde_json::from_str::<GoogleErrorEnvelope>(body) {
            if let Some(error_message) = envelope.error.message {
                message = error_message;
            }

            if let Some(error_status) = envelope.error.status {
                if matches!(
                    error_status.as_str(),
                    "RESOURCE_EXHAUSTED" | "UNAVAILABLE" | "DEADLINE_EXCEEDED" | "ABORTED"
                ) {
                    retryability = Retryability::Retryable;
                }
                code = Some(error_status);
            }

            if let Some(provider_status_code) = envelope.error.code {
                retryability = status_retryability(provider_status_code);
            }
        }
        let message = append_request_diagnostics(message, diagnostics);

        let failure = match retryability {
            Retryability::Retryable => {
                ProviderFailure::retryable(self.kind(), message, code, Some(status_code), trace_id)
            }
            Retryability::NonRetryable => ProviderFailure::non_retryable(
                self.kind(),
                message,
                code,
                Some(status_code),
                trace_id,
            ),
        };

        EmbeddingError::Provider(failure)
    }

    fn parse_success_response(
        &self,
        body: &str,
        request: &EmbeddingRequest,
    ) -> EmbeddingResult<EmbeddingResponse> {
        let parsed =
            serde_json::from_str::<GoogleBatchEmbeddingResponsePayload>(body).map_err(|error| {
                EmbeddingError::Provider(ProviderFailure::non_retryable(
                    self.kind(),
                    format!("failed to parse Google success response: {error}"),
                    Some("invalid_response".to_string()),
                    Some(200),
                    request.trace_id.clone(),
                ))
            })?;

        if parsed.embeddings.is_empty() {
            return Err(EmbeddingError::Provider(ProviderFailure::non_retryable(
                self.kind(),
                "Google response did not contain embeddings",
                Some("invalid_response".to_string()),
                Some(200),
                request.trace_id.clone(),
            )));
        }

        let mut vectors = Vec::with_capacity(parsed.embeddings.len());
        for (index, embedding) in parsed.embeddings.into_iter().enumerate() {
            let values = embedding
                .values
                .or_else(|| embedding.embedding.map(|nested| nested.values))
                .ok_or_else(|| {
                    EmbeddingError::Provider(ProviderFailure::non_retryable(
                        self.kind(),
                        "Google response contained an embedding without vector values",
                        Some("invalid_response".to_string()),
                        Some(200),
                        request.trace_id.clone(),
                    ))
                })?;

            vectors.push(EmbeddingVector { index, values });
        }

        let usage = parsed
            .usage_metadata
            .and_then(|usage| usage_from_counts(usage.prompt_token_count, usage.total_token_count));

        Ok(EmbeddingResponse {
            provider: self.kind(),
            model: request.model.clone(),
            vectors,
            trace_id: request.trace_id.clone(),
            usage,
        })
    }

    async fn embed_once(&self, request: &EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse> {
        let http_request = self.build_http_request(request)?;
        let diagnostics = http_request.diagnostics.clone();
        let http_response = self.http.execute(http_request).await.map_err(|error| {
            self.map_transport_error(
                "google_batch_embed_contents",
                request.trace_id.clone(),
                error,
                &diagnostics,
            )
        })?;

        if (200..=299).contains(&http_response.status_code) {
            self.parse_success_response(&http_response.body, request)
        } else {
            Err(self.map_provider_http_error(
                http_response.status_code,
                &http_response.body,
                request.trace_id.clone(),
                &diagnostics,
            ))
        }
    }

    async fn embed_with_retry(
        &self,
        request: &EmbeddingRequest,
    ) -> EmbeddingResult<EmbeddingResponse> {
        let mut retries = 0usize;

        loop {
            match self.embed_once(request).await {
                Ok(response) => return Ok(response),
                Err(error)
                    if error.is_retryable() && retries < self.config.retry_policy.max_retries =>
                {
                    let backoff = self.config.retry_policy.backoff_for_retry(retries);
                    retries += 1;
                    self.sleeper.sleep(backoff).await;
                }
                Err(error) => return Err(error),
            }
        }
    }
}

#[async_trait]
impl EmbeddingProvider for GoogleEmbeddingProvider {
    fn kind(&self) -> EmbeddingProviderKind {
        EmbeddingProviderKind::Google
    }

    async fn embed(&self, request: EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse> {
        request.validate()?;

        if self.api_key.trim().is_empty() {
            return Err(EmbeddingError::Validation(ValidationFailure::new(
                "api_key",
                "api_key must not be empty",
            )));
        }

        self.embed_with_retry(&request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Json, Router, body::Bytes, extract::State, http::HeaderMap, routing::post};
    use serde_json::json;
    use std::collections::VecDeque;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::{env, fs};

    struct DummyProvider;

    #[async_trait]
    impl EmbeddingProvider for DummyProvider {
        fn kind(&self) -> EmbeddingProviderKind {
            EmbeddingProviderKind::OpenAi
        }

        async fn embed(&self, request: EmbeddingRequest) -> EmbeddingResult<EmbeddingResponse> {
            request.validate()?;
            Err(EmbeddingError::Provider(ProviderFailure::non_retryable(
                self.kind(),
                "dummy provider is intentionally unimplemented",
                Some("dummy_not_implemented".to_string()),
                None,
                request.trace_id,
            )))
        }
    }

    #[derive(Default)]
    struct MockSleeper {
        delays: Mutex<Vec<Duration>>,
    }

    impl MockSleeper {
        fn delays(&self) -> Vec<Duration> {
            self.delays.lock().expect("mutex poisoned").clone()
        }
    }

    #[async_trait]
    impl BackoffSleeper for MockSleeper {
        async fn sleep(&self, duration: Duration) {
            self.delays.lock().expect("mutex poisoned").push(duration);
        }
    }

    struct MockHttpExecutor {
        outcomes: Mutex<VecDeque<Result<HttpResponse, HttpTransportError>>>,
        requests: Mutex<Vec<HttpRequest>>,
    }

    impl MockHttpExecutor {
        fn new(outcomes: Vec<Result<HttpResponse, HttpTransportError>>) -> Self {
            Self {
                outcomes: Mutex::new(VecDeque::from(outcomes)),
                requests: Mutex::new(Vec::new()),
            }
        }

        fn requests(&self) -> Vec<HttpRequest> {
            self.requests.lock().expect("mutex poisoned").clone()
        }
    }

    #[async_trait]
    impl HttpExecutor for MockHttpExecutor {
        async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpTransportError> {
            self.requests.lock().expect("mutex poisoned").push(request);
            self.outcomes
                .lock()
                .expect("mutex poisoned")
                .pop_front()
                .expect("mock outcome missing")
        }
    }

    fn openai_provider_for_test(
        http: Arc<MockHttpExecutor>,
        sleeper: Arc<MockSleeper>,
        retry_policy: RetryPolicy,
    ) -> OpenAiEmbeddingProvider {
        OpenAiEmbeddingProvider::with_runtime(
            "test-openai-key".to_string(),
            OpenAiEmbeddingProviderConfig {
                endpoint: "https://api.openai.com/v1/embeddings".to_string(),
                timeout: Duration::from_secs(5),
                retry_policy,
            },
            http,
            sleeper,
        )
    }

    fn google_provider_for_test(
        http: Arc<MockHttpExecutor>,
        sleeper: Arc<MockSleeper>,
        retry_policy: RetryPolicy,
    ) -> GoogleEmbeddingProvider {
        GoogleEmbeddingProvider::with_runtime(
            "test-google-key".to_string(),
            GoogleEmbeddingProviderConfig {
                endpoint: "https://generativelanguage.googleapis.com".to_string(),
                timeout: Duration::from_secs(5),
                retry_policy,
            },
            http,
            sleeper,
        )
    }

    fn sample_request(purpose: EmbeddingPurpose) -> EmbeddingRequest {
        EmbeddingRequest {
            model: "text-embedding-3-small".to_string(),
            input: vec!["alpha".to_string(), "beta".to_string()],
            purpose,
            dimensions: Some(3),
            trace_id: Some("trace-123".to_string()),
        }
    }

    fn temp_vector_db_path(test_name: &str) -> PathBuf {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();

        env::temp_dir().join(format!("frigg-embeddings-{test_name}-{nonce}.sqlite3"))
    }

    fn cleanup_db(path: &Path) {
        let _ = fs::remove_file(path);
    }

    #[test]
    fn provider_trait_exposes_kind_and_default_limits() {
        let provider = DummyProvider;
        let dyn_provider: &dyn EmbeddingProvider = &provider;

        assert_eq!(provider.kind(), EmbeddingProviderKind::OpenAi);
        assert_eq!(dyn_provider.kind(), EmbeddingProviderKind::OpenAi);
        assert_eq!(provider.limits(), EmbeddingProviderLimits::default());
    }

    #[test]
    fn provider_trait_helpers_expose_stable_defaults_and_strings() {
        let retry_policy = RetryPolicy::default();
        assert_eq!(EmbeddingProviderKind::OpenAi.as_str(), "openai");
        assert_eq!(EmbeddingProviderKind::Google.as_str(), "google");
        assert_eq!(EmbeddingProviderKind::VectorStore.as_str(), "vector_store");
        assert_eq!(EmbeddingPurpose::default(), EmbeddingPurpose::Document);
        assert_eq!(
            EmbeddingProviderLimits::default(),
            EmbeddingProviderLimits {
                max_inputs_per_request: None,
                max_input_chars: None,
                max_dimensions: None,
            }
        );
        assert_eq!(retry_policy.max_retries, 2);
        assert_eq!(retry_policy.initial_backoff, Duration::from_millis(200));
        assert_eq!(retry_policy.max_backoff, Duration::from_secs(2));
        assert_eq!(
            retry_policy.backoff_for_retry(0),
            Duration::from_millis(200)
        );
        assert_eq!(
            retry_policy.backoff_for_retry(1),
            Duration::from_millis(400)
        );
        assert_eq!(retry_policy.backoff_for_retry(4), Duration::from_secs(2));
        assert_eq!(retry_policy.backoff_for_retry(32), Duration::from_secs(2));

        assert_eq!(
            OpenAiEmbeddingProviderConfig::default(),
            OpenAiEmbeddingProviderConfig {
                endpoint: "https://api.openai.com/v1/embeddings".to_string(),
                timeout: Duration::from_secs(30),
                retry_policy: RetryPolicy::default(),
            }
        );
        assert_eq!(
            GoogleEmbeddingProviderConfig::default(),
            GoogleEmbeddingProviderConfig {
                endpoint: "https://generativelanguage.googleapis.com".to_string(),
                timeout: Duration::from_secs(30),
                retry_policy: RetryPolicy::default(),
            }
        );
    }

    #[test]
    fn provider_trait_request_validation_rejects_empty_model() {
        let request = EmbeddingRequest {
            model: "   ".to_string(),
            input: vec!["hello".to_string()],
            purpose: EmbeddingPurpose::Document,
            dimensions: Some(128),
            trace_id: Some("trace-1".to_string()),
        };

        let error = request.validate().expect_err("empty model should fail");
        assert_eq!(error.field, "model");
    }

    #[test]
    fn provider_trait_request_validation_rejects_blank_inputs() {
        let request = EmbeddingRequest {
            model: "text-embedding-3-small".to_string(),
            input: vec!["valid".to_string(), "   ".to_string()],
            purpose: EmbeddingPurpose::Query,
            dimensions: None,
            trace_id: Some("trace-2".to_string()),
        };

        let error = request.validate().expect_err("blank input should fail");
        assert_eq!(error.field, "input");
    }

    #[test]
    fn provider_trait_request_validation_rejects_empty_input_segments() {
        let request = EmbeddingRequest {
            model: "text-embedding-3-small".to_string(),
            input: Vec::new(),
            purpose: EmbeddingPurpose::Document,
            dimensions: Some(128),
            trace_id: Some("trace-empty-input".to_string()),
        };

        let error = request.validate().expect_err("empty input should fail");
        assert_eq!(error.field, "input");
        assert_eq!(
            error.message,
            "input must contain at least one text segment"
        );
    }

    #[test]
    fn provider_trait_request_validation_rejects_zero_dimensions() {
        let request = EmbeddingRequest {
            model: "text-embedding-3-small".to_string(),
            input: vec!["hello".to_string()],
            purpose: EmbeddingPurpose::Query,
            dimensions: Some(0),
            trace_id: Some("trace-zero-dimensions".to_string()),
        };

        let error = request.validate().expect_err("zero dimensions should fail");
        assert_eq!(error.field, "dimensions");
        assert_eq!(error.message, "dimensions must be greater than zero");
    }

    #[test]
    fn provider_trait_error_category_and_retryability_behavior() {
        let validation =
            EmbeddingError::Validation(ValidationFailure::new("model", "model must not be empty"));
        let provider = EmbeddingError::Provider(ProviderFailure::retryable(
            EmbeddingProviderKind::Google,
            "rate limited",
            Some("rate_limited".to_string()),
            Some(429),
            Some("trace-provider".to_string()),
        ));
        let transport = EmbeddingError::Transport(TransportFailure::non_retryable(
            EmbeddingProviderKind::OpenAi,
            "send_request",
            "TLS configuration invalid",
            Some("trace-transport".to_string()),
        ));

        assert_eq!(validation.category(), EmbeddingErrorCategory::Validation);
        assert_eq!(validation.retryability(), Retryability::NonRetryable);
        assert!(!validation.is_retryable());
        assert_eq!(validation.trace_id(), None);

        assert_eq!(provider.category(), EmbeddingErrorCategory::Provider);
        assert_eq!(provider.retryability(), Retryability::Retryable);
        assert!(provider.is_retryable());
        assert_eq!(provider.trace_id(), Some("trace-provider"));

        assert_eq!(transport.category(), EmbeddingErrorCategory::Transport);
        assert_eq!(transport.retryability(), Retryability::NonRetryable);
        assert!(!transport.is_retryable());
        assert_eq!(transport.trace_id(), Some("trace-transport"));
    }

    #[tokio::test]
    async fn provider_trait_display_helpers_include_expected_context() {
        let validation =
            ValidationFailure::new("dimensions", "dimensions must be greater than zero");
        let provider = ProviderFailure::non_retryable(
            EmbeddingProviderKind::Google,
            "quota exceeded",
            Some("RESOURCE_EXHAUSTED".to_string()),
            Some(429),
            Some("trace-provider-display".to_string()),
        );
        let transport = TransportFailure::retryable(
            EmbeddingProviderKind::VectorStore,
            "vector_store_verify",
            "backend unavailable",
            Some("trace-transport-display".to_string()),
        );
        let request = sample_request(EmbeddingPurpose::Document);
        let body = json!({
            "model": request.model,
            "input": request.input,
            "encoding_format": "float",
            "dimensions": request.dimensions,
        });
        let diagnostics =
            HttpRequestDiagnostics::from_request(EmbeddingProviderKind::OpenAi, &request, &body)
                .expect("diagnostics should serialize");
        let body_bytes = serde_json::to_vec(&body).expect("body should serialize");
        let body_blake3 = blake3::hash(&body_bytes).to_hex().to_string();

        assert_eq!(
            validation.to_string(),
            "validation failure on 'dimensions': dimensions must be greater than zero"
        );
        assert_eq!(
            provider.to_string(),
            "google provider failure (status 429) [code=RESOURCE_EXHAUSTED]: quota exceeded"
        );
        assert_eq!(
            transport.to_string(),
            "vector_store transport failure during vector_store_verify: backend unavailable"
        );
        assert_eq!(
            EmbeddingError::Provider(provider.clone()).to_string(),
            provider.to_string()
        );
        assert_eq!(diagnostics.provider, EmbeddingProviderKind::OpenAi);
        assert_eq!(diagnostics.body_bytes, body_bytes.len());
        assert_eq!(diagnostics.body_blake3, body_blake3);
        assert_eq!(
            diagnostics.to_string(),
            format!(
                "request_context{{model=text-embedding-3-small, inputs=2, input_chars_total=9, max_input_chars=5, body_bytes={}, body_blake3={}, trace_id=trace-123}}",
                body_bytes.len(),
                body_blake3
            )
        );
        assert_eq!(
            append_request_diagnostics("invalid payload", &diagnostics),
            format!("invalid payload {}", diagnostics)
        );
    }

    #[test]
    fn provider_trait_retryability_helpers_and_status_mapping_are_consistent() {
        let provider = ProviderFailure::retryable(
            EmbeddingProviderKind::OpenAi,
            "rate limited",
            Some("rate_limit_exceeded".to_string()),
            Some(429),
            Some("trace-retryable-provider".to_string()),
        );
        let transport = TransportFailure::non_retryable(
            EmbeddingProviderKind::Google,
            "google_batch_embed_contents",
            "request body malformed",
            Some("trace-non-retryable-transport".to_string()),
        );
        let retryable_http = HttpTransportError::retryable("timeout");
        let non_retryable_http = HttpTransportError::non_retryable("bad request");

        assert_eq!(provider.retryability, Retryability::Retryable);
        assert_eq!(transport.retryability, Retryability::NonRetryable);
        assert_eq!(retryable_http.retryability, Retryability::Retryable);
        assert_eq!(non_retryable_http.retryability, Retryability::NonRetryable);
        assert_eq!(status_retryability(408), Retryability::Retryable);
        assert_eq!(status_retryability(429), Retryability::Retryable);
        assert_eq!(status_retryability(503), Retryability::Retryable);
        assert_eq!(status_retryability(400), Retryability::NonRetryable);
        assert_eq!(status_retryability(401), Retryability::NonRetryable);
        assert_eq!(status_retryability(404), Retryability::NonRetryable);
    }

    #[tokio::test]
    async fn provider_adapters_openai_retries_retryable_transport_then_succeeds() {
        let http = Arc::new(MockHttpExecutor::new(vec![
            Err(HttpTransportError::retryable(
                "timeout while sending request",
            )),
            Ok(HttpResponse {
                status_code: 200,
                body: json!({
                    "data": [
                        {"index": 0, "embedding": [0.1, 0.2, 0.3]},
                        {"index": 1, "embedding": [0.4, 0.5, 0.6]}
                    ],
                    "model": "text-embedding-3-small",
                    "usage": {"prompt_tokens": 7, "total_tokens": 7}
                })
                .to_string(),
            }),
        ]));
        let sleeper = Arc::new(MockSleeper::default());
        let provider = openai_provider_for_test(
            http.clone(),
            sleeper.clone(),
            RetryPolicy {
                max_retries: 2,
                initial_backoff: Duration::from_millis(10),
                max_backoff: Duration::from_millis(100),
            },
        );

        let response = provider
            .embed(sample_request(EmbeddingPurpose::Document))
            .await
            .expect("expected retry then success");

        assert_eq!(response.provider, EmbeddingProviderKind::OpenAi);
        assert_eq!(response.vectors.len(), 2);
        assert_eq!(sleeper.delays(), vec![Duration::from_millis(10)]);

        let requests = http.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].url, "https://api.openai.com/v1/embeddings");
        assert_eq!(requests[0].body["model"], json!("text-embedding-3-small"));
        assert_eq!(requests[0].body["encoding_format"], json!("float"));
    }

    #[tokio::test]
    async fn provider_adapters_openai_stops_after_retry_budget() {
        let http = Arc::new(MockHttpExecutor::new(vec![
            Ok(HttpResponse {
                status_code: 429,
                body: json!({
                    "error": {
                        "message": "rate limit",
                        "code": "rate_limit_exceeded",
                        "type": "rate_limit_error"
                    }
                })
                .to_string(),
            }),
            Ok(HttpResponse {
                status_code: 429,
                body: json!({
                    "error": {
                        "message": "rate limit",
                        "code": "rate_limit_exceeded",
                        "type": "rate_limit_error"
                    }
                })
                .to_string(),
            }),
            Ok(HttpResponse {
                status_code: 429,
                body: json!({
                    "error": {
                        "message": "rate limit",
                        "code": "rate_limit_exceeded",
                        "type": "rate_limit_error"
                    }
                })
                .to_string(),
            }),
        ]));
        let sleeper = Arc::new(MockSleeper::default());
        let provider = openai_provider_for_test(
            http.clone(),
            sleeper.clone(),
            RetryPolicy {
                max_retries: 2,
                initial_backoff: Duration::from_millis(10),
                max_backoff: Duration::from_millis(100),
            },
        );

        let error = provider
            .embed(sample_request(EmbeddingPurpose::Document))
            .await
            .expect_err("expected final retryable provider failure");

        assert!(
            matches!(&error, EmbeddingError::Provider(_)),
            "expected provider error, got {error:?}"
        );
        if let EmbeddingError::Provider(failure) = error {
            assert_eq!(failure.status_code, Some(429));
            assert_eq!(failure.retryability, Retryability::Retryable);
            assert_eq!(failure.code.as_deref(), Some("rate_limit_exceeded"));
        }

        assert_eq!(http.requests().len(), 3);
        assert_eq!(
            sleeper.delays(),
            vec![Duration::from_millis(10), Duration::from_millis(20)]
        );
    }

    #[tokio::test]
    async fn provider_adapters_openai_provider_error_includes_request_diagnostics() {
        let http = Arc::new(MockHttpExecutor::new(vec![Ok(HttpResponse {
            status_code: 400,
            body: json!({
                "error": {
                    "message": "invalid json body",
                    "code": "invalid_request_error",
                    "type": "invalid_request_error"
                }
            })
            .to_string(),
        })]));
        let sleeper = Arc::new(MockSleeper::default());
        let provider = openai_provider_for_test(
            http,
            sleeper,
            RetryPolicy {
                max_retries: 0,
                initial_backoff: Duration::from_millis(10),
                max_backoff: Duration::from_millis(10),
            },
        );

        let error = provider
            .embed(sample_request(EmbeddingPurpose::Document))
            .await
            .expect_err("expected mapped provider error with request diagnostics");

        let EmbeddingError::Provider(failure) = error else {
            panic!("expected provider error");
        };
        assert_eq!(failure.status_code, Some(400));
        assert_eq!(failure.code.as_deref(), Some("invalid_request_error"));
        assert!(
            failure
                .message
                .contains("request_context{model=text-embedding-3-small"),
            "provider error should include request diagnostics: {}",
            failure.message
        );
        assert!(
            failure.message.contains("inputs=2"),
            "provider error should include input count: {}",
            failure.message
        );
        assert!(
            failure.message.contains("trace_id=trace-123"),
            "provider error should preserve trace id in diagnostics: {}",
            failure.message
        );
    }

    #[tokio::test]
    async fn reqwest_http_executor_sends_parseable_openai_json_body() {
        async fn capture_openai_body(
            State(captured): State<Arc<Mutex<Vec<(String, usize, String)>>>>,
            headers: HeaderMap,
            body: Bytes,
        ) -> Json<serde_json::Value> {
            let body_text =
                String::from_utf8(body.to_vec()).expect("axum test server should receive utf-8");
            let parsed: serde_json::Value =
                serde_json::from_str(&body_text).expect("reqwest JSON body should parse");
            let content_type_count = headers.get_all("content-type").iter().count();
            let authorization = headers
                .get("authorization")
                .and_then(|value| value.to_str().ok())
                .unwrap_or_default()
                .to_owned();
            captured.lock().expect("capture mutex poisoned").push((
                body_text,
                content_type_count,
                authorization,
            ));
            let input_count = parsed["input"]
                .as_array()
                .map(|items| items.len())
                .expect("openai request should contain input array");
            let data = (0..input_count)
                .map(|index| json!({ "index": index, "embedding": [0.1, 0.2, 0.3] }))
                .collect::<Vec<_>>();
            Json(json!({
                "data": data,
                "model": "text-embedding-3-small",
                "usage": {"prompt_tokens": input_count, "total_tokens": input_count}
            }))
        }

        let captured = Arc::new(Mutex::new(Vec::new()));
        let app = Router::new()
            .route("/v1/embeddings", post(capture_openai_body))
            .with_state(captured.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("test listener should bind");
        let addr = listener
            .local_addr()
            .expect("test listener should expose address");
        let server = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("axum test server should serve");
        });

        let provider = OpenAiEmbeddingProvider::with_runtime(
            "test-openai-key".to_string(),
            OpenAiEmbeddingProviderConfig {
                endpoint: format!("http://{addr}/v1/embeddings"),
                timeout: Duration::from_secs(5),
                retry_policy: RetryPolicy::default(),
            },
            Arc::new(ReqwestHttpExecutor::new(Client::new())),
            Arc::new(MockSleeper::default()),
        );

        let response = provider
            .embed(sample_request(EmbeddingPurpose::Document))
            .await
            .expect("local reqwest JSON roundtrip should succeed");
        assert_eq!(response.provider, EmbeddingProviderKind::OpenAi);
        assert_eq!(response.vectors.len(), 2);

        let captured_requests = captured.lock().expect("capture mutex poisoned").clone();
        assert_eq!(captured_requests.len(), 1);
        let (captured_body, content_type_count, authorization) = &captured_requests[0];
        let parsed: serde_json::Value = serde_json::from_str(captured_body)
            .expect("captured reqwest payload should remain parseable");
        assert_eq!(parsed["model"], json!("text-embedding-3-small"));
        assert_eq!(parsed["encoding_format"], json!("float"));
        assert_eq!(parsed["dimensions"], json!(3));
        assert_eq!(
            *content_type_count, 1,
            "reqwest JSON execution should emit exactly one content-type header"
        );
        assert_eq!(
            authorization, "Bearer test-openai-key",
            "authorization header should be preserved across the reqwest JSON path"
        );
        assert_eq!(
            parsed["input"]
                .as_array()
                .map(|items| items.len())
                .expect("captured payload should contain input array"),
            2
        );

        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn provider_adapters_google_maps_non_retryable_provider_error() {
        let http = Arc::new(MockHttpExecutor::new(vec![Ok(HttpResponse {
            status_code: 400,
            body: json!({
                "error": {
                    "code": 400,
                    "message": "invalid request",
                    "status": "INVALID_ARGUMENT"
                }
            })
            .to_string(),
        })]));
        let sleeper = Arc::new(MockSleeper::default());
        let provider = google_provider_for_test(
            http.clone(),
            sleeper.clone(),
            RetryPolicy {
                max_retries: 3,
                initial_backoff: Duration::from_millis(10),
                max_backoff: Duration::from_millis(100),
            },
        );

        let mut request = sample_request(EmbeddingPurpose::Document);
        request.model = "text-embedding-004".to_string();

        let error = provider
            .embed(request)
            .await
            .expect_err("expected mapped provider error");

        assert!(
            matches!(&error, EmbeddingError::Provider(_)),
            "expected provider error, got {error:?}"
        );
        if let EmbeddingError::Provider(failure) = error {
            assert_eq!(failure.status_code, Some(400));
            assert_eq!(failure.retryability, Retryability::NonRetryable);
            assert_eq!(failure.code.as_deref(), Some("INVALID_ARGUMENT"));
        }

        assert_eq!(http.requests().len(), 1);
        assert!(sleeper.delays().is_empty());
    }

    #[tokio::test]
    async fn provider_adapters_google_builds_request_and_retries_retryable_provider_error() {
        let http = Arc::new(MockHttpExecutor::new(vec![
            Ok(HttpResponse {
                status_code: 503,
                body: json!({
                    "error": {
                        "code": 503,
                        "message": "temporarily unavailable",
                        "status": "UNAVAILABLE"
                    }
                })
                .to_string(),
            }),
            Ok(HttpResponse {
                status_code: 200,
                body: json!({
                    "embeddings": [
                        {"values": [0.9, 0.1, 0.2]},
                        {"embedding": {"values": [0.4, 0.5, 0.6]}}
                    ],
                    "usageMetadata": {
                        "promptTokenCount": 13,
                        "totalTokenCount": 13
                    }
                })
                .to_string(),
            }),
        ]));
        let sleeper = Arc::new(MockSleeper::default());
        let provider = google_provider_for_test(
            http.clone(),
            sleeper.clone(),
            RetryPolicy {
                max_retries: 2,
                initial_backoff: Duration::from_millis(5),
                max_backoff: Duration::from_millis(50),
            },
        );

        let mut request = sample_request(EmbeddingPurpose::Query);
        request.model = "text-embedding-004".to_string();

        let response = provider
            .embed(request)
            .await
            .expect("expected retry then success");

        assert_eq!(response.provider, EmbeddingProviderKind::Google);
        assert_eq!(response.vectors.len(), 2);
        assert_eq!(sleeper.delays(), vec![Duration::from_millis(5)]);

        let requests = http.requests();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].url,
            "https://generativelanguage.googleapis.com/v1beta/models/text-embedding-004:batchEmbedContents?key=test-google-key"
        );

        let payload_requests = requests[0].body["requests"]
            .as_array()
            .expect("requests field must be array");
        assert_eq!(payload_requests.len(), 2);
        assert_eq!(payload_requests[0]["taskType"], json!("RETRIEVAL_QUERY"));
        assert_eq!(
            payload_requests[0]["model"],
            json!("models/text-embedding-004")
        );
        assert_eq!(payload_requests[0]["outputDimensionality"], json!(3));
    }

    #[test]
    fn provider_trait_vector_store_readiness_maps_storage_failure_as_non_retryable_transport() {
        let db_path = temp_vector_db_path("readiness-failure");
        let trace_id = Some("trace-vector-readiness".to_string());

        let error = verify_vector_store_readiness(&db_path, None, trace_id.clone())
            .expect_err("uninitialized db should fail vector readiness check");

        assert!(
            matches!(&error, EmbeddingError::Transport(_)),
            "expected transport error, got {error:?}"
        );
        if let EmbeddingError::Transport(failure) = error {
            assert_eq!(failure.provider, EmbeddingProviderKind::VectorStore);
            assert_eq!(failure.operation, "vector_store_verify");
            assert_eq!(failure.retryability, Retryability::NonRetryable);
            assert_eq!(failure.trace_id, trace_id);
        }

        cleanup_db(&db_path);
    }

    #[test]
    fn provider_trait_vector_store_strict_helper_requires_sqlite_vec_backend() {
        let db_path = temp_vector_db_path("readiness-strict");
        let storage = Storage::new(&db_path);
        storage
            .initialize()
            .expect("storage init should prepare vector backend");

        let readiness =
            verify_vector_store_readiness(&db_path, None, Some("trace-compat".to_string()))
                .expect("compatible readiness should succeed");
        assert_eq!(readiness.backend, "sqlite_vec");
        assert_eq!(readiness.table_name, "embedding_vectors");
        assert_eq!(readiness.expected_dimensions, DEFAULT_VECTOR_DIMENSIONS);
        assert!(
            !readiness.extension_version.is_empty(),
            "sqlite-vec extension version should be reported"
        );
        verify_sqlite_vec_readiness(&db_path, None, Some("trace-strict".to_string()))
            .expect("strict readiness should pass when sqlite-vec backend is required");

        cleanup_db(&db_path);
    }

    #[test]
    fn provider_trait_sqlite_vec_readiness_propagates_storage_failures() {
        let db_path = temp_vector_db_path("readiness-strict-failure");
        let trace_id = Some("trace-strict-failure".to_string());

        let error = verify_sqlite_vec_readiness(&db_path, None, trace_id.clone())
            .expect_err("uninitialized db should fail strict sqlite-vec readiness check");

        assert!(
            matches!(&error, EmbeddingError::Transport(_)),
            "expected transport error, got {error:?}"
        );
        if let EmbeddingError::Transport(failure) = error {
            assert_eq!(failure.provider, EmbeddingProviderKind::VectorStore);
            assert_eq!(failure.operation, "vector_store_verify");
            assert_eq!(failure.retryability, Retryability::NonRetryable);
            assert_eq!(failure.trace_id, trace_id);
        }

        cleanup_db(&db_path);
    }
}
