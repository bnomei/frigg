//! Clap-derived CLI surface: global runtime flags and utility or serve subcommands.
//!
//! Defines the `Cli` parser, subcommand enums, and flag defaults that `cli_dispatch` and HTTP
//! runtime wiring deserialize before startup gates and MCP serve begin.

use std::net::IpAddr;
use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};
use frigg::settings::{LexicalBackendMode, SemanticRuntimeProvider, WatchMode};
use frigg::storage::DEFAULT_RETAINED_MANIFEST_SNAPSHOTS;

/// Root Clap parser for global runtime flags and utility or serve subcommands.
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

    /// Full embeddings POST URL for `provider=openai_compat` (required for that provider).
    ///
    /// Example: `http://127.0.0.1:1234/v1/embeddings`
    ///
    /// [env: FRIGG_SEMANTIC_RUNTIME_OPENAI_COMPAT_ENDPOINT]
    #[arg(
        long,
        value_name = "URL",
        env = "FRIGG_SEMANTIC_RUNTIME_OPENAI_COMPAT_ENDPOINT",
        hide_env = true,
        global = true
    )]
    pub(crate) semantic_runtime_openai_compat_endpoint: Option<String>,

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

    /// Watch manifest-fast concurrency limit.
    ///
    /// [env: FRIGG_WATCH_MANIFEST_FAST_CONCURRENCY=1]
    #[arg(
        long,
        value_name = "COUNT",
        env = "FRIGG_WATCH_MANIFEST_FAST_CONCURRENCY",
        hide_env = true,
        global = true
    )]
    pub(crate) watch_manifest_fast_concurrency: Option<usize>,

    /// Watch semantic-followup concurrency limit.
    ///
    /// [env: FRIGG_WATCH_SEMANTIC_FOLLOWUP_CONCURRENCY=1]
    #[arg(
        long,
        value_name = "COUNT",
        env = "FRIGG_WATCH_SEMANTIC_FOLLOWUP_CONCURRENCY",
        hide_env = true,
        global = true
    )]
    pub(crate) watch_semantic_followup_concurrency: Option<usize>,

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

/// Utility and serve subcommands exposed by the `frigg` binary.
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
        /// Managed markdown policy body for agent docs (`AGENTS.md`, `CLAUDE.md`, etc.).
        ///
        /// Default `lightweight` keeps a short Frigg-first pointer to the
        /// `frigg-first-code-search` skill. Use `expanded` for a compact routing
        /// policy (picker + shell→Frigg one-liners) without dumping the full skill.
        #[arg(long = "policy", value_enum, default_value_t = AdoptAgentsPolicy::Lightweight)]
        policy: AdoptAgentsPolicy,
        /// Best-effort copy of `frigg-first-code-search` into a host skill directory.
        ///
        /// Only writes when the provider's parent skills directory already exists;
        /// never creates `…/skills` itself. Repeatable. Source is the workspace
        /// `skills/frigg-first-code-search` tree (or `FRIGG_SKILL_SOURCE`).
        #[arg(long = "skill-provider", value_enum)]
        skill_provider: Vec<SkillProvider>,
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
    #[command(visible_alias = "content")]
    Context {
        /// Start date or RFC3339 time. Defaults to `now - Duration::days(30)`.
        #[arg(long, value_name = "DATE_OR_RFC3339")]
        since: Option<String>,
        /// End date or RFC3339 time. Defaults to `now`.
        #[arg(long, value_name = "DATE_OR_RFC3339")]
        until: Option<String>,
        /// Print the full JSON summary.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Show live opt-in routing stats from a running HTTP MCP server.
    ///
    /// Process-local counters only (no cloud). Enable recording before `frigg serve`
    /// with `FRIGG_ROUTING_STATS=1`, then read this command or the
    /// `frigg://stats/routing` MCP resource from that server.
    Stats {
        /// Print the full JSON snapshot.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
}

/// Managed markdown policy size for `frigg adopt` agent-doc targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum, Default)]
pub(crate) enum AdoptAgentsPolicy {
    /// Short Frigg-first rules plus a pointer to the production skill.
    #[default]
    Lightweight,
    /// Compact routing policy for repos that want detail without loading the skill.
    Expanded,
}

impl From<AdoptAgentsPolicy> for frigg::agent_directive::AgentsPolicy {
    fn from(value: AdoptAgentsPolicy) -> Self {
        match value {
            AdoptAgentsPolicy::Lightweight => Self::Lightweight,
            AdoptAgentsPolicy::Expanded => Self::Expanded,
        }
    }
}

/// Project files and configs that `frigg adopt` can install or remove managed Frigg entries in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum AdoptTarget {
    ClaudeMd,
    AgentsMd,
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
            Self::Copilot => ".github/copilot-instructions.md",
            Self::Cursor => ".cursor/rules/frigg.mdc",
            Self::McpProject => ".mcp.json",
            Self::McpCursor => ".cursor/mcp.json",
            Self::Hook => ".claude/settings.json",
        }
    }
}

/// Host skill directories that `frigg adopt --skill-provider` may target (best-effort).
///
/// Paths are researched defaults (macOS/`~` style). Install only proceeds when the
/// parent skills directory already exists — Frigg never creates that parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum SkillProvider {
    /// Personal `~/.claude/skills`, else project `.claude/skills`.
    Claude,
    /// Personal `~/.codex/skills`.
    Codex,
    /// Project `.cursor/skills`, else personal `~/.cursor/skills`.
    Cursor,
    /// Project `.github/skills` (CI-friendly), else personal `~/.copilot/skills`.
    Copilot,
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use clap::Parser;

    use super::{
        AdoptAgentsPolicy, AdoptTarget, Cli, Command, HiddenHookCli, HiddenHookCommand, HookEvent,
        SkillProvider,
    };

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
                policy,
                skill_provider,
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
                assert_eq!(policy, AdoptAgentsPolicy::Lightweight);
                assert!(skill_provider.is_empty());
                assert!(uninstall);
                assert!(check);
                assert!(dry_run);
                assert!(force);
            }
            other => panic!("expected adopt command, got {other:?}"),
        }
    }

    #[test]
    fn adopt_command_parses_skill_providers() {
        let cli = Cli::try_parse_from([
            "frigg",
            "adopt",
            "--skill-provider",
            "claude",
            "--skill-provider",
            "copilot",
        ])
        .expect("adopt skill-provider should parse");

        match cli.command {
            Some(Command::Adopt { skill_provider, .. }) => {
                assert_eq!(
                    skill_provider,
                    vec![SkillProvider::Claude, SkillProvider::Copilot]
                );
            }
            other => panic!("expected adopt command, got {other:?}"),
        }
    }

    #[test]
    fn adopt_command_parses_expanded_policy() {
        let cli = Cli::try_parse_from(["frigg", "adopt", "--policy", "expanded"])
            .expect("adopt command should parse expanded policy");

        match cli.command {
            Some(Command::Adopt {
                policy: AdoptAgentsPolicy::Expanded,
                ..
            }) => {}
            other => panic!("expected expanded adopt policy, got {other:?}"),
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
