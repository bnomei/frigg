#![allow(clippy::panic, clippy::type_complexity)]

use super::*;
use axum::{Json, Router, body::Bytes, extract::State, http::HeaderMap, routing::post};
use reqwest::Client;
use serde_json::json;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
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
    let validation = ValidationFailure::new("dimensions", "dimensions must be greater than zero");
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

    let readiness = verify_vector_store_readiness(&db_path, None, Some("trace-compat".to_string()))
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
