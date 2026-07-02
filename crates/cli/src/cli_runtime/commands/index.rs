//! CLI `index` command: full or changed-only manifest rebuild across configured workspace roots.
//!
//! After each repository index completes, changed and deleted paths may trigger synchronous
//! SCIP precise-artifact generation when configured generators need refresh.

use std::error::Error;

use frigg::domain::FriggError;
use frigg::indexer::{
    IndexMode, ManifestDiagnosticKind, index_repository_with_runtime_config_and_plan_callback,
};
use frigg::mcp::FriggMcpServer;
use frigg::settings::{FriggConfig, SemanticRuntimeCredentials};
use frigg::storage::Storage;

use super::super::storage_auto_heal::{
    initialize_storage_with_auto_repair, verify_storage_with_auto_repair,
};
use super::{CliPreciseGenerationCounters, precise_counter_fields, run_cli_precise_generation};
use crate::cli_runtime::storage_paths::ensure_storage_db_path_for_write;
use crate::cli_runtime::{
    CliOutput, OutputLevel, emit_index_plan_events, field, format_output_event_line,
    report_storage_failure, reported_error,
};

#[cfg(test)]
pub(crate) fn run_index_command(config: &FriggConfig, changed: bool) -> Result<(), Box<dyn Error>> {
    run_index_command_with_output(config, changed, &CliOutput::normal())
}

