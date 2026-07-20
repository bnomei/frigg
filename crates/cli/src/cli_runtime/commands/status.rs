//! Side-effect-free local status inspection for plugins and operators.

use std::error::Error;

use frigg::domain::model::RepositoryId;
use frigg::mcp::types::{WorkspaceStorageIndexState, WorkspaceStorageSummary};
use frigg::settings::FriggConfig;
use frigg::storage::{Storage, latest_storage_schema_version, resolve_provenance_db_path};
use serde::Serialize;

const STATUS_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Serialize)]
struct StatusSnapshot {
    schema_version: u32,
    frigg_version: &'static str,
    watch: StatusWatch,
    repositories: Vec<StatusRepository>,
}

#[derive(Debug, Serialize)]
struct StatusWatch {
    configured_mode: String,
}

#[derive(Debug, Serialize)]
struct StatusRepository {
    repository_id: String,
    display_name: String,
    root_path: String,
    storage: WorkspaceStorageSummary,
}

/// Prints local storage readiness without starting MCP, adopting a session, or acquiring a watch.
pub(crate) fn run_status_command(config: &FriggConfig, json: bool) -> Result<(), Box<dyn Error>> {
    let snapshot = build_status_snapshot(config);
    if json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
    } else {
        let ready = snapshot
            .repositories
            .iter()
            .filter(|repository| {
                repository.storage.index_state == WorkspaceStorageIndexState::Ready
            })
            .count();
        println!(
            "frigg {}, {ready}/{} repositories ready, watch {}",
            snapshot.frigg_version,
            snapshot.repositories.len(),
            snapshot.watch.configured_mode
        );
    }
    Ok(())
}

fn build_status_snapshot(config: &FriggConfig) -> StatusSnapshot {
    let repositories = config
        .repositories()
        .into_iter()
        .zip(&config.workspace_roots)
        .map(|(repository, root)| {
            let repository_id = RepositoryId::for_root(root).0;
            let storage = inspect_storage(root, &repository_id);
            StatusRepository {
                repository_id,
                display_name: repository.display_name,
                root_path: repository.root_path,
                storage,
            }
        })
        .collect();

    StatusSnapshot {
        schema_version: STATUS_SCHEMA_VERSION,
        frigg_version: env!("CARGO_PKG_VERSION"),
        watch: StatusWatch {
            configured_mode: config.watch.mode.as_str().to_owned(),
        },
        repositories,
    }
}

fn inspect_storage(root: &std::path::Path, repository_id: &str) -> WorkspaceStorageSummary {
    let db_path = match resolve_provenance_db_path(root) {
        Ok(path) => path,
        Err(err) => {
            return WorkspaceStorageSummary {
                db_path: root.join(".frigg/storage.sqlite3").display().to_string(),
                exists: false,
                initialized: false,
                index_state: WorkspaceStorageIndexState::Error,
                error: Some(err.to_string()),
            };
        }
    };
    if !db_path.is_file() {
        return WorkspaceStorageSummary {
            db_path: db_path.display().to_string(),
            exists: false,
            initialized: false,
            index_state: WorkspaceStorageIndexState::MissingDb,
            error: None,
        };
    }

    match Storage::new(&db_path).inspect_repository_read_only(repository_id) {
        Ok(inspection) if inspection.schema_version == 0 => WorkspaceStorageSummary {
            db_path: db_path.display().to_string(),
            exists: true,
            initialized: false,
            index_state: WorkspaceStorageIndexState::Uninitialized,
            error: None,
        },
        Ok(inspection) if inspection.schema_version != latest_storage_schema_version() => {
            WorkspaceStorageSummary {
                db_path: db_path.display().to_string(),
                exists: true,
                initialized: true,
                index_state: WorkspaceStorageIndexState::Error,
                error: Some(format!(
                    "storage schema is incompatible (found {}, expected {})",
                    inspection.schema_version,
                    latest_storage_schema_version()
                )),
            }
        }
        Ok(inspection) => WorkspaceStorageSummary {
            db_path: db_path.display().to_string(),
            exists: true,
            initialized: true,
            index_state: if inspection.has_manifest {
                WorkspaceStorageIndexState::Ready
            } else {
                WorkspaceStorageIndexState::Uninitialized
            },
            error: None,
        },
        Err(err) => WorkspaceStorageSummary {
            db_path: db_path.display().to_string(),
            exists: true,
            initialized: false,
            index_state: WorkspaceStorageIndexState::Error,
            error: Some(err.to_string()),
        },
    }
}
