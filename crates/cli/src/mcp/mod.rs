//! MCP delivery layer that packages Frigg's retrieval and indexing capabilities as a stable tool
//! surface for agents. This is where runtime state, schemas, and transport-facing orchestration
//! meet the lower-level search and storage subsystems.

pub mod advanced;
mod explorer;
mod guidance;
mod provenance_cache;
mod server;
mod server_cache;
mod server_state;
pub mod tool_surface;
pub mod types;
pub(crate) mod workspace_registry;

pub use advanced::deep_search::{
    DeepSearchCitation, DeepSearchCitationPayload, DeepSearchClaim, DeepSearchFileSpan,
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchPlaybookStep, DeepSearchReplayCheck,
    DeepSearchTraceArtifact, DeepSearchTraceOutcome, DeepSearchTraceStep,
};
pub use server::{FriggMcpServer, FriggMcpService};
pub use server_state::RuntimeTaskRegistry;
