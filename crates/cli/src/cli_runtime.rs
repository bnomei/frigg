//! CLI runtime helpers that resolve configuration, run startup gates, and dispatch utility
//! commands without pulling watch or HTTP transport wiring into every handler.
//!
//! Semantic map:
//! - `config_resolution` — maps CLI flags into `FriggConfig` for serve and utility commands.
//! - `startup_gates` — semantic-runtime and vector-readiness checks before serving or index.
//! - `storage_paths` — storage database path resolution for workspace roots.
//! - `commands` — init, index, storage maintenance, and context summaries.

mod commands;
mod config_resolution;
mod output;
mod startup_gates;
mod storage_auto_heal;
mod storage_paths;

pub(crate) use commands::{
    StorageMaintenanceCommand, report_storage_failure, run_adopt_command_with_output,
    run_context_summary_command, run_hash_command, run_index_command_with_output,
    run_pretooluse_hook_command, run_storage_init_command_with_output,
    run_storage_maintenance_command_with_output,
};
pub(crate) use config_resolution::{
    resolve_command_config, resolve_startup_config, resolve_watch_runtime_config,
};
pub(crate) use output::{
    CliOutput, OutputField, OutputLevel, emit_index_plan_events, emit_index_progress_event,
    error_was_reported, field, format_output_event_line, reported_error, reported_io_error,
};
pub(crate) use startup_gates::{
    run_semantic_runtime_startup_gate_with_output,
    run_semantic_runtime_startup_gate_with_stderr_prepare_output,
    run_strict_startup_vector_readiness_gate_with_output,
};

#[cfg(test)]
pub(crate) use commands::{
    run_index_command, run_storage_init_command, run_storage_maintenance_command,
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
