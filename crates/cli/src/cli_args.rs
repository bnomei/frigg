//! Clap-derived CLI surface: global runtime flags and utility or serve subcommands.

use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use frigg::settings::{LexicalBackendMode, SemanticRuntimeProvider, WatchMode};
use frigg::storage::DEFAULT_RETAINED_MANIFEST_SNAPSHOTS;

#[derive(Debug, Parser)]
#[command(name = "frigg", version, about = "Frigg MCP server")]
pub(crate) struct Cli {
    /// Suppress normal output.
    #[arg(long, global = true)]
    pub(crate) quiet: bool,

    /// Show per-repository progress.
    #[arg(long, global = true)]
    pub(crate) verbose: bool,

    /// Repository root to operate on.
    #[arg(long = "workspace-root", value_name = "PATH", global = true)]
    pub(crate) workspace_roots: Vec<PathBuf>,

    /// Skip files larger than this many bytes.
    ///
    /// [env: FRIGG_MAX_FILE_BYTES=2097152]
    #[arg(
        long = "max-file-bytes",
        value_name = "BYTES",
        env = "FRIGG_MAX_FILE_BYTES",
        hide_env = true,
        global = true
    )]
    pub(crate) max_file_bytes: Option<usize>,

    /// Ingest full SCIP artifacts when present.
    ///
    /// [env: FRIGG_FULL_SCIP_INGEST=true]
    #[arg(
        long,
        env = "FRIGG_FULL_SCIP_INGEST",
        hide_env = true,
        global = true,
        default_value_t = true
    )]
    pub(crate) full_scip_ingest: bool,

    /// HTTP port for `frigg serve`.
    #[arg(long, value_name = "PORT", global = true)]
    pub(crate) mcp_http_port: Option<u16>,

    /// HTTP host for `frigg serve`.
    #[arg(long, value_name = "HOST", global = true)]
    pub(crate) mcp_http_host: Option<IpAddr>,

    /// Allow serving on non-loopback hosts.
    #[arg(long, global = true)]
    pub(crate) allow_remote_http: bool,

    /// Bearer token for HTTP MCP requests.
    #[arg(
        long,
        value_name = "TOKEN",
        env = "FRIGG_MCP_HTTP_AUTH_TOKEN",
        hide_env_values = true,
        global = true
    )]
    pub(crate) mcp_http_auth_token: Option<String>,

    /// Enable semantic indexing and recall.
    ///
    /// [env: FRIGG_SEMANTIC_RUNTIME_ENABLED=false]
    #[arg(
        long,
        value_name = "BOOL",
        env = "FRIGG_SEMANTIC_RUNTIME_ENABLED",
        hide_env = true,
        global = true
    )]
    pub(crate) semantic_runtime_enabled: Option<bool>,

    /// Semantic provider when semantic indexing is enabled.
    ///
    /// [env: FRIGG_SEMANTIC_RUNTIME_PROVIDER=local]
    #[arg(
        long,
        value_name = "PROVIDER",
        env = "FRIGG_SEMANTIC_RUNTIME_PROVIDER",
        hide_env = true,
        global = true
    )]
    pub(crate) semantic_runtime_provider: Option<SemanticRuntimeProvider>,

    /// Embedding model override.
    ///
    /// [env: FRIGG_SEMANTIC_RUNTIME_MODEL=provider default]
    #[arg(
        long,
        value_name = "MODEL",
        env = "FRIGG_SEMANTIC_RUNTIME_MODEL",
        hide_env = true,
        global = true
    )]
    pub(crate) semantic_runtime_model: Option<String>,

    /// Fail startup when semantic runtime is unhealthy.
    ///
    /// [env: FRIGG_SEMANTIC_RUNTIME_STRICT_MODE=false]
    #[arg(
        long,
        value_name = "BOOL",
        env = "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE",
        hide_env = true,
        global = true
    )]
    pub(crate) semantic_runtime_strict_mode: Option<bool>,

    /// Watch behavior for served workspaces.
    ///
    /// [env: FRIGG_WATCH_MODE=auto]
    #[arg(
        long,
        value_name = "MODE",
        env = "FRIGG_WATCH_MODE",
        hide_env = true,
        global = true
    )]
    pub(crate) watch_mode: Option<WatchMode>,

    /// Lexical search backend.
    ///
    /// [env: FRIGG_LEXICAL_BACKEND=auto]
    #[arg(
        long,
        value_name = "MODE",
        env = "FRIGG_LEXICAL_BACKEND",
        hide_env = true,
        global = true
    )]
    pub(crate) lexical_backend: Option<LexicalBackendMode>,

    /// Path to the `rg` executable.
    ///
    /// [env: FRIGG_RIPGREP_EXECUTABLE=PATH lookup]
    #[arg(
        long,
        value_name = "PATH",
        env = "FRIGG_RIPGREP_EXECUTABLE",
        hide_env = true,
        global = true
    )]
    pub(crate) ripgrep_executable: Option<PathBuf>,

    /// Watch debounce delay.
    ///
    /// [env: FRIGG_WATCH_DEBOUNCE_MS=2000]
    #[arg(
        long,
        value_name = "MILLISECONDS",
        env = "FRIGG_WATCH_DEBOUNCE_MS",
        hide_env = true,
        global = true
    )]
    pub(crate) watch_debounce_ms: Option<u64>,

    /// Watch retry delay.
    ///
    /// [env: FRIGG_WATCH_RETRY_MS=5000]
    #[arg(
        long,
        value_name = "MILLISECONDS",
        env = "FRIGG_WATCH_RETRY_MS",
        hide_env = true,
        global = true
    )]
    pub(crate) watch_retry_ms: Option<u64>,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Parser)]
