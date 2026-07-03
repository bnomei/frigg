//! Runtime cache contracts and shared cache value types for the MCP server.
//!
//! The hot-path caches here are process-wide but explicitly budgeted.

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::io;
use std::ops::Range;
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
    ExploreAnchor, ExploreCursor, ExploreLineWindow, WorkspacePreciseGenerationSummary,
    WorkspacePreciseGeneratorState,
};

/// Named runtime cache family governed by budget, freshness, and reuse policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RuntimeCacheFamily {
    ValidatedManifestCandidate,
    ProjectionFamily,
    ProjectedGraphContext,
    HeuristicReference,
    CompiledSafeRegex,
    SearcherProjectionStore,
    SearcherHybridGraphFileAnalysis,
    SearcherHybridGraphArtifact,
    SearchCandidateUniverse,
}

impl RuntimeCacheFamily {
    pub(crate) const ALL: [Self; 9] = [
        Self::ValidatedManifestCandidate,
        Self::ProjectionFamily,
        Self::ProjectedGraphContext,
        Self::HeuristicReference,
        Self::CompiledSafeRegex,
        Self::SearcherProjectionStore,
        Self::SearcherHybridGraphFileAnalysis,
        Self::SearcherHybridGraphArtifact,
        Self::SearchCandidateUniverse,
    ];

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::ValidatedManifestCandidate => "validated_manifest_candidate",
            Self::ProjectionFamily => "projection_family",
            Self::ProjectedGraphContext => "projected_graph_context",
            Self::HeuristicReference => "heuristic_reference",
            Self::CompiledSafeRegex => "compiled_safe_regex",
            Self::SearcherProjectionStore => "searcher_projection_store",
            Self::SearcherHybridGraphFileAnalysis => "searcher_hybrid_graph_file_analysis",
            Self::SearcherHybridGraphArtifact => "searcher_hybrid_graph_artifact",
            Self::SearchCandidateUniverse => "search_candidate_universe",
        }
    }
}

/// Whether a cache family may survive across MCP requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheResidency {
    ProcessWide,
    RequestLocal,
}

/// Reuse semantics for one runtime cache family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheReuseClass {
    SnapshotScopedReusable,
    ProcessMetadata,
    RequestLocalOnly,
    DeferredUntilReadOnly,
}

/// Freshness inputs required before a cached value may be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheFreshnessContract {
    RepositorySnapshot,
    RepositoryId,
    ExactInput,
    RequestLocal,
}

/// Entry and byte limits applied to one cache family or the global runtime cache envelope.
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

/// Policy bundle describing how one runtime cache family may be stored and invalidated.
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

/// Default runtime cache registry with per-family budgets and freshness contracts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RuntimeCacheRegistry {
    pub(crate) global_budget: RuntimeCacheBudget,
    families: BTreeMap<RuntimeCacheFamily, RuntimeCacheFamilyPolicy>,
}

/// Hit, miss, bypass, and eviction counters for runtime cache instrumentation.
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
            RuntimeCacheEvent::Insert => self.inserts += count,
            RuntimeCacheEvent::Eviction => self.evictions += count,
            RuntimeCacheEvent::Invalidation => self.invalidations += count,
        }
    }
}

/// Telemetry event recorded against one runtime cache family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RuntimeCacheEvent {
    Hit,
    Miss,
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
        Family::HeuristicReference => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::ProcessMetadata,
            freshness_contract: Freshness::RepositoryId,
            budget: RuntimeCacheBudget::entry_and_byte_bound(128, 32 * 1024 * 1024),
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
    }
}

/// Freshness basis mode used when deciding whether a response may be cached.
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

/// Repository snapshot and semantic inputs that scope one response freshness payload.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RepositoryFreshnessCacheScope {
    pub(crate) repository_id: String,
    pub(crate) snapshot_id: String,
    pub(crate) semantic_state: Option<String>,
    pub(crate) semantic_provider: Option<String>,
    pub(crate) semantic_model: Option<String>,
}

/// Serialized freshness basis attached to search and navigation responses.
#[derive(Debug, Clone)]
pub(crate) struct RepositoryResponseCacheFreshness {
    pub(crate) scopes: Option<Vec<RepositoryFreshnessCacheScope>>,
    pub(crate) basis: Value,
}

/// Planned semantic refresh keyed to the latest repository snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceSemanticRefreshPlan {
    pub(crate) latest_snapshot_id: String,
    pub(crate) reason: &'static str,
}

/// Cached precise-generation summary for one workspace generator probe.
#[derive(Debug, Clone)]
pub(crate) struct CachedWorkspacePreciseGeneration {
    pub(crate) summary: WorkspacePreciseGenerationSummary,
    #[allow(dead_code)]
    pub(crate) generated_at: Instant,
}

/// Cache key for repository response-freshness metadata.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct RepositoryResponseFreshnessCacheKey {
    pub(crate) scoped_repository_ids: Vec<String>,
    pub(crate) mode: &'static str,
}

/// Cached response-freshness payload invalidated by repository events.
#[derive(Debug, Clone)]
pub(crate) struct CachedRepositoryResponseFreshness {
    pub(crate) freshness: RepositoryResponseCacheFreshness,
    pub(crate) epoch: u64,
}

