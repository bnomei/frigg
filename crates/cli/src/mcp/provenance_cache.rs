use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProvenanceStorageCacheKey {
    pub repository_id: String,
    pub db_path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProvenancePersistenceStage {
    ResolveStoragePath,
    InitializeStorage,
    AppendEvent,
}

impl ProvenancePersistenceStage {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ResolveStoragePath => "resolve_storage_path",
            Self::InitializeStorage => "initialize_storage",
            Self::AppendEvent => "append_event",
        }
    }

    pub(crate) fn retryable(self) -> bool {
        matches!(self, Self::InitializeStorage | Self::AppendEvent)
    }
}
