use std::error::Error;
use std::io;

use frigg::settings::FriggConfig;
use frigg::storage::Storage;

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
    Prune {
        keep_manifest_snapshots: usize,
        keep_provenance_events: usize,
    },
}

pub(crate) fn run_storage_bootstrap_command(
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

pub(crate) fn run_storage_maintenance_command(
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