/// Cache key for one precise-generator availability probe.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PreciseGeneratorProbeCacheKey {
    pub(crate) repository_id: String,
    pub(crate) generator_id: String,
}

/// Cached precise-generator probe result for workspace status reporting.
#[derive(Debug, Clone)]
pub(crate) struct CachedPreciseGeneratorProbe {
    pub(crate) state: WorkspacePreciseGeneratorState,
    pub(crate) tool: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) reason: Option<String>,
    pub(crate) generated_at: Instant,
}

/// Cache key for heuristic reference evidence built without precise coverage.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HeuristicReferenceCacheKey {
    pub(crate) repository_id: String,
    pub(crate) symbol_id: String,
    pub(crate) corpus_signature: String,
    pub(crate) scip_signature: String,
}

/// Cached heuristic reference set plus source-load diagnostics.
#[derive(Debug, Clone)]
pub(crate) struct CachedHeuristicReferences {
    pub(crate) references: Arc<Vec<HeuristicReference>>,
    pub(crate) source_files_discovered: usize,
    pub(crate) source_read_diagnostics_count: usize,
    pub(crate) source_files_loaded: usize,
    pub(crate) source_bytes_loaded: u64,
}

/// Repository path anchor stored for one `result_handle` match id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResultHandleMatchAnchor {
    pub(crate) repository_id: String,
    pub(crate) path: String,
    pub(crate) line: usize,
    pub(crate) column: Option<usize>,
}

/// Session-scoped `result_handle` entry mapping match ids to source anchors.
#[derive(Debug, Clone)]
pub(crate) struct SessionResultHandleEntry {
    pub(crate) generated_at: Instant,
    pub(crate) matches: BTreeMap<String, ResultHandleMatchAnchor>,
}

/// Session-local cache backing `read_match` lookups from prior search or navigation handles.
#[derive(Debug, Clone, Default)]
pub(crate) struct SessionResultHandleCache {
    pub(crate) entries: BTreeMap<String, SessionResultHandleEntry>,
    pub(crate) insertion_order: VecDeque<String>,
    pub(crate) next_id: u64,
}

#[derive(Debug, Clone)]
/// File snapshot used by both `read_file` and `explore`.
///
/// Raw bytes are preserved for exact full-file reads, while a single normalized text buffer plus
/// per-line ranges supports bounded line windows without allocating one `String` per line.
pub(crate) struct FileContentSnapshot {
    raw_bytes: Arc<[u8]>,
    normalized_content: Arc<str>,
    line_ranges: Arc<Vec<Range<usize>>>,
    line_lossy_utf8: Arc<Vec<bool>>,
    total_lines: usize,
}

impl FileContentSnapshot {
    pub(crate) fn from_path(path: &std::path::Path) -> Result<Self, io::Error> {
        fs::read(path).map(Self::from_bytes)
    }

    pub(crate) fn from_bytes(bytes: Vec<u8>) -> Self {
        let mut normalized_content = String::new();
        let mut line_ranges = Vec::new();
        let mut line_lossy_utf8 = Vec::new();
        let mut line_start = 0usize;

        for index in memchr_iter(b'\n', &bytes) {
            let raw_line = &bytes[line_start..=index];
            let (normalized_line, had_lossy_utf8) = normalize_lossy_line_bytes(raw_line);
            let start = normalized_content.len();
            normalized_content.push_str(&normalized_line);
            line_ranges.push(start..normalized_content.len());
            line_lossy_utf8.push(had_lossy_utf8);
            line_start = index.saturating_add(1);
        }

        if line_start < bytes.len() {
            let raw_line = &bytes[line_start..];
            let (normalized_line, had_lossy_utf8) = normalize_lossy_line_bytes(raw_line);
            let start = normalized_content.len();
            normalized_content.push_str(&normalized_line);
            line_ranges.push(start..normalized_content.len());
            line_lossy_utf8.push(had_lossy_utf8);
        }

        let total_lines = line_ranges.len();
        Self {
            raw_bytes: Arc::<[u8]>::from(bytes),
            normalized_content: Arc::<str>::from(normalized_content),
            line_ranges: Arc::new(line_ranges),
            line_lossy_utf8: Arc::new(line_lossy_utf8),
            total_lines,
        }
    }

    pub(crate) fn raw_bytes_len(&self) -> usize {
        self.raw_bytes.len()
    }

    pub(crate) fn read_file_content(&self) -> String {
        String::from_utf8_lossy(&self.raw_bytes).to_string()
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
            let line = self
                .line_ranges
                .get(line_index)
                .map(|range| &self.normalized_content[range.start..range.end])
                .unwrap_or("");
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

        for (line_index, range) in self.line_ranges.iter().enumerate() {
            let line = &self.normalized_content[range.start..range.end];
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
                            excerpt: line.to_owned(),
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

#[cfg(test)]
mod tests {
    use super::{
        ExploreCursor, ExploreMatcher, ExploreScopeRequest, FileContentSnapshot,
        RuntimeCacheFamily, RuntimeCacheFreshnessContract, RuntimeCacheRegistry,
        RuntimeCacheResidency, RuntimeCacheReuseClass,
    };

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
    fn runtime_cache_registry_distinguishes_snapshot_metadata_and_request_local_families() {
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
}
