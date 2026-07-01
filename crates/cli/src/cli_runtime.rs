//! CLI runtime helpers that resolve configuration, run startup gates, and dispatch utility
//! commands without pulling watch or HTTP transport wiring into every handler.
//!
//! Semantic map:
//! - `config_resolution` — maps CLI flags into `FriggConfig` for serve and utility commands.
//! - `startup_gates` — semantic-runtime and vector-readiness checks before serving or reindex.
//! - `storage_paths` — provenance database path resolution for workspace roots.
//! - `commands` — init, verify, reindex, storage maintenance, playbook, and workload corpus.

mod commands;
mod config_resolution;
mod startup_gates;
mod storage_paths;

pub(crate) use commands::{
    StorageBootstrapCommand, StorageMaintenanceCommand, run_adopt_command, run_hash_command,
    run_hybrid_playbook_command, run_reindex_command, run_storage_bootstrap_command,
    run_storage_maintenance_command, run_workload_corpus_export_command,
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