/// Runs full or changed-only indexing for every configured repository and reports structured progress.
pub(crate) fn run_index_command_with_output(
    config: &FriggConfig,
    changed: bool,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let mode = if changed {
        IndexMode::ChangedOnly
    } else {
        IndexMode::Full
    };
    let mode_name = mode.as_str();
    let mut total_files_scanned = 0usize;
    let mut total_files_changed = 0usize;
    let mut total_files_deleted = 0usize;
    let mut total_diagnostics = 0usize;
    let mut total_walk_diagnostics = 0usize;
    let mut total_read_diagnostics = 0usize;
    let mut total_duration_ms = 0u128;
    let precise_server = FriggMcpServer::new(config.clone());
    let mut total_precise = CliPreciseGenerationCounters::default();

    for repo in &repositories {
        let root = match config.root_by_repository_id(&repo.repository_id.0) {
            Some(root) => root,
            None => {
                let message = format_output_event_line(
                    OutputLevel::Error,
                    "index",
                    "failed",
                    &[
                        field("status", "failed"),
                        field("mode", mode_name),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                );
                output.error_event(
                    "index",
                    "failed",
                    &[
                        field("status", "failed"),
                        field("mode", mode_name),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                )?;
                return Err(reported_error(message));
            }
        };
        let db_path = ensure_storage_db_path_for_write(root, "index")?;
        let storage = Storage::new(&db_path);

        match initialize_storage_with_auto_repair(&storage) {
            Ok(repaired_categories) => {
                if !repaired_categories.is_empty() {
                    output.progress_event(
                        OutputLevel::Warn,
                        "storage",
                        "auto_repair",
                        &[
                            field("status", "ok"),
                            field("command", "index"),
                            field("mode", mode_name),
                            field("repo", &repo.repository_id.0),
                            field("repaired", repaired_categories.join(",")),
                            field("db", db_path.display()),
                        ],
                        Some(&root.display().to_string()),
                    )?;
                }
            }
            Err(err) => {
                report_storage_failure(
                    output,
                    "index",
                    repositories.len(),
                    &repo.repository_id.0,
                    root,
                    &db_path,
                    &err,
                )?;
                return Err(reported_error(format!(
                    "index failed mode={mode_name} repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                )));
            }
        }

        let summary = match index_repository_with_runtime_config_and_plan_callback(
            &repo.repository_id.0,
            root,
            &db_path,
            mode,
            &config.semantic_runtime,
            &SemanticRuntimeCredentials::from_process_env(),
            |plan| {
                emit_index_plan_events(*output, &repo.repository_id.0, plan, &[])
                    .map_err(FriggError::from)
            },
        ) {
            Ok(summary) => summary,
            Err(err) => {
                output.error_event(
                    "index",
                    "failed",
                    &[
                        field("status", "failed"),
                        field("mode", mode_name),
                        field("repos", repositories.len()),
                        field("repo", &repo.repository_id.0),
                        field("db", db_path.display()),
                        field("error", &err),
                    ],
                    Some(&root.display().to_string()),
                )?;
                return Err(reported_error(format!(
                    "index failed mode={mode_name} repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                )));
            }
        };

        let storage_sanity = if config.semantic_runtime.enabled {
            verify_storage_with_auto_repair(&storage).map(|_| ())
        } else {
            match storage.verify_relational_schema() {
                Ok(()) => Ok(()),
                Err(_) => verify_storage_with_auto_repair(&storage).map(|_| ()),
            }
        };
        if let Err(err) = storage_sanity {
            report_storage_failure(
                output,
                "index",
                repositories.len(),
                &repo.repository_id.0,
                root,
                &db_path,
                &err,
            )?;
            return Err(reported_error(format!(
                "index failed mode={mode_name} repository_id={} root={} db={}: {err}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            )));
        }

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

        if summary.semantic_refresh_mode.as_str() != "disabled"
            && summary.semantic_refresh_mode.as_str() != "reuse_existing"
        {
            output.progress_event(
                OutputLevel::Info,
                "index",
                "semantic",
                &[
                    field("status", "ok"),
                    field("repo", &repo.repository_id.0),
                    field("mode", summary.semantic_refresh_mode.as_str()),
                    field(
                        "provider",
                        summary.semantic_provider.as_deref().unwrap_or("-"),
                    ),
                    field("model", summary.semantic_model.as_deref().unwrap_or("-")),
                    field("records", summary.semantic_records),
                ],
                None,
            )?;
        }

        for diagnostic in &summary.diagnostics.entries {
            let path = diagnostic
                .path
                .as_ref()
                .map(|path| path.display().to_string());
            output.diagnostic_event(
                OutputLevel::Warn,
                "index",
                "diagnostic",
                &[
                    field("kind", diagnostic.kind.as_str()),
                    field("repo", &repo.repository_id.0),
                    field("mode", mode_name),
                    field("message", &diagnostic.message),
                ],
                path.as_deref(),
            )?;
        }

        let precise_counters = run_cli_precise_generation(
            &precise_server,
            *output,
            "index",
            &repo.repository_id.0,
            root,
            &summary.changed_paths,
            &summary.deleted_paths,
        );
        total_precise.add(precise_counters);

        let mut repo_fields = vec![
            field("status", "ok"),
            field("repo", &repo.repository_id.0),
            field("mode", mode_name),
            field("snapshot", &summary.snapshot_id),
            field("scanned", summary.files_scanned),
            field("changed", summary.files_changed),
            field("deleted", summary.files_deleted),
            field("diagnostics", diagnostics_total),
            field("diagnostics_walk", diagnostics_walk),
            field("diagnostics_read", diagnostics_read),
            field("duration_ms", summary.duration_ms),
            field("db", db_path.display()),
        ];
        repo_fields.extend(precise_counter_fields(precise_counters));
        output.progress_event(
            OutputLevel::Ok,
            "index",
            "repo",
            &repo_fields,
            Some(&root.display().to_string()),
        )?;
    }

    let mut summary_fields = vec![
        field("status", "ok"),
        field("mode", mode_name),
        field("repos", repositories.len()),
        field("scanned", total_files_scanned),
        field("changed", total_files_changed),
        field("deleted", total_files_deleted),
        field("diagnostics", total_diagnostics),
        field("diagnostics_walk", total_walk_diagnostics),
        field("diagnostics_read", total_read_diagnostics),
        field("duration_ms", total_duration_ms),
    ];
    summary_fields.extend(precise_counter_fields(total_precise));
    output.summary_event(OutputLevel::Ok, "index", "complete", &summary_fields, None)?;
    Ok(())
}
