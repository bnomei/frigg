//! Frigg binary entrypoint: builds the tokio runtime, initializes tracing, and hands off to CLI
//! dispatch for serve, utility commands, and HTTP runtime startup.

use frigg::settings::RuntimeTransportKind;
use std::error::Error;
use tracing_subscriber::{EnvFilter, fmt};

mod cli_args;
mod cli_dispatch;
mod cli_runtime;
mod http_runtime;
#[cfg(test)]
use axum::http::{StatusCode, header};
pub(crate) use cli_args::{Cli, Command};
use cli_dispatch::async_main;
use cli_runtime::{CliOutput, error_was_reported, field};
#[cfg(test)]
use cli_runtime::{
    StorageMaintenanceCommand, ensure_storage_db_path_for_write, find_enclosing_git_root,
    resolve_command_config, resolve_semantic_runtime_config, resolve_startup_config,
    resolve_storage_db_path, resolve_watch_config, resolve_watch_runtime_config, run_index_command,
    run_semantic_runtime_startup_gate_with_credentials, run_storage_init_command,
    run_storage_maintenance_command, run_strict_startup_vector_readiness_gate,
};
#[cfg(test)]
use frigg::settings::{
    FriggConfig, SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider,
    WatchMode,
};
#[cfg(test)]
use frigg::storage::SemanticChunkEmbeddingRecord;
#[cfg(test)]
use frigg::storage::{DEFAULT_RETAINED_MANIFEST_SNAPSHOTS, Storage};
#[cfg(test)]
use http_runtime::{
    HttpRuntimeConfig, allowed_authorities_for_bind, authority_allowed, constant_time_equals,
    host_header_allowed, origin_header_allowed, parse_host_authority, parse_origin_authority,
    resolve_http_runtime_config, typed_access_denied_response,
};
#[cfg(test)]
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
#[cfg(test)]
use std::path::{Path, PathBuf};

fn main() {
    if let Err(error) = run_main() {
        report_unhandled_cli_error(error.as_ref());
        std::process::exit(1);
    }
}

fn run_main() -> Result<(), Box<dyn Error>> {
    let startup_trace_active = startup_trace_enabled();
    startup_trace(startup_trace_active, "main: entered");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    startup_trace(startup_trace_active, "main: tokio runtime ready");
    runtime.block_on(async_main(startup_trace_active))
}

fn report_unhandled_cli_error(error: &(dyn Error + 'static)) {
    if error_was_reported(error) {
        return;
    }

    let message = error.to_string();
    if is_structured_error_line(&message) {
        eprintln!("{message}");
        return;
    }

    let _ = CliOutput::normal().error_event(
        "frigg",
        "failed",
        &[field("status", "failed"), field("error", message)],
        None,
    );
}

fn is_structured_error_line(message: &str) -> bool {
    message.starts_with("error ") && message.contains(": ") && !message.contains('\n')
}

fn startup_trace_enabled() -> bool {
    std::env::var_os("FRIGG_STARTUP_TRACE").is_some()
}

fn startup_trace(enabled: bool, message: &str) {
    if enabled {
        eprintln!("[frigg-startup] {message}");
    }
}

fn default_tracing_filter(cli: &Cli, transport: RuntimeTransportKind) -> &'static str {
    if cli.quiet || (cli.command.is_none() && transport == RuntimeTransportKind::Stdio) {
        "error"
    } else if cli.verbose
        && matches!(cli.command, None | Some(Command::Serve))
        && transport != RuntimeTransportKind::Stdio
    {
        "warn"
    } else if cli.verbose {
        "info"
    } else {
        "warn"
    }
}

fn tracing_env_override_allowed(cli: &Cli, transport: RuntimeTransportKind) -> bool {
    !(cli.quiet || cli.command.is_none() && transport == RuntimeTransportKind::Stdio)
}

