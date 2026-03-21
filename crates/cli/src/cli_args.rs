use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use frigg::settings::{LexicalBackendMode, SemanticRuntimeProvider, WatchMode};
use frigg::storage::{DEFAULT_RETAINED_MANIFEST_SNAPSHOTS, DEFAULT_RETAINED_PROVENANCE_EVENTS};

#[derive(Debug, Parser)]
#[command(name = "frigg", version, about = "Frigg MCP server")]
pub(crate) struct Cli {
    #[arg(long, global = true)]
    pub(crate) quiet: bool,

    #[arg(long = "workspace-root", value_name = "PATH", global = true)]
    pub(crate) workspace_roots: Vec<PathBuf>,

    #[arg(
        long = "max-file-bytes",
        value_name = "BYTES",
        env = "FRIGG_MAX_FILE_BYTES",
        global = true
    )]
    pub(crate) max_file_bytes: Option<usize>,

    #[arg(long, value_name = "PORT", global = true)]
    pub(crate) mcp_http_port: Option<u16>,

    #[arg(long, value_name = "HOST", global = true)]
    pub(crate) mcp_http_host: Option<IpAddr>,

    #[arg(long, global = true)]
    pub(crate) allow_remote_http: bool,

    #[arg(
        long,
        value_name = "TOKEN",
        env = "FRIGG_MCP_HTTP_AUTH_TOKEN",
        hide_env_values = true,
        global = true
    )]
    pub(crate) mcp_http_auth_token: Option<String>,

    #[arg(
        long,
        value_name = "BOOL",
        env = "FRIGG_SEMANTIC_RUNTIME_ENABLED",
        global = true
    )]
    pub(crate) semantic_runtime_enabled: Option<bool>,

    #[arg(
        long,
        value_name = "PROVIDER",
        env = "FRIGG_SEMANTIC_RUNTIME_PROVIDER",
        global = true
    )]
    pub(crate) semantic_runtime_provider: Option<SemanticRuntimeProvider>,

    #[arg(
        long,
        value_name = "MODEL",
        env = "FRIGG_SEMANTIC_RUNTIME_MODEL",
        global = true
    )]
    pub(crate) semantic_runtime_model: Option<String>,

    #[arg(
        long,
        value_name = "BOOL",
        env = "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE",
        global = true
    )]
    pub(crate) semantic_runtime_strict_mode: Option<bool>,

    #[arg(long, value_name = "MODE", env = "FRIGG_WATCH_MODE", global = true)]
    pub(crate) watch_mode: Option<WatchMode>,

    #[arg(
        long,
        value_name = "MODE",
        env = "FRIGG_LEXICAL_BACKEND",
        global = true
    )]
    pub(crate) lexical_backend: Option<LexicalBackendMode>,

    #[arg(
        long,
        value_name = "PATH",
        env = "FRIGG_RIPGREP_EXECUTABLE",
        global = true
    )]
    pub(crate) ripgrep_executable: Option<PathBuf>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        env = "FRIGG_WATCH_DEBOUNCE_MS",
        global = true
    )]
    pub(crate) watch_debounce_ms: Option<u64>,

    #[arg(
        long,
        value_name = "MILLISECONDS",
        env = "FRIGG_WATCH_RETRY_MS",
        global = true
    )]
    pub(crate) watch_retry_ms: Option<u64>,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Command {
    /// Serve Frigg over loopback HTTP for shared local MCP sessions.
    Serve,
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
    /// Rebuild the derived sqlite-vec semantic projection from live semantic rows.
    RepairStorage,
    /// Prune retained manifest snapshots and provenance events for each workspace root.
    PruneStorage {
        /// Number of latest manifest snapshots to retain per repository.
        #[arg(
            long = "keep-manifest-snapshots",
            default_value_t = DEFAULT_RETAINED_MANIFEST_SNAPSHOTS
        )]
        keep_manifest_snapshots: usize,
        /// Number of latest provenance events to retain per repository.
        #[arg(
            long = "keep-provenance-events",
            default_value_t = DEFAULT_RETAINED_PROVENANCE_EVENTS
        )]
        keep_provenance_events: usize,
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
        /// Optional directory for per-playbook trace packets.
        #[arg(long = "trace-root", value_name = "PATH")]
        trace_root: Option<PathBuf>,
    },
    /// Export a deterministic sanitized workload corpus from stored provenance rows.
    ExportWorkloadCorpus {
        /// Output file path for JSON or JSONL export.
        #[arg(long, value_name = "PATH")]
        output: PathBuf,
        /// Export encoding.
        #[arg(long, value_enum, default_value_t = WorkloadCorpusExportFormat::Jsonl)]
        format: WorkloadCorpusExportFormat,
        /// Number of recent provenance rows to export per repository.
        #[arg(
            long,
            value_name = "COUNT",
            default_value_t = DEFAULT_RETAINED_PROVENANCE_EVENTS
        )]
        limit: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum WorkloadCorpusExportFormat {
    Json,
    Jsonl,
}

impl WorkloadCorpusExportFormat {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Jsonl => "jsonl",
        }
    }
}
