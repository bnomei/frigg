//! CLI storage bootstrap and maintenance commands for init, repair, and prune.

use std::error::Error;
use std::io;
use std::path::Path;

use frigg::settings::FriggConfig;
use frigg::storage::Storage;

use super::super::storage_auto_heal::initialize_storage_with_auto_repair;
use crate::cli_runtime::storage_paths::{
    ensure_storage_db_path_for_write, resolve_storage_db_path,
};
use crate::cli_runtime::{CliOutput, OutputLevel, field, format_output_event_line, reported_error};

#[derive(Debug, Clone, Copy)]
pub(crate) enum StorageMaintenanceCommand {
    RepairSemanticVectorStore,
    Prune { keep_manifest_snapshots: usize },
}

fn storage_failure_next_step(command_name: &str, error: &str, db_path: &Path) -> String {
    if error.contains("storage db file is missing") {
        return format!(
            "run `frigg init` or `frigg index` to create current storage at {}",
            db_path.display()
        );
    }

    if error.contains("exists but is not a file") {
        return format!(
            "move or delete {}, then run `frigg init` or `frigg index`",
            db_path.display()
        );
    }

    if error.contains("storage schema is uninitialized") {
        return format!(
            "run `frigg init` or `frigg index`; if {} is not a Frigg database, delete it first",
            db_path.display()
        );
    }

    if error.contains("legacy non-sqlite-vec schema")
        || error.contains("vector table schema mismatch")
        || error.contains("missing vector table")
    {
        if command_name == "repair-storage" {
            return format!(
                "`frigg repair-storage` could not rebuild vector storage; delete {} and run `frigg index`",
                db_path.display()
            );
        }
        return format!(
            "rerun `frigg init` or `frigg index`; if repair fails, delete {} and run `frigg index`",
            db_path.display()
        );
    }

    if error.contains("storage schema is incompatible")
        || error.contains("schema version mismatch")
        || error.contains("missing required table")
    {
        return format!(
            "delete {} and run `frigg index` to rebuild Frigg's local index state",
            db_path.display()
        );
    }

    if error.contains("semantic_vector_partition_in_sync") {
        if command_name == "repair-storage" {
            return format!(
                "`frigg repair-storage` could not restore invariants; delete {} and run `frigg index`",
                db_path.display()
            );
        }
        return "rerun `frigg init` or `frigg index`; if it still fails, delete the storage DB and run `frigg index`"
            .to_owned();
    }

    if error.contains("readonly")
        || error.contains("read-only")
        || error.contains("permission")
        || error.contains("Permission")
    {
        return format!(
            "check write permissions for {} and its parent directory, then rerun `{command_name}`",
            db_path.display()
        );
    }

    format!(
        "rerun `frigg --verbose {command_name}`; if the error persists, delete {db_path} and run `frigg index`",
        db_path = db_path.display()
    )
}

pub(crate) fn report_storage_failure(
    output: &CliOutput,
    command_name: &str,
    repositories_len: usize,
    repository_id: &str,
    root: &Path,
    db_path: &Path,
    err: &dyn Error,
) -> io::Result<()> {
    let error = err.to_string();
    output.error_event(
        command_name,
        "failed",
        &[
            field("status", "failed"),
            field("repos", repositories_len),
            field("repo", repository_id),
            field("db", db_path.display()),
            field(
                "next",
                storage_failure_next_step(command_name, &error, db_path),
            ),
            field("error", error),
        ],
        Some(&root.display().to_string()),
    )
}

#[cfg(test)]
pub(crate) fn run_storage_init_command(config: &FriggConfig) -> Result<(), Box<dyn Error>> {
    run_storage_init_command_with_output(config, &CliOutput::normal())
}

pub(crate) fn run_storage_init_command_with_output(
    config: &FriggConfig,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let command_name = "init";

    for repo in &repositories {
        let root = match config.root_by_repository_id(&repo.repository_id.0) {
            Some(root) => root,
            None => {
                let message = format_output_event_line(
                    OutputLevel::Error,
                    command_name,
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                );
                output.error_event(
                    command_name,
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                )?;
                return Err(reported_error(message));
            }
        };
        let db_path = ensure_storage_db_path_for_write(root, command_name)?;
        let storage = Storage::new(&db_path);

        let repaired_categories = match initialize_storage_with_auto_repair(&storage) {
            Ok(categories) => categories,
            Err(err) => {
                report_storage_failure(
                    output,
                    command_name,
                    repositories.len(),
                    &repo.repository_id.0,
                    root,
                    &db_path,
                    &err,
                )?;
                return Err(reported_error(format!(
                    "{command_name} failed for repository_id={} root={} db={}: {err}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display()
                )));
            }
        };
        if !repaired_categories.is_empty() {
            output.progress_event(
                OutputLevel::Warn,
                "storage",
                "auto_repair",
                &[
                    field("status", "ok"),
                    field("command", command_name),
                    field("repo", &repo.repository_id.0),
                    field("repaired", repaired_categories.join(",")),
                    field("db", db_path.display()),
                ],
                Some(&root.display().to_string()),
            )?;
        }

        output.progress_event(
            OutputLevel::Ok,
            command_name,
            "repo",
            &[
                field("status", "ok"),
                field("repo", &repo.repository_id.0),
                field("db", db_path.display()),
            ],
            Some(&root.display().to_string()),
        )?;
    }

    output.summary_event(
        OutputLevel::Ok,
        command_name,
        "complete",
        &[field("status", "ok"), field("repos", repositories.len())],
        None,
    )?;
    Ok(())
}

