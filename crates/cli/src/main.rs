use std::error::Error;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use clap::{Parser, Subcommand};
use frigg::indexer::{ManifestDiagnosticKind, ReindexMode, reindex_repository_with_runtime_config};
use frigg::mcp::FriggMcpServer;
use frigg::settings::{
    FriggConfig, RuntimeTransportKind, SemanticRuntimeConfig, SemanticRuntimeCredentials,
    SemanticRuntimeProvider, SemanticRuntimeStartupError, WatchConfig, WatchMode,
};
use frigg::storage::{
    DEFAULT_VECTOR_DIMENSIONS, Storage, VectorStoreBackend, ensure_provenance_db_parent_dir,
    resolve_provenance_db_path,
};
use frigg::watch::maybe_start_watch_runtime;
use rmcp::transport::StreamableHttpServerConfig;
use tracing::{info, warn};
use tracing_subscriber::{EnvFilter, fmt};

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

#[derive(Debug, Clone, Copy, Subcommand)]
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
}

#[derive(Debug, Clone, Copy)]
enum StorageBootstrapCommand {
    Init,
    Verify,
}

#[derive(Debug, Clone)]
struct HttpRuntimeConfig {
    bind_addr: SocketAddr,
    auth_token: Option<String>,
    allowed_authorities: Option<Vec<String>>,
}

#[derive(Clone)]
struct HttpAuthState {
    expected_bearer_header: Option<String>,
    allowed_authorities: Option<Vec<String>>,
}

#[derive(Debug)]
enum SemanticStartupGateError {
    InvalidConfig(SemanticRuntimeStartupError),
}

impl SemanticStartupGateError {
    fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig(err) => err.code(),
        }
    }
}

impl HttpRuntimeConfig {
    fn transport_kind(&self) -> RuntimeTransportKind {
        if self.bind_addr.ip().is_loopback() {
            RuntimeTransportKind::LoopbackHttp
        } else {
            RuntimeTransportKind::RemoteHttp
        }
    }
}

impl std::fmt::Display for SemanticStartupGateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(err) => write!(f, "{err}"),
        }
    }
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

    if let Some(command) = cli.command.as_ref().copied() {
        match command {
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
        }
        return Ok(());
    }

    let config = resolve_startup_config(&cli, transport_kind)?;
    run_strict_startup_vector_readiness_gate(&config)?;
    run_semantic_runtime_startup_gate(&config)?;
    let watch_runtime_config = resolve_watch_runtime_config(&config, transport_kind)?;
    let _watch_runtime = maybe_start_watch_runtime(&watch_runtime_config, transport_kind)?;

    let server = FriggMcpServer::new(config);
    if let Some(runtime) = http_runtime {
        serve_http(runtime, server).await?;
    } else {
        server.auto_attach_stdio_default_workspace_from_current_dir()?;
        server.serve_stdio().await?;
    }

    Ok(())
}

fn resolve_base_config(
    cli: &Cli,
    workspace_roots_required: bool,
    watch_default_transport: Option<RuntimeTransportKind>,
) -> Result<FriggConfig, Box<dyn Error>> {
    if workspace_roots_required && cli.workspace_roots.is_empty() {
        return Err(Box::new(io::Error::other(
            "at least one workspace root is required",
        )));
    }

    let mut config = if workspace_roots_required {
        FriggConfig::from_workspace_roots(cli.workspace_roots.clone())?
    } else {
        FriggConfig::from_optional_workspace_roots(cli.workspace_roots.clone())?
    };
    if let Some(max_file_bytes) = cli.max_file_bytes {
        config.max_file_bytes = max_file_bytes;
    }
    config.watch = resolve_watch_config(cli, watch_default_transport);
    if workspace_roots_required {
        config.validate()?;
    } else {
        config.validate_for_serving()?;
    }
    Ok(config)
}

fn resolve_command_config(cli: &Cli, command: Command) -> Result<FriggConfig, Box<dyn Error>> {
    match command {
        Command::Init | Command::Verify => resolve_base_config(cli, true, None),
        Command::Reindex { .. } => resolve_startup_config(cli, RuntimeTransportKind::Stdio),
    }
}

