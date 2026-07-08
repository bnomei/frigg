//! CLI-to-config resolution for serve startup and per-command operating profiles.
//!
//! Maps CLI flags and workspace roots into `FriggConfig` profiles for serve, watch, lexical, and
//! semantic runtime contracts.

use std::error::Error;
use std::io;

use frigg::settings::{
    FriggConfig, LexicalRuntimeConfig, RuntimeTransportKind, SemanticRuntimeConfig,
    SemanticRuntimeProvider, WatchConfig,
};

use crate::{Cli, Command};

/// Builds base `FriggConfig` from CLI flags and optional watch defaults for utility commands.
pub(crate) fn resolve_base_config(
    cli: &Cli,
    workspace_roots_required: bool,
    watch_default_transport: Option<RuntimeTransportKind>,
) -> Result<FriggConfig, Box<dyn Error>> {
    let mut config = if workspace_roots_required {
        FriggConfig::from_workspace_roots(cli.workspace_roots.clone())?
    } else {
        FriggConfig::from_optional_workspace_roots(cli.workspace_roots.clone())?
    };
    if let Some(max_file_bytes) = cli.max_file_bytes {
        config.max_file_bytes = max_file_bytes;
    }
    config.full_scip_ingest = cli.full_scip_ingest;
    config.watch = resolve_watch_config(cli, watch_default_transport);
    config.lexical_runtime = resolve_lexical_runtime_config(cli);
    if workspace_roots_required {
        config.validate()?;
    } else {
        config.validate_for_serving()?;
    }
    Ok(config)
}

/// Resolves the operating profile for one utility subcommand, including index semantic settings.
pub(crate) fn resolve_command_config(
    cli: &Cli,
    command: Command,
) -> Result<FriggConfig, Box<dyn Error>> {
    match command {
        Command::Serve => Err(Box::new(io::Error::other(
            "`frigg serve` uses startup serving config, not command config resolution",
        ))),
        Command::Adopt { .. }
        | Command::Context { .. }
        | Command::Init
        | Command::RepairStorage
        | Command::PruneStorage { .. }
        | Command::Stats { .. } => resolve_base_config(cli, true, None),
        Command::Hash => resolve_base_config(cli, false, None),
        Command::Index { .. } => {
            let mut config = resolve_base_config(cli, true, Some(RuntimeTransportKind::Stdio))?;
            config.semantic_runtime = resolve_semantic_runtime_config(cli);
            config.validate()?;
            Ok(config)
        }
    }
}

/// Resolves serve-time configuration, including semantic runtime flags and serving validation.
pub(crate) fn resolve_startup_config(
    cli: &Cli,
    transport_kind: RuntimeTransportKind,
) -> Result<FriggConfig, Box<dyn Error>> {
    let mut config = resolve_base_config(cli, false, Some(transport_kind))?;
    config.semantic_runtime = resolve_semantic_runtime_config(cli);
    config.validate_for_serving()?;
    Ok(config)
}

/// Maps CLI semantic-runtime flags into the normalized runtime contract used by index and serve.
pub(crate) fn resolve_semantic_runtime_config(cli: &Cli) -> SemanticRuntimeConfig {
    let enabled = cli.semantic_runtime_enabled.unwrap_or(false);
    SemanticRuntimeConfig {
        enabled,
        provider: cli
            .semantic_runtime_provider
            .or(enabled.then_some(SemanticRuntimeProvider::Local)),
        model: cli.semantic_runtime_model.clone(),
        strict_mode: cli.semantic_runtime_strict_mode.unwrap_or(false),
    }
}

pub(crate) fn resolve_lexical_runtime_config(cli: &Cli) -> LexicalRuntimeConfig {
    LexicalRuntimeConfig {
        backend: cli.lexical_backend.unwrap_or_default(),
        ripgrep_executable: cli.ripgrep_executable.clone(),
    }
}

/// Maps CLI watch flags onto transport-aware debounce and retry defaults.
pub(crate) fn resolve_watch_config(
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
    if let Some(manifest_fast_concurrency) = cli.watch_manifest_fast_concurrency {
        watch.manifest_fast_concurrency = manifest_fast_concurrency;
    }
    if let Some(semantic_followup_concurrency) = cli.watch_semantic_followup_concurrency {
        watch.semantic_followup_concurrency = semantic_followup_concurrency;
    }
    watch
}

pub(crate) fn resolve_watch_runtime_config(
    config: &FriggConfig,
    transport_kind: RuntimeTransportKind,
) -> io::Result<FriggConfig> {
    let _ = transport_kind;
    Ok(config.clone())
}
