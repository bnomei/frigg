mod commands;
mod config_resolution;
mod startup_gates;
mod storage_paths;

pub(crate) use commands::{
    StorageBootstrapCommand, StorageMaintenanceCommand, run_hybrid_playbook_command,
    run_reindex_command, run_storage_bootstrap_command, run_storage_maintenance_command,
    run_workload_corpus_export_command,
};
pub(crate) use config_resolution::{
    resolve_command_config, resolve_startup_config, resolve_watch_runtime_config,
};
pub(crate) use startup_gates::{
    run_semantic_runtime_startup_gate, run_strict_startup_vector_readiness_gate,
};

#[cfg(test)]
pub(crate) use config_resolution::{resolve_semantic_runtime_config, resolve_watch_config};
#[cfg(test)]
pub(crate) use startup_gates::run_semantic_runtime_startup_gate_with_credentials;
#[cfg(test)]
pub(crate) use storage_paths::{
    ensure_storage_db_path_for_write, find_enclosing_git_root, resolve_storage_db_path,
};
