use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::indexer::HeuristicReference;
use crate::mcp::types::{
    FindDeclarationsResponse, GoToDefinitionResponse, RepositorySummary, SearchHybridResponse,
    SearchSymbolResponse, SearchTextResponse,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceSemanticRefreshPlan {
    pub(crate) latest_snapshot_id: String,
    pub(crate) reason: &'static str,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedRepositorySummary {
    pub(crate) summary: RepositorySummary,
    pub(crate) generated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SearchTextResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) query: String,
    pub(crate) pattern_type: &'static str,
    pub(crate) path_regex: Option<String>,
    pub(crate) limit: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedSearchTextResponse {
    pub(crate) response: SearchTextResponse,
    pub(crate) source_refs: Value,
    pub(crate) generated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SearchHybridResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) query: String,
    pub(crate) language: Option<String>,
    pub(crate) limit: usize,
    pub(crate) semantic: Option<bool>,
    pub(crate) lexical_weight_bits: u32,
    pub(crate) graph_weight_bits: u32,
    pub(crate) semantic_weight_bits: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedSearchHybridResponse {
    pub(crate) response: SearchHybridResponse,
    pub(crate) source_refs: Value,
    pub(crate) generated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SearchSymbolResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) query: String,
    pub(crate) path_class: Option<String>,
    pub(crate) path_regex: Option<String>,
    pub(crate) limit: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedSearchSymbolResponse {
    pub(crate) response: SearchSymbolResponse,
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) diagnostics_count: usize,
    pub(crate) manifest_walk_diagnostics_count: usize,
    pub(crate) manifest_read_diagnostics_count: usize,
    pub(crate) symbol_extraction_diagnostics_count: usize,
    pub(crate) effective_limit: usize,
    pub(crate) generated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GoToDefinitionResponseCacheKey {
    pub(crate) repository_id: Option<String>,
    pub(crate) symbol: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) limit: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedGoToDefinitionResponse {
    pub(crate) response: GoToDefinitionResponse,
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) selected_symbol_id: Option<String>,
    pub(crate) selected_precise_symbol: Option<String>,
    pub(crate) resolution_precision: Option<String>,
    pub(crate) resolution_source: Option<String>,
    pub(crate) effective_limit: usize,
    pub(crate) precise_artifacts_ingested: usize,
    pub(crate) precise_artifacts_failed: usize,
    pub(crate) match_count: usize,
    pub(crate) generated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FindDeclarationsResponseCacheKey {
    pub(crate) repository_id: Option<String>,
    pub(crate) symbol: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) limit: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedFindDeclarationsResponse {
    pub(crate) response: FindDeclarationsResponse,
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) selected_symbol_id: Option<String>,
    pub(crate) selected_precise_symbol: Option<String>,
    pub(crate) resolution_precision: Option<String>,
    pub(crate) resolution_source: Option<String>,
    pub(crate) effective_limit: usize,
    pub(crate) precise_artifacts_ingested: usize,
    pub(crate) precise_artifacts_failed: usize,
    pub(crate) match_count: usize,
    pub(crate) generated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HeuristicReferenceCacheKey {
    pub(crate) repository_id: String,
    pub(crate) symbol_id: String,
    pub(crate) corpus_signature: String,
    pub(crate) scip_signature: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedHeuristicReferences {
    pub(crate) references: Arc<Vec<HeuristicReference>>,
    pub(crate) source_read_diagnostics_count: usize,
    pub(crate) source_files_loaded: usize,
    pub(crate) source_bytes_loaded: u64,
    pub(crate) generated_at: Instant,
}
