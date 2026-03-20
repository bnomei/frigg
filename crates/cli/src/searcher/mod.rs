//! Search orchestration that turns manifests, projections, lexical scans, graph edges, and
//! optional embeddings into stable retrieval results. This layer sits between raw repository
//! artifacts and delivery surfaces such as MCP tools or playbook probes.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, RwLock};

mod attribution;
mod candidate_universe;
mod candidates;
mod content_scrub;
mod graph_channel;
mod hybrid_execution;
mod hybrid_match;
mod intent;
mod laravel;
mod lexical_channel;
mod lexical_recall;
mod ordering;
mod overlay_projection;
mod path_witness_projection;
mod path_witness_search;
mod policy;
mod projection_service;
mod query_terms;
mod ranker;
mod regex_support;
mod reranker;
mod retrieval_projection;
mod ripgrep_backend;
mod scan_engine;
mod semantic;
mod surfaces;
mod types;

use crate::domain::{FriggError, FriggResult, model::TextMatch};
use crate::languages::{LanguageCapability, SymbolLanguage, parse_supported_language};
pub use crate::manifest_validation::ValidatedManifestCandidateCache;
use crate::settings::{FriggConfig, SemanticRuntimeCredentials};
use aho_corasick::AhoCorasick;
pub use attribution::{SearchStageAttribution, SearchStageSample};
use graph_channel::{HybridGraphArtifact, HybridGraphArtifactCacheKey, search_graph_channel_hits};
pub(crate) use hybrid_match::{
    hybrid_match_definition_navigation_supported, hybrid_match_document_symbols_supported,
    hybrid_match_is_live_navigation_pivot, hybrid_match_source_class,
    hybrid_match_surface_families,
};
pub(crate) use intent::HybridRankingIntent;
use laravel::{
    is_laravel_blade_component_path, is_laravel_bootstrap_entrypoint_path,
    is_laravel_command_or_middleware_path, is_laravel_core_provider_path,
    is_laravel_form_action_blade_path, is_laravel_job_or_listener_path,
    is_laravel_layout_blade_view_path, is_laravel_livewire_component_path,
    is_laravel_livewire_view_path, is_laravel_nested_blade_component_path,
    is_laravel_non_livewire_blade_view_path, is_laravel_provider_path, is_laravel_route_path,
    is_laravel_view_component_class_path,
};
#[cfg(test)]
use lexical_channel::{build_hybrid_lexical_hits, build_hybrid_lexical_hits_for_query};
use lexical_channel::{
    build_hybrid_path_witness_hits_with_intent, hybrid_path_has_exact_stem_match,
    hybrid_path_quality_multiplier_with_intent, merge_hybrid_lexical_search_output,
    semantic_excerpt,
};
use lexical_recall::{build_hybrid_lexical_recall_regex, hybrid_lexical_recall_tokens};
use ordering::{
    sort_matches_deterministically, sort_search_diagnostics_deterministically,
    text_match_candidate_order,
};
#[cfg(test)]
use overlay_projection::StoredEntrypointSurfaceProjection;
#[cfg(test)]
use overlay_projection::StoredTestSubjectProjection;
use path_witness_projection::StoredPathWitnessProjection;
#[cfg(test)]
pub(crate) use path_witness_projection::{
    PATH_WITNESS_PROJECTION_HEURISTIC_VERSION, build_path_witness_projection_records_from_paths,
};
pub(crate) use policy::{
    apply_post_selection_guardrails_with_trace, path_quality_rule_trace, path_witness_rule_trace,
    selection_rule_trace,
};
pub(crate) use projection_service::ProjectionStoreService;
use query_terms::{
    hybrid_excerpt_has_build_flow_anchor, hybrid_excerpt_has_test_double_anchor,
    hybrid_identifier_tokens, hybrid_overlap_count, hybrid_path_overlap_count,
    hybrid_path_overlap_tokens, hybrid_query_exact_terms, hybrid_query_overlap_terms,
};
use ranker::blend_hybrid_evidence;
#[cfg(test)]
use ranker::group_hybrid_ranked_evidence;
pub use ranker::rank_hybrid_evidence;
use regex::Regex;
pub use regex_support::{RegexSearchError, compile_safe_regex};
use regex_support::{build_regex_prefilter_plan, regex_error_to_frigg_error};
#[cfg(test)]
use reranker::diversify_hybrid_ranked_evidence;
#[cfg(test)]
pub(crate) use retrieval_projection::TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION;
pub(crate) use retrieval_projection::build_retrieval_projection_bundle;
#[cfg(test)]
pub(crate) use ripgrep_backend::clear_ripgrep_availability_cache;
use ripgrep_backend::{
    RipgrepPatternMode, resolve_ripgrep_executable, search_with_ripgrep_in_universe,
};
use semantic::{
    RuntimeSemanticQueryEmbeddingExecutor, SemanticRuntimeQueryEmbeddingExecutor,
    search_semantic_channel_hits,
};
#[cfg(test)]
use surfaces::HybridSourceClass;
use surfaces::{
    coverage_subtree_root, hybrid_source_class, is_bench_support_path,
    is_build_config_surface_path, is_ci_workflow_path, is_cli_test_support_path,
    is_entrypoint_build_workflow_path, is_entrypoint_runtime_path, is_example_support_path,
    is_frontend_runtime_noise_path, is_kotlin_android_ui_runtime_surface_path,
    is_package_surface_path, is_python_runtime_config_path, is_python_test_witness_path,
    is_runtime_companion_surface_path, is_runtime_config_artifact_path, is_scripts_ops_path,
    is_test_harness_path, is_test_support_path, is_workspace_config_surface_path,
};
pub use types::{
    HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridExecutionNote,
    HybridRankedEvidence, HybridSemanticStatus, SearchDiagnostic, SearchDiagnosticKind,
    SearchExecutionDiagnostics, SearchExecutionOutput, SearchFilters, SearchHybridExecutionOutput,
    SearchHybridQuery, SearchLexicalBackend, SearchTextQuery,
};
pub(crate) use types::{
    HybridGraphFileAnalysis, HybridGraphFileAnalysisCacheKey, ManifestCandidateFilesBuild,
    NormalizedSearchFilters, RepositoryCandidateUniverse, SearchCandidateFile,
    SearchCandidateUniverse, SearchCandidateUniverseBuild, empty_channel_result,
    hybrid_execution_note_from_channel_results, match_count_for_hits,
    search_diagnostics_to_channel_diagnostics,
};

