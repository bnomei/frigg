//! Async CLI dispatch: utility commands, startup gates, watch supervisor attach, and serve over
//! stdio or HTTP runtime.

use std::error::Error;
use std::sync::{Arc, RwLock};

use clap::Parser;
use frigg::mcp::{FriggMcpServer, RuntimeTaskRegistry};
use frigg::searcher::ValidatedManifestCandidateCache;
use frigg::settings::{RuntimeTransportKind, runtime_profile_for_transport};
use frigg::watch::maybe_start_watch_runtime;

#[path = "cli_runtime/commands/hook.rs"]
mod hook_command;

use crate::cli_args::{HiddenHookCli, HiddenHookCommand, HookEvent};
use crate::cli_runtime::{
    CliOutput, StorageBootstrapCommand, StorageMaintenanceCommand, resolve_command_config,
    resolve_startup_config, resolve_watch_runtime_config, run_adopt_command_with_output,
    run_context_summary_command, run_hash_command, run_prepare_semantic_model_command_with_output,
    run_reindex_command_with_output, run_semantic_runtime_startup_gate_with_output,
    run_semantic_runtime_startup_gate_with_stderr_prepare_output,
    run_storage_bootstrap_command_with_output, run_storage_maintenance_command_with_output,
    run_strict_startup_vector_readiness_gate_with_output,
};
use crate::http_runtime::{HttpRuntimeConfig, resolve_http_runtime_config, serve_http};
use crate::{Cli, Command, default_tracing_filter, init_tracing, startup_trace};

pub(super) async fn async_main(startup_trace_enabled: bool) -> Result<(), Box<dyn Error>> {
    startup_trace(startup_trace_enabled, "async_main: entered");
    if let Some(event) = parse_hidden_hook_event() {
        startup_trace(startup_trace_enabled, "async_main: hidden hook parsed");
        match event {
            HookEvent::Pretooluse => {
                hook_command::run_pretooluse_hook_command(std::io::stdin(), std::io::stdout())?
            }
        }
        startup_trace(startup_trace_enabled, "async_main: hidden hook complete");
        return Ok(());
    }
    let cli = Cli::parse();
    let cli_output = CliOutput::from_flags(cli.quiet, cli.verbose)?;
    startup_trace(startup_trace_enabled, "async_main: cli parsed");
    let serve_requested = matches!(cli.command, Some(Command::Serve));
    let http_runtime = resolve_http_runtime_config(&cli, serve_requested)?;
    startup_trace(startup_trace_enabled, "async_main: http runtime resolved");
    let transport_kind = http_runtime
        .as_ref()
        .map(HttpRuntimeConfig::transport_kind)
        .unwrap_or(RuntimeTransportKind::Stdio);
    init_tracing(
        default_tracing_filter(&cli, transport_kind),
        crate::tracing_env_override_allowed(&cli, transport_kind),
    );
    startup_trace(startup_trace_enabled, "async_main: tracing initialized");

    if let Some(command) = cli.command.clone() {
        match command.clone() {
            Command::Serve => {}
            Command::Adopt {
                target,
                all,
                legacy_cursor,
                uninstall,
                check,
                dry_run,
                force,
            } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_adopt_command_with_output(
                    &config,
                    target,
                    all,
                    legacy_cursor,
                    uninstall,
                    check,
                    dry_run,
                    force,
                    &cli_output,
                )?
            }
            Command::Init => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_bootstrap_command_with_output(
                    &config,
                    StorageBootstrapCommand::Init,
                    &cli_output,
                )?
            }
            Command::Verify => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_bootstrap_command_with_output(
                    &config,
                    StorageBootstrapCommand::Verify,
                    &cli_output,
                )?
            }
            Command::Reindex {
                changed,
                prepare_semantic_model,
            } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_semantic_runtime_startup_gate_with_output(&config, &cli_output)?;
                run_reindex_command_with_output(
                    &config,
                    changed,
                    prepare_semantic_model,
                    &cli_output,
                )?
            }
            Command::PrepareSemanticModel => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_prepare_semantic_model_command_with_output(&config, &cli_output)?
            }
            Command::RepairStorage => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_maintenance_command_with_output(
                    &config,
                    StorageMaintenanceCommand::RepairSemanticVectorStore,
                    &cli_output,
                )?
            }
            Command::Hash => run_hash_command()?,
            Command::PruneStorage {
                keep_manifest_snapshots,
            } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_maintenance_command_with_output(
                    &config,
                    StorageMaintenanceCommand::Prune {
                        keep_manifest_snapshots,
                    },
                    &cli_output,
                )?
            }
            Command::Context { since, until } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_context_summary_command(&config, since.as_deref(), until.as_deref())?
            }
        }
        if !matches!(command, Command::Serve) {
            startup_trace(
                startup_trace_enabled,
                "async_main: non-serve command complete",
            );
            return Ok(());
        }
    }

    let config = resolve_startup_config(&cli, transport_kind)?;
    startup_trace(startup_trace_enabled, "async_main: startup config resolved");
    run_strict_startup_vector_readiness_gate_with_output(&config, &cli_output)?;
    startup_trace(startup_trace_enabled, "async_main: vector readiness passed");
    if transport_kind == RuntimeTransportKind::Stdio {
        run_semantic_runtime_startup_gate_with_stderr_prepare_output(&config, &cli_output)?;
    } else {
        run_semantic_runtime_startup_gate_with_output(&config, &cli_output)?;
    }
    startup_trace(startup_trace_enabled, "async_main: semantic gate passed");
    let watch_runtime_config = resolve_watch_runtime_config(&config, transport_kind)?;
    startup_trace(startup_trace_enabled, "async_main: watch config resolved");
    let runtime_watch_active = watch_runtime_config
        .watch
        .enabled_for_transport(transport_kind);
    let runtime_profile = runtime_profile_for_transport(transport_kind, runtime_watch_active);
    let runtime_task_registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
    let validated_manifest_candidate_cache =
        Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
    let server = FriggMcpServer::new_with_runtime(
        config,
        runtime_profile,
        runtime_watch_active,
        Arc::clone(&runtime_task_registry),
        Arc::clone(&validated_manifest_candidate_cache),
    );
    // Watch supervisor starts only when the resolved transport enables incremental freshness.
    let watch_runtime = maybe_start_watch_runtime(
        &watch_runtime_config,
        transport_kind,
        runtime_task_registry,
        validated_manifest_candidate_cache,
        Some(server.repository_cache_invalidation_callback()),
    )?;
    let _watch_runtime = watch_runtime.map(Arc::new);
    server.set_watch_runtime(_watch_runtime.clone());
    if let Some(runtime) = http_runtime {
        startup_trace(startup_trace_enabled, "async_main: serving http");
        // HTTP runtime path: loopback or remote MCP over streamable HTTP.
        serve_http(runtime, server).await?;
    } else {
        startup_trace(startup_trace_enabled, "async_main: serving stdio");
        server.serve_stdio().await?;
    }

    Ok(())
}

fn parse_hidden_hook_event() -> Option<HookEvent> {
    match HiddenHookCli::try_parse_from(std::env::args_os())
        .ok()?
        .command
    {
        HiddenHookCommand::Hook { event } => Some(event),
    }
}
