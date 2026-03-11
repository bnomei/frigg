//! MCP delivery layer for Frigg's stable default tool surface and optional advanced-consumer extensions.

pub mod advanced;
mod explorer;
mod guidance;
mod provenance_cache;
mod server;
mod server_state;
pub mod tool_surface;
pub mod types;
mod workspace_registry;

pub use advanced::deep_search::{
    DeepSearchCitation, DeepSearchCitationPayload, DeepSearchClaim, DeepSearchFileSpan,
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchPlaybookStep, DeepSearchReplayCheck,
    DeepSearchTraceArtifact, DeepSearchTraceOutcome, DeepSearchTraceStep,
};
pub use server::{FriggMcpServer, FriggMcpService};
pub use server_state::RuntimeTaskRegistry;
