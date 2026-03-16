use super::*;

#[derive(Debug, Clone)]
pub(super) struct HttpRuntimeConfig {
    pub bind_addr: SocketAddr,
    pub auth_token: Option<String>,
    pub allowed_authorities: Option<Vec<String>>,
}

#[derive(Clone)]
struct HttpAuthState {
    expected_bearer_header: Option<String>,
    allowed_authorities: Option<Vec<String>>,
}

impl HttpRuntimeConfig {
    pub(super) fn transport_kind(&self) -> RuntimeTransportKind {
        if self.bind_addr.ip().is_loopback() {
            RuntimeTransportKind::LoopbackHttp
        } else {
            RuntimeTransportKind::RemoteHttp
        }
    }
}

pub(super) fn resolve_http_runtime_config(
    cli: &Cli,
    serve_requested: bool,
) -> Result<Option<HttpRuntimeConfig>, Box<dyn Error>> {
    let has_http_port = cli.mcp_http_port.is_some();
    let has_http_related_flags =
        cli.mcp_http_host.is_some() || cli.allow_remote_http || cli.mcp_http_auth_token.is_some();

    if !has_http_port {
        if serve_requested {
            let bind_addr = SocketAddr::new(
                cli.mcp_http_host.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST)),
                37_444,
            );
            return Ok(Some(HttpRuntimeConfig {
                bind_addr,
                auth_token: None,
                allowed_authorities: allowed_authorities_for_bind(bind_addr),
            }));
        }
        if has_http_related_flags {
            return Err(Box::new(io::Error::other(
                "HTTP transport flags require --mcp-http-port",
            )));
        }
        return Ok(None);
    }

    let host = cli.mcp_http_host.unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let port = cli
        .mcp_http_port
        .expect("checked: mcp_http_port is set when has_http_port is true");
    let bind_addr = SocketAddr::new(host, port);

    let auth_token = match cli.mcp_http_auth_token.as_deref() {
        Some(raw) if raw.trim().is_empty() => {
            return Err(Box::new(io::Error::other(
                "--mcp-http-auth-token must not be blank",
            )));
        }
        Some(raw) => Some(raw.trim().to_owned()),
        None => None,
    };

    if !host.is_loopback() && !cli.allow_remote_http {
        return Err(Box::new(io::Error::other(format!(
            "refusing non-loopback HTTP bind at {bind_addr}; pass --allow-remote-http and set --mcp-http-auth-token"
        ))));
    }

    if !host.is_loopback() && auth_token.is_none() {
        return Err(Box::new(io::Error::other(
            "HTTP mode requires --mcp-http-auth-token for non-loopback binds",
        )));
    }

    let allowed_authorities = allowed_authorities_for_bind(bind_addr);

    Ok(Some(HttpRuntimeConfig {
        bind_addr,
        auth_token,
        allowed_authorities,
    }))
}

pub(super) async fn serve_http(
    runtime: HttpRuntimeConfig,
    server: FriggMcpServer,
) -> Result<(), Box<dyn Error>> {
    let listener = tokio::net::TcpListener::bind(runtime.bind_addr).await?;
    let config = StreamableHttpServerConfig {
        stateful_mode: true,
        ..StreamableHttpServerConfig::default()
    };
    let shutdown = config.cancellation_token.clone();
    let service = server.streamable_http_service(config);

    info!(
        bind_addr = %runtime.bind_addr,
        "serving MCP over streamable HTTP at /mcp"
    );

    if let Some(authorities) = runtime.allowed_authorities.as_ref() {
        info!(
            ?authorities,
            "HTTP origin/host allowlist enabled for MCP endpoint"
        );
    } else {
        warn!("HTTP origin/host allowlist disabled because bind host is unspecified");
    }

    if runtime.auth_token.is_some() {
        info!("HTTP bearer token auth enabled for MCP endpoint");
    } else {
        warn!("HTTP bearer token auth disabled for loopback MCP endpoint");
    }

    let router = Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(
            HttpAuthState {
                expected_bearer_header: runtime.auth_token.map(|token| format!("Bearer {token}")),
                allowed_authorities: runtime.allowed_authorities,
            },
            bearer_auth_middleware,
        ));

    axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = tokio::signal::ctrl_c().await;
            shutdown.cancel();
        })
        .await?;

    Ok(())
}

