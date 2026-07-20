//! CLI command handlers for storage bootstrap, indexing, context summaries, and precise generation.
//!
//! Dispatches CLI handlers that bootstrap storage, rebuild manifests, and run synchronous precise
//! generation hooks.

mod adopt;
mod context;
mod hash;
#[allow(dead_code)]
mod hook;
mod index;
mod precise;
mod stats;
mod status;
mod storage;

pub(crate) use adopt::run_adopt_command_with_output;
pub(crate) use context::run_context_summary_command;
pub(crate) use hash::run_hash_command;
pub(crate) use hook::run_pretooluse_hook_command;
pub(crate) use index::run_index_command_with_output;
pub(crate) use precise::{
    CliPreciseGenerationCounters, precise_counter_fields, run_cli_precise_generation,
};
pub(crate) use stats::run_stats_command;
pub(crate) use status::run_status_command;
pub(crate) use storage::{
    StorageMaintenanceCommand, report_storage_failure, run_storage_init_command_with_output,
    run_storage_maintenance_command_with_output,
};

#[cfg(test)]
pub(crate) use index::{run_index_command, run_index_command_with_embedding_validation};
#[cfg(test)]
pub(crate) use storage::{run_storage_init_command, run_storage_maintenance_command};
