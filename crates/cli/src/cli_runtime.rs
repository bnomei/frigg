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
mod output;
mod startup_gates;
mod storage_paths;

pub(crate) use commands::{
    StorageBootstrapCommand, StorageMaintenanceCommand, run_adopt_command_with_output,
    run_context_summary_command, run_hash_command, run_hybrid_playbook_command_with_output,
    run_prepare_semantic_model_command_with_output, run_reindex_command_with_output,
    run_storage_bootstrap_command_with_output, run_storage_maintenance_command_with_output,
    run_workload_corpus_export_command_with_output,
};
pub(crate) use config_resolution::{
    resolve_command_config, resolve_startup_config, resolve_watch_runtime_config,
};
pub(crate) use output::CliOutput;
pub(crate) use startup_gates::{
    run_semantic_runtime_startup_gate_with_output,
    run_strict_startup_vector_readiness_gate_with_output,
};

#[cfg(test)]
pub(crate) use commands::{
    run_reindex_command, run_storage_bootstrap_command, run_storage_maintenance_command,
    run_workload_corpus_export_command,
};
#[cfg(test)]
pub(crate) use config_resolution::{resolve_semantic_runtime_config, resolve_watch_config};
#[cfg(test)]
pub(crate) use startup_gates::{
    run_semantic_runtime_startup_gate_with_credentials, run_strict_startup_vector_readiness_gate,
};
#[cfg(test)]
pub(crate) use storage_paths::{
    ensure_storage_db_path_for_write, find_enclosing_git_root, resolve_storage_db_path,
};
