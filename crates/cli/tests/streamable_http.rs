//! Real-process regression coverage for stateful Streamable HTTP session loss and recovery hints.
//!
//! Exercises process replacement, explicit termination, session isolation, and real idle expiry
//! while preserving rmcp's 404 body and requiring clients to perform a fresh initialization. No
//! external services or semantic models are needed.

use std::net::{SocketAddr, TcpListener};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use reqwest::{Client, Response, StatusCode};
use serde_json::{Value, json};

const TOKEN: &str = "http-lifecycle-test-token";
const SESSION: &str = "mcp-session-id";
const INSTANCE: &str = "x-frigg-instance-id";
const RECOVERY: &str = "x-frigg-session-recovery";

struct Server {
    child: Child,
    root: std::path::PathBuf,
    addr: SocketAddr,
    client: Client,
}

impl Server {
    async fn start(addr: Option<SocketAddr>) -> Self {
        let addr = addr.unwrap_or_else(|| {
            TcpListener::bind("127.0.0.1:0")
                .expect("bind an available loopback port")
                .local_addr()
                .expect("resolve the loopback listener address")
        });
        let root = std::env::temp_dir().join(format!("frigg-http-{}", uuid::Uuid::now_v7()));
        std::fs::create_dir(&root).expect("create HTTP lifecycle test workspace");
        let child = Command::new(env!("CARGO_BIN_EXE_frigg"))
            .current_dir(&root)
            .args([
                "serve",
                "--mcp-http-port",
                &addr.port().to_string(),
                "--mcp-http-auth-token",
                TOKEN,
                "--semantic-runtime-enabled",
                "false",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("start Frigg HTTP test server");
        let mut server = Self {
            child,
            root,
            addr,
            client: Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("build HTTP lifecycle test client"),
        };
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            assert!(
                server
                    .child
                    .try_wait()
                    .expect("inspect Frigg HTTP test process")
                    .is_none(),
                "server exited"
            );
            if let Ok(response) = server
                .client
                .get(server.url("/healthz"))
                .bearer_auth(TOKEN)
                .send()
                .await
                && response.status() == StatusCode::OK
            {
                break;
            }
            assert!(Instant::now() < deadline, "server did not become ready");
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        server
    }

    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    async fn post(&self, session: Option<&str>, message: Value) -> Response {
        let mut request = self
            .client
            .post(self.url("/mcp"))
            .bearer_auth(TOKEN)
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", "2025-03-26")
            .json(&message);
        if let Some(session) = session {
            request = request.header(SESSION, session);
        }
        request.send().await.expect("send MCP request")
    }

    async fn initialize(&self) -> (String, String) {
        let response = self
            .post(
                None,
                json!({
                    "jsonrpc": "2.0", "id": 1, "method": "initialize",
                    "params": {
                        "protocolVersion": "2025-03-26", "capabilities": {},
                        "clientInfo": {"name": "frigg-http-test", "version": "1"}
                    }
                }),
            )
            .await;
        assert_eq!(response.status(), StatusCode::OK);
        let session = response.headers()[SESSION]
            .to_str()
            .expect("session header is valid text")
            .to_owned();
        let instance = response.headers()[INSTANCE]
            .to_str()
            .expect("instance header is valid text")
            .to_owned();
        let result = rpc_result(response).await;
        assert_eq!(result["protocolVersion"], "2025-03-26");
        let initialized = self
            .post(
                Some(&session),
                json!({
                    "jsonrpc": "2.0", "method": "notifications/initialized"
                }),
            )
            .await;
        assert_eq!(initialized.status(), StatusCode::ACCEPTED);
        (session, instance)
    }

    async fn assert_tools_work(&self, session: &str) {
        let response = self.post(Some(session), tools_list()).await;
        assert_eq!(response.status(), StatusCode::OK);
        let result = rpc_result(response).await;
        assert!(
            result["tools"]
                .as_array()
                .expect("tools/list returns an array")
                .iter()
                .any(|tool| tool["name"] == "workspace")
        );
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn tools_list() -> Value {
    json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}})
}

async fn rpc_result(response: Response) -> Value {
    let body = response.text().await.expect("read MCP response body");
    let message = body
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter_map(|data| serde_json::from_str::<Value>(data).ok())
        .find(|message| message.get("id").is_some())
        .expect("SSE response must contain a JSON-RPC response");
    assert!(message.get("error").is_none(), "{message}");
    message["result"].clone()
}

async fn assert_lost_session(response: Response) -> String {
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()[RECOVERY], "initialize");
    let instance = response.headers()[INSTANCE]
        .to_str()
        .expect("instance header is valid text")
        .to_owned();
    assert_eq!(
        response.text().await.expect("read missing-session body"),
        "Not Found: Session not found"
    );
    instance
}

