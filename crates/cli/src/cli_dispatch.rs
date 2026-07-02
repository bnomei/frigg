//! Async CLI dispatch: utility commands, startup gates, watch supervisor attach, and serve over
//! stdio or HTTP runtime.

use std::error::Error;
use std::sync::{Arc, RwLock};

use clap::Parser;
use frigg::mcp::{FriggMcpServer, RuntimeTaskRegistry};
use frigg::searcher::ValidatedManifestCandidateCache;
use frigg::settings::{RuntimeTransportKind, runtime_profile_for_transport};
use frigg::watch::{WatchEvent, WatchEventReporter, maybe_start_watch_runtime_with_reporter};

#[path = "cli_runtime/commands/hook.rs"]
mod hook_command;

use crate::cli_args::{HiddenHookCli, HiddenHookCommand, HookEvent};
use crate::cli_runtime::{
    CliOutput, OutputField, OutputLevel, StorageMaintenanceCommand, emit_index_plan_events, field,
    resolve_command_config, resolve_startup_config, resolve_watch_runtime_config,
    run_adopt_command_with_output, run_context_summary_command, run_hash_command,
    run_index_command_with_output, run_semantic_runtime_startup_gate_with_output,
    run_semantic_runtime_startup_gate_with_stderr_prepare_output,
    run_storage_init_command_with_output, run_storage_maintenance_command_with_output,
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
                    uninstall,
                    check,
                    dry_run,
                    force,
                    &cli_output,
                )?
            }
            Command::Init => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_storage_init_command_with_output(&config, &cli_output)?
            }
            Command::Index { changed } => {
                let config = resolve_command_config(&cli, command.clone())?;
                run_semantic_runtime_startup_gate_with_output(&config, &cli_output)?;
                run_index_command_with_output(&config, changed, &cli_output)?
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
    let watch_runtime = maybe_start_watch_runtime_with_reporter(
        &watch_runtime_config,
        transport_kind,
        runtime_task_registry,
        validated_manifest_candidate_cache,
        Some(server.repository_cache_invalidation_callback()),
        watch_event_reporter(cli_output),
    )?;
    let _watch_runtime = watch_runtime.map(Arc::new);
    server.set_watch_runtime(_watch_runtime.clone());
    if let Some(runtime) = http_runtime {
        startup_trace(startup_trace_enabled, "async_main: serving http");
        // HTTP runtime path: loopback or remote MCP over streamable HTTP.
        serve_http(runtime, server, cli_output).await?;
    } else {
        startup_trace(startup_trace_enabled, "async_main: serving stdio");
        server.serve_stdio().await?;
    }

    Ok(())
}

fn watch_event_reporter(output: CliOutput) -> Option<WatchEventReporter> {
    if !output.is_verbose() {
        return None;
    }
    Some(Arc::new(move |event| {
        let _ = emit_watch_event(output, event);
    }))
}

