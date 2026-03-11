use std::error::Error;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use clap::{Parser, Subcommand};
use frigg::indexer::{ManifestDiagnosticKind, ReindexMode, reindex_repository_with_runtime_config};
use frigg::mcp::{FriggMcpServer, RuntimeTaskRegistry};
use frigg::playbooks::run_hybrid_playbook_regressions;
use frigg::searcher::{TextSearcher, ValidatedManifestCandidateCache};
use frigg::settings::{
    FriggConfig, RuntimeTransportKind, SemanticRuntimeConfig, SemanticRuntimeCredentials,
    SemanticRuntimeProvider, SemanticRuntimeStartupError, WatchConfig, WatchMode,
    runtime_profile_for_transport,
};
use frigg::storage::{
    DEFAULT_VECTOR_DIMENSIONS, Storage, VectorStoreBackend, ensure_provenance_db_parent_dir,
    resolve_provenance_db_path,
};
use frigg::watch::maybe_start_watch_runtime;
use rmcp::transport::StreamableHttpServerConfig;
use serde_json::to_string_pretty;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

mod cli_runtime;
mod http_runtime;
use cli_runtime::{
    StorageBootstrapCommand, resolve_command_config, resolve_startup_config,
    resolve_watch_runtime_config, run_hybrid_playbook_command, run_reindex_command,
    run_semantic_runtime_startup_gate, run_storage_bootstrap_command,
    run_strict_startup_vector_readiness_gate,
};
#[cfg(test)]
use cli_runtime::{
    ensure_storage_db_path_for_write, find_enclosing_git_root, resolve_semantic_runtime_config,
    resolve_storage_db_path, resolve_watch_config,
    run_semantic_runtime_startup_gate_with_credentials,
};
use http_runtime::{HttpRuntimeConfig, resolve_http_runtime_config, serve_http};
#[cfg(test)]
use http_runtime::{
    allowed_authorities_for_bind, authority_allowed, constant_time_equals, host_header_allowed,
    origin_header_allowed, parse_host_authority, parse_origin_authority,
    typed_access_denied_response,
};

#[derive(Debug, Parser)]
#[command(name = "frigg", version, about = "Frigg MCP server")]
struct Cli {
    #[arg(long = "workspace-root", value_name = "PATH", global = true)]
    workspace_roots: Vec<PathBuf>,

    #[arg(
        long = "max-file-bytes",
        value_name = "BYTES",
        env = "FRIGG_MAX_FILE_BYTES",
        global = true
    )]
    max_file_bytes: Option<usize>,

    #[arg(long, value_name = "PORT", global = true)]
    mcp_http_port: Option<u16>,

    #[arg(long, value_name = "HOST", global = true)]
    mcp_http_host: Option<IpAddr>,

    #[arg(long, global = true)]
    allow_remote_http: bool,

    #[arg(
        long,
        value_name = "TOKEN",
        env = "FRIGG_MCP_HTTP_AUTH_TOKEN",
        hide_env_values = true,
        global = true
    )]
    mcp_http_auth_token: Option<String>,

    #[arg(
        long,
        value_name = "BOOL",
        env = "FRIGG_SEMANTIC_RUNTIME_ENABLED",
        global = true
    )]
    semantic_runtime_enabled: Option<bool>,

    #[arg(
        long,
        value_name = "PROVIDER",
        env = "FRIGG_SEMANTIC_RUNTIME_PROVIDER",
        global = true
    )]
    semantic_runtime_provider: Option<SemanticRuntimeProvider>,

    #[arg(
        long,
        value_name = "MODEL",
        env = "FRIGG_SEMANTIC_RUNTIME_MODEL",
        global = true
    )]
    semantic_runtime_model: Option<String>,

    #[arg(
        long,
        value_name = "BOOL",
        env = "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE",
        global = true
    )]
    semantic_runtime_strict_mode: Option<bool>,

    #[arg(long, value_name = "MODE", env = "FRIGG_WATCH_MODE", global = true)]
    watch_mode: Option<WatchMode>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        env = "FRIGG_WATCH_DEBOUNCE_MS",
        global = true
    )]
    watch_debounce_ms: Option<u64>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        env = "FRIGG_WATCH_RETRY_MS",
        global = true
    )]
    watch_retry_ms: Option<u64>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