#[tokio::test]
async fn restart_rejects_old_session_and_reinitialization_recovers() {
    let server = Server::start(None).await;
    let (session, instance) = server.initialize().await;
    server.assert_tools_work(&session).await;
    let addr = server.addr;
    drop(server);

    let replacement = Server::start(Some(addr)).await;
    let new_instance =
        assert_lost_session(replacement.post(Some(&session), tools_list()).await).await;
    assert_ne!(instance, new_instance);
    let get = replacement
        .client
        .get(replacement.url("/mcp"))
        .bearer_auth(TOKEN)
        .header("accept", "text/event-stream")
        .header(SESSION, &session)
        .send()
        .await
        .expect("send stale-session GET");
    assert_eq!(assert_lost_session(get).await, new_instance);
    let (new_session, initialized_instance) = replacement.initialize().await;
    assert_ne!(session, new_session);
    assert_eq!(new_instance, initialized_instance);
    replacement.assert_tools_work(&new_session).await;
    let response = replacement
        .post(
            Some(&new_session),
            json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
            "params": {"name": "workspace", "arguments": {
                "path": replacement.root, "resolve_mode": "direct"
            }}}),
        )
        .await;
    assert_eq!(response.status(), StatusCode::OK);
    let result = rpc_result(response).await;
    assert_ne!(result["isError"], true, "{result}");
    assert!(
        !result["content"]
            .as_array()
            .expect("workspace result contains content")
            .is_empty()
    );
}

#[tokio::test]
async fn termination_preserves_other_sessions_and_auth_protocol_boundaries() {
    let server = Server::start(None).await;
    let (session, instance) = server.initialize().await;
    let (other, _) = server.initialize().await;
    let deleted = server
        .client
        .delete(server.url("/mcp"))
        .bearer_auth(TOKEN)
        .header(SESSION, &session)
        .send()
        .await
        .expect("terminate MCP session");
    assert_eq!(deleted.status(), StatusCode::ACCEPTED);
    assert_eq!(
        assert_lost_session(server.post(Some(&session), tools_list()).await).await,
        instance
    );
    server.assert_tools_work(&other).await;

    let no_session = server.post(None, tools_list()).await;
    assert_eq!(no_session.status(), StatusCode::UNPROCESSABLE_ENTITY);
    assert!(!no_session.headers().contains_key(RECOVERY));
    let unauthorized = server
        .client
        .post(server.url("/mcp"))
        .header(SESSION, &session)
        .json(&tools_list())
        .send()
        .await
        .expect("send unauthenticated session request");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
    assert!(!unauthorized.headers().contains_key(RECOVERY));
    let bad_version = server
        .client
        .post(server.url("/mcp"))
        .bearer_auth(TOKEN)
        .header(SESSION, &other)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-protocol-version", "not-a-version")
        .json(&tools_list())
        .send()
        .await
        .expect("send invalid-version request");
    assert_eq!(bad_version.status(), StatusCode::BAD_REQUEST);
    assert!(!bad_version.headers().contains_key(RECOVERY));
    let missing_route = server
        .client
        .get(server.url("/missing"))
        .bearer_auth(TOKEN)
        .header(SESSION, &session)
        .send()
        .await
        .expect("send unrelated missing-route request");
    assert_eq!(missing_route.status(), StatusCode::NOT_FOUND);
    assert!(!missing_route.headers().contains_key(RECOVERY));
    let health = server
        .client
        .get(server.url("/healthz"))
        .bearer_auth(TOKEN)
        .send()
        .await
        .expect("request health endpoint");
    assert_eq!(health.headers()[INSTANCE], instance);
    let (new_session, _) = server.initialize().await;
    server.assert_tools_work(&new_session).await;
}

#[tokio::test]
#[ignore = "real rmcp idle timeout: takes just over five minutes"]
async fn idle_expiry_requires_reinitialization_without_process_restart() {
    let server = Server::start(None).await;
    let (session, instance) = server.initialize().await;
    server.assert_tools_work(&session).await;
    tokio::time::sleep(Duration::from_secs(305)).await;
    assert_eq!(
        assert_lost_session(server.post(Some(&session), tools_list()).await).await,
        instance
    );
    let (new_session, new_instance) = server.initialize().await;
    assert_ne!(session, new_session);
    assert_eq!(instance, new_instance);
    server.assert_tools_work(&new_session).await;
}