async fn bearer_auth_middleware(
    State(state): State<HttpAuthState>,
    request: Request,
    next: Next,
) -> Response {
    if !host_header_allowed(request.headers(), &state.allowed_authorities) {
        return typed_access_denied_response(StatusCode::FORBIDDEN, "unauthorized host header");
    }

    if !origin_header_allowed(request.headers(), &state.allowed_authorities) {
        return typed_access_denied_response(StatusCode::FORBIDDEN, "unauthorized origin header");
    }

    let Some(expected_bearer_header) = state.expected_bearer_header.as_deref() else {
        return next.run(request).await;
    };

    let provided = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("");
    let authorized = constant_time_equals(provided, expected_bearer_header);

    if !authorized {
        return typed_access_denied_response(
            StatusCode::UNAUTHORIZED,
            "missing or invalid bearer authorization",
        )
        .into_response();
    }

    next.run(request).await
}

pub(super) fn allowed_authorities_for_bind(bind_addr: SocketAddr) -> Option<Vec<String>> {
    if bind_addr.ip().is_unspecified() {
        return None;
    }

    let mut authorities = Vec::new();
    let port = bind_addr.port();

    match bind_addr {
        SocketAddr::V4(addr) => {
            push_authority_variants(&mut authorities, &addr.ip().to_string(), port);
            if addr.ip().is_loopback() {
                push_authority_variants(&mut authorities, "localhost", port);
            }
        }
        SocketAddr::V6(addr) => {
            push_authority_variants(&mut authorities, &format!("[{}]", addr.ip()), port);
            if addr.ip().is_loopback() {
                push_authority_variants(&mut authorities, "localhost", port);
            }
        }
    }

    authorities.sort();
    authorities.dedup();
    Some(authorities)
}

fn push_authority_variants(authorities: &mut Vec<String>, host: &str, port: u16) {
    authorities.push(host.to_ascii_lowercase());
    authorities.push(format!("{host}:{port}").to_ascii_lowercase());
}

pub(super) fn host_header_allowed(
    headers: &axum::http::HeaderMap,
    allowed_authorities: &Option<Vec<String>>,
) -> bool {
    let Some(authority) = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(parse_host_authority)
    else {
        return false;
    };

    authority_allowed(&authority, allowed_authorities)
}

pub(super) fn origin_header_allowed(
    headers: &axum::http::HeaderMap,
    allowed_authorities: &Option<Vec<String>>,
) -> bool {
    let Some(raw_origin) = headers.get(header::ORIGIN) else {
        return true;
    };
    let Some(authority) = raw_origin.to_str().ok().and_then(parse_origin_authority) else {
        return false;
    };

    authority_allowed(&authority, allowed_authorities)
}

pub(super) fn parse_host_authority(raw: &str) -> Option<String> {
    let authority = raw.trim().trim_end_matches('.');
    if authority.is_empty() {
        return None;
    }
    Some(authority.to_ascii_lowercase())
}

pub(super) fn parse_origin_authority(raw: &str) -> Option<String> {
    let origin = raw.trim();
    if origin.is_empty() || origin.eq_ignore_ascii_case("null") {
        return None;
    }
    let (_scheme, rest) = origin.split_once("://")?;
    let authority = rest.split('/').next()?.trim().trim_end_matches('.');
    if authority.is_empty() {
        return None;
    }
    Some(authority.to_ascii_lowercase())
}

pub(super) fn authority_allowed(
    authority: &str,
    allowed_authorities: &Option<Vec<String>>,
) -> bool {
    match allowed_authorities {
        None => true,
        Some(allowlist) => allowlist
            .iter()
            .any(|candidate| constant_time_equals(candidate, authority)),
    }
}

pub(super) fn constant_time_equals(left: &str, right: &str) -> bool {
    let left_bytes = left.as_bytes();
    let right_bytes = right.as_bytes();
    let max_len = left_bytes.len().max(right_bytes.len());
    let mut diff = left_bytes.len() ^ right_bytes.len();

    for idx in 0..max_len {
        let lhs = *left_bytes.get(idx).unwrap_or(&0);
        let rhs = *right_bytes.get(idx).unwrap_or(&0);
        diff |= (lhs ^ rhs) as usize;
    }

    diff == 0
}

pub(super) fn typed_access_denied_response(status: StatusCode, message: &str) -> Response {
    let escaped_message = message
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t");
    (
        status,
        [(header::CONTENT_TYPE, "application/json")],
        format!(
            r#"{{"error_code":"access_denied","retryable":false,"message":"{escaped_message}"}}"#
        ),
    )
        .into_response()
}