enum Command {
    /// Initialize storage schema for each workspace root.
    Init,
    /// Verify storage schema and read/write sanity for each workspace root.
    Verify,
    /// Reindex all files and persist an updated manifest snapshot.
    Reindex {
        /// Reindex changed files only using persisted manifest delta.
        #[arg(long, default_value_t = false)]
        changed: bool,
    },
    /// Execute markdown hybrid playbooks against the selected workspace root(s).
    PlaybookHybridRun {
        /// Directory containing executable markdown playbooks.
        #[arg(long = "playbooks-root", value_name = "PATH")]
        playbooks_root: PathBuf,
        /// Enforce target witness groups in addition to required witness groups.
        #[arg(long, default_value_t = false)]
        enforce_targets: bool,
        /// Optional path for pretty JSON summary output.
        #[arg(long, value_name = "PATH")]
        output: Option<PathBuf>,
    },
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let cli = Cli::parse();
    let http_runtime = resolve_http_runtime_config(&cli)?;
    let transport_kind = http_runtime
        .as_ref()
        .map(HttpRuntimeConfig::transport_kind)
        .unwrap_or(RuntimeTransportKind::Stdio);
    init_tracing(default_tracing_filter(&cli, transport_kind));

    if let Some(command) = cli.command.clone() {
        match command.clone() {
            Command::Init => {
                let config = resolve_command_config(&cli, command)?;
                run_storage_bootstrap_command(&config, StorageBootstrapCommand::Init)?
            }
            Command::Verify => {
                let config = resolve_command_config(&cli, command)?;
                run_storage_bootstrap_command(&config, StorageBootstrapCommand::Verify)?
            }
            Command::Reindex { changed } => {
                let config = resolve_command_config(&cli, command)?;
                run_semantic_runtime_startup_gate(&config)?;
                run_reindex_command(&config, changed)?
            }
            Command::PlaybookHybridRun {
                playbooks_root,
                enforce_targets,
                output,
            } => {
                let config = resolve_command_config(&cli, command)?;
                run_semantic_runtime_startup_gate(&config)?;
                run_hybrid_playbook_command(
                    &config,
                    &playbooks_root,
                    enforce_targets,
                    output.as_deref(),
                )?
            }
        }
        return Ok(());
    }

    let config = resolve_startup_config(&cli, transport_kind)?;
    run_strict_startup_vector_readiness_gate(&config)?;
    run_semantic_runtime_startup_gate(&config)?;
    let watch_runtime_config = resolve_watch_runtime_config(&config, transport_kind)?;
    let runtime_task_registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
    let validated_manifest_candidate_cache =
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    let _watch_runtime = maybe_start_watch_runtime(
        &watch_runtime_config,
        transport_kind,
        Arc::clone(&runtime_task_registry),
        Arc::clone(&validated_manifest_candidate_cache),
    )?;
    let runtime_watch_active = _watch_runtime.is_some();
    let runtime_profile = runtime_profile_for_transport(transport_kind, runtime_watch_active);

    let server = FriggMcpServer::new_with_runtime(
        config,
        runtime_profile,
        runtime_watch_active,
        runtime_task_registry,
        validated_manifest_candidate_cache,
    );
    if let Some(runtime) = http_runtime {
        serve_http(runtime, server).await?;
    } else {
        server.auto_attach_stdio_default_workspace_from_current_dir()?;
        server.serve_stdio().await?;
    }

    Ok(())
}

fn default_tracing_filter(cli: &Cli, transport: RuntimeTransportKind) -> &'static str {
    if cli.command.is_none() && transport == RuntimeTransportKind::Stdio {
        "error"
    } else {
        "info"
    }
}

