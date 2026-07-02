//! CLI command handlers for storage bootstrap, index, and context summaries.

mod adopt;
mod context;
mod hash;
#[allow(dead_code)]
mod hook;
mod index;
mod storage;

pub(crate) use adopt::run_adopt_command_with_output;
pub(crate) use context::run_context_summary_command;
pub(crate) use hash::run_hash_command;
pub(crate) use index::run_index_command_with_output;
pub(crate) use storage::{
    StorageMaintenanceCommand, report_storage_failure, run_storage_init_command_with_output,
    run_storage_maintenance_command_with_output,
};

#[cfg(test)]
pub(crate) use index::run_index_command;
#[cfg(test)]
pub(crate) use storage::{run_storage_init_command, run_storage_maintenance_command};
