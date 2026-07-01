//! Clap-derived CLI surface: global runtime flags and utility or serve subcommands.

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

    #[arg(
        long,
        env = "FRIGG_FULL_SCIP_INGEST",
        global = true,
        default_value_t = true
    )]
    pub(crate) full_scip_ingest: bool,

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
    /// Serve Frigg over loopback HTTP for shared local MCP sessions.
    Serve,
    /// Plan project-local Frigg client adoption for configured workspace roots.
    Adopt {
        /// Limit adoption to one or more client targets.
        #[arg(
            long,
            value_enum,
            alias = "hook",
            num_args = 0..=1,
            default_missing_value = "hook"
        )]
        target: Vec<AdoptTarget>,
        /// Plan every supported non-hook v1 target.
        #[arg(long, default_value_t = false)]
        all: bool,
        /// Include the legacy Cursor rules file target.
        #[arg(long = "legacy-cursor", default_value_t = false)]
        legacy_cursor: bool,
        /// Plan removal of Frigg-owned adoption content.
        #[arg(long, default_value_t = false)]
        uninstall: bool,
        /// Report pending adoption changes and fail once apply support exists.
        #[arg(long, default_value_t = false)]
        check: bool,
        /// Print the adoption plan without writing files.
        #[arg(long = "dry-run", default_value_t = false)]
        dry_run: bool,
        /// Allow replacing diverged Frigg MCP JSON entries once apply support exists.
        #[arg(long, default_value_t = false)]
        force: bool,
    },
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
    /// Emit Frigg's stable cache fingerprint for installer and CI cache keys.
    Hash,
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
pub(crate) enum AdoptTarget {
    ClaudeMd,
    AgentsMd,
    GeminiMd,
    Copilot,
    Cursor,
    LegacyCursor,
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
            Self::LegacyCursor => ".cursorrules",
            Self::McpProject => ".mcp.json",
            Self::McpCursor => ".cursor/mcp.json",
            Self::Hook => ".claude/settings.json",
        }
    }
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
            "--legacy-cursor",
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
                legacy_cursor,
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
                assert!(legacy_cursor);
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