#[cfg(test)]
fn rank_hybrid_evidence_for_query(
    lexical_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
    limit: usize,
    query_text: &str,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    rank_hybrid_evidence_for_query_with_witness(
        lexical_hits,
        &[],
        graph_hits,
        semantic_hits,
        weights,
        limit,
        query_text,
    )
}

fn rank_hybrid_anchor_evidence_for_query_with_witness(
    lexical_hits: &[HybridChannelHit],
    witness_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
    limit: usize,
    _query_text: &str,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut lexical_and_witness_hits = lexical_hits.to_vec();
    lexical_and_witness_hits.extend_from_slice(witness_hits);
    let ranked = blend_hybrid_evidence(
        &lexical_and_witness_hits,
        graph_hits,
        semantic_hits,
        weights,
    )?;
    Ok(bounded_ranked_anchor_pool(ranked, limit))
}

fn bounded_ranked_anchor_pool(
    ranked: Vec<HybridRankedEvidence>,
    limit: usize,
) -> Vec<HybridRankedEvidence> {
    if ranked.len() <= limit {
        return ranked;
    }

    let exemplar_reserve = usize::min(8, usize::max(2, limit / 4)).min(limit);
    let base_take = limit.saturating_sub(exemplar_reserve);
    let mut selected_anchor_keys = BTreeSet::new();
    let mut seen_documents = BTreeSet::new();

    for anchor in ranked.iter().take(base_take) {
        selected_anchor_keys.insert((anchor.document.clone(), anchor.anchor.clone()));
        seen_documents.insert((
            anchor.document.repository_id.clone(),
            anchor.document.path.clone(),
        ));
    }

    let mut added_exemplars = 0usize;
    for anchor in ranked.iter().skip(base_take) {
        let document_key = (
            anchor.document.repository_id.clone(),
            anchor.document.path.clone(),
        );
        if !seen_documents.insert(document_key) {
            continue;
        }
        selected_anchor_keys.insert((anchor.document.clone(), anchor.anchor.clone()));
        added_exemplars = added_exemplars.saturating_add(1);
        if added_exemplars >= exemplar_reserve {
            break;
        }
    }

    for anchor in &ranked {
        if selected_anchor_keys.len() >= limit {
            break;
        }
        selected_anchor_keys.insert((anchor.document.clone(), anchor.anchor.clone()));
    }

    let mut pool = ranked
        .into_iter()
        .filter(|anchor| {
            selected_anchor_keys.contains(&(anchor.document.clone(), anchor.anchor.clone()))
        })
        .collect::<Vec<_>>();
    pool.truncate(limit);
    pool
}

