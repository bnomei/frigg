//! Hybrid search execution: multi-channel retrieval orchestration and fusion.
//!
//! Coordinates lexical widening, path-witness recall, graph channel, optional semantic retrieval,
//! ranker blending, and post-selection guardrails into one `SearchHybridExecutionOutput`.

#[path = "hybrid_execution/fusion.rs"]
mod fusion;
#[path = "hybrid_execution/pipeline.rs"]
mod pipeline;

pub(in crate::searcher) use pipeline::search_hybrid_with_filters_using_executor;
