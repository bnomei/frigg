//! Shared domain vocabulary used across indexing, search, storage, provenance, and MCP delivery.
//! These types stay intentionally neutral so higher layers can change behavior without redefining
//! the core concepts they exchange.

pub mod error;
pub mod evidence;
pub mod model;
pub mod provenance;
pub mod search;

pub use error::{FriggError, FriggResult};
pub use evidence::{
    ChannelDiagnostic, ChannelHealth, ChannelHealthStatus, ChannelResult, ChannelStats,
    EvidenceAnchor, EvidenceAnchorKind, EvidenceChannel, EvidenceDocumentRef, EvidenceHit,
    FriggLayer, ProductRing, SupportLevel,
};
pub use provenance::{
    NormalizedWorkloadMetadata, WorkloadFallbackReason, WorkloadPrecisionMode,
    WorkloadRepositoryScope, WorkloadRepositoryScopeKind, WorkloadStageAttribution,
    WorkloadStageSample, WorkloadToolClass, WorkloadToolFamily,
};
pub use search::{
    ArtifactBias, FrameworkHint, PathClass, PlannerStrictness, PlaybookReferencePolicy, SearchGoal,
    SearchIntentRuleId, SourceClass,
};
