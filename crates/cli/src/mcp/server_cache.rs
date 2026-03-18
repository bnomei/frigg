use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Instant;

use serde_json::Value;

use crate::indexer::HeuristicReference;
use crate::mcp::types::{
    FindDeclarationsResponse, GoToDefinitionResponse, RepositorySummary, SearchHybridResponse,
    SearchSymbolResponse, SearchTextResponse,
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
}

impl RuntimeCacheFamily {
    pub(crate) const ALL: [Self; 15] = [
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
            budget: RuntimeCacheBudget::entry_and_byte_bound(256, 1 * 1024 * 1024),
            dirty_root_bypass: true,
        },
        Family::CompiledSafeRegex => RuntimeCacheFamilyPolicy {
            residency: Residency::ProcessWide,
            reuse_class: Reuse::ProcessMetadata,
            freshness_contract: Freshness::ExactInput,
            budget: RuntimeCacheBudget::entry_and_byte_bound(128, 1 * 1024 * 1024),
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
}