#[cfg(test)]
pub(crate) fn run_storage_maintenance_command(
    config: &FriggConfig,
    command: StorageMaintenanceCommand,
) -> Result<(), Box<dyn Error>> {
    run_storage_maintenance_command_with_output(config, command, &CliOutput::normal())
}

pub(crate) fn run_storage_maintenance_command_with_output(
    config: &FriggConfig,
    command: StorageMaintenanceCommand,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let command_name = match command {
        StorageMaintenanceCommand::RepairSemanticVectorStore => "repair-storage",
        StorageMaintenanceCommand::Prune { .. } => "prune-storage",
    };
    let mut total_repaired = 0usize;
    let mut total_manifest_snapshots_deleted = 0usize;

    for repo in &repositories {
        let root = match config.root_by_repository_id(&repo.repository_id.0) {
            Some(root) => root,
            None => {
                let message = format_output_event_line(
                    OutputLevel::Error,
                    command_name,
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                );
                output.error_event(
                    command_name,
                    "failed",
                    &[
                        field("status", "failed"),
                        field("repo", &repo.repository_id.0),
                        field("error", "workspace root lookup failed"),
                    ],
                    None,
                )?;
                return Err(reported_error(message));
            }
        };
        let db_path = resolve_storage_db_path(root, command_name)?;
        let storage = Storage::new(&db_path);
        if let Err(err) = storage.require_current_schema() {
            report_storage_failure(
                output,
                command_name,
                repositories.len(),
                &repo.repository_id.0,
                root,
                &db_path,
                &err,
            )?;
            return Err(reported_error(format!(
                "{command_name} failed for repository_id={} root={} db={}: {err}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            )));
        }

        match command {
            StorageMaintenanceCommand::RepairSemanticVectorStore => {
                let repair_summary = match storage.repair_storage_invariants() {
                    Ok(summary) => summary,
                    Err(err) => {
                        report_storage_failure(
                            output,
                            command_name,
                            repositories.len(),
                            &repo.repository_id.0,
                            root,
                            &db_path,
                            &err,
                        )?;
                        return Err(reported_error(format!(
                            "{command_name} failed for repository_id={} root={} db={}: {err}",
                            repo.repository_id.0,
                            root.display(),
                            db_path.display()
                        )));
                    }
                };

                if let Err(err) = storage.verify() {
                    report_storage_failure(
                        output,
                        command_name,
                        repositories.len(),
                        &repo.repository_id.0,
                        root,
                        &db_path,
                        &err,
                    )?;
                    return Err(reported_error(format!(
                        "{command_name} failed for repository_id={} root={} db={}: {err}",
                        repo.repository_id.0,
                        root.display(),
                        db_path.display()
                    )));
                }

                total_repaired += 1;
                let repaired_categories = if repair_summary.repaired_categories.is_empty() {
                    "none".to_string()
                } else {
                    repair_summary.repaired_categories.join(",")
                };
                output.progress_event(
                    OutputLevel::Ok,
                    command_name,
                    "repo",
                    &[
                        field("status", "ok"),
                        field("repo", &repo.repository_id.0),
                        field("repaired", repaired_categories),
                        field("db", db_path.display()),
                    ],
                    Some(&root.display().to_string()),
                )?;
            }
            StorageMaintenanceCommand::Prune {
                keep_manifest_snapshots,
            } => {
                let deleted_manifest_snapshots = match storage
                    .prune_repository_snapshots(&repo.repository_id.0, keep_manifest_snapshots)
                {
                    Ok(deleted) => deleted,
                    Err(err) => {
                        report_storage_failure(
                            output,
                            command_name,
                            repositories.len(),
                            &repo.repository_id.0,
                            root,
                            &db_path,
                            &err,
                        )?;
                        return Err(reported_error(format!(
                            "{command_name} failed for repository_id={} root={} db={}: {err}",
                            repo.repository_id.0,
                            root.display(),
                            db_path.display()
                        )));
                    }
                };

                if let Err(err) = storage.verify_relational_schema() {
                    report_storage_failure(
                        output,
                        command_name,
                        repositories.len(),
                        &repo.repository_id.0,
                        root,
                        &db_path,
                        &err,
                    )?;
                    return Err(reported_error(format!(
                        "{command_name} failed for repository_id={} root={} db={}: {err}",
                        repo.repository_id.0,
                        root.display(),
                        db_path.display()
                    )));
                }

                total_manifest_snapshots_deleted += deleted_manifest_snapshots;
                output.progress_event(
                    OutputLevel::Ok,
                    command_name,
                    "repo",
                    &[
                        field("status", "ok"),
                        field("repo", &repo.repository_id.0),
                        field("keep_manifest_snapshots", keep_manifest_snapshots),
                        field("manifest_snapshots_deleted", deleted_manifest_snapshots),
                        field("db", db_path.display()),
                    ],
                    Some(&root.display().to_string()),
                )?;
            }
        }
    }

    match command {
        StorageMaintenanceCommand::RepairSemanticVectorStore => {
            output.summary_event(
                OutputLevel::Ok,
                command_name,
                "complete",
                &[
                    field("status", "ok"),
                    field("repos", repositories.len()),
                    field("repaired", total_repaired),
                ],
                None,
            )?;
        }
        StorageMaintenanceCommand::Prune {
            keep_manifest_snapshots,
        } => {
            output.summary_event(
                OutputLevel::Ok,
                command_name,
                "complete",
                &[
                    field("status", "ok"),
                    field("repos", repositories.len()),
                    field("keep_manifest_snapshots", keep_manifest_snapshots),
                    field(
                        "manifest_snapshots_deleted",
                        total_manifest_snapshots_deleted,
                    ),
                ],
                None,
            )?;
        }
    }

    Ok(())
}
