use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use memchr::memchr_iter;
use serde_json::Value;

use crate::indexer::HeuristicReference;
use crate::mcp::explorer::{
    ExploreMatcher, ExploreScanResult, ExploreScopeRequest, ExploreSpanMatch, LossyLineSlice,
    LossyLineSliceError, normalize_lossy_line_bytes, position_is_before_cursor,
};
use crate::mcp::types::{
    ExploreAnchor, ExploreCursor, ExploreLineWindow, FindDeclarationsResponse,
    GoToDefinitionResponse, RepositorySummary, SearchHybridResponse, SearchSymbolResponse,
    SearchTextResponse, WorkspacePreciseGenerationSummary,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RuntimeCacheFamily {
    ValidatedManifestCandidate,
    ProjectionFamily,
    ProjectedGraphContext,
    SearchTextResponse,
    SearchHybridResponse,
    SearchSymbolResponse,
    GoToDefinitionResponse,
    FindDeclarationsResponse,
    HeuristicReference,
    RepositorySummary,
    CompiledSafeRegex,
    SearcherProjectionStore,
    SearcherHybridGraphFileAnalysis,
    SearcherHybridGraphArtifact,
    SearchCandidateUniverse,
    FileContentWindow,
}

impl RuntimeCacheFamily {
    pub(crate) const ALL: [Self; 16] = [
        Self::ValidatedManifestCandidate,
        Self::ProjectionFamily,
        Self::ProjectedGraphContext,
        Self::SearchTextResponse,
        Self::SearchHybridResponse,
        Self::SearchSymbolResponse,
        Self::GoToDefinitionResponse,
        Self::FindDeclarationsResponse,
        Self::HeuristicReference,
        Self::RepositorySummary,
        Self::CompiledSafeRegex,
        Self::SearcherProjectionStore,
        Self::SearcherHybridGraphFileAnalysis,
        Self::SearcherHybridGraphArtifact,
        Self::SearchCandidateUniverse,
        Self::FileContentWindow,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ValidatedManifestCandidate => "validated_manifest_candidate",
            Self::ProjectionFamily => "projection_family",
            Self::ProjectedGraphContext => "projected_graph_context",
            Self::SearchTextResponse => "search_text_response",
            Self::SearchHybridResponse => "search_hybrid_response",
            Self::SearchSymbolResponse => "search_symbol_response",
            Self::GoToDefinitionResponse => "go_to_definition_response",
            Self::FindDeclarationsResponse => "find_declarations_response",
            Self::HeuristicReference => "heuristic_reference",
            Self::RepositorySummary => "repository_summary",
            Self::CompiledSafeRegex => "compiled_safe_regex",
            Self::SearcherProjectionStore => "searcher_projection_store",
            Self::SearcherHybridGraphFileAnalysis => "searcher_hybrid_graph_file_analysis",
            Self::SearcherHybridGraphArtifact => "searcher_hybrid_graph_artifact",
            Self::SearchCandidateUniverse => "search_candidate_universe",
            Self::FileContentWindow => "file_content_window",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheResidency {
    ProcessWide,
    RequestLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheReuseClass {
    SnapshotScopedReusable,
    QueryResultMicroCache,
    ProcessMetadata,
    RequestLocalOnly,
    DeferredUntilReadOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheFreshnessContract {
    RepositorySnapshot,
    RepositoryFreshnessScopes,
    RepositoryId,
    ExactInput,
    RequestLocal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeCacheBudget {
    pub(crate) max_entries: Option<usize>,
    pub(crate) max_bytes: Option<usize>,
}

impl RuntimeCacheBudget {
    pub(crate) const fn new(max_entries: Option<usize>, max_bytes: Option<usize>) -> Self {
        Self {
            max_entries,
            max_bytes,
        }
    }

    pub(crate) const fn entry_and_byte_bound(max_entries: usize, max_bytes: usize) -> Self {
        Self::new(Some(max_entries), Some(max_bytes))
    }

    #[cfg(test)]
    pub(crate) const fn is_defined(self) -> bool {
        self.max_entries.is_some() || self.max_bytes.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuntimeCacheFamilyPolicy {
    pub(crate) residency: RuntimeCacheResidency,
    pub(crate) reuse_class: RuntimeCacheReuseClass,
    pub(crate) freshness_contract: RuntimeCacheFreshnessContract,
    pub(crate) budget: RuntimeCacheBudget,
    pub(crate) dirty_root_bypass: bool,
}

impl RuntimeCacheFamilyPolicy {
    #[cfg(test)]
    pub(crate) const fn supports_cross_request_reuse(self) -> bool {
        matches!(self.residency, RuntimeCacheResidency::ProcessWide)
            && !matches!(self.reuse_class, RuntimeCacheReuseClass::RequestLocalOnly)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCacheRegistry {
    pub(crate) global_budget: RuntimeCacheBudget,
    families: BTreeMap<RuntimeCacheFamily, RuntimeCacheFamilyPolicy>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct RuntimeCacheTelemetry {
    pub(crate) hits: usize,
    pub(crate) misses: usize,
    pub(crate) bypasses: usize,
    pub(crate) inserts: usize,
    pub(crate) evictions: usize,
    pub(crate) invalidations: usize,
}

impl RuntimeCacheTelemetry {
    pub(crate) fn record(&mut self, event: RuntimeCacheEvent, count: usize) {
        match event {
            RuntimeCacheEvent::Hit => self.hits += count,
            RuntimeCacheEvent::Miss => self.misses += count,
            RuntimeCacheEvent::Bypass => self.bypasses += count,
            RuntimeCacheEvent::Insert => self.inserts += count,
            RuntimeCacheEvent::Eviction => self.evictions += count,
            RuntimeCacheEvent::Invalidation => self.invalidations += count,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheEvent {
    Hit,
    Miss,
    Bypass,
    Insert,
    Eviction,
    Invalidation,
}

impl Default for RuntimeCacheRegistry {
    fn default() -> Self {
        let mut families = BTreeMap::new();
        for family in RuntimeCacheFamily::ALL {
            families.insert(family, runtime_cache_family_policy(family));
        }

        Self {
            global_budget: RuntimeCacheBudget::entry_and_byte_bound(1024, 96 * 1024 * 1024),
            families,
        }
    }
}

impl RuntimeCacheRegistry {
    pub(crate) fn policy(&self, family: RuntimeCacheFamily) -> Option<&RuntimeCacheFamilyPolicy> {
        self.families.get(&family)
    }

    #[cfg(test)]
    pub(crate) fn families(&self) -> &BTreeMap<RuntimeCacheFamily, RuntimeCacheFamilyPolicy> {
        &self.families
    }
}

const fn runtime_cache_family_policy(family: RuntimeCacheFamily) -> RuntimeCacheFamilyPolicy {
    use RuntimeCacheFamily as Family;
    use RuntimeCacheFreshnessContract as Freshness;
    use RuntimeCacheResidency as Residency;
    use RuntimeCacheReuseClass as Reuse;

    match family {
        Family::ValidatedManifestCandidate => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::SnapshotScopedReusable,
            freshness_contract: Freshness::RepositorySnapshot,
            budget: RuntimeCacheBudget::entry_and_byte_bound(128, 16 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::ProjectionFamily => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::DeferredUntilReadOnly,
            freshness_contract: Freshness::RepositorySnapshot,
            budget: RuntimeCacheBudget::entry_and_byte_bound(64, 24 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::ProjectedGraphContext => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::DeferredUntilReadOnly,
            freshness_contract: Freshness::RepositorySnapshot,
            budget: RuntimeCacheBudget::entry_and_byte_bound(64, 16 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::SearchTextResponse => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::QueryResultMicroCache,
            freshness_contract: Freshness::RepositoryFreshnessScopes,
            budget: RuntimeCacheBudget::entry_and_byte_bound(32, 4 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::SearchHybridResponse => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::QueryResultMicroCache,
            freshness_contract: Freshness::RepositoryFreshnessScopes,
            budget: RuntimeCacheBudget::entry_and_byte_bound(32, 8 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::SearchSymbolResponse => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::QueryResultMicroCache,
            freshness_contract: Freshness::RepositoryFreshnessScopes,
            budget: RuntimeCacheBudget::entry_and_byte_bound(32, 4 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::GoToDefinitionResponse => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::QueryResultMicroCache,
            freshness_contract: Freshness::RepositoryFreshnessScopes,
            budget: RuntimeCacheBudget::entry_and_byte_bound(32, 4 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::FindDeclarationsResponse => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::QueryResultMicroCache,
            freshness_contract: Freshness::RepositoryFreshnessScopes,
            budget: RuntimeCacheBudget::entry_and_byte_bound(32, 4 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::HeuristicReference => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::ProcessMetadata,
            freshness_contract: Freshness::RepositoryId,
            budget: RuntimeCacheBudget::entry_and_byte_bound(128, 32 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::RepositorySummary => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::ProcessMetadata,
            freshness_contract: Freshness::RepositoryId,
            budget: RuntimeCacheBudget::entry_and_byte_bound(256, 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::CompiledSafeRegex => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::ProcessMetadata,
            freshness_contract: Freshness::ExactInput,
            budget: RuntimeCacheBudget::entry_and_byte_bound(128, 1024 * 1024),
            dirty_root_bypass: false,
        },
        Family::SearcherProjectionStore
        | Family::SearcherHybridGraphFileAnalysis
        | Family::SearcherHybridGraphArtifact
        | Family::SearchCandidateUniverse => RuntimeCacheFamilyPolicy {
            residency: Residency::RequestLocal,
            reuse_class: Reuse::RequestLocalOnly,
            freshness_contract: Freshness::RequestLocal,
            budget: RuntimeCacheBudget::new(None, None),
            dirty_root_bypass: false,
        },
        Family::FileContentWindow => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::SnapshotScopedReusable,
            freshness_contract: Freshness::RepositoryFreshnessScopes,
            budget: RuntimeCacheBudget::entry_and_byte_bound(64, 32 * 1024 * 1024),
            dirty_root_bypass: true,
        },
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepositoryResponseCacheFreshnessMode {
    ManifestOnly,
    SemanticAware,
}

impl RepositoryResponseCacheFreshnessMode {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ManifestOnly => "manifest_only",
            Self::SemanticAware => "semantic_aware",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RepositoryFreshnessCacheScope {
    pub(crate) repository_id: String,
    pub(crate) snapshot_id: String,
    pub(crate) semantic_state: Option<String>,
    pub(crate) semantic_provider: Option<String>,
    pub(crate) semantic_model: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct RepositoryResponseCacheFreshness {
    pub(crate) scopes: Option<Vec<RepositoryFreshnessCacheScope>>,
    pub(crate) basis: Value,
}

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

#[derive(Debug, Clone)]
pub(crate) struct CachedWorkspacePreciseGeneration {
    pub(crate) summary: WorkspacePreciseGenerationSummary,
    #[allow(dead_code)]
    pub(crate) generated_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SearchTextResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) freshness_scopes: Vec<RepositoryFreshnessCacheScope>,
    pub(crate) query: String,
    pub(crate) pattern_type: &'static str,
    pub(crate) path_regex: Option<String>,
    pub(crate) limit: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedSearchTextResponse {
    pub(crate) response: SearchTextResponse,
    pub(crate) source_refs: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SearchHybridResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) freshness_scopes: Vec<RepositoryFreshnessCacheScope>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SearchSymbolResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) freshness_scopes: Vec<RepositoryFreshnessCacheScope>,
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
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct GoToDefinitionResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) freshness_scopes: Vec<RepositoryFreshnessCacheScope>,
    pub(crate) repository_id: Option<String>,
    pub(crate) symbol: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) include_follow_up_structural: bool,
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
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FindDeclarationsResponseCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) freshness_scopes: Vec<RepositoryFreshnessCacheScope>,
    pub(crate) repository_id: Option<String>,
    pub(crate) symbol: Option<String>,
    pub(crate) path: Option<String>,
    pub(crate) line: Option<usize>,
    pub(crate) column: Option<usize>,
    pub(crate) include_follow_up_structural: bool,
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
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct FileContentWindowCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) freshness_scopes: Vec<RepositoryFreshnessCacheScope>,
    pub(crate) canonical_path: PathBuf,
}

#[derive(Debug, Clone)]
pub(crate) struct FileContentSnapshot {
    raw_bytes: Arc<Vec<u8>>,
    normalized_lines: Arc<Vec<String>>,
    line_lossy_utf8: Arc<Vec<bool>>,
    total_lines: usize,
    estimated_bytes: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedFileContentWindow {
    pub(crate) snapshot: Arc<FileContentSnapshot>,
    pub(crate) estimated_bytes: usize,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FileContentWindowCache {
    entries: BTreeMap<FileContentWindowCacheKey, CachedFileContentWindow>,
    insertion_order: VecDeque<FileContentWindowCacheKey>,
    total_bytes: usize,
}

impl FileContentSnapshot {
    pub(crate) fn from_path(path: &std::path::Path) -> Result<Self, io::Error> {
        fs::read(path).map(Self::from_bytes)
    }

    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut normalized_lines = Vec::new();
        let mut line_lossy_utf8 = Vec::new();
        let mut line_start = 0usize;

        for index in memchr_iter(b'\n', &bytes) {
            let raw_line = &bytes[line_start..=index];
            let (normalized_line, had_lossy_utf8) = normalize_lossy_line_bytes(raw_line);
            normalized_lines.push(normalized_line);
            line_lossy_utf8.push(had_lossy_utf8);
            line_start = index.saturating_add(1);
        }

        if line_start < bytes.len() {
            let raw_line = &bytes[line_start..];
            let (normalized_line, had_lossy_utf8) = normalize_lossy_line_bytes(raw_line);
            normalized_lines.push(normalized_line);
            line_lossy_utf8.push(had_lossy_utf8);
        }

        let total_lines = normalized_lines.len();
        let estimated_bytes = bytes.len().saturating_add(
            normalized_lines
                .iter()
                .map(|line| line.len())
                .sum::<usize>(),
        );

        Self {
            raw_bytes: Arc::new(bytes),
            normalized_lines: Arc::new(normalized_lines),
            line_lossy_utf8: Arc::new(line_lossy_utf8),
            total_lines,
            estimated_bytes,
        }
    }

    pub(crate) fn raw_bytes_len(&self) -> usize {
        self.raw_bytes.len()
    }

    pub(crate) fn estimated_bytes(&self) -> usize {
        self.estimated_bytes
    }

    pub(crate) fn read_file_content(&self) -> String {
        String::from_utf8_lossy(self.raw_bytes.as_slice()).to_string()
    }

    pub(crate) fn read_line_slice_lossy(
        &self,
        line_start: usize,
        line_end: Option<usize>,
        max_bytes: usize,
    ) -> Result<LossyLineSlice, LossyLineSliceError> {
        if self.total_lines > 0 && line_start > self.total_lines {
            return Err(LossyLineSliceError::LineStartOutside {
                line_start,
                line_end,
                total_lines: self.total_lines,
            });
        }

        let start_index = line_start.saturating_sub(1).min(self.total_lines);
        let end_index = line_end.unwrap_or(self.total_lines).min(self.total_lines);
        let mut content = String::new();
        let mut sliced_bytes = 0usize;
        let mut exceeded_limit = false;
        let mut lossy_utf8 = false;
        let mut first_selected_line = true;

        for line_index in start_index..end_index {
            let line = &self.normalized_lines[line_index];
            lossy_utf8 |= self.line_lossy_utf8[line_index];
            if !first_selected_line {
                sliced_bytes = sliced_bytes.saturating_add(1);
                if !exceeded_limit {
                    content.push('\n');
                }
            }
            sliced_bytes = sliced_bytes.saturating_add(line.len());
            if sliced_bytes > max_bytes {
                exceeded_limit = true;
            }
            if !exceeded_limit {
                content.push_str(line);
            }
            first_selected_line = false;
        }

        Ok(LossyLineSlice {
            content,
            bytes: sliced_bytes,
            total_lines: self.total_lines,
            lossy_utf8,
        })
    }

    pub(crate) fn scan_file_scope_lossy(
        &self,
        scope: ExploreScopeRequest,
        matcher: Option<&ExploreMatcher>,
        max_matches: usize,
        resume_from: Option<&ExploreCursor>,
        include_scope_content: bool,
        max_scope_bytes: Option<usize>,
    ) -> ExploreScanResult {
        let mut total_matches = 0usize;
        let mut matches = Vec::new();
        let mut resume_cursor = None;
        let mut lossy_utf8 = false;
        let mut scope_content = String::new();
        let mut scope_bytes = 0usize;
        let mut scope_within_budget = true;
        let mut first_scope_line = true;

        for (line_index, line) in self.normalized_lines.iter().enumerate() {
            let line_number = line_index.saturating_add(1);
            let in_scope = line_number >= scope.start_line
                && scope
                    .end_line
                    .is_none_or(|end_line| line_number <= end_line);
            if !in_scope {
                continue;
            }

            lossy_utf8 |= self.line_lossy_utf8[line_index];

            if include_scope_content {
                if !first_scope_line {
                    scope_bytes = scope_bytes.saturating_add(1);
                    if scope_within_budget {
                        scope_content.push('\n');
                    }
                }
                scope_bytes = scope_bytes.saturating_add(line.len());
                if let Some(max_scope_bytes) = max_scope_bytes
                    && scope_bytes > max_scope_bytes
                {
                    scope_within_budget = false;
                }
                if scope_within_budget {
                    scope_content.push_str(line);
                }
                first_scope_line = false;
            }

            if let Some(matcher) = matcher {
                for (start, end) in matcher.find_spans(line) {
                    let start_column = start.saturating_add(1);
                    if resume_from.is_some_and(|cursor| {
                        position_is_before_cursor(line_number, start_column, cursor)
                    }) {
                        continue;
                    }

                    total_matches = total_matches.saturating_add(1);
                    let anchor = ExploreAnchor {
                        start_line: line_number,
                        start_column,
                        end_line: line_number,
                        end_column: end.saturating_add(1),
                    };
                    if matches.len() < max_matches {
                        matches.push(ExploreSpanMatch {
                            start_line: line_number,
                            start_column,
                            end_line: line_number,
                            end_column: end.saturating_add(1),
                            excerpt: line.clone(),
                            anchor,
                        });
                    } else if resume_cursor.is_none() {
                        resume_cursor = Some(ExploreCursor {
                            line: line_number,
                            column: start_column,
                        });
                    }
                }
            }
        }

        let effective_scope = match self.total_lines {
            0 => ExploreLineWindow {
                start_line: 0,
                end_line: 0,
            },
            _ => ExploreLineWindow {
                start_line: scope.start_line,
                end_line: scope
                    .end_line
                    .unwrap_or(self.total_lines)
                    .min(self.total_lines),
            },
        };

        ExploreScanResult {
            total_lines: self.total_lines,
            effective_scope,
            scope_content: include_scope_content.then_some(scope_content),
            scope_bytes: include_scope_content.then_some(scope_bytes),
            scope_within_budget,
            total_matches,
            matches,
            truncated: resume_cursor.is_some(),
            resume_from: resume_cursor,
            lossy_utf8,
        }
    }
}

impl FileContentWindowCache {
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub(crate) fn get(
        &self,
        cache_key: &FileContentWindowCacheKey,
    ) -> Option<Arc<FileContentSnapshot>> {
        self.entries
            .get(cache_key)
            .map(|entry| Arc::clone(&entry.snapshot))
    }

    pub(crate) fn insert(
        &mut self,
        cache_key: FileContentWindowCacheKey,
        snapshot: Arc<FileContentSnapshot>,
        budget: RuntimeCacheBudget,
    ) -> (bool, usize) {
        if budget
            .max_bytes
            .is_some_and(|limit| snapshot.estimated_bytes() > limit)
        {
            return (false, 0);
        }

        let estimated_bytes = snapshot.estimated_bytes();
        let previous = self.entries.remove(&cache_key);
        if previous.is_some() {
            self.insertion_order
                .retain(|candidate| candidate != &cache_key);
        }
        self.total_bytes = self
            .total_bytes
            .saturating_sub(
                previous
                    .as_ref()
                    .map(|entry| entry.estimated_bytes)
                    .unwrap_or(0),
            )
            .saturating_add(estimated_bytes);
        self.entries.insert(
            cache_key.clone(),
            CachedFileContentWindow {
                snapshot,
                estimated_bytes,
            },
        );
        self.insertion_order.push_back(cache_key);
        let evictions = self.trim_to_budget(budget);
        (true, evictions)
    }

    pub(crate) fn retain_repository(&mut self, repository_id: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.insertion_order
            .retain(|key| self.entries.contains_key(key));
        self.total_bytes = self
            .entries
            .values()
            .map(|entry| entry.estimated_bytes)
            .sum();
        before.saturating_sub(self.entries.len())
    }

    pub(crate) fn trim_to_budget(&mut self, budget: RuntimeCacheBudget) -> usize {
        let mut evictions = 0usize;
        loop {
            let over_entries = budget
                .max_entries
                .is_some_and(|limit| self.entries.len() > limit);
            let over_bytes = budget
                .max_bytes
                .is_some_and(|limit| self.total_bytes > limit);
            if !(over_entries || over_bytes) {
                break;
            }
            let Some(key) = self.insertion_order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&key) {
                self.total_bytes = self.total_bytes.saturating_sub(entry.estimated_bytes);
                evictions = evictions.saturating_add(1);
            }
        }
        evictions
    }
}

pub(crate) fn response_cache_scopes_include_repository(
    repository_id: &str,
    scoped_repository_ids: &[String],
    freshness_scopes: &[RepositoryFreshnessCacheScope],
) -> bool {
    scoped_repository_ids
        .iter()
        .any(|candidate| candidate == repository_id)
        || freshness_scopes
            .iter()
            .any(|scope| scope.repository_id == repository_id)
}

#[cfg(test)]
mod tests {
    use super::{
        ExploreCursor, ExploreMatcher, ExploreScopeRequest, FileContentSnapshot,
        FileContentWindowCache, FileContentWindowCacheKey, RepositoryFreshnessCacheScope,
        RuntimeCacheBudget, RuntimeCacheFamily, RuntimeCacheFreshnessContract,
        RuntimeCacheRegistry, RuntimeCacheResidency, RuntimeCacheReuseClass,
    };
    use std::path::PathBuf;
    use std::sync::Arc;

    #[test]
    fn runtime_cache_registry_defines_budgets_for_cross_request_families() {
        let registry = RuntimeCacheRegistry::default();

        for policy in registry.families().values() {
            if policy.supports_cross_request_reuse() {
                assert!(
                    policy.budget.is_defined(),
                    "cross-request cache families must define an explicit budget contract"
                );
            }
        }

        assert!(
            registry.global_budget.is_defined(),
            "registry must define a global budget envelope"
        );
    }

    #[test]
    fn runtime_cache_registry_distinguishes_snapshot_query_and_request_local_families() {
        let registry = RuntimeCacheRegistry::default();

        let manifest = registry
            .policy(RuntimeCacheFamily::ValidatedManifestCandidate)
            .expect("manifest cache policy should exist");
        assert_eq!(manifest.residency, RuntimeCacheResidency::ProcessWide);
        assert_eq!(
            manifest.reuse_class,
            RuntimeCacheReuseClass::SnapshotScopedReusable
        );
        assert_eq!(
            manifest.freshness_contract,
            RuntimeCacheFreshnessContract::RepositorySnapshot
        );
        assert!(manifest.dirty_root_bypass);

        let query_result = registry
            .policy(RuntimeCacheFamily::SearchHybridResponse)
            .expect("hybrid response cache policy should exist");
        assert_eq!(query_result.residency, RuntimeCacheResidency::ProcessWide);
        assert_eq!(
            query_result.reuse_class,
            RuntimeCacheReuseClass::QueryResultMicroCache
        );
        assert_eq!(
            query_result.freshness_contract,
            RuntimeCacheFreshnessContract::RepositoryFreshnessScopes
        );
        assert!(query_result.dirty_root_bypass);

        let request_local = registry
            .policy(RuntimeCacheFamily::SearcherProjectionStore)
            .expect("searcher projection store policy should exist");
        assert_eq!(request_local.residency, RuntimeCacheResidency::RequestLocal);
        assert_eq!(
            request_local.reuse_class,
            RuntimeCacheReuseClass::RequestLocalOnly
        );
        assert_eq!(
            request_local.freshness_contract,
            RuntimeCacheFreshnessContract::RequestLocal
        );
        assert!(!request_local.budget.is_defined());

        let deferred = registry
            .policy(RuntimeCacheFamily::ProjectionFamily)
            .expect("projection family policy should exist");
        assert_eq!(
            deferred.reuse_class,
            RuntimeCacheReuseClass::DeferredUntilReadOnly
        );
        assert_eq!(
            deferred.freshness_contract,
            RuntimeCacheFreshnessContract::RepositorySnapshot
        );
        assert!(deferred.dirty_root_bypass);
    }

    #[test]
    fn file_content_snapshot_supports_line_windows_and_scope_scans() {
        let snapshot = FileContentSnapshot::from_bytes(b"first\r\nsecond\nthird".to_vec());

        let slice = snapshot
            .read_line_slice_lossy(2, Some(3), 1024)
            .expect("line slice should succeed");
        assert_eq!(slice.content, "second\nthird");
        assert_eq!(slice.bytes, "second\nthird".len());
        assert_eq!(slice.total_lines, 3);
        assert!(!slice.lossy_utf8);

        let scan = snapshot.scan_file_scope_lossy(
            ExploreScopeRequest {
                start_line: 2,
                end_line: Some(3),
            },
            Some(&ExploreMatcher::Literal("ir".to_owned())),
            4,
            Some(&ExploreCursor { line: 3, column: 1 }),
            true,
            Some(32),
        );
        assert_eq!(scan.total_lines, 3);
        assert_eq!(scan.scope_content.as_deref(), Some("second\nthird"));
        assert_eq!(scan.total_matches, 1);
        assert_eq!(scan.matches.len(), 1);
    }

    #[test]
    fn file_content_window_cache_trims_and_invalidates_by_repository() {
        let mut cache = FileContentWindowCache::default();
        let key = FileContentWindowCacheKey {
            scoped_repository_ids: vec!["repo-001".to_owned()],
            freshness_scopes: vec![RepositoryFreshnessCacheScope {
                repository_id: "repo-001".to_owned(),
                snapshot_id: "snapshot-001".to_owned(),
                semantic_state: None,
                semantic_provider: None,
                semantic_model: None,
            }],
            canonical_path: PathBuf::from("/tmp/repo-001/file.rs"),
        };
        let snapshot = Arc::new(FileContentSnapshot::from_bytes(
            b"pub fn cached() {}\n".to_vec(),
        ));
        assert!(
            cache
                .insert(
                    key.clone(),
                    Arc::clone(&snapshot),
                    RuntimeCacheBudget::entry_and_byte_bound(4, 1024),
                )
                .0
        );
        assert!(cache.get(&key).is_some());
        assert_eq!(cache.retain_repository("repo-001"), 1);
        assert!(cache.get(&key).is_none());
    }
}
