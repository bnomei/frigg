//! CLI storage bootstrap and maintenance commands for init, verify, repair, and prune.

use std::error::Error;
use std::io;
use std::path::Path;

use frigg::settings::FriggConfig;
use frigg::storage::Storage;

use crate::cli_runtime::CliOutput;
use crate::cli_runtime::storage_paths::{
    ensure_storage_db_path_for_write, resolve_storage_db_path,
};

#[derive(Debug, Clone, Copy)]
pub(crate) enum StorageBootstrapCommand {
    Init,
    Verify,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum StorageMaintenanceCommand {
    RepairSemanticVectorStore,
    Prune { keep_manifest_snapshots: usize },
}

fn storage_failure_next_step(command_name: &str, error: &str, db_path: &Path) -> String {
    if error.contains("storage db file is missing") {
        return format!(
            "run `frigg init` or `frigg reindex` to create current storage at {}",
            db_path.display()
        );
    }

    if error.contains("exists but is not a file") {
        return format!(
            "move or delete {}, then run `frigg init` or `frigg reindex`",
            db_path.display()
        );
    }

    if error.contains("storage schema is uninitialized") {
        return format!(
            "run `frigg init` or `frigg reindex`; if {} is not a Frigg database, delete it first",
            db_path.display()
        );
    }

    if error.contains("legacy non-sqlite-vec schema")
        || error.contains("vector table schema mismatch")
        || error.contains("missing vector table")
    {
        if command_name == "repair-storage" {
            return format!(
                "`frigg repair-storage` could not rebuild vector storage; delete {} and run `frigg reindex`",
                db_path.display()
            );
        }
        return format!(
            "run `frigg repair-storage` to rebuild vector storage; if repair fails, delete {} and run `frigg reindex`",
            db_path.display()
        );
    }

    if error.contains("storage schema is incompatible")
        || error.contains("schema version mismatch")
        || error.contains("missing required table")
    {
        return format!(
            "delete {} and run `frigg reindex` to rebuild Frigg's local index state",
            db_path.display()
        );
    }

    if error.contains("semantic_vector_partition_in_sync") {
        if command_name == "repair-storage" {
            return format!(
                "`frigg repair-storage` could not restore invariants; delete {} and run `frigg reindex`",
                db_path.display()
            );
        }
        return "`frigg repair-storage`; if it still fails, delete the storage DB and run `frigg reindex`"
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
        "rerun `frigg --verbose {command_name}`; if the error persists, delete {db_path} and run `frigg reindex`",
        db_path = db_path.display()
    )
}

fn report_storage_failure(
    output: &CliOutput,
    command_name: &str,
    repositories_len: usize,
    repository_id: &str,
    root: &Path,
    db_path: &Path,
    err: &dyn Error,
) -> io::Result<()> {
    let error = err.to_string();
    output.error(format!(
        "{command_name} summary status=failed repositories={repositories_len} repository_id={repository_id} root={} db={} error={error}",
        root.display(),
        db_path.display()
    ))?;
    output.error(format!("{command_name} failure detail: {error}"))?;
    output.error(format!(
        "{command_name} failure next: {}",
        storage_failure_next_step(command_name, &error, db_path)
    ))
}

#[cfg(test)]
pub(crate) fn run_storage_bootstrap_command(
    config: &FriggConfig,
    command: StorageBootstrapCommand,
) -> Result<(), Box<dyn Error>> {
    run_storage_bootstrap_command_with_output(config, command, &CliOutput::normal())
}

pub(crate) fn run_storage_bootstrap_command_with_output(
    config: &FriggConfig,
    command: StorageBootstrapCommand,
    output: &CliOutput,
) -> Result<(), Box<dyn Error>> {
    let repositories = config.repositories();
    let command_name = match command {
        StorageBootstrapCommand::Init => "init",
        StorageBootstrapCommand::Verify => "verify",
    };

    for repo in &repositories {
        let root = match config.root_by_repository_id(&repo.repository_id.0) {
            Some(root) => root,
            None => {
                let message = format!(
                    "{command_name} summary status=failed repository_id={} error=workspace root lookup failed",
                    repo.repository_id.0
                );
                output.error(&message)?;
                return Err(Box::new(io::Error::other(message)));
            }
        };
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
            report_storage_failure(
                output,
                command_name,
                repositories.len(),
                &repo.repository_id.0,
                root,
                &db_path,
                &err,
            )?;
            return Err(Box::new(io::Error::other(format!(
                "{command_name} failed for repository_id={} root={} db={}: {err}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            ))));
        }

        output.progress(format!(
            "{command_name} ok repository_id={} root={} db={}",
            repo.repository_id.0,
            root.display(),
            db_path.display()
        ))?;
    }

    output.summary(format!(
        "{command_name} summary status=ok repositories={}",
        repositories.len()
    ))?;
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
                let message = format!(
                    "{command_name} summary status=failed repository_id={} error=workspace root lookup failed",
                    repo.repository_id.0
                );
                output.error(&message)?;
                return Err(Box::new(io::Error::other(message)));
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
            return Err(Box::new(io::Error::other(format!(
                "{command_name} failed for repository_id={} root={} db={}: {err}",
                repo.repository_id.0,
                root.display(),
                db_path.display()
            ))));
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
                        return Err(Box::new(io::Error::other(format!(
                            "{command_name} failed for repository_id={} root={} db={}: {err}",
                            repo.repository_id.0,
                            root.display(),
                            db_path.display()
                        ))));
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
                output.progress(format!(
                    "{command_name} ok repository_id={} root={} db={} repaired={}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    repaired_categories
                ))?;
            }
            StorageMaintenanceCommand::Prune {
                keep_manifest_snapshots,
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

                total_manifest_snapshots_deleted += deleted_manifest_snapshots;
                output.progress(format!(
                    "{command_name} ok repository_id={} root={} db={} keep_manifest_snapshots={} manifest_snapshots_deleted={}",
                    repo.repository_id.0,
                    root.display(),
                    db_path.display(),
                    keep_manifest_snapshots,
                    deleted_manifest_snapshots
                ))?;
            }
        }
    }

    match command {
        StorageMaintenanceCommand::RepairSemanticVectorStore => {
            output.summary(format!(
                "{command_name} summary status=ok repositories={} repaired={}",
                repositories.len(),
                total_repaired
            ))?;
        }
        StorageMaintenanceCommand::Prune {
            keep_manifest_snapshots,
        } => {
            output.summary(format!(
                "{command_name} summary status=ok repositories={} keep_manifest_snapshots={} manifest_snapshots_deleted={}",
                repositories.len(),
                keep_manifest_snapshots,
                total_manifest_snapshots_deleted
            ))?;
        }
    }

    Ok(())
}
