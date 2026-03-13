use std::sync::Arc;

use async_trait::async_trait;
use reqwest::{Client, Method};
use serde::{Deserialize, Serialize};

use super::*;

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

    pub(super) fn with_runtime(
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
