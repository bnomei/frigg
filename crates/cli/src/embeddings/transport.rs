use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Method};

use super::*;

#[derive(Debug, Clone)]
pub(super) struct HttpRequest {
    pub(super) method: Method,
    pub(super) url: String,
    pub(super) headers: Vec<(String, String)>,
    pub(super) body: serde_json::Value,
    pub(super) timeout: Duration,
    pub(super) diagnostics: HttpRequestDiagnostics,
}

#[derive(Debug, Clone)]
pub(super) struct HttpResponse {
    pub(super) status_code: u16,
    pub(super) body: String,
}

#[derive(Debug, Clone)]
pub(super) struct HttpTransportError {
    pub(super) message: String,
    pub(super) retryability: Retryability,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct HttpRequestDiagnostics {
    pub(super) provider: EmbeddingProviderKind,
    pub(super) model: String,
    pub(super) input_count: usize,
    pub(super) input_chars_total: usize,
    pub(super) max_input_chars: usize,
    pub(super) body_bytes: usize,
    pub(super) body_blake3: String,
    pub(super) trace_id: Option<String>,
}

impl HttpRequestDiagnostics {
    pub(super) fn from_request(
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
    pub(super) fn retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryability: Retryability::Retryable,
        }
    }

    pub(super) fn non_retryable(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            retryability: Retryability::NonRetryable,
        }
    }
}

pub(super) fn append_request_diagnostics(
    message: impl AsRef<str>,
    diagnostics: &HttpRequestDiagnostics,
) -> String {
    format!("{} {}", message.as_ref(), diagnostics)
}

#[async_trait]
pub(super) trait HttpExecutor: Send + Sync {
    async fn execute(&self, request: HttpRequest) -> Result<HttpResponse, HttpTransportError>;
}

#[derive(Clone)]
pub(super) struct ReqwestHttpExecutor {
    client: Client,
}

impl ReqwestHttpExecutor {
    pub(super) fn new(client: Client) -> Self {
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
pub(super) trait BackoffSleeper: Send + Sync {
    async fn sleep(&self, duration: Duration);
}

#[derive(Clone, Default)]
pub(super) struct TokioSleeper;

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

pub(super) fn status_retryability(status_code: u16) -> Retryability {
    if matches!(status_code, 408 | 409 | 425 | 429 | 500 | 502 | 503 | 504) {
        Retryability::Retryable
    } else {
        Retryability::NonRetryable
    }
}

pub(super) fn usage_from_counts(
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
