//! Index orchestration: planning, phased execution, semantic refresh, and manifest persistence.
//!
//! Re-exports the index plan, execution, semantic, and store modules that turn repository changes
//! into durable manifests, retrieval projections, and embedding chunks.

mod execution;
mod plan;
mod semantic;
mod store;

#[cfg(test)]
pub(crate) use plan::build_index_plan_for_tests;
pub use plan::{
    IndexDiagnostics, IndexMode, IndexPlan, IndexProgressEvent, IndexProgressPhase,
    IndexProgressStatus, IndexSummary, ManifestSnapshotPlan, SemanticRefreshMode,
    SemanticRefreshPlan,
};
#[cfg(test)]
pub(crate) use semantic::index_repository_with_semantic_executor;
pub use semantic::{
    index_repository, index_repository_with_runtime_config,
    index_repository_with_runtime_config_and_dirty_paths,
    index_repository_with_runtime_config_and_dirty_paths_and_plan_callback,
    index_repository_with_runtime_config_and_dirty_paths_and_progress_and_commit_callback,
    index_repository_with_runtime_config_and_dirty_paths_and_progress_callback,
    index_repository_with_runtime_config_and_plan_callback,
};
pub use store::ManifestStore;