fn resolve_startup_config(
    cli: &Cli,
    transport_kind: RuntimeTransportKind,
) -> Result<FriggConfig, Box<dyn Error>> {
    let mut config = resolve_base_config(cli, false, Some(transport_kind))?;
    config.semantic_runtime = resolve_semantic_runtime_config(cli);
    config.validate_for_serving()?;
    Ok(config)
}

fn resolve_semantic_runtime_config(cli: &Cli) -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: cli.semantic_runtime_enabled.unwrap_or(false),
        provider: cli.semantic_runtime_provider,
        model: cli.semantic_runtime_model.clone(),
        strict_mode: cli.semantic_runtime_strict_mode.unwrap_or(false),
    }
}

fn resolve_watch_config(
    cli: &Cli,
    watch_default_transport: Option<RuntimeTransportKind>,
) -> WatchConfig {
    let mut watch = watch_default_transport
        .map(WatchConfig::default_for_transport)
        .unwrap_or_default();
    if let Some(mode) = cli.watch_mode {
        watch.mode = mode;
    }
    if let Some(debounce_ms) = cli.watch_debounce_ms {
        watch.debounce_ms = debounce_ms;
    }
    if let Some(retry_ms) = cli.watch_retry_ms {
        watch.retry_ms = retry_ms;
    }
    watch
}

fn resolve_watch_runtime_config(
    config: &FriggConfig,
    transport_kind: RuntimeTransportKind,
) -> io::Result<FriggConfig> {
    if transport_kind != RuntimeTransportKind::Stdio || !config.workspace_roots.is_empty() {
        return Ok(config.clone());
    }

    let mut watch_config = config.clone();
    watch_config.workspace_roots = vec![resolve_stdio_default_workspace_root()?];
    Ok(watch_config)
}

fn resolve_stdio_default_workspace_root() -> io::Result<PathBuf> {
    let current_dir = std::env::current_dir()?;
    Ok(find_enclosing_git_root(&current_dir).unwrap_or(current_dir))
}

fn find_enclosing_git_root(start: &Path) -> Option<PathBuf> {
    start
        .ancestors()
        .find_map(|ancestor| ancestor.join(".git").exists().then(|| ancestor.to_path_buf()))
}