#[cfg(test)]
fn rank_hybrid_evidence_for_query_with_witness(
    lexical_hits: &[HybridChannelHit],
    witness_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
    limit: usize,
    query_text: &str,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    let ranked_anchors = rank_hybrid_anchor_evidence_for_query_with_witness(
        lexical_hits,
        witness_hits,
        graph_hits,
        semantic_hits,
        weights,
        limit.saturating_mul(4).max(32),
        query_text,
    )?;
    let grouped =
        group_hybrid_ranked_evidence(ranked_anchors, weights, limit.saturating_mul(4).max(32));
    Ok(diversify_hybrid_ranked_evidence(grouped, limit, query_text))
}

/// Coordinates Frigg's retrieval pipeline for a configured workspace set, blending lexical,
/// graph, path-witness, and optional semantic signals into ranked evidence for higher layers.
pub struct TextSearcher {
    config: FriggConfig,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    projection_store_service: ProjectionStoreService,
    hybrid_graph_file_analysis_cache:
        Arc<RwLock<BTreeMap<HybridGraphFileAnalysisCacheKey, Arc<HybridGraphFileAnalysis>>>>,
    hybrid_graph_artifact_cache:
        Arc<RwLock<BTreeMap<HybridGraphArtifactCacheKey, Arc<HybridGraphArtifact>>>>,
}

pub const MAX_REGEX_PATTERN_BYTES: usize = 512;
pub const MAX_REGEX_ALTERNATIONS: usize = 32;
pub const MAX_REGEX_GROUPS: usize = 32;
pub const MAX_REGEX_QUANTIFIERS: usize = 64;
pub const MAX_REGEX_SIZE_LIMIT_BYTES: usize = 1_000_000;
pub const MAX_REGEX_DFA_SIZE_LIMIT_BYTES: usize = 1_000_000;
const BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD: usize = 256;
const HYBRID_LEXICAL_RECALL_MAX_TOKENS: usize = 12;
const HYBRID_LEXICAL_RECALL_MIN_TOKEN_LEN: usize = 4;
const HYBRID_GRAPH_MAX_ANCHORS: usize = 8;
const HYBRID_GRAPH_MAX_NEIGHBORS_PER_ANCHOR: usize = 12;
const HYBRID_GRAPH_CANDIDATE_POOL_MULTIPLIER: usize = 4;
const HYBRID_GRAPH_CANDIDATE_POOL_MIN: usize = 16;
const HYBRID_SEMANTIC_CANDIDATE_POOL_MULTIPLIER: usize = 6;
const HYBRID_SEMANTIC_CANDIDATE_POOL_MIN: usize = 24;
const HYBRID_SEMANTIC_RETAINED_DOCUMENT_MULTIPLIER: usize = 8;
const HYBRID_SEMANTIC_RETAINED_DOCUMENT_MIN: usize = 24;
const HYBRID_SEMANTIC_RETAIN_RELATIVE_FLOOR: f32 = 0.72;
const REGEX_TRIGRAM_BITMAP_BITS: usize = 1 << 16;
const REGEX_TRIGRAM_BITMAP_WORDS: usize = REGEX_TRIGRAM_BITMAP_BITS / 64;
const REGEX_TRIGRAM_HASH_MULTIPLIER: u32 = 0x9E37_79B1;