fn init_tracing(default_filter: &str, allow_env_override: bool) {
    let filter = if allow_env_override {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter))
    } else {
        EnvFilter::new(default_filter)
    };
    // Side effect: MCP stdio transport requires stdout for protocol frames only.
    let _ = fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_writer(std::io::stderr)
        .try_init();
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::net::Ipv6Addr;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use frigg::storage::{
        DEFAULT_VECTOR_DIMENSIONS, PROVENANCE_STORAGE_DB_FILE, PROVENANCE_STORAGE_DIR,
        VECTOR_TABLE_NAME,
    };
    use rusqlite::Connection;

    fn base_cli() -> Cli {
        Cli {
            quiet: false,
            verbose: false,
            workspace_roots: vec![PathBuf::from(".")],
            max_file_bytes: None,
            full_scip_ingest: true,
            mcp_http_port: None,
            mcp_http_host: None,
            allow_remote_http: false,
            mcp_http_auth_token: None,
            semantic_runtime_enabled: None,
            semantic_runtime_provider: None,
            semantic_runtime_model: None,
            semantic_runtime_strict_mode: None,
            watch_mode: None,
            lexical_backend: None,
            ripgrep_executable: None,
            watch_debounce_ms: None,
            watch_retry_ms: None,
            watch_manifest_fast_concurrency: None,
            watch_semantic_followup_concurrency: None,
            command: None,
        }
    }

    #[test]
    fn transport_defaults_to_stdio_when_http_port_absent() {
        let cli = base_cli();
        let runtime = resolve_http_runtime_config(&cli, false).expect("stdio mode should resolve");
        assert!(runtime.is_none());
    }

    #[test]
    fn transport_http_defaults_to_loopback_bind() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4000);
        cli.mcp_http_auth_token = Some("test-token".to_owned());

        let runtime = resolve_http_runtime_config(&cli, false)
            .expect("http runtime should resolve")
            .expect("http runtime should be enabled");
        assert_eq!(
            runtime.bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4000)
        );
        assert_eq!(runtime.auth_token, Some("test-token".to_owned()));
        assert_eq!(
            runtime.allowed_authorities,
            Some(vec![
                "127.0.0.1".to_owned(),
                "127.0.0.1:4000".to_owned(),
                "localhost".to_owned(),
                "localhost:4000".to_owned(),
            ])
        );
    }

    #[test]
    fn serve_command_defaults_to_loopback_bind_and_port() {
        let mut cli = base_cli();
        cli.command = Some(Command::Serve);

        let runtime = resolve_http_runtime_config(&cli, true)
            .expect("serve runtime should resolve")
            .expect("serve runtime should be enabled");
        assert_eq!(
            runtime.bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 37_444)
        );
        assert_eq!(runtime.auth_token, None);
    }

    #[test]
    fn serve_command_rejects_non_loopback_bind_without_override() {
        let mut cli = base_cli();
        cli.command = Some(Command::Serve);
        cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));

        let error = resolve_http_runtime_config(&cli, true)
            .expect_err("serve default port must not bind non-loopback without override");
        assert!(
            error
                .to_string()
                .contains("refusing non-loopback HTTP bind"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn serve_command_rejects_non_loopback_bind_without_auth_token() {
        let mut cli = base_cli();
        cli.command = Some(Command::Serve);
        cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        cli.allow_remote_http = true;

        let error = resolve_http_runtime_config(&cli, true)
            .expect_err("serve default port must require an auth token for remote binds");
        assert!(
            error
                .to_string()
                .contains("HTTP mode requires --mcp-http-auth-token"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn serve_command_honors_auth_token_on_default_port() {
        let mut cli = base_cli();
        cli.command = Some(Command::Serve);
        cli.mcp_http_auth_token = Some("serve-token".to_owned());

        let runtime = resolve_http_runtime_config(&cli, true)
            .expect("serve runtime should resolve")
            .expect("serve runtime should be enabled");
        assert_eq!(
            runtime.bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 37_444)
        );
        assert_eq!(runtime.auth_token, Some("serve-token".to_owned()));
    }

    #[test]
    fn transport_rejects_http_flags_without_port() {
        let mut cli = base_cli();
        cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));

        let error =
            resolve_http_runtime_config(&cli, false).expect_err("host flag without port must fail");
        assert!(
            error.to_string().contains("require --mcp-http-port"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn transport_rejects_non_loopback_bind_without_override() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4001);
        cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        cli.mcp_http_auth_token = Some("test-token".to_owned());

        let error = resolve_http_runtime_config(&cli, false)
            .expect_err("non-loopback bind should fail without override");
        assert!(
            error
                .to_string()
                .contains("refusing non-loopback HTTP bind"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn transport_rejects_remote_bind_without_auth_token() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4002);
        cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        cli.allow_remote_http = true;

        let error = resolve_http_runtime_config(&cli, false)
            .expect_err("remote bind without auth token should fail");
        assert!(
            error
                .to_string()
                .contains("HTTP mode requires --mcp-http-auth-token"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn transport_accepts_remote_bind_with_override_and_auth_token() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4003);
        cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        cli.allow_remote_http = true;
        cli.mcp_http_auth_token = Some("test-token".to_owned());

        let runtime = resolve_http_runtime_config(&cli, false)
            .expect("remote bind with auth should be allowed")
            .expect("http runtime should be enabled");
        assert_eq!(
            runtime.bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4003)
        );
        assert_eq!(runtime.auth_token, Some("test-token".to_owned()));
        assert_eq!(runtime.allowed_authorities, None);
    }

    #[test]
    fn transport_accepts_loopback_bind_without_auth_token() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4010);

        let runtime = resolve_http_runtime_config(&cli, false)
            .expect("loopback bind without auth token should resolve")
            .expect("http runtime should be enabled");
        assert_eq!(
            runtime.bind_addr,
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4010)
        );
        assert_eq!(runtime.auth_token, None);
        assert_eq!(
            runtime.allowed_authorities,
            Some(vec![
                "127.0.0.1".to_owned(),
                "127.0.0.1:4010".to_owned(),
                "localhost".to_owned(),
                "localhost:4010".to_owned(),
            ])
        );
    }

    #[test]
    fn transport_rejects_blank_auth_token() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4014);
        cli.mcp_http_auth_token = Some("   ".to_owned());

        let error =
            resolve_http_runtime_config(&cli, false).expect_err("blank auth token should fail");
        assert!(
            error
                .to_string()
                .contains("--mcp-http-auth-token must not be blank"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn transport_host_allowlist_rejects_unknown_host() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::HOST,
            "evil.example:4000".parse().expect("host header must parse"),
        );

        let allowed = Some(vec![
            "127.0.0.1:4000".to_owned(),
            "localhost:4000".to_owned(),
        ]);
        assert!(!host_header_allowed(&headers, &allowed));
    }

    #[test]
    fn transport_origin_allowlist_rejects_unknown_origin() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            "https://evil.example:4000"
                .parse()
                .expect("origin header must parse"),
        );

        let allowed = Some(vec!["localhost:4000".to_owned()]);
        assert!(!origin_header_allowed(&headers, &allowed));
    }

    #[test]
    fn transport_authority_helpers_normalize_loopback_and_unspecified_hosts() {
        assert_eq!(
            allowed_authorities_for_bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 4020)),
            Some(vec![
                "127.0.0.1".to_owned(),
                "127.0.0.1:4020".to_owned(),
                "localhost".to_owned(),
                "localhost:4020".to_owned(),
            ])
        );
        assert_eq!(
            allowed_authorities_for_bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 4020)),
            None
        );
        assert_eq!(
            allowed_authorities_for_bind(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 4020)),
            Some(vec![
                "[::1]".to_owned(),
                "[::1]:4020".to_owned(),
                "localhost".to_owned(),
                "localhost:4020".to_owned(),
            ])
        );
    }

    #[test]
    fn transport_parsers_normalize_authorities_and_reject_invalid_values() {
        assert_eq!(
            parse_host_authority("LOCALHOST:4020."),
            Some("localhost:4020".to_owned())
        );
        assert_eq!(parse_host_authority("   "), None);
        assert_eq!(
            parse_origin_authority("https://LOCALHOST:4020/path?q=1"),
            Some("localhost:4020".to_owned())
        );
        assert_eq!(parse_origin_authority("null"), None);
        assert_eq!(parse_origin_authority(""), None);
    }

    #[test]
    fn transport_authority_allowlist_uses_constant_time_equality() {
        let allowed = Some(vec!["localhost:4020".to_owned()]);

        assert!(authority_allowed("localhost:4020", &allowed));
        assert!(!authority_allowed("localhost:4021", &allowed));
        assert!(authority_allowed("anything", &None));
    }

    #[tokio::test]
    async fn typed_access_denied_response_escapes_json_message() {
        let response = typed_access_denied_response(StatusCode::FORBIDDEN, "bad \"host\"\nvalue\t");
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("response body should be readable");
        let body_text = String::from_utf8(body.to_vec()).expect("body should be utf-8");

        assert_eq!(
            body_text,
            "{\"error_code\":\"access_denied\",\"retryable\":false,\"message\":\"bad \\\"host\\\"\\nvalue\\t\"}"
        );
    }

    #[test]
    fn transport_constant_time_compare_requires_exact_match() {
        assert!(constant_time_equals("Bearer token", "Bearer token"));
        assert!(!constant_time_equals("Bearer token", "Bearer token "));
        assert!(!constant_time_equals("Bearer token", "bearer token"));
    }

    #[test]
    fn semantic_runtime_defaults_to_disabled_in_cli_resolution() {
        let cli = base_cli();
        let semantic = resolve_semantic_runtime_config(&cli);
        assert!(!semantic.enabled);
        assert!(semantic.provider.is_none());
        assert!(semantic.model.is_none());
        assert!(!semantic.strict_mode);
    }

    #[test]
    fn semantic_runtime_cli_resolution_applies_explicit_values() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Google);
        cli.semantic_runtime_model = Some("gemini-embedding-001".to_owned());
        cli.semantic_runtime_strict_mode = Some(true);

        let semantic = resolve_semantic_runtime_config(&cli);
        assert!(semantic.enabled);
        assert_eq!(semantic.provider, Some(SemanticRuntimeProvider::Google));
        assert_eq!(semantic.model.as_deref(), Some("gemini-embedding-001"));
        assert!(semantic.strict_mode);
    }

    #[test]
    fn semantic_runtime_cli_resolution_defaults_enabled_provider_to_local() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);

        let semantic = resolve_semantic_runtime_config(&cli);
        assert!(semantic.enabled);
        assert_eq!(semantic.provider, Some(SemanticRuntimeProvider::Local));
        assert_eq!(semantic.normalized_model(), Some("all-MiniLM-L6-v2"));
    }

    #[test]
    fn semantic_runtime_provider_parses_local_and_reports_defaults() {
        let provider: SemanticRuntimeProvider = " local "
            .parse()
            .expect("local provider should parse with surrounding whitespace");

        assert_eq!(provider, SemanticRuntimeProvider::Local);
        assert_eq!(provider.as_str(), "local");
        assert_eq!(provider.to_string(), "local");
        assert_eq!(provider.default_model(), "all-MiniLM-L6-v2");
        assert_eq!(provider.api_key_env_var(), None);
    }

    #[test]
    fn semantic_runtime_provider_rejects_unknown_provider_clearly() {
        let error = "ollama"
            .parse::<SemanticRuntimeProvider>()
            .expect_err("unknown semantic provider must be rejected");

        assert!(
            error.contains("openai, google, local"),
            "unexpected provider parse error: {error}"
        );
        assert!(
            error.contains("ollama"),
            "provider parse error should include received value: {error}"
        );
    }

    #[test]
    fn semantic_runtime_cli_resolution_accepts_local_provider() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Local);

        let semantic = resolve_semantic_runtime_config(&cli);
        assert!(semantic.enabled);
        assert_eq!(semantic.provider, Some(SemanticRuntimeProvider::Local));
        assert_eq!(semantic.normalized_model(), Some("all-MiniLM-L6-v2"));
    }

    #[test]
    fn semantic_runtime_readme_documents_local_provider_contract() {
        let readme = include_str!("../../../README.md");

        assert!(readme.contains(
            "| `--semantic-runtime-provider` / `FRIGG_SEMANTIC_RUNTIME_PROVIDER` | `local` when semantic runtime is enabled | Semantic provider: `openai`, `google`, or `local`. |"
        ));
        assert!(readme.contains("export FRIGG_SEMANTIC_RUNTIME_PROVIDER=local"));
        assert!(readme.contains("`local` -> `all-MiniLM-L6-v2`"));
        assert!(readme.contains(
            "When `provider=local`, Frigg prepares missing local model artifacts automatically during startup"
        ));
        assert!(readme.contains("After enabling semantic search for an existing repository, or after changing the semantic provider or model, run one semantic index pass"));
        assert!(readme.contains("startup fails with `local_model_prepare_failed`"));

        let zero_cloud_claims = readme.matches("Zero-cloud semantic").count()
            + readme.matches("zero-cloud semantic").count();
        assert_eq!(
            zero_cloud_claims, 1,
            "README zero-cloud semantic claims should stay scoped and singular"
        );
        assert!(readme.contains("Zero-cloud semantic behavior applies only when `provider=local`"));
        assert!(
            readme.contains(
                "OpenAI and Google semantic providers call their external embedding APIs"
            )
        );
    }

    #[test]
    fn watch_runtime_defaults_to_off_for_stdio_with_standard_timers() {
        let cli = base_cli();
        let watch = resolve_watch_config(&cli, Some(RuntimeTransportKind::Stdio));
        assert_eq!(watch.mode, WatchMode::Off);
        assert_eq!(watch.debounce_ms, 2_000);
        assert_eq!(watch.retry_ms, 5_000);
        assert_eq!(watch.manifest_fast_concurrency, 1);
        assert_eq!(watch.semantic_followup_concurrency, 1);
    }

    #[test]
    fn watch_runtime_defaults_to_auto_for_http_with_standard_timers() {
        let cli = base_cli();
        let watch = resolve_watch_config(&cli, Some(RuntimeTransportKind::LoopbackHttp));
        assert_eq!(watch.mode, WatchMode::Auto);
        assert_eq!(watch.debounce_ms, 2_000);
        assert_eq!(watch.retry_ms, 5_000);
        assert_eq!(watch.manifest_fast_concurrency, 1);
        assert_eq!(watch.semantic_followup_concurrency, 1);
    }

    #[test]
    fn watch_runtime_cli_resolution_applies_explicit_values() {
        let mut cli = base_cli();
        cli.watch_mode = Some(WatchMode::On);
        cli.watch_debounce_ms = Some(1_250);
        cli.watch_retry_ms = Some(9_000);
        cli.watch_manifest_fast_concurrency = Some(2);
        cli.watch_semantic_followup_concurrency = Some(3);

        let watch = resolve_watch_config(&cli, Some(RuntimeTransportKind::Stdio));
        assert_eq!(watch.mode, WatchMode::On);
        assert_eq!(watch.debounce_ms, 1_250);
        assert_eq!(watch.retry_ms, 9_000);
        assert_eq!(watch.manifest_fast_concurrency, 2);
        assert_eq!(watch.semantic_followup_concurrency, 3);
    }

    #[test]
    fn watch_runtime_transport_kind_matches_http_runtime() {
        let cli = base_cli();
        assert_eq!(
            resolve_http_runtime_config(&cli, false)
                .expect("stdio should resolve")
                .as_ref()
                .map(HttpRuntimeConfig::transport_kind)
                .unwrap_or(RuntimeTransportKind::Stdio),
            RuntimeTransportKind::Stdio
        );

        let mut loopback_cli = base_cli();
        loopback_cli.mcp_http_port = Some(4011);
        let loopback_runtime = resolve_http_runtime_config(&loopback_cli, false)
            .expect("loopback http should resolve")
            .expect("loopback runtime should be enabled");
        assert_eq!(
            loopback_runtime.transport_kind(),
            RuntimeTransportKind::LoopbackHttp
        );

        let mut remote_cli = base_cli();
        remote_cli.mcp_http_port = Some(4012);
        remote_cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        remote_cli.allow_remote_http = true;
        remote_cli.mcp_http_auth_token = Some("test-token".to_owned());
        let remote_runtime = resolve_http_runtime_config(&remote_cli, false)
            .expect("remote http should resolve with override")
            .expect("remote runtime should be enabled");
        assert_eq!(
            remote_runtime.transport_kind(),
            RuntimeTransportKind::RemoteHttp
        );
    }

    #[test]
    fn stdio_server_defaults_to_error_log_filter() {
        let cli = base_cli();
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::Stdio),
            "error"
        );
    }

    #[test]
    fn http_server_defaults_to_info_log_filter() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4013);
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::LoopbackHttp),
            "warn"
        );
    }

    #[test]
    fn utility_commands_default_to_warning_log_filter() {
        let mut cli = base_cli();
        cli.command = Some(Command::Index { changed: false });
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::Stdio),
            "warn"
        );
    }

    #[test]
    fn verbose_flag_enables_info_log_filter_for_utility_commands() {
        let mut cli = base_cli();
        cli.verbose = true;
        cli.command = Some(Command::Index { changed: false });
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::Stdio),
            "info"
        );
    }

    #[test]
    fn verbose_serve_keeps_raw_info_tracing_suppressed() {
        let mut cli = base_cli();
        cli.verbose = true;
        cli.command = Some(Command::Serve);
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::LoopbackHttp),
            "warn"
        );
    }

    #[test]
    fn verbose_flag_does_not_raise_stdio_server_log_filter() {
        let mut cli = base_cli();
        cli.verbose = true;
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::Stdio),
            "error"
        );
    }

    #[test]
    fn quiet_flag_forces_error_log_filter() {
        let mut cli = base_cli();
        cli.command = Some(Command::Serve);
        cli.quiet = true;
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::LoopbackHttp),
            "error"
        );
    }

    #[test]
    fn quiet_flag_disables_rust_log_override() {
        let mut cli = base_cli();
        cli.command = Some(Command::Index { changed: false });
        cli.quiet = true;

        assert!(!tracing_env_override_allowed(
            &cli,
            RuntimeTransportKind::Stdio
        ));
    }

    #[test]
    fn stdio_server_disables_rust_log_override_for_protocol_safety() {
        let cli = base_cli();

        assert!(!tracing_env_override_allowed(
            &cli,
            RuntimeTransportKind::Stdio
        ));
    }

    #[test]
    fn non_quiet_utility_commands_keep_rust_log_override() {
        let mut cli = base_cli();
        cli.command = Some(Command::Index { changed: false });

        assert!(tracing_env_override_allowed(
            &cli,
            RuntimeTransportKind::Stdio
        ));
    }

    #[test]
    fn startup_config_defaults_enabled_semantic_runtime_to_local_provider() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);

        let config = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect("startup config should default enabled semantic runtime to local provider");
        assert_eq!(
            config.semantic_runtime.provider,
            Some(SemanticRuntimeProvider::Local)
        );
        assert_eq!(
            config.semantic_runtime.normalized_model(),
            Some("all-MiniLM-L6-v2")
        );
    }

    #[test]
    fn startup_config_accepts_provider_default_semantic_model() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::OpenAi);

        let config = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect("startup config should accept provider default semantic model");
        assert_eq!(
            config.semantic_runtime.normalized_model(),
            Some("text-embedding-3-small")
        );
    }

    #[test]
    fn startup_config_accepts_local_provider_default_semantic_model() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Local);

        let config = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect("startup config should accept local provider default semantic model");
        assert_eq!(
            config.semantic_runtime.normalized_model(),
            Some("all-MiniLM-L6-v2")
        );
    }

    #[test]
    fn startup_config_rejects_blank_semantic_runtime_model() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Local);
        cli.semantic_runtime_model = Some("   ".to_owned());

        let error = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect_err("startup config should reject a blank semantic model");
        assert!(
            error
                .to_string()
                .contains("semantic_runtime.model must not be blank"),
            "unexpected startup config error: {error}"
        );
    }

    #[test]
    fn startup_config_rejects_zero_watch_timers() {
        let mut cli = base_cli();
        cli.watch_debounce_ms = Some(0);
        let debounce_error = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect_err("startup config should reject watch-debounce-ms=0");
        assert!(
            debounce_error
                .to_string()
                .contains("watch.debounce_ms must be greater than zero"),
            "unexpected startup config error: {debounce_error}"
        );

        let mut retry_cli = base_cli();
        retry_cli.watch_retry_ms = Some(0);
        let retry_error = resolve_startup_config(&retry_cli, RuntimeTransportKind::Stdio)
            .expect_err("startup config should reject watch-retry-ms=0");
        assert!(
            retry_error
                .to_string()
                .contains("watch.retry_ms must be greater than zero"),
            "unexpected startup config error: {retry_error}"
        );

        let mut manifest_fast_cli = base_cli();
        manifest_fast_cli.watch_manifest_fast_concurrency = Some(0);
        let manifest_fast_error =
            resolve_startup_config(&manifest_fast_cli, RuntimeTransportKind::Stdio)
                .expect_err("startup config should reject watch-manifest-fast-concurrency=0");
        assert!(
            manifest_fast_error
                .to_string()
                .contains("watch.manifest_fast_concurrency must be greater than zero"),
            "unexpected startup config error: {manifest_fast_error}"
        );

        let mut semantic_followup_cli = base_cli();
        semantic_followup_cli.watch_semantic_followup_concurrency = Some(0);
        let semantic_followup_error =
            resolve_startup_config(&semantic_followup_cli, RuntimeTransportKind::Stdio)
                .expect_err("startup config should reject watch-semantic-followup-concurrency=0");
        assert!(
            semantic_followup_error
                .to_string()
                .contains("watch.semantic_followup_concurrency must be greater than zero"),
            "unexpected startup config error: {semantic_followup_error}"
        );
    }

    #[test]
    fn index_command_resolution_uses_startup_semantic_config() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Google);

        let config = resolve_command_config(&cli, Command::Index { changed: true })
            .expect("index command should resolve startup config");
        assert!(config.semantic_runtime.enabled);
        assert_eq!(
            config.semantic_runtime.provider,
            Some(SemanticRuntimeProvider::Google)
        );
        assert_eq!(
            config.semantic_runtime.normalized_model(),
            Some("gemini-embedding-001")
        );
    }

    #[test]
    fn index_command_rejects_prepare_semantic_model_flag() {
        let error = <Cli as clap::Parser>::try_parse_from([
            "frigg",
            "--semantic-runtime-enabled",
            "true",
            "--semantic-runtime-provider",
            "local",
            "index",
            "--prepare-semantic-model",
        ])
        .expect_err("index --prepare-semantic-model should no longer parse");

        let message = error.to_string();
        assert!(
            message.contains("--prepare-semantic-model"),
            "unexpected parse error: {message}"
        );
    }

    #[test]
    fn index_command_accepts_legacy_reindex_alias() {
        let cli = <Cli as clap::Parser>::try_parse_from(["frigg", "reindex", "--changed"])
            .expect("legacy reindex alias should parse as index");

        assert!(matches!(
            cli.command,
            Some(Command::Index { changed: true })
        ));
    }

    #[test]
    fn hidden_storage_maintenance_commands_parse_but_do_not_render_in_help() {
        let repair_cli = <Cli as clap::Parser>::try_parse_from(["frigg", "repair-storage"])
            .expect("hidden repair-storage command should remain callable");
        assert!(matches!(repair_cli.command, Some(Command::RepairStorage)));

        let prune_cli = <Cli as clap::Parser>::try_parse_from(["frigg", "prune-storage"])
            .expect("hidden prune-storage command should remain callable");
        assert!(matches!(
            prune_cli.command,
            Some(Command::PruneStorage {
                keep_manifest_snapshots: DEFAULT_RETAINED_MANIFEST_SNAPSHOTS,
            })
        ));

        let mut command = <Cli as clap::CommandFactory>::command();
        let help = command.render_help().to_string();
        assert!(help.contains("init"));
        assert!(help.contains("index"));
        assert!(!help.contains("repair-storage"));
        assert!(!help.contains("prune-storage"));
    }

    #[test]
    fn prepare_semantic_model_subcommand_is_not_registered() {
        let error = <Cli as clap::Parser>::try_parse_from([
            "frigg",
            "--workspace-root",
            ".",
            "--semantic-runtime-enabled",
            "true",
            "--semantic-runtime-provider",
            "local",
            "prepare-semantic-model",
        ])
        .expect_err("prepare-semantic-model should no longer parse");

        let message = error.to_string();
        assert!(
            message.contains("prepare-semantic-model"),
            "unexpected parse error: {message}"
        );
    }

    #[test]
    fn init_command_resolution_keeps_semantic_runtime_unset() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Google);

        let config = resolve_command_config(&cli, Command::Init)
            .expect("init command should resolve base config");
        assert!(!config.semantic_runtime.enabled);
        assert!(config.semantic_runtime.provider.is_none());
    }

    #[test]
    fn repair_storage_command_resolution_keeps_semantic_runtime_unset() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Google);

        let config = resolve_command_config(&cli, Command::RepairStorage)
            .expect("repair-storage command should resolve base config");
        assert!(!config.semantic_runtime.enabled);
        assert!(config.semantic_runtime.provider.is_none());
    }

    #[test]
    fn prune_storage_command_resolution_keeps_semantic_runtime_unset() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Google);

        let config = resolve_command_config(
            &cli,
            Command::PruneStorage {
                keep_manifest_snapshots: DEFAULT_RETAINED_MANIFEST_SNAPSHOTS,
            },
        )
        .expect("prune-storage command should resolve base config");
        assert!(!config.semantic_runtime.enabled);
        assert!(config.semantic_runtime.provider.is_none());
    }

    #[test]
    fn startup_config_applies_max_file_bytes_override() {
        let mut cli = base_cli();
        cli.max_file_bytes = Some(2 * 1024 * 1024);

        let config = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect("startup config should accept explicit max-file-bytes override");
        assert_eq!(config.max_file_bytes, 2 * 1024 * 1024);
    }

    #[test]
    fn startup_config_defaults_to_full_scip_ingest() {
        let config = resolve_startup_config(&base_cli(), RuntimeTransportKind::Stdio)
            .expect("startup config should default to full-scip-ingest");
        assert!(config.full_scip_ingest);
    }

    #[test]
    fn startup_config_rejects_zero_max_file_bytes_override() {
        let mut cli = base_cli();
        cli.max_file_bytes = Some(0);

        let error = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect_err("startup config should reject max-file-bytes=0");
        assert!(
            error
                .to_string()
                .contains("max_file_bytes must be greater than zero"),
            "unexpected startup config error: {error}"
        );
    }

    #[test]
    fn frigg_config_rejects_non_git_workspace_root_by_default() {
        let workspace_root = temp_workspace_root("startup-non-git-root");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        let mut cli = base_cli();
        cli.workspace_roots = vec![workspace_root.clone()];

        let error = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect_err("default startup config should reject non-git workspace roots");
        assert!(
            error
                .to_string()
                .contains("workspace root is not a Git repository"),
            "unexpected startup config error: {error}"
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn startup_config_allows_empty_workspace_roots_for_http_serving() {
        let mut cli = base_cli();
        cli.workspace_roots.clear();

        let config = resolve_startup_config(&cli, RuntimeTransportKind::LoopbackHttp)
            .expect("startup config should allow empty workspace roots for serving");
        assert!(config.workspace_roots.is_empty());
        assert_eq!(config.watch.mode, WatchMode::Auto);
    }

    #[test]
    fn command_config_defaults_empty_workspace_roots_to_current_directory() {
        let mut cli = base_cli();
        cli.workspace_roots.clear();

        let config = resolve_command_config(&cli, Command::Init)
            .expect("utility commands should default to the current directory");
        assert_eq!(config.workspace_roots, vec![PathBuf::from(".")]);
    }

    #[test]
    fn index_command_defaults_empty_workspace_roots_to_current_directory() {
        let mut cli = base_cli();
        cli.workspace_roots.clear();

        let config = resolve_command_config(&cli, Command::Index { changed: true })
            .expect("index command should default to the current directory");
        assert_eq!(config.workspace_roots, vec![PathBuf::from(".")]);
    }

    #[test]
    fn stdio_watch_runtime_config_keeps_empty_startup_roots() {
        let config = FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid");
        let watch_config = resolve_watch_runtime_config(&config, RuntimeTransportKind::Stdio)
            .expect("stdio watch runtime config should preserve empty startup roots");
        assert!(watch_config.workspace_roots.is_empty());
    }

    #[test]
    fn http_watch_runtime_config_keeps_empty_startup_roots() {
        let config = FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid");
        let watch_config =
            resolve_watch_runtime_config(&config, RuntimeTransportKind::LoopbackHttp)
                .expect("http watch runtime config should preserve empty startup roots");
        assert!(watch_config.workspace_roots.is_empty());
    }

    #[test]
    fn stdio_watch_runtime_config_preserves_existing_startup_roots() {
        let workspace_root = temp_workspace_root("watch-runtime-existing-root");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::create_dir_all(workspace_root.join(".git"))
            .expect("workspace git marker should be creatable");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        let watch_config = resolve_watch_runtime_config(&config, RuntimeTransportKind::Stdio)
            .expect("stdio watch runtime config should preserve explicit startup roots");
        assert_eq!(watch_config.workspace_roots, vec![workspace_root.clone()]);

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn semantic_startup_gate_skips_validation_when_disabled() {
        let config = FriggConfig::from_workspace_roots(vec![PathBuf::from(".")])
            .expect("config should load for semantic disabled gate check");
        let credentials = SemanticRuntimeCredentials::default();
        run_semantic_runtime_startup_gate_with_credentials(&config, &credentials)
            .expect("semantic startup gate should no-op when disabled");
    }

    #[test]
    fn semantic_startup_gate_rejects_blank_credentials() {
        let mut config = FriggConfig::from_workspace_roots(vec![PathBuf::from(".")])
            .expect("config should load from workspace root");
        config.semantic_runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode: false,
        };

        let error = run_semantic_runtime_startup_gate_with_credentials(
            &config,
            &SemanticRuntimeCredentials {
                openai_api_key: Some("   ".to_owned()),
                gemini_api_key: None,
            },
        )
        .expect_err("semantic gate must fail when provider credentials are blank");
        assert!(
            error
                .to_string()
                .contains("startup semantic runtime readiness failed code=invalid_params"),
            "unexpected semantic startup error: {error}"
        );
        assert!(
            error.to_string().contains("OPENAI_API_KEY"),
            "unexpected semantic startup error detail: {error}"
        );
    }

    #[test]
    fn semantic_startup_gate_fails_on_missing_credentials() {
        let mut config = FriggConfig::from_workspace_roots(vec![PathBuf::from(".")])
            .expect("config should load from workspace root");
        config.semantic_runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode: false,
        };

        let error = run_semantic_runtime_startup_gate_with_credentials(
            &config,
            &SemanticRuntimeCredentials::default(),
        )
        .expect_err("semantic gate must fail when provider credentials are missing");
        assert!(
            error
                .to_string()
                .contains("startup semantic runtime readiness failed code=invalid_params"),
            "unexpected semantic startup error: {error}"
        );
        assert!(
            error.to_string().contains("OPENAI_API_KEY"),
            "unexpected semantic startup error detail: {error}"
        );
    }

    #[test]
    fn semantic_runtime_startup_gate_accepts_local_provider_without_credentials() {
        let mut config = FriggConfig::from_workspace_roots(vec![PathBuf::from(".")])
            .expect("config should load from workspace root");
        config.semantic_runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::Local),
            model: None,
            strict_mode: false,
        };

        run_semantic_runtime_startup_gate_with_credentials(
            &config,
            &SemanticRuntimeCredentials::default(),
        )
        .expect("local semantic runtime should not require API credentials");
    }

    #[test]
    fn semantic_runtime_startup_gate_keeps_google_credential_requirement() {
        let mut config = FriggConfig::from_workspace_roots(vec![PathBuf::from(".")])
            .expect("config should load from workspace root");
        config.semantic_runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::Google),
            model: None,
            strict_mode: false,
        };

        let error = run_semantic_runtime_startup_gate_with_credentials(
            &config,
            &SemanticRuntimeCredentials::default(),
        )
        .expect_err("google semantic runtime must still require Gemini credentials");
        assert!(
            error.to_string().contains("GEMINI_API_KEY"),
            "unexpected semantic startup error detail: {error}"
        );

        run_semantic_runtime_startup_gate_with_credentials(
            &config,
            &SemanticRuntimeCredentials {
                openai_api_key: None,
                gemini_api_key: Some("test-gemini-key".to_owned()),
            },
        )
        .expect("google semantic runtime should accept a valid Gemini credential");
    }

    #[test]
    fn semantic_startup_gate_accepts_valid_credentials() {
        let mut config = FriggConfig::from_workspace_roots(vec![PathBuf::from(".")])
            .expect("config should load from workspace root");
        config.semantic_runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: None,
            strict_mode: true,
        };

        run_semantic_runtime_startup_gate_with_credentials(
            &config,
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
        )
        .expect("semantic gate should pass with a valid provider key");
    }

    #[test]
    fn startup_gate_rejects_uninitialized_vector_store() {
        let workspace_root = temp_workspace_root("startup-uninitialized");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::create_dir_all(workspace_root.join(".git"))
            .expect("workspace git marker should be creatable");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        let error = run_strict_startup_vector_readiness_gate(&config)
            .expect_err("startup gate must fail when vector store is uninitialized");
        assert!(
            error
                .to_string()
                .contains("startup strict vector readiness failed"),
            "unexpected startup gate error: {error}"
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn startup_gate_rejects_legacy_non_sqlite_vec_schema() {
        let workspace_root = temp_workspace_root("startup-fallback");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::create_dir_all(workspace_root.join(".git"))
            .expect("workspace git marker should be creatable");
        let db_dir = workspace_root.join(PROVENANCE_STORAGE_DIR);
        fs::create_dir_all(&db_dir).expect("storage directory should be creatable");
        let db_path = db_dir.join(PROVENANCE_STORAGE_DB_FILE);

        let conn = Connection::open(&db_path)
            .expect("fallback fixture sqlite db should open successfully");
        conn.execute_batch(
            r#"
            CREATE TABLE embedding_vectors (
              embedding_id TEXT PRIMARY KEY,
              embedding BLOB NOT NULL,
              dimensions INTEGER NOT NULL,
              created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .expect("legacy vector table should be creatable");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        let error = run_strict_startup_vector_readiness_gate(&config)
            .expect_err("startup gate must fail when non-sqlite-vec schema is active");
        assert!(
            error.to_string().contains("storage schema is incompatible"),
            "unexpected startup gate incompatible-schema error: {error}"
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn startup_gate_repairs_semantic_vector_partition_drift() {
        let workspace_root = temp_workspace_root("startup-vector-auto-repair");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        run_storage_init_command(&config)
            .expect("init should create storage before startup repair");
        let db_path = resolve_storage_db_path(&workspace_root, "startup")
            .expect("storage db path should resolve after init");
        seed_semantic_vector_drift(&db_path);

        run_strict_startup_vector_readiness_gate(&config)
            .expect("startup gate should auto-repair semantic vector drift");

        let storage = Storage::new(&db_path);
        let repaired = storage
            .collect_semantic_storage_health_for_repository_model(
                "repo-001",
                "openai",
                "text-embedding-3-small",
            )
            .expect("semantic health should be readable after startup repair");
        assert!(repaired.vector_consistent);

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn storage_db_path_for_write_creates_parent_dir() {
        let workspace_root = temp_workspace_root("storage-db-write-path");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        let canonical_root = workspace_root
            .canonicalize()
            .expect("workspace root should canonicalize");

        let db_path = ensure_storage_db_path_for_write(&workspace_root, "init")
            .expect("storage db path for write should resolve");
        assert!(
            db_path.starts_with(&canonical_root),
            "storage db path should stay inside the canonical workspace root"
        );
        assert!(
            db_path.ends_with(Path::new(PROVENANCE_STORAGE_DIR).join(PROVENANCE_STORAGE_DB_FILE)),
            "storage db path should use the Frigg storage suffix"
        );
        assert!(
            db_path
                .parent()
                .expect("db path should have a parent directory")
                .is_dir(),
            "storage db parent directory should be created"
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn storage_maintenance_rejects_incompatible_schema_before_writes() {
        let workspace_root = temp_workspace_root("storage-maintenance-incompatible-schema");
        create_simple_workspace(&workspace_root);
        let db_path = ensure_storage_db_path_for_write(&workspace_root, "storage-maintenance")
            .expect("storage db path should be creatable for incompatible-schema fixture");
        let conn = Connection::open(&db_path).expect("incompatible fixture sqlite db should open");
        conn.execute_batch(
            r#"
            CREATE TABLE schema_version (
              id INTEGER PRIMARY KEY CHECK (id = 1),
              version INTEGER NOT NULL,
              updated_at TEXT NOT NULL
            );
            INSERT INTO schema_version (id, version, updated_at)
            VALUES (1, 10, CURRENT_TIMESTAMP);
            "#,
        )
        .expect("incompatible schema version should seed");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        let prune_error = run_storage_maintenance_command(
            &config,
            StorageMaintenanceCommand::Prune {
                keep_manifest_snapshots: 1,
            },
        )
        .expect_err("prune-storage should reject incompatible storage before pruning");
        assert!(
            prune_error.to_string().contains("schema is incompatible"),
            "unexpected prune incompatible-schema error: {prune_error}"
        );

        let repair_error = run_storage_maintenance_command(
            &config,
            StorageMaintenanceCommand::RepairSemanticVectorStore,
        )
        .expect_err("repair-storage should reject incompatible storage before repair");
        assert!(
            repair_error.to_string().contains("schema is incompatible"),
            "unexpected repair incompatible-schema error: {repair_error}"
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn storage_db_path_resolution_wraps_missing_workspace_root() {
        let workspace_root = temp_workspace_root("storage-db-missing-root");

        let error = resolve_storage_db_path(&workspace_root, "init")
            .expect_err("storage db path resolution should fail for a missing workspace root");
        let message = error.to_string();
        assert!(
            message.contains("error init: failed status=failed"),
            "unexpected storage db path error: {message}"
        );
        assert!(
            message.contains(&format!("root={}", workspace_root.display())),
            "unexpected storage db path error: {message}"
        );
        assert!(
            message.contains("failed to canonicalize workspace root"),
            "unexpected storage db path error: {message}"
        );
    }

    #[test]
    fn storage_init_reports_workspace_path_resolution_failure() {
        let workspace_root = temp_workspace_root("storage-init-missing-root");
        let config = FriggConfig {
            workspace_roots: vec![workspace_root.clone()],
            ..FriggConfig::default()
        };

        let error = run_storage_init_command(&config)
            .expect_err("init bootstrap should fail for a missing workspace root");
        let message = error.to_string();
        assert!(
            message.contains("error init: failed status=failed"),
            "unexpected init bootstrap error: {message}"
        );
        assert!(
            message.contains(&format!("root={}", workspace_root.display())),
            "unexpected init bootstrap error: {message}"
        );
        assert!(
            message.contains("failed to canonicalize workspace root"),
            "unexpected init bootstrap error: {message}"
        );
    }

    #[test]
    fn storage_maintenance_reports_missing_db_without_creating_file() {
        let workspace_root = temp_workspace_root("storage-maintenance-missing-db");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::create_dir_all(workspace_root.join(".git"))
            .expect("workspace git marker should be creatable");
        let db_path = resolve_storage_db_path(&workspace_root, "repair-storage")
            .expect("storage db path should resolve for an existing workspace root");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        let error = run_storage_maintenance_command(
            &config,
            StorageMaintenanceCommand::RepairSemanticVectorStore,
        )
        .expect_err("repair-storage should fail when the storage db file is missing");
        let message = error.to_string();
        assert!(
            message.contains("repair-storage failed for repository_id=repo-001"),
            "unexpected repair-storage error: {message}"
        );
        assert!(
            message.contains("storage db file is missing"),
            "unexpected repair-storage error: {message}"
        );
        assert!(
            !db_path.exists(),
            "repair-storage should not create a missing storage db while reporting the error"
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn storage_init_verifies_simple_workspace() {
        let workspace_root = temp_workspace_root("storage-bootstrap-success");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");

        run_storage_init_command(&config).expect("init should succeed for a simple workspace");

        let db_path = resolve_storage_db_path(&workspace_root, "init")
            .expect("storage db path should resolve after init");
        assert!(db_path.is_file(), "storage db should exist after init");

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn storage_init_repairs_mismatched_vector_table() {
        let workspace_root = temp_workspace_root("storage-init-vector-repair");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        let db_path = ensure_storage_db_path_for_write(&workspace_root, "init")
            .expect("storage db path should be creatable");
        let storage = Storage::new(&db_path);
        storage
            .initialize()
            .expect("storage should initialize before vector corruption");
        corrupt_vector_table_schema(&db_path);

        let error = storage
            .verify_vector_store(DEFAULT_VECTOR_DIMENSIONS)
            .expect_err("mismatched vector table should fail before init repair");
        assert!(
            error.to_string().contains("vector table schema mismatch"),
            "unexpected vector mismatch error: {error}"
        );

        run_storage_init_command(&config).expect("init should repair mismatched vector table");
        storage
            .verify()
            .expect("storage should verify after init vector repair");

        let conn = Connection::open(&db_path).expect("storage db should open for missing fixture");
        conn.execute_batch(&format!("DROP TABLE IF EXISTS {VECTOR_TABLE_NAME};"))
            .expect("vector table should drop for missing fixture");
        drop(conn);
        let error = storage
            .verify_vector_store(DEFAULT_VECTOR_DIMENSIONS)
            .expect_err("missing vector table should fail before init repair");
        assert!(
            error.to_string().contains("missing vector table"),
            "unexpected missing vector table error: {error}"
        );

        run_storage_init_command(&config).expect("init should repair missing vector table");
        storage
            .verify()
            .expect("storage should verify after missing vector repair");

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn index_command_succeeds_for_simple_workspace() {
        let workspace_root = temp_workspace_root("index-success");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        run_index_command(&config, false)
            .expect("full index should succeed for a simple workspace");
        run_index_command(&config, true)
            .expect("changed-only index should succeed for a simple workspace");

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn index_command_repairs_semantic_vectors_before_refresh() {
        let workspace_root = temp_workspace_root("index-vector-auto-repair");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        run_storage_init_command(&config).expect("init should create storage before index repair");
        let db_path = resolve_storage_db_path(&workspace_root, "index")
            .expect("storage db path should resolve after init");
        seed_semantic_vector_drift(&db_path);

        run_index_command(&config, false).expect("index should auto-repair vector drift");

        let storage = Storage::new(&db_path);
        let repaired = storage
            .collect_semantic_storage_health_for_repository_model(
                "repo-001",
                "openai",
                "text-embedding-3-small",
            )
            .expect("semantic health should be readable after index repair");
        assert!(repaired.vector_consistent);

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn prune_storage_command_prunes_manifest_history() {
        let workspace_root = temp_workspace_root("prune-storage-success");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        run_storage_init_command(&config).expect("init should succeed before storage pruning");
        let db_path = resolve_storage_db_path(&workspace_root, "prune-storage")
            .expect("storage db path should resolve after init");
        let storage = Storage::new(&db_path);
        for idx in 1..=3 {
            storage
                .upsert_manifest(
                    "repo-001",
                    &format!("snapshot-00{idx}"),
                    &[frigg::storage::ManifestEntry {
                        path: "src/main.rs".to_owned(),
                        sha256: format!("hash-{idx}"),
                        size_bytes: 10 + idx as u64,
                        mtime_ns: Some(100 + idx as u64),
                    }],
                )
                .expect("manifest snapshots should seed before prune");
        }
        run_storage_maintenance_command(
            &config,
            StorageMaintenanceCommand::Prune {
                keep_manifest_snapshots: 1,
            },
        )
        .expect("prune-storage command should succeed");

        assert!(
            storage
                .load_manifest_for_snapshot("snapshot-003")
                .expect("latest manifest snapshot should remain readable")
                .len()
                == 1
        );
        assert!(
            storage
                .load_manifest_for_snapshot("snapshot-001")
                .expect("oldest manifest snapshot lookup should succeed")
                .is_empty()
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn repair_storage_command_rebuilds_semantic_vectors() {
        let workspace_root = temp_workspace_root("repair-storage-success");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        run_storage_init_command(&config).expect("init should succeed before storage repair");
        let db_path = resolve_storage_db_path(&workspace_root, "repair-storage")
            .expect("storage db path should resolve after init");
        let storage = Storage::new(&db_path);
        storage
            .upsert_manifest(
                "repo-001",
                "snapshot-001",
                &[frigg::storage::ManifestEntry {
                    path: "src/main.rs".to_owned(),
                    sha256: "hash-main".to_owned(),
                    size_bytes: 42,
                    mtime_ns: Some(100),
                }],
            )
            .expect("manifest snapshot should seed before repair");
        storage
            .replace_semantic_embeddings_for_repository(
                "repo-001",
                "snapshot-001",
                "openai",
                "text-embedding-3-small",
                &[semantic_record("snapshot-001")],
            )
            .expect("semantic rows should seed before repair");

        let conn = Connection::open(&db_path).expect("storage db should open for repair fixture");
        conn.execute(
            "DELETE FROM embedding_vectors WHERE repository_id = ?1 AND provider = ?2 AND model = ?3 AND chunk_id = ?4",
            (
                "repo-001",
                "openai",
                "text-embedding-3-small",
                "chunk-main",
            ),
        )
        .expect("vector row corruption fixture should succeed");

        let broken = storage
            .collect_semantic_storage_health_for_repository_model(
                "repo-001",
                "openai",
                "text-embedding-3-small",
            )
            .expect("broken semantic health should be readable");
        assert!(!broken.vector_consistent);

        run_storage_maintenance_command(
            &config,
            StorageMaintenanceCommand::RepairSemanticVectorStore,
        )
        .expect("repair-storage command should succeed");

        let repaired = storage
            .collect_semantic_storage_health_for_repository_model(
                "repo-001",
                "openai",
                "text-embedding-3-small",
            )
            .expect("repaired semantic health should be readable");
        assert!(repaired.vector_consistent);

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn find_enclosing_git_root_returns_matching_ancestor() {
        let workspace_root = temp_workspace_root("git-root-match");
        let nested = workspace_root.join("nested").join("deeper");
        fs::create_dir_all(workspace_root.join(".git"))
            .expect("git directory marker should be creatable");
        fs::create_dir_all(&nested).expect("nested workspace path should be creatable");

        assert_eq!(
            find_enclosing_git_root(&nested),
            Some(workspace_root.clone())
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn find_enclosing_git_root_returns_none_without_git_marker() {
        let workspace_root = temp_workspace_root("git-root-miss");
        let nested = workspace_root.join("nested").join("deeper");
        fs::create_dir_all(&nested).expect("nested workspace path should be creatable");

        assert_eq!(find_enclosing_git_root(&nested), None);

        cleanup_workspace(&workspace_root);
    }

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "frigg-cli-{test_name}-{}-{now}",
            std::process::id()
        ))
    }

    fn create_simple_workspace(root: &Path) {
        fs::create_dir_all(root.join(".git")).expect("workspace git marker should be creatable");
        fs::create_dir_all(root.join("src")).expect("workspace src directory should be creatable");
        fs::write(root.join("README.md"), "hello from frigg\n")
            .expect("workspace readme should be writable");
        fs::write(
            root.join("src/main.rs"),
            "fn main() { println!(\"hello from frigg\"); }\n",
        )
        .expect("workspace main source should be writable");
    }

    fn semantic_record(snapshot_id: &str) -> SemanticChunkEmbeddingRecord {
        SemanticChunkEmbeddingRecord {
            chunk_id: "chunk-main".to_owned(),
            repository_id: "repo-001".to_owned(),
            snapshot_id: snapshot_id.to_owned(),
            path: "src/main.rs".to_owned(),
            language: "rust".to_owned(),
            chunk_index: 0,
            start_line: 1,
            end_line: 1,
            provider: "openai".to_owned(),
            model: "text-embedding-3-small".to_owned(),
            trace_id: Some("trace-main".to_owned()),
            content_hash_blake3: "hash-main".to_owned(),
            content_text: "fn main() { println!(\"hello from frigg\"); }".to_owned(),
            embedding: vec![0.25, 0.75],
        }
    }

    fn corrupt_vector_table_schema(db_path: &Path) {
        let conn = Connection::open(db_path).expect("storage db should open for vector corruption");
        conn.execute_batch(&format!("DROP TABLE IF EXISTS {VECTOR_TABLE_NAME};"))
            .expect("vector table should drop for corruption fixture");
        conn.execute_batch(&format!(
            "CREATE TABLE {VECTOR_TABLE_NAME} (embedding float[{}] NOT NULL);",
            DEFAULT_VECTOR_DIMENSIONS + 1
        ))
        .expect("mismatched vector table should be creatable");
    }

    fn seed_semantic_vector_drift(db_path: &Path) {
        let storage = Storage::new(db_path);
        storage
            .upsert_manifest(
                "repo-001",
                "snapshot-001",
                &[frigg::storage::ManifestEntry {
                    path: "src/main.rs".to_owned(),
                    sha256: "hash-main".to_owned(),
                    size_bytes: 42,
                    mtime_ns: Some(100),
                }],
            )
            .expect("manifest snapshot should seed before vector drift");
        storage
            .replace_semantic_embeddings_for_repository(
                "repo-001",
                "snapshot-001",
                "openai",
                "text-embedding-3-small",
                &[semantic_record("snapshot-001")],
            )
            .expect("semantic rows should seed before vector drift");

        let conn = Connection::open(db_path).expect("storage db should open for drift fixture");
        conn.execute(
            &format!(
                "DELETE FROM {VECTOR_TABLE_NAME} WHERE repository_id = ?1 AND provider = ?2 AND model = ?3 AND chunk_id = ?4"
            ),
            (
                "repo-001",
                "openai",
                "text-embedding-3-small",
                "chunk-main",
            ),
        )
        .expect("vector row drift fixture should succeed");

        let broken = storage
            .collect_semantic_storage_health_for_repository_model(
                "repo-001",
                "openai",
                "text-embedding-3-small",
            )
            .expect("broken semantic health should be readable");
        assert!(!broken.vector_consistent);
    }

    fn cleanup_workspace(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