fn run_storage_bootstrap_command(
    config: &FriggConfig,
    command: StorageBootstrapCommand,
) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let command_name = match command {
        StorageBootstrapCommand::Init => "init",
        StorageBootstrapCommand::Verify => "verify",
    };

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "{command_name} summary status=failed repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
        let db_path = match command {
            StorageBootstrapCommand::Init => ensure_storage_db_path_for_write(root, command_name)?,
            StorageBootstrapCommand::Verify => resolve_storage_db_path(root, command_name)?,
        };
        let storage = Storage::new(&db_path);

        let operation_result = match command {
            StorageBootstrapCommand::Init => storage.initialize(),
            StorageBootstrapCommand::Verify => storage.verify(),
        };

        if let Err(err) = operation_result {
            println!(
                "{command_name} summary status=failed repositories={} repository_id={} root={} db={} error={}",
                repositories.len(),
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                err
            );
            return Err(Box::new(io::Error::other(format!(
                "{command_name} failed for repository_id={} root={} db={}: {err}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            ))));
        }

        println!(
            "{command_name} ok repository_id={} root={} db={}",
            repo.repository_id.0,
            root.display(),
            db_path.display()
        );
    }

    println!(
        "{command_name} summary status=ok repositories={}",
        repositories.len()
    );
    Ok(())
}

fn run_reindex_command(config: &FriggConfig, changed: bool) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let mode = if changed {
        ReindexMode::ChangedOnly
    } else {
        ReindexMode::Full
    };
    let mode_name = mode.as_str();
    let mut total_files_scanned = 0usize;
    let mut total_files_changed = 0usize;
    let mut total_files_deleted = 0usize;
    let mut total_diagnostics = 0usize;
    let mut total_walk_diagnostics = 0usize;
    let mut total_read_diagnostics = 0usize;
    let mut total_duration_ms = 0u128;

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "reindex summary status=failed mode={mode_name} repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
        let db_path = ensure_storage_db_path_for_write(root, "reindex")?;

        let summary = match reindex_repository_with_runtime_config(
            &repo.repository_id.0,
            root,
            &db_path,
            mode,
            &config.semantic_runtime,
            &SemanticRuntimeCredentials::from_process_env(),
        ) {
            Ok(summary) => summary,
            Err(err) => {
                println!(
                    "reindex summary status=failed mode={mode_name} repositories={} repository_id={} root={} db={} error={}",
                    repositories.len(),
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    err
                );
                return Err(Box::new(io::Error::other(format!(
                    "reindex failed mode={mode_name} repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                ))));
            }
        };

        total_files_scanned += summary.files_scanned;
        total_files_changed += summary.files_changed;
        total_files_deleted += summary.files_deleted;
        let diagnostics_total = summary.diagnostics.total_count();
        let diagnostics_walk = summary
            .diagnostics
            .count_by_kind(ManifestDiagnosticKind::Walk);
        let diagnostics_read = summary
            .diagnostics
            .count_by_kind(ManifestDiagnosticKind::Read);
        total_diagnostics += diagnostics_total;
        total_walk_diagnostics += diagnostics_walk;
        total_read_diagnostics += diagnostics_read;
        total_duration_ms += summary.duration_ms;

        for diagnostic in &summary.diagnostics.entries {
            println!(
                "reindex diagnostic mode={mode_name} repository_id={} kind={} path={} message={}",
                repo.repository_id.0,
                diagnostic.kind.as_str(),
                diagnostic
                    .path
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".to_owned()),
                diagnostic.message
            );
        }

        println!(
            "reindex ok mode={mode_name} repository_id={} root={} db={} snapshot_id={} files_scanned={} files_changed={} files_deleted={} diagnostics_total={} diagnostics_walk={} diagnostics_read={} duration_ms={}",
            repo.repository_id.0,
            root.display(),
            db_path.display(),
            summary.snapshot_id,
            summary.files_scanned,
            summary.files_changed,
            summary.files_deleted,
            diagnostics_total,
            diagnostics_walk,
            diagnostics_read,
            summary.duration_ms
        );
    }

    println!(
        "reindex summary status=ok mode={mode_name} repositories={} files_scanned={} files_changed={} files_deleted={} diagnostics_total={} diagnostics_walk={} diagnostics_read={} duration_ms={}",
        repositories.len(),
        total_files_scanned,
        total_files_changed,
        total_files_deleted,
        total_diagnostics,
        total_walk_diagnostics,
        total_read_diagnostics,
        total_duration_ms
    );
    Ok(())
}

fn run_strict_startup_vector_readiness_gate(config: &FriggConfig) -> io::Result<()> {
    let repositories = config.repositories();

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "startup summary status=failed repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
        let db_path = resolve_storage_db_path(root, "startup")?;
        if !db_path.is_file() {
            let err_message = format!(
                "startup strict vector readiness failed repository_id={} root={} db={}: storage db file is missing; run `frigg init --workspace-root {}` first",
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                root.display()
            );
            println!(
                "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                repositories.len(),
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                err_message
            );
            return Err(io::Error::other(err_message));
        }
        let storage = Storage::new(&db_path);
        let status = storage
            .verify_vector_store(DEFAULT_VECTOR_DIMENSIONS)
            .map_err(|err| {
                io::Error::other(format!(
                    "startup strict vector readiness failed repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                ))
            });

        let status = match status {
            Ok(status) => status,
            Err(err) => {
                println!(
                    "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                    repositories.len(),
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    err
                );
                return Err(err);
            }
        };

        if status.backend != VectorStoreBackend::SqliteVec {
            let err_message = format!(
                "vector subsystem not ready: sqlite-vec backend unavailable (active backend: {})",
                status.backend.as_str()
            );
            println!(
                "startup summary status=failed repositories={} repository_id={} root={} db={} error={}",
                repositories.len(),
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                err_message
            );
            return Err(io::Error::other(format!(
                "startup strict vector readiness failed repository_id={} root={} db={}: {err_message}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            )));
        }

        info!(
            repository_id = %repo.repository_id.0,
            root = %root.display(),
            db = %db_path.display(),
            extension_version = %status.extension_version,
            "startup strict vector readiness passed"
        );
    }

    Ok(())
}

fn run_semantic_runtime_startup_gate(config: &FriggConfig) -> io::Result<()> {
    let credentials = SemanticRuntimeCredentials::from_process_env();
    run_semantic_runtime_startup_gate_with_credentials(config, &credentials)
}

fn run_semantic_runtime_startup_gate_with_credentials(
    config: &FriggConfig,
    credentials: &SemanticRuntimeCredentials,
) -> io::Result<()> {
    if !config.semantic_runtime.enabled {
        return Ok(());
    }

    if let Err(err) = config.semantic_runtime.validate_startup(credentials) {
        let startup_error = SemanticStartupGateError::InvalidConfig(err);
        let provider = config
            .semantic_runtime
            .provider
            .map(SemanticRuntimeProvider::as_str)
            .unwrap_or("-");
        let model = config.semantic_runtime.normalized_model().unwrap_or("-");
        println!(
            "startup summary status=failed semantic_enabled=true semantic_provider={} semantic_model={} semantic_code={} error={}",
            provider,
            model,
            startup_error.code(),
            startup_error
        );
        return Err(io::Error::other(format!(
            "startup semantic runtime readiness failed code={}: {}",
            startup_error.code(),
            startup_error
        )));
    }

    let provider = config
        .semantic_runtime
        .provider
        .expect("semantic runtime provider must exist after successful validation");
    let model = config
        .semantic_runtime
        .normalized_model()
        .expect("semantic runtime model must exist after successful validation");
    info!(
        semantic_provider = %provider.as_str(),
        semantic_model = %model,
        semantic_strict_mode = config.semantic_runtime.strict_mode,
        "startup semantic runtime readiness passed"
    );
    Ok(())
}

fn resolve_storage_db_path(workspace_root: &Path, command_name: &str) -> io::Result<PathBuf> {
    resolve_provenance_db_path(workspace_root).map_err(|err| {
        io::Error::other(format!(
            "{command_name} summary status=failed root={} error={err}",
            workspace_root.display()
        ))
    })
}

fn ensure_storage_db_path_for_write(
    workspace_root: &Path,
    command_name: &str,
) -> io::Result<PathBuf> {
    ensure_provenance_db_parent_dir(workspace_root).map_err(|err| {
        io::Error::other(format!(
            "{command_name} summary status=failed root={} error={err}",
            workspace_root.display()
        ))
    })
}

fn resolve_http_runtime_config(cli: &Cli) -> Result<Option<HttpRuntimeConfig>, Box<dyn Error>> {
    let has_http_port = cli.mcp_http_port.is_some();
    let has_http_related_flags =
        cli.mcp_http_host.is_some() || cli.allow_remote_http || cli.mcp_http_auth_token.is_some();

    if !has_http_port {
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

async fn serve_http(
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

fn allowed_authorities_for_bind(bind_addr: SocketAddr) -> Option<Vec<String>> {
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

fn host_header_allowed(
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

fn origin_header_allowed(
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

fn parse_host_authority(raw: &str) -> Option<String> {
    let authority = raw.trim().trim_end_matches('.');
    if authority.is_empty() {
        return None;
    }
    Some(authority.to_ascii_lowercase())
}

fn parse_origin_authority(raw: &str) -> Option<String> {
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

fn authority_allowed(authority: &str, allowed_authorities: &Option<Vec<String>>) -> bool {
    match allowed_authorities {
        None => true,
        Some(allowlist) => allowlist
            .iter()
            .any(|candidate| constant_time_equals(candidate, authority)),
    }
}

fn constant_time_equals(left: &str, right: &str) -> bool {
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

fn typed_access_denied_response(status: StatusCode, message: &str) -> Response {
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

#[cfg(test)]
mod tests {
    use std::fs;
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
        let watch_config = resolve_watch_runtime_config(&config, RuntimeTransportKind::LoopbackHttp)
            .expect("http watch runtime config should preserve empty startup roots");
        assert!(watch_config.workspace_roots.is_empty());
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

    fn cleanup_workspace(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
