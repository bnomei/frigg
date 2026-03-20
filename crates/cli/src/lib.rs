//! Frigg is organized as a pipeline: shared domain and settings types describe the contract,
//! indexing and storage build durable repository artifacts, search and graph layers answer
//! retrieval questions from those artifacts, and MCP plus watch turn the whole system into a
//! long-lived agent-facing service.

/// Shared domain vocabulary reused across indexing, retrieval, provenance, and MCP responses so
/// each layer can exchange the same evidence model without translation glue.
pub mod domain;
/// Embedding providers and vector-readiness checks used when semantic search is enabled so
/// indexing and serving can gate semantic work through one capability boundary.
pub mod embeddings;
/// Symbol and relation graph primitives that power navigation-style retrieval on top of heuristic
/// and precise artifact ingestion.
pub mod graph;
/// Repository artifact construction, including manifests, reindex planning, symbol extraction, and
/// semantic chunk generation that feed the search and MCP layers.
pub mod indexer;
pub(crate) mod language_support;
pub(crate) mod languages;
mod manifest_validation;
/// MCP delivery surface that exposes Frigg's repository tooling as stable agent-facing methods and
/// schemas.
pub mod mcp;
pub(crate) mod path_class;
/// Playbook parsing and regression helpers used to turn retrieval expectations into executable
/// probes.
pub mod playbooks;
/// Retrieval orchestration that blends lexical, graph, and semantic evidence into stable ranked
/// results.
pub mod searcher;
/// Runtime configuration shared by CLI, indexing, watch, and MCP startup so every entry point
/// resolves the same operating profile.
pub mod settings;
/// Durable repository state for manifests, retrieval projections, semantic artifacts, and
/// provenance.
pub mod storage;
/// Shared helpers for exercising the production wiring from tests without rebuilding fixture
/// setup in every suite.
pub mod test_support;
pub(crate) mod text_sanitization;
/// Incremental freshness runtime that keeps attached workspaces reindexed without pushing watch
/// logic into request handlers.
pub mod watch;
pub(crate) mod workspace_ignores;