#[command(name = "frigg", version, about = "Frigg MCP server")]
pub(crate) struct HiddenHookCli {
    #[command(subcommand)]
    pub(crate) command: HiddenHookCommand,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum HiddenHookCommand {
    #[command(hide = true)]
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Subcommand)]
pub(crate) enum HookEvent {
    #[command(name = "pretooluse")]
    Pretooluse,
}

#[derive(Debug, Clone, Subcommand)]
pub(crate) enum Command {
    /// Serve MCP over loopback HTTP.
    ///
    /// Starts Frigg's local MCP server for editor and agent clients.
    Serve,
    /// Add Frigg entries to agent docs and MCP configs.
    ///
    /// Writes managed entries to files such as `AGENTS.md`, `CLAUDE.md`, Cursor/Copilot
    /// instruction files, and `.mcp.json`.
    Adopt {
        /// Choose which project files or configs to update.
        #[arg(
            long,
            value_enum,
            alias = "hook",
            num_args = 0..=1,
            default_missing_value = "hook"
        )]
        target: Vec<AdoptTarget>,
        /// Update every supported docs and MCP target.
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Remove Frigg-managed entries instead.
        #[arg(long, default_value_t = false)]
        uninstall: bool,
        /// Fail if any selected target would change.
        #[arg(long, default_value_t = false)]
        check: bool,
        /// Print planned changes without writing files.
        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
        /// Replace a diverged Frigg MCP entry.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
    /// Create or repair `.frigg/storage.sqlite3`.
    ///
    /// Prepares Frigg's local database without scanning source files.
    Init,
    /// Scan files and refresh the local search index.
    ///
    /// Walks the workspace, updates file metadata and search data, and refreshes semantic rows
    /// when semantic runtime is enabled.
    #[command(alias = "reindex")]
    Index {
        /// Recheck only files that changed since the last index.
        #[arg(long, default_value_t = false)]
        changed: bool,
    },
    /// Rebuild the derived sqlite-vec semantic projection from live semantic rows.
    #[command(hide = true)]
    RepairStorage,
    /// Print the CI cache fingerprint.
    ///
    /// Useful for CI cache keys; most local workflows do not need it.
    Hash,
    /// Prune retained manifest snapshots for each workspace root.
    #[command(hide = true)]
    PruneStorage {
        /// Number of latest manifest snapshots to retain per repository.
        #[arg(
            long = "keep-manifest-snapshots",
            default_value_t = DEFAULT_RETAINED_MANIFEST_SNAPSHOTS
        )]
        keep_manifest_snapshots: usize,
    },
    /// Summarize local context-efficiency logs.
    ///
    /// Reads Frigg JSONL logs and reports recent context-use totals.
    Context {
        /// Start date or RFC3339 time. Defaults to `now - Duration::days(30)`.
        #[arg(long, value_name = "DATE_OR_RFC3339")]
        since: Option<String>,
        /// End date or RFC3339 time. Defaults to `now`.
        #[arg(long, value_name = "DATE_OR_RFC3339")]
        until: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AdoptTarget {
    ClaudeMd,
    AgentsMd,
    GeminiMd,
    Copilot,
    Cursor,
    McpProject,
    McpCursor,
    Hook,
}

impl AdoptTarget {
    pub(crate) fn path(self) -> &'static str {
        match self {
            Self::ClaudeMd => "CLAUDE.md",
            Self::AgentsMd => "AGENTS.md",
            Self::GeminiMd => "GEMINI.md",
            Self::Copilot => ".github/copilot-instructions.md",
            Self::Cursor => ".cursor/rules/frigg.mdc",
            Self::McpProject => ".mcp.json",
            Self::McpCursor => ".cursor/mcp.json",
            Self::Hook => ".claude/settings.json",
        }
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use super::{AdoptTarget, Cli, Command, HiddenHookCli, HiddenHookCommand, HookEvent};

    #[test]
    fn hash_command_parses_without_workspace_root() {
        let cli = Cli::try_parse_from(["frigg", "hash"]).expect("hash command should parse");
        assert!(cli.workspace_roots.is_empty());
        assert!(matches!(cli.command, Some(Command::Hash)));
    }

    #[test]
    fn adopt_command_parses_non_hook_flags() {
        let cli = Cli::try_parse_from([
            "frigg",
            "adopt",
            "--target",
            "agents-md",
            "--target",
            "mcp-project",
            "--dry-run",
            "--check",
            "--force",
            "--uninstall",
            "--hook",
        ])
        .expect("adopt command should parse");

        match cli.command {
            Some(Command::Adopt {
                target,
                all,
                uninstall,
                check,
                dry_run,
                force,
            }) => {
                assert_eq!(
                    target,
                    vec![
                        AdoptTarget::AgentsMd,
                        AdoptTarget::McpProject,
                        AdoptTarget::Hook
                    ]
                );
                assert!(!all);
                assert!(uninstall);
                assert!(check);
                assert!(dry_run);
                assert!(force);
            }
            other => panic!("expected adopt command, got {other:?}"),
        }
    }

    #[test]
    fn hidden_hook_pretooluse_command_parses() {
        let cli = HiddenHookCli::try_parse_from(["frigg", "hook", "pretooluse"])
            .expect("hidden hook command should parse");

        match cli.command {
            HiddenHookCommand::Hook { event } => assert_eq!(event, HookEvent::Pretooluse),
        }
    }
}
