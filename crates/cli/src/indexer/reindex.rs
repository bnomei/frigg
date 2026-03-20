mod execution;
mod plan;
mod semantic;
mod store;

#[cfg(test)]
pub(crate) use plan::build_reindex_plan_for_tests;
pub use plan::{
    ManifestSnapshotPlan, ReindexDiagnostics, ReindexMode, ReindexPlan, ReindexSummary,
    SemanticRefreshMode, SemanticRefreshPlan,
};
#[cfg(test)]
pub(crate) use semantic::reindex_repository_with_semantic_executor;
pub use semantic::{
    reindex_repository, reindex_repository_with_runtime_config,
    reindex_repository_with_runtime_config_and_dirty_paths,
};
pub use store::ManifestStore;
