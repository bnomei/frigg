//! MCP delivery layer that packages Frigg's retrieval and indexing capabilities as a stable tool
//! surface for agents. This is where runtime state, schemas, and transport-facing orchestration
//! meet the lower-level search and storage subsystems.

/// Advanced-consumer MCP extensions that build on the stable runtime surface.
pub mod advanced;
mod explorer;
mod guidance;
/// Local opt-in routing stats — process-local counters, no cloud telemetry.
pub mod routing_stats;
mod server;
mod server_cache;
mod server_state;
/// Stable MCP tool names, schemas, and handler wiring shared by stdio and HTTP transports.
pub mod tool_surface;
/// Wire contracts for Frigg's public MCP tool surface.
pub mod types;
pub(crate) mod workspace_registry;

pub use advanced::deep_search::{
    DeepSearchCitation, DeepSearchCitationPayload, DeepSearchClaim, DeepSearchFileSpan,
    DeepSearchHarness, DeepSearchPlaybook, DeepSearchPlaybookStep, DeepSearchReplayCheck,
    DeepSearchTraceArtifact, DeepSearchTraceOutcome, DeepSearchTraceStep,
};
pub use server::{
    FriggMcpServer, FriggMcpService, PreciseGraphBenchmarkSummary, SymbolCorpusBenchmarkSummary,
    ToolCallDisplayEvent, ToolCallDisplaySink, ToolCallDisplayStatus,
    benchmark_build_symbol_corpora, benchmark_build_symbol_corpora_for_server,
    benchmark_precise_graph_for_server,
};
pub use server_state::{RuntimeTaskGuard, RuntimeTaskRegistry};
