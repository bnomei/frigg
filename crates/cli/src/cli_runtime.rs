use super::*;
use serde::Serialize;
use serde_json::{Map, Value};

const WORKLOAD_CORPUS_MAX_STRING_CHARS: usize = 256;
const WORKLOAD_CORPUS_MAX_ARRAY_ITEMS: usize = 8;
const WORKLOAD_CORPUS_MAX_OBJECT_ENTRIES: usize = 16;
const WORKLOAD_CORPUS_MAX_DEPTH: usize = 6;

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
        Command::Serve => Err(Box::new(io::Error::other(
            "`frigg serve` uses startup serving config, not command config resolution",
        ))),
        Command::Init
        | Command::Verify
        | Command::RepairStorage
        | Command::PruneStorage { .. }
        | Command::ExportWorkloadCorpus { .. } => resolve_base_config(cli, true, None),
        Command::Reindex { .. } => {
            let mut config = resolve_base_config(cli, true, Some(RuntimeTransportKind::Stdio))?;
            config.semantic_runtime = resolve_semantic_runtime_config(cli);
            config.validate()?;
            Ok(config)
        }
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

#[cfg(test)]
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

#[derive(Debug, Clone, Serialize)]
struct WorkloadCorpusExportRow {
    trace_id: String,
    created_at: String,
    repository_id: String,
    tool_name: String,
    parameter_summary: Value,
    outcome_summary: Value,
    source_refs_summary: Value,
    source_ref_count: usize,
    normalized_workload: Option<Value>,
}

fn bounded_workload_corpus_text(value: &str) -> String {
    if value.chars().count() <= WORKLOAD_CORPUS_MAX_STRING_CHARS {
        return value.to_owned();
    }

    let mut bounded = value
        .chars()
        .take(WORKLOAD_CORPUS_MAX_STRING_CHARS)
        .collect::<String>();
    bounded.push_str("...");
    bounded
}

fn sanitize_workload_corpus_value(value: &Value, remaining_depth: usize) -> Value {
    if remaining_depth == 0 {
        return Value::String("[truncated-depth]".to_owned());
    }

    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => value.clone(),
        Value::String(text) => Value::String(bounded_workload_corpus_text(text)),
        Value::Array(items) => Value::Array(
            items
                .iter()
                .take(WORKLOAD_CORPUS_MAX_ARRAY_ITEMS)
                .map(|item| sanitize_workload_corpus_value(item, remaining_depth - 1))
                .collect(),
        ),
        Value::Object(entries) => {
            let mut ordered_keys = entries.keys().cloned().collect::<Vec<_>>();
            ordered_keys.sort();

            let mut sanitized = Map::new();
            for key in ordered_keys
                .into_iter()
                .take(WORKLOAD_CORPUS_MAX_OBJECT_ENTRIES)
            {
                if let Some(entry_value) = entries.get(&key) {
                    sanitized.insert(
                        key,
                        sanitize_workload_corpus_value(entry_value, remaining_depth - 1),
                    );
                }
            }

            Value::Object(sanitized)
        }
    }
}

fn workload_corpus_summary_field(payload: &Value, key: &str) -> Value {
    payload
        .get(key)
        .map(|value| sanitize_workload_corpus_value(value, WORKLOAD_CORPUS_MAX_DEPTH))
        .unwrap_or(Value::Null)
}

pub(super) fn run_workload_corpus_export_command(
    config: &FriggConfig,
    output_path: &Path,
    format: WorkloadCorpusExportFormat,
    limit: usize,
) -> Result<(), Box<dyn Error>> {
    if limit == 0 {
        return Err(Box::new(io::Error::other(
            "export-workload-corpus limit must be greater than zero",
        )));
    }

    let repositories = config.repositories();
    let mut rows = Vec::new();

    for repo in &repositories {
        let root = config.root_by_repository_id(&repo.repository_id.0).ok_or_else(|| {
            io::Error::other(format!(
                "export-workload-corpus summary status=failed repository_id={} error=workspace root lookup failed",
                repo.repository_id.0
            ))
        })?;
        let db_path = resolve_storage_db_path(root, "export-workload-corpus")?;
        let storage = Storage::new(&db_path);
        let repo_rows = storage
            .load_recent_provenance_events(limit)
            .map_err(|err| {
                io::Error::other(format!(
                    "export-workload-corpus failed for repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                ))
            })?;

        let exported_count = repo_rows.len();
        for row in repo_rows {
            let payload = serde_json::from_str::<Value>(&row.payload_json).unwrap_or_else(|_| {
                Value::Object(Map::from_iter([(
                    "payload_decode_error".to_owned(),
                    Value::String(bounded_workload_corpus_text(&row.payload_json)),
                )]))
            });
            let repository_id = payload
                .get("target_repository_id")
                .and_then(|value| value.as_str())
                .unwrap_or(&repo.repository_id.0)
                .to_owned();
            let source_refs_summary = workload_corpus_summary_field(&payload, "source_refs");
            let source_ref_count = payload
                .get("source_refs")
                .and_then(|value| value.as_array())
                .map_or(0, Vec::len);

            rows.push(WorkloadCorpusExportRow {
                trace_id: row.trace_id,
                created_at: row.created_at,
                repository_id,
                tool_name: row.tool_name,
                parameter_summary: workload_corpus_summary_field(&payload, "params"),
                outcome_summary: workload_corpus_summary_field(&payload, "outcome"),
                source_refs_summary,
                source_ref_count,
                normalized_workload: payload
                    .get("normalized_workload")
                    .map(|value| sanitize_workload_corpus_value(value, WORKLOAD_CORPUS_MAX_DEPTH)),
            });
        }

        println!(
            "export-workload-corpus ok repository_id={} root={} db={} rows={}",
            repo.repository_id.0,
            root.display(),
            db_path.display(),
            exported_count
        );
    }

    rows.sort_by(|left, right| {
        left.repository_id
            .cmp(&right.repository_id)
            .then(left.created_at.cmp(&right.created_at))
            .then(left.trace_id.cmp(&right.trace_id))
            .then(left.tool_name.cmp(&right.tool_name))
    });

    let parent = output_path.parent().ok_or_else(|| {
        io::Error::other(format!(
            "export-workload-corpus output path has no parent: {}",
            output_path.display()
        ))
    })?;
    std::fs::create_dir_all(parent)?;

    match format {
        WorkloadCorpusExportFormat::Json => {
            std::fs::write(output_path, to_string_pretty(&rows)?)?;
        }
        WorkloadCorpusExportFormat::Jsonl => {
            let mut encoded = String::new();
            for row in &rows {
                encoded.push_str(&serde_json::to_string(row)?);
                encoded.push('\n');
            }
            std::fs::write(output_path, encoded)?;
        }
    }

    println!(
        "export-workload-corpus summary status=ok repositories={} rows={} format={} output={} limit={}",
        repositories.len(),
        rows.len(),
        format.as_str(),
        output_path.display(),
        limit
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
                let repair_summary = match storage.repair_storage_invariants() {
                    Ok(summary) => summary,
                    Err(err) => {
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
                };

                if let Err(err) = storage.verify() {
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
                let repaired_categories = if repair_summary.repaired_categories.is_empty() {
                    "none".to_string()
                } else {
                    repair_summary.repaired_categories.join(",")
                };
                println!(
                    "{command_name} ok repository_id={} root={} db={} repaired={}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    repaired_categories
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
                "startup strict vector readiness failed repository_id={} root={} db={}: storage db file is missing; run `frigg init` from {} or `frigg init --workspace-root {}` first",
                repo.repository_id.0,
                root.display(),
                db_path.display(),
                root.display(),
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