fn init_tracing(default_filter: &str) {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_filter));
    // MCP stdio transport requires stdout to carry protocol frames only.
    // Force tracing output to stderr so logs never corrupt stdio framing.
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
    use frigg::storage::{PROVENANCE_STORAGE_DB_FILE, PROVENANCE_STORAGE_DIR};
    use rusqlite::Connection;

    fn base_cli() -> Cli {
        Cli {
            workspace_roots: vec![PathBuf::from(".")],
            max_file_bytes: None,
            mcp_http_port: None,
            mcp_http_host: None,
            allow_remote_http: false,
            mcp_http_auth_token: None,
            semantic_runtime_enabled: None,
            semantic_runtime_provider: None,
            semantic_runtime_model: None,
            semantic_runtime_strict_mode: None,
            watch_mode: None,
            watch_debounce_ms: None,
            watch_retry_ms: None,
            command: None,
        }
    }

    #[test]
    fn transport_defaults_to_stdio_when_http_port_absent() {
        let cli = base_cli();
        let runtime = resolve_http_runtime_config(&cli).expect("stdio mode should resolve");
        assert!(runtime.is_none());
    }

    #[test]
    fn transport_http_defaults_to_loopback_bind() {
        let mut cli = base_cli();
        cli.mcp_http_port = Some(4000);
        cli.mcp_http_auth_token = Some("test-token".to_owned());

        let runtime = resolve_http_runtime_config(&cli)
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
    fn transport_rejects_http_flags_without_port() {
        let mut cli = base_cli();
        cli.mcp_http_host = Some(IpAddr::V4(Ipv4Addr::LOCALHOST));

        let error =
            resolve_http_runtime_config(&cli).expect_err("host flag without port must fail");
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

        let error = resolve_http_runtime_config(&cli)
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

        let error = resolve_http_runtime_config(&cli)
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

        let runtime = resolve_http_runtime_config(&cli)
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

        let runtime = resolve_http_runtime_config(&cli)
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

        let error = resolve_http_runtime_config(&cli).expect_err("blank auth token should fail");
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
    fn watch_runtime_defaults_to_off_for_stdio_with_standard_timers() {
        let cli = base_cli();
        let watch = resolve_watch_config(&cli, Some(RuntimeTransportKind::Stdio));
        assert_eq!(watch.mode, WatchMode::Off);
        assert_eq!(watch.debounce_ms, 750);
        assert_eq!(watch.retry_ms, 5_000);
    }

    #[test]
    fn watch_runtime_defaults_to_auto_for_http_with_standard_timers() {
        let cli = base_cli();
        let watch = resolve_watch_config(&cli, Some(RuntimeTransportKind::LoopbackHttp));
        assert_eq!(watch.mode, WatchMode::Auto);
        assert_eq!(watch.debounce_ms, 750);
        assert_eq!(watch.retry_ms, 5_000);
    }

    #[test]
    fn watch_runtime_cli_resolution_applies_explicit_values() {
        let mut cli = base_cli();
        cli.watch_mode = Some(WatchMode::On);
        cli.watch_debounce_ms = Some(1_250);
        cli.watch_retry_ms = Some(9_000);

        let watch = resolve_watch_config(&cli, Some(RuntimeTransportKind::Stdio));
        assert_eq!(watch.mode, WatchMode::On);
        assert_eq!(watch.debounce_ms, 1_250);
        assert_eq!(watch.retry_ms, 9_000);
    }

    #[test]
    fn watch_runtime_transport_kind_matches_http_runtime() {
        let cli = base_cli();
        assert_eq!(
            resolve_http_runtime_config(&cli)
                .expect("stdio should resolve")
                .as_ref()
                .map(HttpRuntimeConfig::transport_kind)
                .unwrap_or(RuntimeTransportKind::Stdio),
            RuntimeTransportKind::Stdio
        );

        let mut loopback_cli = base_cli();
        loopback_cli.mcp_http_port = Some(4011);
        let loopback_runtime = resolve_http_runtime_config(&loopback_cli)
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
        let remote_runtime = resolve_http_runtime_config(&remote_cli)
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
            "info"
        );
    }

    #[test]
    fn utility_commands_keep_info_log_filter() {
        let mut cli = base_cli();
        cli.command = Some(Command::Reindex { changed: false });
        assert_eq!(
            default_tracing_filter(&cli, RuntimeTransportKind::Stdio),
            "info"
        );
    }

    #[test]
    fn startup_config_rejects_invalid_semantic_runtime_contract() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_model = Some("text-embedding-3-small".to_owned());

        let error = resolve_startup_config(&cli, RuntimeTransportKind::Stdio)
            .expect_err("startup config should reject enabled semantic runtime without provider");
        assert!(
            error
                .to_string()
                .contains("semantic_runtime.provider is required"),
            "unexpected startup config error: {error}"
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
    }

    #[test]
    fn reindex_command_resolution_uses_startup_semantic_config() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Google);

        let config = resolve_command_config(&cli, Command::Reindex { changed: true })
            .expect("reindex command should resolve startup config");
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
    fn verify_command_resolution_keeps_semantic_runtime_unset() {
        let mut cli = base_cli();
        cli.semantic_runtime_enabled = Some(true);
        cli.semantic_runtime_provider = Some(SemanticRuntimeProvider::Google);

        let config = resolve_command_config(&cli, Command::Verify)
            .expect("verify command should resolve base config");
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
    fn startup_config_allows_empty_workspace_roots_for_http_serving() {
        let mut cli = base_cli();
        cli.workspace_roots.clear();

        let config = resolve_startup_config(&cli, RuntimeTransportKind::LoopbackHttp)
            .expect("startup config should allow empty workspace roots for serving");
        assert!(config.workspace_roots.is_empty());
        assert_eq!(config.watch.mode, WatchMode::Auto);
    }

    #[test]
    fn command_config_rejects_empty_workspace_roots() {
        let mut cli = base_cli();
        cli.workspace_roots.clear();

        let error = resolve_command_config(&cli, Command::Init)
            .expect_err("utility commands should still require at least one workspace root");
        assert!(
            error
                .to_string()
                .contains("at least one workspace root is required"),
            "unexpected command config error: {error}"
        );
    }

    #[test]
    fn stdio_watch_runtime_config_uses_current_workspace_when_startup_roots_are_empty() {
        let config = FriggConfig::from_optional_workspace_roots(Vec::new())
            .expect("empty serving config should be valid");
        let watch_config = resolve_watch_runtime_config(&config, RuntimeTransportKind::Stdio)
            .expect("stdio watch runtime config should resolve current workspace");
        assert_eq!(watch_config.workspace_roots.len(), 1);
        assert!(
            watch_config.workspace_roots[0].exists(),
            "resolved stdio watch workspace root should exist"
        );
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
        let db_dir = workspace_root.join(PROVENANCE_STORAGE_DIR);
        fs::create_dir_all(&db_dir).expect("provenance directory should be creatable");
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
            error
                .to_string()
                .contains("legacy non-sqlite-vec schema detected"),
            "unexpected startup gate legacy-schema error: {error}"
        );

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
            "storage db path should use the provenance storage suffix"
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
    fn storage_db_path_resolution_wraps_missing_workspace_root() {
        let workspace_root = temp_workspace_root("storage-db-missing-root");

        let error = resolve_storage_db_path(&workspace_root, "verify")
            .expect_err("storage db path resolution should fail for a missing workspace root");
        let message = error.to_string();
        assert!(
            message.contains("verify summary status=failed"),
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
    fn storage_bootstrap_init_reports_workspace_path_resolution_failure() {
        let workspace_root = temp_workspace_root("storage-init-missing-root");
        let config = FriggConfig {
            workspace_roots: vec![workspace_root.clone()],
            ..FriggConfig::default()
        };

        let error = run_storage_bootstrap_command(&config, StorageBootstrapCommand::Init)
            .expect_err("init bootstrap should fail for a missing workspace root");
        let message = error.to_string();
        assert!(
            message.contains("init summary status=failed"),
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
    fn storage_bootstrap_verify_reports_missing_db_file() {
        let workspace_root = temp_workspace_root("storage-verify-missing-db");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        let db_path = resolve_storage_db_path(&workspace_root, "verify")
            .expect("storage db path should resolve for an existing workspace root");

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        let error = run_storage_bootstrap_command(&config, StorageBootstrapCommand::Verify)
            .expect_err("verify bootstrap should fail when the storage db file is missing");
        let message = error.to_string();
        assert!(
            message.contains("verify failed for repository_id=repo-001"),
            "unexpected verify bootstrap error: {message}"
        );
        assert!(
            message.contains(&format!("root={}", workspace_root.display())),
            "unexpected verify bootstrap error: {message}"
        );
        assert!(
            message.contains(&format!("db={}", db_path.display())),
            "unexpected verify bootstrap error: {message}"
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn storage_bootstrap_init_and_verify_succeed_for_simple_workspace() {
        let workspace_root = temp_workspace_root("storage-bootstrap-success");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");

        run_storage_bootstrap_command(&config, StorageBootstrapCommand::Init)
            .expect("init bootstrap should succeed for a simple workspace");
        run_storage_bootstrap_command(&config, StorageBootstrapCommand::Verify)
            .expect("verify bootstrap should succeed after init");

        let db_path = resolve_storage_db_path(&workspace_root, "verify")
            .expect("storage db path should resolve after init");
        assert!(db_path.is_file(), "storage db should exist after init");

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn reindex_command_succeeds_for_simple_workspace() {
        let workspace_root = temp_workspace_root("reindex-success");
        create_simple_workspace(&workspace_root);

        let config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from temp workspace root");
        run_reindex_command(&config, false)
            .expect("full reindex should succeed for a simple workspace");
        run_reindex_command(&config, true)
            .expect("changed-only reindex should succeed for a simple workspace");

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
        fs::create_dir_all(root.join("src")).expect("workspace src directory should be creatable");
        fs::write(root.join("README.md"), "hello from frigg\n")
            .expect("workspace readme should be writable");
        fs::write(
            root.join("src/main.rs"),
            "fn main() { println!(\"hello from frigg\"); }\n",
        )
        .expect("workspace main source should be writable");
    }

    fn cleanup_workspace(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