impl TextSearcher {
    pub fn new(config: FriggConfig) -> Self {
        Self::with_validated_manifest_candidate_cache(
            config,
            Arc::new(RwLock::new(ValidatedManifestCandidateCache::default())),
        )
    }

    pub(crate) fn with_validated_manifest_candidate_cache(
        config: FriggConfig,
        validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    ) -> Self {
        Self::with_runtime_projection_store_service(
            config,
            validated_manifest_candidate_cache,
            ProjectionStoreService::new(),
        )
    }

    pub(crate) fn with_runtime_projection_store_service(
        config: FriggConfig,
        validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
        projection_store_service: ProjectionStoreService,
    ) -> Self {
        Self {
            config,
            validated_manifest_candidate_cache,
            projection_store_service,
            hybrid_graph_file_analysis_cache: Arc::new(RwLock::new(BTreeMap::new())),
            hybrid_graph_artifact_cache: Arc::new(RwLock::new(BTreeMap::new())),
        }
    }

    #[cfg(test)]
    pub(crate) fn load_or_build_entrypoint_surface_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredEntrypointSurfaceProjection>>> {
        self.projection_store_service
            .load_or_build_entrypoint_surface_projections_for_repository(repository, snapshot_id)
    }

    #[cfg(test)]
    pub(crate) fn entrypoint_surface_projection_cache_len(&self) -> usize {
        self.projection_store_service.entrypoint_surface_cache_len()
    }

    #[cfg(test)]
    pub(crate) fn load_or_build_test_subject_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredTestSubjectProjection>>> {
        self.projection_store_service
            .load_or_build_test_subject_projections_for_repository(repository, snapshot_id)
    }

    #[cfg(test)]
    pub(crate) fn path_witness_projection_cache_len(&self) -> usize {
        self.projection_store_service.path_witness_cache_len()
    }

    #[cfg(test)]
    pub(crate) fn projected_graph_adjacency_cache_len(&self) -> usize {
        self.projection_store_service
            .projected_graph_adjacency_cache_len()
    }

