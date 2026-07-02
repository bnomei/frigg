//! CLI command handlers for storage bootstrap, reindex, and context summaries.

mod adopt;
mod context;
mod hash;
#[allow(dead_code)]
mod hook;
mod prepare_semantic_model;
mod reindex;
mod storage;

pub(crate) use adopt::run_adopt_command_with_output;
pub(crate) use context::run_context_summary_command;
pub(crate) use hash::run_hash_command;
pub(crate) use prepare_semantic_model::run_prepare_semantic_model_command_with_output;
pub(crate) use reindex::run_reindex_command_with_output;
pub(crate) use storage::{
    StorageBootstrapCommand, StorageMaintenanceCommand, run_storage_bootstrap_command_with_output,
    run_storage_maintenance_command_with_output,
};

#[cfg(test)]
pub(crate) use reindex::run_reindex_command;
#[cfg(test)]
pub(crate) use storage::{run_storage_bootstrap_command, run_storage_maintenance_command};
