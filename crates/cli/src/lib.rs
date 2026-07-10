//! Frigg is organized as a pipeline: shared domain and settings types describe the contract,
//! indexing and storage build durable repository artifacts, search and graph layers answer
//! retrieval questions from those artifacts, and MCP plus watch turn the whole system into a
//! long-lived agent-facing service.
//!
//! Semantic map:
//! - `domain` — shared error, evidence, path-class, and workload vocabulary across layers.
//! - `settings` — runtime profile, semantic provider, lexical backend, and watch switches.
//! - `indexer` — manifests, symbol extraction, semantic chunks, and index planning.
//! - `storage` — durable SQLite state for manifests, projections, vectors, and provenance.
//! - `searcher` — lexical, path-witness, graph, and semantic hybrid retrieval + ranking policy.
//! - `graph` — heuristic and SCIP-backed precise navigation substrate.
//! - `embeddings` — local and remote embedding providers behind one readiness boundary.
//! - `mcp` — agent-facing tool surface, session adoption, recovery, and runtime caches.
//! - `watch` — lease-gated filesystem supervisor and debounced index scheduler.
//! - `agent_directive` / `context_efficiency` — Frigg-first policy text and savings telemetry.
//! - `test_support` — lightweight config helpers for integration tests.

/// Canonical agent directive text and rendering helpers reused across MCP, hooks, and docs checks.
pub mod agent_directive;
/// Shared context-efficiency contracts, guards, and process-local summary caches.
pub mod context_efficiency;
/// Shared domain vocabulary reused across indexing, retrieval, provenance, and MCP responses so
/// each layer can exchange the same evidence model without translation glue.
pub mod domain;
/// Embedding providers and vector-readiness checks used when semantic search is enabled so
/// indexing and serving can gate semantic work through one capability boundary.
pub mod embeddings;
/// Symbol and relation graph primitives that power navigation-style retrieval on top of heuristic
/// and precise artifact ingestion.
pub mod graph;
/// Shared human-output block primitives used by CLI-facing renderers.
#[doc(hidden)]
pub mod human_output;
/// Repository artifact construction, including manifests, index planning, symbol extraction, and
/// semantic chunk generation that feed the search and MCP layers.
pub mod indexer;
pub(crate) mod language_support;
pub(crate) mod languages;
mod manifest_validation;
/// MCP delivery surface that exposes Frigg's repository tooling as stable agent-facing methods and
/// schemas.
pub mod mcp;
pub(crate) mod path_class;
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
pub(crate) mod vendor_grammars;
/// Incremental freshness runtime that keeps attached workspaces indexed without pushing watch
/// logic into request handlers.
pub mod watch;
pub(crate) mod workspace_ignores;