    #[cfg(test)]
    pub(crate) fn first_repository_candidate_universe(
        &self,
    ) -> Option<RepositoryCandidateUniverse> {
        self.build_candidate_universe_with_attribution(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 32,
            },
            &normalize_search_filters(SearchFilters::default()).ok()?,
        )
        .universe
        .repositories
        .into_iter()
        .next()
    }

    #[cfg(test)]
    pub(crate) fn load_or_build_path_witness_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredPathWitnessProjection>>> {
        self.projection_store_service
            .load_or_build_path_witness_projections_for_repository(repository, snapshot_id)
    }

    pub fn search(&self, query: SearchTextQuery) -> FriggResult<Vec<TextMatch>> {
        self.search_literal_with_filters(query, SearchFilters::default())
    }

    pub fn search_literal(
        &self,
        query: SearchTextQuery,
        repository_id_filter: Option<&str>,
    ) -> FriggResult<Vec<TextMatch>> {
        self.search_literal_with_filters(
            query,
            SearchFilters {
                repository_id: repository_id_filter.map(ToOwned::to_owned),
                language: None,
            },
        )
    }

    pub fn search_literal_with_filters(
        &self,
        query: SearchTextQuery,
        filters: SearchFilters,
    ) -> FriggResult<Vec<TextMatch>> {
        self.search_literal_with_filters_diagnostics(query, filters)
            .map(|output| output.matches)
    }

    pub fn search_literal_with_filters_diagnostics(
        &self,
        query: SearchTextQuery,
        filters: SearchFilters,
    ) -> FriggResult<SearchExecutionOutput> {
        if query.query.is_empty() {
            return Err(FriggError::InvalidInput(
                "literal search query must not be empty".to_owned(),
            ));
        }

        if query.limit == 0 {
            return Ok(SearchExecutionOutput::default());
        }

        let matcher = AhoCorasick::new([query.query.as_str()])
            .map_err(|err| FriggError::InvalidInput(format!("invalid query: {err}")))?;
        let normalized_filters = normalize_search_filters(filters)?;
        let candidate_universe = self.build_candidate_universe(&query, &normalized_filters);
        self.search_literal_with_candidate_universe_using_matcher(
            &query,
            &candidate_universe,
            &matcher,
        )
    }

    fn search_literal_with_candidate_universe(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
    ) -> FriggResult<SearchExecutionOutput> {
        let matcher = AhoCorasick::new([query.query.as_str()])
            .map_err(|err| FriggError::InvalidInput(format!("invalid query: {err}")))?;
        self.search_literal_with_candidate_universe_using_matcher(
            query,
            candidate_universe,
            &matcher,
        )
    }

    fn search_literal_with_candidate_universe_using_matcher(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        matcher: &AhoCorasick,
    ) -> FriggResult<SearchExecutionOutput> {
        if let Some(output) =
            self.search_literal_with_ripgrep_if_available(query, candidate_universe)?
        {
            return Ok(output);
        }
        self.search_with_streaming_lines_in_universe(query, candidate_universe, |line, columns| {
            columns.clear();
            columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
        })
    }

    fn search_literal_prefix_with_candidate_universe(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
    ) -> FriggResult<SearchExecutionOutput> {
        if let Some(output) =
            self.search_literal_with_ripgrep_if_available(query, candidate_universe)?
        {
            return Ok(output);
        }
        let matcher = AhoCorasick::new([query.query.as_str()])
            .map_err(|err| FriggError::InvalidInput(format!("invalid query: {err}")))?;
        self.search_with_streaming_lines_prefix_in_universe(
            query,
            candidate_universe,
            |line, columns| {
                columns.clear();
                columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
            },
        )
    }

    pub fn search_regex(
        &self,
        query: SearchTextQuery,
        repository_id_filter: Option<&str>,
    ) -> FriggResult<Vec<TextMatch>> {
        self.search_regex_with_filters(
            query,
            SearchFilters {
                repository_id: repository_id_filter.map(ToOwned::to_owned),
                language: None,
            },
        )
    }

    pub fn search_regex_with_filters(
        &self,
        query: SearchTextQuery,
        filters: SearchFilters,
    ) -> FriggResult<Vec<TextMatch>> {
        self.search_regex_with_filters_diagnostics(query, filters)
            .map(|output| output.matches)
    }

    pub fn search_regex_with_filters_diagnostics(
        &self,
        query: SearchTextQuery,
        filters: SearchFilters,
    ) -> FriggResult<SearchExecutionOutput> {
        if query.limit == 0 {
            return Ok(SearchExecutionOutput::default());
        }

        let matcher = compile_safe_regex(&query.query).map_err(regex_error_to_frigg_error)?;
        let prefilter_plan = build_regex_prefilter_plan(&query.query);
        let normalized_filters = normalize_search_filters(filters)?;
        let candidate_universe = self.build_candidate_universe(&query, &normalized_filters);
        self.search_regex_with_candidate_universe(
            &query,
            &candidate_universe,
            matcher,
            prefilter_plan,
        )
    }

    fn search_regex_with_candidate_universe(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        matcher: Regex,
        prefilter_plan: Option<regex_support::RegexPrefilterPlan>,
    ) -> FriggResult<SearchExecutionOutput> {
        if let Some(output) =
            self.search_regex_with_ripgrep_if_available(query, candidate_universe)?
        {
            return Ok(output);
        }
        if prefilter_plan.is_none() {
            return self.search_with_streaming_lines_in_universe(
                query,
                candidate_universe,
                |line, columns| {
                    columns.clear();
                    columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
                },
            );
        }
        self.search_with_matcher_in_universe(
            query,
            candidate_universe,
            |content| {
                prefilter_plan
                    .as_ref()
                    .is_none_or(|plan| plan.file_may_match(content))
            },
            |line, columns| {
                columns.clear();
                columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
            },
        )
    }

    pub fn search_hybrid(
        &self,
        query: SearchHybridQuery,
    ) -> FriggResult<SearchHybridExecutionOutput> {
        self.search_hybrid_with_filters(query, SearchFilters::default())
    }

    pub fn search_hybrid_with_filters(
        &self,
        query: SearchHybridQuery,
        filters: SearchFilters,
    ) -> FriggResult<SearchHybridExecutionOutput> {
        let credentials = SemanticRuntimeCredentials::from_process_env();
        let semantic_executor = RuntimeSemanticQueryEmbeddingExecutor::new(credentials.clone());
        self.search_hybrid_with_filters_using_executor(
            query,
            filters,
            &credentials,
            &semantic_executor,
        )
    }

    pub(crate) fn search_hybrid_with_filters_with_trace(
        &self,
        query: SearchHybridQuery,
        filters: SearchFilters,
    ) -> FriggResult<SearchHybridExecutionOutput> {
        let credentials = SemanticRuntimeCredentials::from_process_env();
        let semantic_executor = RuntimeSemanticQueryEmbeddingExecutor::new(credentials.clone());
        self.search_hybrid_with_filters_using_executor_with_trace(
            query,
            filters,
            &credentials,
            &semantic_executor,
        )
    }

    fn search_hybrid_with_filters_using_executor(
        &self,
        query: SearchHybridQuery,
        filters: SearchFilters,
        credentials: &SemanticRuntimeCredentials,
        semantic_executor: &dyn SemanticRuntimeQueryEmbeddingExecutor,
    ) -> FriggResult<SearchHybridExecutionOutput> {
        hybrid_execution::search_hybrid_with_filters_using_executor(
            self,
            query,
            filters,
            credentials,
            semantic_executor,
            false,
        )
    }

    fn search_hybrid_with_filters_using_executor_with_trace(
        &self,
        query: SearchHybridQuery,
        filters: SearchFilters,
        credentials: &SemanticRuntimeCredentials,
        semantic_executor: &dyn SemanticRuntimeQueryEmbeddingExecutor,
    ) -> FriggResult<SearchHybridExecutionOutput> {
        hybrid_execution::search_hybrid_with_filters_using_executor(
            self,
            query,
            filters,
            credentials,
            semantic_executor,
            true,
        )
    }

    fn search_with_streaming_lines_in_universe<F>(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        match_columns: F,
    ) -> FriggResult<SearchExecutionOutput>
    where
        F: FnMut(&str, &mut Vec<usize>),
    {
        scan_engine::search_with_streaming_lines_in_universe(
            query,
            candidate_universe,
            match_columns,
        )
    }

    fn search_with_streaming_lines_prefix_in_universe<F>(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        match_columns: F,
    ) -> FriggResult<SearchExecutionOutput>
    where
        F: FnMut(&str, &mut Vec<usize>),
    {
        scan_engine::search_with_streaming_lines_prefix_in_universe(
            query,
            candidate_universe,
            match_columns,
        )
    }

    fn search_with_matcher_in_universe<F, P>(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        file_may_match: P,
        match_columns: F,
    ) -> FriggResult<SearchExecutionOutput>
    where
        P: FnMut(&str) -> bool,
        F: FnMut(&str, &mut Vec<usize>),
    {
        scan_engine::search_with_matcher_in_universe(
            query,
            candidate_universe,
            file_may_match,
            match_columns,
        )
    }

    #[cfg(test)]
    pub(crate) fn search_with_matcher<F, P>(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
        file_may_match: P,
        match_columns: F,
    ) -> FriggResult<SearchExecutionOutput>
    where
        P: FnMut(&str) -> bool,
        F: FnMut(&str, &mut Vec<usize>),
    {
        let candidate_universe = self.build_candidate_universe(query, filters);
        self.search_with_matcher_in_universe(
            query,
            &candidate_universe,
            file_may_match,
            match_columns,
        )
    }

    fn search_literal_with_ripgrep_if_available(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
    ) -> FriggResult<Option<SearchExecutionOutput>> {
        self.search_with_ripgrep_if_available(
            query,
            candidate_universe,
            RipgrepPatternMode::Literal,
        )
    }

    fn search_regex_with_ripgrep_if_available(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
    ) -> FriggResult<Option<SearchExecutionOutput>> {
        self.search_with_ripgrep_if_available(query, candidate_universe, RipgrepPatternMode::Regex)
    }

    fn search_with_ripgrep_if_available(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        mode: RipgrepPatternMode,
    ) -> FriggResult<Option<SearchExecutionOutput>> {
        match self.config.lexical_runtime.backend {
            crate::settings::LexicalBackendMode::Native => return Ok(None),
            crate::settings::LexicalBackendMode::Auto
            | crate::settings::LexicalBackendMode::Ripgrep => {}
        }

        let Some((ripgrep_universe, native_universe)) =
            partition_candidate_universe_for_ripgrep(query, candidate_universe)
        else {
            return Ok(None);
        };
        let Some(ripgrep_universe) = ripgrep_universe else {
            return Ok(None);
        };

        let executable = match resolve_ripgrep_executable(&self.config.lexical_runtime) {
            Ok(Some(executable)) => executable,
            Ok(None) => return Ok(None),
            Err(reason) => {
                let mut fallback =
                    self.search_with_native_backend(query, candidate_universe, mode)?;
                fallback.lexical_backend_note = Some(format!(
                    "ripgrep unavailable; fell back to native scanner: {reason}"
                ));
                return Ok(Some(fallback));
            }
        };

        let mut output =
            match search_with_ripgrep_in_universe(&executable, query, &ripgrep_universe, mode) {
                Ok(output) => output,
                Err(err) => {
                    let mut fallback =
                        self.search_with_native_backend(query, candidate_universe, mode)?;
                    fallback.lexical_backend_note = Some(format!(
                        "ripgrep execution failed; fell back to native scanner: {err}"
                    ));
                    return Ok(Some(fallback));
                }
            };
        output
            .diagnostics
            .entries
            .extend(candidate_universe.diagnostics.entries.clone());
        sort_search_diagnostics_deterministically(&mut output.diagnostics.entries);
        output.diagnostics.entries.dedup();

        if let Some(native_universe) = native_universe {
            let native_output = self.search_with_native_backend(query, &native_universe, mode)?;
            merge_hybrid_lexical_search_output(&mut output, native_output, query.limit);
            output.lexical_backend = Some(SearchLexicalBackend::Mixed);
            output.lexical_backend_note = Some(
                "ripgrep accelerator active with native fallback for scrubbed content".to_owned(),
            );
        }

        Ok(Some(output))
    }

    fn search_with_native_backend(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        mode: RipgrepPatternMode,
    ) -> FriggResult<SearchExecutionOutput> {
        match mode {
            RipgrepPatternMode::Literal => {
                let matcher = AhoCorasick::new([query.query.as_str()])
                    .map_err(|err| FriggError::InvalidInput(format!("invalid query: {err}")))?;
                self.search_with_streaming_lines_in_universe(
                    query,
                    candidate_universe,
                    |line, columns| {
                        columns.clear();
                        columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
                    },
                )
            }
            RipgrepPatternMode::Regex => {
                let matcher =
                    compile_safe_regex(&query.query).map_err(regex_error_to_frigg_error)?;
                let prefilter_plan = build_regex_prefilter_plan(&query.query);
                if prefilter_plan.is_none() {
                    return self.search_with_streaming_lines_in_universe(
                        query,
                        candidate_universe,
                        |line, columns| {
                            columns.clear();
                            columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
                        },
                    );
                }
                self.search_with_matcher_in_universe(
                    query,
                    candidate_universe,
                    |content| {
                        prefilter_plan
                            .as_ref()
                            .is_none_or(|plan| plan.file_may_match(content))
                    },
                    |line, columns| {
                        columns.clear();
                        columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
                    },
                )
            }
        }
    }
}

