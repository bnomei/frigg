//! CLI command handlers for storage bootstrap, reindex, playbooks, and workload corpus export.

mod adopt;
mod hash;
#[allow(dead_code)]
mod hook;
mod playbooks;
mod reindex;
mod storage;
mod workload_corpus;

pub(crate) use adopt::run_adopt_command;
pub(crate) use hash::run_hash_command;
pub(crate) use playbooks::run_hybrid_playbook_command;
pub(crate) use reindex::run_reindex_command;
pub(crate) use storage::{
    StorageBootstrapCommand, StorageMaintenanceCommand, run_storage_bootstrap_command,
    run_storage_maintenance_command,
};
pub(crate) use workload_corpus::run_workload_corpus_export_command;
