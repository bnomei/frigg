use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

use super::*;

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

    pub(super) fn with_runtime(
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