fn partition_candidate_universe_for_ripgrep(
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
) -> Option<(
    Option<SearchCandidateUniverse>,
    Option<SearchCandidateUniverse>,
)> {
    let mut ripgrep_repositories = Vec::new();
    let mut native_repositories = Vec::new();

    for repository in &candidate_universe.repositories {
        let mut ripgrep_candidates = Vec::new();
        let mut native_candidates = Vec::new();
        for candidate in &repository.candidates {
            if query
                .path_regex
                .as_ref()
                .is_some_and(|path_regex| !path_regex.is_match(&candidate.relative_path))
            {
                continue;
            }
            if content_scrub::should_scrub_leading_markdown_comment(&candidate.relative_path) {
                native_candidates.push(candidate.clone());
            } else {
                ripgrep_candidates.push(candidate.clone());
            }
        }

        if !ripgrep_candidates.is_empty() {
            ripgrep_repositories.push(RepositoryCandidateUniverse {
                repository_id: repository.repository_id.clone(),
                root: repository.root.clone(),
                snapshot_id: repository.snapshot_id.clone(),
                candidates: ripgrep_candidates,
            });
        }
        if !native_candidates.is_empty() {
            native_repositories.push(RepositoryCandidateUniverse {
                repository_id: repository.repository_id.clone(),
                root: repository.root.clone(),
                snapshot_id: repository.snapshot_id.clone(),
                candidates: native_candidates,
            });
        }
    }

    let ripgrep_universe = (!ripgrep_repositories.is_empty()).then_some(SearchCandidateUniverse {
        repositories: ripgrep_repositories,
        diagnostics: SearchExecutionDiagnostics::default(),
    });
    let native_universe = (!native_repositories.is_empty()).then_some(SearchCandidateUniverse {
        repositories: native_repositories,
        diagnostics: SearchExecutionDiagnostics::default(),
    });
    if ripgrep_universe.is_none() && native_universe.is_none() {
        None
    } else {
        Some((ripgrep_universe, native_universe))
    }
}

fn normalize_search_filters(filters: SearchFilters) -> FriggResult<NormalizedSearchFilters> {
    let repository_id = filters
        .repository_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);

    let language = match filters
        .language
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(raw) => Some(
            parse_supported_language(raw, LanguageCapability::SourceFilter).ok_or_else(|| {
                FriggError::InvalidInput(format!(
                    "unsupported language filter '{raw}'; supported values: {}",
                    SymbolLanguage::supported_search_filter_values().join(", ")
                ))
            })?,
        ),
        None => None,
    };

    Ok(NormalizedSearchFilters {
        repository_id,
        language,
    })
}

#[cfg(test)]
mod tests;
