//! MCP delivery layer for Frigg's stable default tool surface and advanced runtime extensions.

pub mod deep_search;
mod explorer;
mod provenance_cache;
mod server;
mod server_state;
pub mod tool_surface;
pub mod types;
mod workspace_registry;

pub use deep_search::{
    DeepSearchCitation, DeepSearchCitationPayload, DeepSearchClaim, DeepSearchFileSpan,
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchPlaybookStep, DeepSearchReplayCheck,
    DeepSearchTraceArtifact, DeepSearchTraceOutcome, DeepSearchTraceStep,
};
pub use server::{FriggMcpServer, FriggMcpService};
