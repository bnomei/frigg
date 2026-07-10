//! Graph channel entry point for hybrid search.
//!
//! Expands lexical seeds into **hybrid ranking** hits via durable path-relation projections and/or
//! an **ephemeral** on-demand `SymbolGraph` (file analysis). This pipeline shares relation types
//! with MCP navigation (`crates/cli/src/graph`) but is **not** the navigation graph: it does not
//! run `go_to_definition` / `incoming_calls` / SCIP resolution. Agent-facing honesty:
//! `SearchHybridMatch.graph_mode` + ranking_note (EXP-nav-hybrid-graph-channel).

use super::*;

use super::projection_service::ProjectedGraphContext;

#[path = "graph_channel/internal.rs"]
mod internal;

pub(in crate::searcher) use internal::*;
