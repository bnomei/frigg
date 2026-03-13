use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) enum StorageBootstrapCommand {
    Init,
    Verify,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum StorageMaintenanceCommand {
    RepairSemanticVectorStore,
    Prune {
        keep_manifest_snapshots: usize,
        keep_provenance_events: usize,
    },
}

#[derive(Debug)]
pub(super) enum SemanticStartupGateError {
    InvalidConfig(SemanticRuntimeStartupError),
}

impl SemanticStartupGateError {
    pub(super) fn code(&self) -> &'static str {
        match self {
            Self::InvalidConfig(err) => err.code(),
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

pub(super) fn resolve_base_config(
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

pub(super) fn resolve_command_config(
    cli: &Cli,
    command: Command,
) -> Result<FriggConfig, Box<dyn Error>> {
    match command {
        Command::Init | Command::Verify | Command::RepairStorage | Command::PruneStorage { .. } => {
            resolve_base_config(cli, true, None)
        }
        Command::Reindex { .. } => resolve_startup_config(cli, RuntimeTransportKind::Stdio),
        Command::PlaybookHybridRun { .. } => {
            let mut config = resolve_base_config(cli, true, None)?;
            config.semantic_runtime = resolve_semantic_runtime_config(cli);
            config.validate()?;
            Ok(config)
        }
    }
}

pub(super) fn resolve_startup_config(
    cli: &Cli,
    transport_kind: RuntimeTransportKind,
) -> Result<FriggConfig, Box<dyn Error>> {
    let mut config = resolve_base_config(cli, false, Some(transport_kind))?;
    config.semantic_runtime = resolve_semantic_runtime_config(cli);
    config.validate_for_serving()?;
    Ok(config)
}

pub(super) fn resolve_semantic_runtime_config(cli: &Cli) -> SemanticRuntimeConfig {
    SemanticRuntimeConfig {
        enabled: cli.semantic_runtime_enabled.unwrap_or(false),
        provider: cli.semantic_runtime_provider,
        model: cli.semantic_runtime_model.clone(),
        strict_mode: cli.semantic_runtime_strict_mode.unwrap_or(false),
    }
}

pub(super) fn resolve_watch_config(
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

pub(super) fn resolve_watch_runtime_config(
    config: &FriggConfig,
    transport_kind: RuntimeTransportKind,
) -> io::Result<FriggConfig> {
    let _ = transport_kind;
    Ok(config.clone())
}

pub(super) fn find_enclosing_git_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|ancestor| {
        ancestor
            .join(".git")
            .exists()
            .then(|| ancestor.to_path_buf())
    })
}

pub(super) fn run_storage_bootstrap_command(
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

pub(super) fn run_storage_maintenance_command(
    config: &FriggConfig,
    command: StorageMaintenanceCommand,
) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let command_name = match command {
        StorageMaintenanceCommand::RepairSemanticVectorStore => "repair-storage",
        StorageMaintenanceCommand::Prune { .. } => "prune-storage",
    };
    let mut total_repaired = 0usize;
    let mut total_manifest_snapshots_deleted = 0usize;
    let mut total_provenance_events_deleted = 0usize;

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "{command_name} summary status=failed repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
        let db_path = resolve_storage_db_path(root, command_name)?;
        let storage = Storage::new(&db_path);

        match command {
            StorageMaintenanceCommand::RepairSemanticVectorStore => {
                if let Err(err) = storage.repair_semantic_vector_store() {
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

                total_repaired += 1;
                println!(
                    "{command_name} ok repository_id={} root={} db={} repaired=semantic_vectors",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                );
            }
            StorageMaintenanceCommand::Prune {
                keep_manifest_snapshots,
                keep_provenance_events,
            } => {
                let deleted_manifest_snapshots = storage
                    .prune_repository_snapshots(&repo.repository_id.0, keep_manifest_snapshots)
                    .map_err(|err| {
                        io::Error::other(format!(
                            "{command_name} failed for repository_id={} root={} db={}: {err}",
                            repo.repository_id.0,
                            root.display(),
                            db_path.display()
                        ))
                    })?;
                let deleted_provenance_events = storage
                    .prune_provenance_events(keep_provenance_events)
                    .map_err(|err| {
                        io::Error::other(format!(
                            "{command_name} failed for repository_id={} root={} db={}: {err}",
                            repo.repository_id.0,
                            root.display(),
                            db_path.display()
                        ))
                    })?;

                total_manifest_snapshots_deleted += deleted_manifest_snapshots;
                total_provenance_events_deleted += deleted_provenance_events;
                println!(
                    "{command_name} ok repository_id={} root={} db={} keep_manifest_snapshots={} keep_provenance_events={} manifest_snapshots_deleted={} provenance_events_deleted={}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    keep_manifest_snapshots,
                    keep_provenance_events,
                    deleted_manifest_snapshots,
                    deleted_provenance_events
                );
            }
        }
    }

    match command {
        StorageMaintenanceCommand::RepairSemanticVectorStore => {
            println!(
                "{command_name} summary status=ok repositories={} repaired={}",
                repositories.len(),
                total_repaired
            );
        }
        StorageMaintenanceCommand::Prune {
            keep_manifest_snapshots,
            keep_provenance_events,
        } => {
            println!(
                "{command_name} summary status=ok repositories={} keep_manifest_snapshots={} keep_provenance_events={} manifest_snapshots_deleted={} provenance_events_deleted={}",
                repositories.len(),
                keep_manifest_snapshots,
                keep_provenance_events,
                total_manifest_snapshots_deleted,
                total_provenance_events_deleted
            );
        }
    }

    Ok(())
}

pub(super) fn run_reindex_command(
    config: &FriggConfig,
    changed: bool,
) -> Result<(), Box<dyn Error>> {
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

pub(super) fn run_hybrid_playbook_command(
    config: &FriggConfig,
    playbooks_root: &Path,
    enforce_targets: bool,
    output_path: Option<&Path>,
) -> Result<(), Box<dyn Error>> {
    let searcher = TextSearcher::new(config.clone());
    let summary = run_hybrid_playbook_regressions(&searcher, playbooks_root, enforce_targets)?;

    for outcome in &summary.outcomes {
        println!(
            "playbook result playbook_id={} file={} semantic_status={} status_allowed={} duration_ms={} execution_error={} required_missing={:?} target_missing={:?} hits={:?}",
            outcome.playbook_id,
            outcome.file_name,
            outcome.semantic_status,
            outcome.status_allowed,
            outcome.duration_ms,
            outcome.execution_error.as_deref().unwrap_or("-"),
            outcome.required_missing(),
            outcome.target_missing(),
            outcome.matched_paths
        );
    }

    if let Some(output_path) = output_path {
        let parent = output_path.parent().ok_or_else(|| {
            io::Error::other(format!(
                "playbook summary output path has no parent: {}",
                output_path.display()
            ))
        })?;
        std::fs::create_dir_all(parent)?;
        std::fs::write(output_path, to_string_pretty(&summary)?)?;
    }

    println!(
        "playbook summary status=ok playbooks={} required_failures={} target_failures={} enforce_targets={} output={}",
        summary.playbook_count,
        summary.required_failures,
        summary.target_failures,
        summary.enforce_targets,
        output_path
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "-".to_owned())
    );
    Ok(())
}

pub(super) fn run_strict_startup_vector_readiness_gate(config: &FriggConfig) -> io::Result<()> {
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

pub(super) fn run_semantic_runtime_startup_gate(config: &FriggConfig) -> io::Result<()> {
    let credentials = SemanticRuntimeCredentials::from_process_env();
    run_semantic_runtime_startup_gate_with_credentials(config, &credentials)
}

pub(super) fn run_semantic_runtime_startup_gate_with_credentials(
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

pub(super) fn resolve_storage_db_path(
    workspace_root: &Path,
    command_name: &str,
) -> io::Result<PathBuf> {
    resolve_provenance_db_path(workspace_root).map_err(|err| {
        io::Error::other(format!(
            "{command_name} summary status=failed root={} error={err}",
            workspace_root.display()
        ))
    })
}

pub(super) fn ensure_storage_db_path_for_write(
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