fn emit_watch_event(output: CliOutput, event: WatchEvent) -> std::io::Result<()> {
    match event {
        WatchEvent::RuntimeStarted {
            watch_mode,
            transport,
            debounce_ms,
            retry_ms,
        } => output.progress_event(
            OutputLevel::Info,
            "watch",
            "runtime",
            &[
                field("status", "enabled"),
                field("mode", watch_mode),
                field("transport", transport.as_str()),
                field("debounce_ms", debounce_ms),
                field("retry_ms", retry_ms),
            ],
            None,
        ),
        WatchEvent::RuntimeDisabled {
            watch_mode,
            transport,
        } => output.progress_event(
            OutputLevel::Skip,
            "watch",
            "runtime",
            &[
                field("status", "disabled"),
                field("mode", watch_mode),
                field("transport", transport.as_str()),
            ],
            None,
        ),
        WatchEvent::LeaseAcquired {
            repository_id,
            lease_count,
            root,
        } => output.progress_event(
            OutputLevel::Info,
            "watch",
            "lease",
            &[
                field("status", "acquired"),
                field("repo", repository_id),
                field("leases", lease_count),
            ],
            Some(&root.display().to_string()),
        ),
        WatchEvent::LeaseReleased {
            repository_id,
            remaining,
            root,
        } => {
            let path = root.map(|root| root.display().to_string());
            output.progress_event(
                OutputLevel::Info,
                "watch",
                "lease",
                &[
                    field("status", "released"),
                    field("repo", repository_id),
                    field("leases", remaining),
                ],
                path.as_deref(),
            )
        }
        WatchEvent::PathAccepted {
            repository_id,
            path,
        } => output.progress_event(
            OutputLevel::Info,
            "watch",
            "path",
            &[field("action", "changed"), field("repo", repository_id)],
            Some(&path.display().to_string()),
        ),
        WatchEvent::StartupFresh {
            repository_id,
            root,
            snapshot_id,
        } => output.progress_event(
            OutputLevel::Skip,
            "watch",
            "queue",
            &[
                field("status", "fresh"),
                field("repo", repository_id),
                field("snapshot", snapshot_id.as_deref().unwrap_or("-")),
            ],
            Some(&root.display().to_string()),
        ),
        WatchEvent::RefreshQueued {
            repository_id,
            root,
            refresh_class,
            reason,
            snapshot_id,
            debounce_ms,
        } => {
            let mut fields = vec![
                field("status", "queued"),
                field("repo", repository_id),
                field("class", refresh_class),
                field("reason", reason),
                field("snapshot", snapshot_id.as_deref().unwrap_or("-")),
            ];
            if let Some(debounce_ms) = debounce_ms {
                fields.push(field("debounce_ms", debounce_ms));
            }
            output.progress_event(
                OutputLevel::Info,
                "watch",
                "queue",
                &fields,
                Some(&root.display().to_string()),
            )
        }
        WatchEvent::RefreshStarting {
            repository_id,
            root,
            refresh_class,
            debounce_ms,
            sampled_paths,
        } => output.progress_event(
            OutputLevel::Info,
            "watch",
            "refresh",
            &[
                field("status", "starting"),
                field("repo", repository_id),
                field("class", refresh_class),
                field("debounce_ms", debounce_ms),
                field("sampled_paths", sampled_paths),
            ],
            Some(&root.display().to_string()),
        ),
        WatchEvent::RefreshDeferred {
            repository_id,
            root,
            refresh_class,
            reason,
            retry_ms,
        } => output.progress_event(
            OutputLevel::Skip,
            "watch",
            "refresh",
            &[
                field("status", "deferred"),
                field("repo", repository_id),
                field("class", refresh_class),
                field("reason", reason),
                field("retry_ms", retry_ms),
            ],
            Some(&root.display().to_string()),
        ),
        WatchEvent::IndexPlan {
            repository_id,
            refresh_class,
            plan,
        } => {
            let extra_fields: [OutputField; 2] =
                [field("source", "watch"), field("class", refresh_class)];
            emit_index_plan_events(output, &repository_id, &plan, &extra_fields)
        }
        WatchEvent::RefreshSucceeded {
            repository_id,
            root,
            refresh_class,
            summary,
        } => output.progress_event(
            OutputLevel::Ok,
            "watch",
            "refresh",
            &[
                field("status", "ok"),
                field("repo", repository_id),
                field("class", refresh_class),
                field("snapshot", &summary.snapshot_id),
                field("scanned", summary.files_scanned),
                field("changed", summary.files_changed),
                field("deleted", summary.files_deleted),
                field("duration_ms", summary.duration_ms),
            ],
            Some(&root.display().to_string()),
        ),
        WatchEvent::RefreshFailed {
            repository_id,
            root,
            refresh_class,
            retry_ms,
            error,
        } => output.progress_event(
            OutputLevel::Warn,
            "watch",
            "refresh",
            &[
                field("status", "retry"),
                field("repo", repository_id),
                field("class", refresh_class),
                field("retry_ms", retry_ms),
                field("error", error),
            ],
            Some(&root.display().to_string()),
        ),
        WatchEvent::RefreshBlocked {
            repository_id,
            root,
            refresh_class,
            reason,
            error,
        } => output.progress_event(
            OutputLevel::Warn,
            "watch",
            "refresh",
            &[
                field("status", "blocked"),
                field("repo", repository_id),
                field("class", refresh_class),
                field("reason", reason),
                field("retry", false),
                field("action", "delete_storage_and_reindex"),
                field("error", error),
            ],
            Some(&root.display().to_string()),
        ),
        WatchEvent::StaleCompletionIgnored {
            repository_id,
            refresh_class,
            completion_epoch,
            current_epoch,
        } => output.progress_event(
            OutputLevel::Warn,
            "watch",
            "refresh",
            &[
                field("status", "stale"),
                field("repo", repository_id),
                field("class", refresh_class),
                field("completion_epoch", completion_epoch),
                field("current_epoch", current_epoch),
            ],
            None,
        ),
        WatchEvent::NotifyDropped { error } => output.progress_event(
            OutputLevel::Warn,
            "watch",
            "notify",
            &[field("status", "dropped"), field("error", error)],
            None,
        ),
    }
}

fn parse_hidden_hook_event() -> Option<HookEvent> {
    match HiddenHookCli::try_parse_from(std::env::args_os())
        .ok()?
        .command
    {
        HiddenHookCommand::Hook { event } => Some(event),
    }
}
