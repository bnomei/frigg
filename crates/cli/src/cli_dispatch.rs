use std::error::Error;
use std::sync::{Arc, RwLock};

use clap::Parser;
use frigg::mcp::{FriggMcpServer, RuntimeTaskRegistry};
use frigg::searcher::ValidatedManifestCandidateCache;
use frigg::settings::{RuntimeTransportKind, runtime_profile_for_transport};
use frigg::watch::maybe_start_watch_runtime;

use crate::cli_runtime::{
    StorageBootstrapCommand, StorageMaintenanceCommand, resolve_command_config,
    resolve_startup_config, resolve_watch_runtime_config, run_hybrid_playbook_command,
    run_reindex_command, run_semantic_runtime_startup_gate, run_storage_bootstrap_command,
    run_storage_maintenance_command, run_strict_startup_vector_readiness_gate,
    run_workload_corpus_export_command,
};
use crate::http_runtime::{HttpRuntimeConfig, resolve_http_runtime_config, serve_http};
use crate::{Cli, Command, default_tracing_filter, init_tracing, startup_trace};

pub(super) async fn async_main(startup_trace_enabled: bool) -> Result<(), Box<dyn Error>> {
    startup_trace(startup_trace_enabled, "async_main: entered");
    let cli = Cli::parse();
    startup_trace(startup_trace_enabled, "async_main: cli parsed");
    let serve_requested = matches!(cli.command, Some(Command::Serve));
    let http_runtime = resolve_http_runtime_config(&cli, serve_requested)?;
    startup_trace(startup_trace_enabled, "async_main: http runtime resolved");
    let transport_kind = http_runtime
        .as_ref()
        .map(HttpRuntimeConfig::transport_kind)
        .unwrap_or(RuntimeTransportKind::Stdio);
    init_tracing(default_tracing_filter(&cli, transport_kind));
    startup_trace(startup_trace_enabled, "async_main: tracing initialized");

    if let Some(command) = cli.command.clone() {
        match command.clone() {
            Command::Serve => {}
            Command::Init => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_bootstrap_command(&config, StorageBootstrapCommand::Init)?
            }
            Command::Verify => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_bootstrap_command(&config, StorageBootstrapCommand::Verify)?
            }
            Command::Reindex { changed } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_semantic_runtime_startup_gate(&config)?;
                run_reindex_command(&config, changed)?
            }
            Command::RepairStorage => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_maintenance_command(
                    &config,
                    StorageMaintenanceCommand::RepairSemanticVectorStore,
                )?
            }
            Command::PruneStorage {
                keep_manifest_snapshots,
                keep_provenance_events,
            } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_maintenance_command(
                    &config,
                    StorageMaintenanceCommand::Prune {
                        keep_manifest_snapshots,
                        keep_provenance_events,
                    },
                )?
            }
            Command::PlaybookHybridRun {
                playbooks_root,
                enforce_targets,
                output,
                trace_root,
            } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_semantic_runtime_startup_gate(&config)?;
                run_hybrid_playbook_command(
                    &config,
                    &playbooks_root,
                    enforce_targets,
                    output.as_deref(),
                    trace_root.as_deref(),
                )?
            }
            Command::ExportWorkloadCorpus {
                output,
                format,
                limit,
            } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_workload_corpus_export_command(&config, &output, format, limit)?
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
    run_strict_startup_vector_readiness_gate(&config)?;
    startup_trace(startup_trace_enabled, "async_main: vector readiness passed");
    run_semantic_runtime_startup_gate(&config)?;
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
        serve_http(runtime, server).await?;
    } else {
        startup_trace(startup_trace_enabled, "async_main: serving stdio");
        server.serve_stdio().await?;
    }

    Ok(())
}
