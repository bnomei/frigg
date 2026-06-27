//! Graph channel entry point for hybrid search.
//!
//! Re-exports symbol-graph expansion that turns lexical seeds into precise graph-channel hits via
//! projected relations and on-demand file analysis.

use super::*;

use super::projection_service::ProjectedGraphContext;

#[path = "graph_channel/internal.rs"]
mod internal;

pub(in crate::searcher) use internal::*;
