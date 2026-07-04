//! Shared domain vocabulary used across indexing, search, storage, provenance, and MCP delivery.
//! These types stay intentionally neutral so higher layers can change behavior without redefining
//! the core concepts they exchange.

/// Stable error vocabulary shared across CLI, storage, and MCP surfaces.
pub mod error;
/// Evidence-channel contracts for multi-source retrieval, health reporting, and replay.
pub mod evidence;
/// Core repository and symbol model types exchanged by indexing and retrieval layers.
pub mod model;
/// Normalized workload metadata emitted by MCP tool invocations.
pub mod provenance;
/// Search planner vocabulary for path classes, source bias, and intent routing.
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
