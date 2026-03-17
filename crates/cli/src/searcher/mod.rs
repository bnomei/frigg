use std::borrow::Cow;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Instant;

mod attribution;
mod candidates;
mod graph_channel;
mod hybrid_execution;
mod intent;
mod laravel;
mod lexical_channel;
mod lexical_recall;
mod ordering;
mod overlay_projection;
mod path_witness_projection;
mod policy;
mod projection_service;
mod query_terms;
mod ranker;
mod regex_support;
mod reranker;
mod retrieval_projection;
mod scan_engine;
mod semantic;
mod surfaces;
mod types;

use crate::domain::{FriggError, FriggResult, model::TextMatch};
use crate::languages::{LanguageCapability, SymbolLanguage, parse_supported_language};
pub use crate::manifest_validation::ValidatedManifestCandidateCache;
use crate::manifest_validation::latest_validated_manifest_snapshot;
use crate::settings::{FriggConfig, SemanticRuntimeCredentials};
use crate::storage::{Storage, resolve_provenance_db_path};
use crate::text_sanitization::scrub_leading_html_comment;
use crate::workspace_ignores::{build_root_ignore_matcher, should_ignore_runtime_path};
use aho_corasick::AhoCorasick;
use attribution::elapsed_us;
pub use attribution::{SearchStageAttribution, SearchStageSample};
use candidates::{
    hidden_workflow_candidates_for_repository, merge_candidate_files,
    normalize_repository_relative_path, root_scoped_runtime_config_candidates_for_repository,
    walk_candidate_files_for_repository,
};
use graph_channel::{HybridGraphArtifact, HybridGraphArtifactCacheKey, search_graph_channel_hits};
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
use lexical_channel::{
    HybridPathWitnessQueryContext, best_path_witness_anchor_in_file,
    build_hybrid_lexical_hits_with_intent, build_hybrid_path_witness_hits_with_intent,
    hybrid_path_has_exact_stem_match, hybrid_path_quality_multiplier_with_intent,
    hybrid_path_witness_recall_score, merge_hybrid_lexical_search_output, semantic_excerpt,
};
#[cfg(test)]
use lexical_channel::{build_hybrid_lexical_hits, build_hybrid_lexical_hits_for_query};
use lexical_recall::{build_hybrid_lexical_recall_regex, hybrid_lexical_recall_tokens};
use ordering::{
    retain_bounded_match, sort_matches_deterministically,
    sort_search_diagnostics_deterministically, text_match_candidate_order,
};
#[cfg(test)]
use overlay_projection::StoredEntrypointSurfaceProjection;
pub(crate) use overlay_projection::{
    StoredTestSubjectProjection, build_entrypoint_surface_projection_records_from_paths,
    build_test_subject_projection_records as build_test_subject_projection_records_from_paths,
    decode_entrypoint_surface_projection_records, decode_test_subject_projection_records,
};
pub(crate) use path_witness_projection::{
    PATH_WITNESS_PROJECTION_HEURISTIC_VERSION, StoredPathWitnessProjection,
    build_path_witness_projection_records_from_paths, decode_path_witness_projection_records,
};
pub(crate) use policy::{
    apply_post_selection_guardrails_with_trace, path_quality_rule_trace, path_witness_rule_trace,
    selection_rule_trace,
};
use projection_service::ProjectionStoreService;
use query_terms::{
    hybrid_excerpt_has_build_flow_anchor, hybrid_excerpt_has_exact_identifier_anchor,
    hybrid_excerpt_has_test_double_anchor, hybrid_identifier_tokens, hybrid_overlap_count,
    hybrid_path_overlap_count, hybrid_path_overlap_tokens, hybrid_query_exact_terms,
    hybrid_query_overlap_terms,
};
#[cfg(test)]
use ranker::group_hybrid_ranked_evidence;
pub use ranker::rank_hybrid_evidence;
use ranker::{blend_hybrid_evidence, group_all_hybrid_ranked_evidence, rank_lexical_hybrid_hits};
use regex::Regex;
pub use regex_support::{RegexSearchError, compile_safe_regex};
use regex_support::{build_regex_prefilter_plan, regex_error_to_frigg_error};
use reranker::{build_coverage_grouped_pool, diversify_hybrid_ranked_evidence};
pub(crate) use retrieval_projection::{
    ENTRYPOINT_SURFACE_PROJECTION_HEURISTIC_VERSION,
    RETRIEVAL_PROJECTION_FAMILY_ENTRYPOINT_SURFACE, RETRIEVAL_PROJECTION_FAMILY_PATH_WITNESS,
    RETRIEVAL_PROJECTION_FAMILY_TEST_SUBJECT, TEST_SUBJECT_PROJECTION_HEURISTIC_VERSION,
    build_retrieval_projection_bundle,
};
use semantic::{
    RuntimeSemanticQueryEmbeddingExecutor, SemanticRuntimeQueryEmbeddingExecutor,
    retain_semantic_hits_for_query, search_semantic_channel_hits,
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
    SearchHybridQuery, SearchTextQuery,
};
pub(crate) use types::{
    HybridGraphFileAnalysis, HybridGraphFileAnalysisCacheKey, HybridPathWitnessProjectionCacheKey,
    ManifestCandidateFilesBuild, NormalizedSearchFilters, RepositoryCandidateUniverse,
    SearchCandidateFile, SearchCandidateUniverse, SearchCandidateUniverseBuild,
    empty_channel_result, hybrid_execution_note_from_channel_results, match_count_for_hits,
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
    Ok(ranked.into_iter().take(limit).collect())
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
        Self {
            config,
            validated_manifest_candidate_cache,
            projection_store_service: ProjectionStoreService::new(),
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
        self.search_with_streaming_lines(&query, &normalized_filters, |line, columns| {
            columns.clear();
            columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
        })
    }

    fn search_literal_with_candidate_universe(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
    ) -> FriggResult<SearchExecutionOutput> {
        let matcher = AhoCorasick::new([query.query.as_str()])
            .map_err(|err| FriggError::InvalidInput(format!("invalid query: {err}")))?;
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
        if prefilter_plan.is_none() {
            return self.search_with_streaming_lines(
                &query,
                &normalized_filters,
                |line, columns| {
                    columns.clear();
                    columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
                },
            );
        }
        self.search_with_matcher(
            &query,
            &normalized_filters,
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

    fn search_regex_with_candidate_universe(
        &self,
        query: &SearchTextQuery,
        candidate_universe: &SearchCandidateUniverse,
        matcher: Regex,
        prefilter_plan: Option<regex_support::RegexPrefilterPlan>,
    ) -> FriggResult<SearchExecutionOutput> {
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

    fn search_with_streaming_lines<F>(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
        match_columns: F,
    ) -> FriggResult<SearchExecutionOutput>
    where
        F: FnMut(&str, &mut Vec<usize>),
    {
        let candidate_universe = self.build_candidate_universe(query, filters);
        self.search_with_streaming_lines_in_universe(query, &candidate_universe, match_columns)
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

    fn search_with_matcher<F, P>(
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
}

fn should_scrub_leading_markdown_comment(path: &str) -> bool {
    matches!(
        Path::new(path.trim_start_matches("./"))
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "markdown" | "mdown")
    )
}

fn scrub_search_content<'a>(path: &str, content: &'a str) -> Cow<'a, str> {
    if should_scrub_leading_markdown_comment(path) {
        return scrub_leading_html_comment(content);
    }

    Cow::Borrowed(content)
}

impl TextSearcher {
    fn build_candidate_universe(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
    ) -> SearchCandidateUniverse {
        self.build_candidate_universe_with_attribution(query, filters)
            .universe
    }

    fn build_candidate_universe_with_attribution(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
    ) -> SearchCandidateUniverseBuild {
        let mut diagnostics = SearchExecutionDiagnostics::default();
        let mut repositories = self.config.repositories();
        let mut candidate_intake_elapsed_us = 0_u64;
        let mut freshness_validation_elapsed_us = 0_u64;
        let mut manifest_backed_repository_count = 0_usize;
        repositories.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.root_path.cmp(&right.root_path))
        });

        let repositories = repositories
            .into_iter()
            .filter(|repository| {
                filters
                    .repository_id
                    .as_ref()
                    .is_none_or(|repository_id| repository_id == &repository.repository_id.0)
            })
            .map(|repository| {
                let repository_id = repository.repository_id.0;
                let root = PathBuf::from(repository.root_path);
                let (snapshot_id, candidates) = self
                    .manifest_candidate_files_for_repository_with_attribution(
                        &repository_id,
                        &root,
                        query,
                        filters,
                    )
                    .map(|manifest| {
                        candidate_intake_elapsed_us = candidate_intake_elapsed_us
                            .saturating_add(manifest.candidate_intake_elapsed_us);
                        freshness_validation_elapsed_us = freshness_validation_elapsed_us
                            .saturating_add(manifest.freshness_validation_elapsed_us);
                        manifest_backed_repository_count =
                            manifest_backed_repository_count.saturating_add(1);
                        (Some(manifest.snapshot_id), manifest.candidates)
                    })
                    .unwrap_or_else(|| {
                        let walk_started_at = Instant::now();
                        let walked = walk_candidate_files_for_repository(
                            &repository_id,
                            &root,
                            query,
                            filters,
                            &mut diagnostics,
                        );
                        candidate_intake_elapsed_us =
                            candidate_intake_elapsed_us.saturating_add(elapsed_us(walk_started_at));
                        (None, walked)
                    });
                let candidates = candidates
                    .into_iter()
                    .map(|(relative_path, absolute_path)| SearchCandidateFile {
                        relative_path,
                        absolute_path,
                    })
                    .collect::<Vec<_>>();
                RepositoryCandidateUniverse {
                    repository_id,
                    root,
                    snapshot_id,
                    candidates,
                }
            })
            .collect::<Vec<_>>();
        let repository_count = repositories.len();
        let candidate_count = repositories
            .iter()
            .map(|repository| repository.candidates.len())
            .sum();

        sort_search_diagnostics_deterministically(&mut diagnostics.entries);

        SearchCandidateUniverseBuild {
            universe: SearchCandidateUniverse {
                repositories,
                diagnostics,
            },
            repository_count,
            candidate_count,
            manifest_backed_repository_count,
            candidate_intake_elapsed_us,
            freshness_validation_elapsed_us,
        }
    }

    fn candidate_universe_with_hidden_workflows(
        &self,
        candidate_universe: &SearchCandidateUniverse,
        filters: &NormalizedSearchFilters,
        intent: &HybridRankingIntent,
    ) -> SearchCandidateUniverse {
        let mut candidate_universe = candidate_universe.clone();
        for repository in &mut candidate_universe.repositories {
            let mut candidates = repository
                .candidates
                .iter()
                .map(|candidate| {
                    (
                        candidate.relative_path.clone(),
                        candidate.absolute_path.clone(),
                    )
                })
                .collect::<Vec<_>>();
            merge_candidate_files(
                &mut candidates,
                hidden_workflow_candidates_for_repository(
                    &repository.repository_id,
                    &repository.root,
                    filters,
                    intent,
                    &mut candidate_universe.diagnostics,
                ),
            );
            merge_candidate_files(
                &mut candidates,
                root_scoped_runtime_config_candidates_for_repository(
                    &repository.repository_id,
                    &repository.root,
                    filters,
                    intent,
                    &mut candidate_universe.diagnostics,
                ),
            );
            repository.candidates = candidates
                .into_iter()
                .map(|(relative_path, absolute_path)| SearchCandidateFile {
                    relative_path,
                    absolute_path,
                })
                .collect::<Vec<_>>();
        }

        sort_search_diagnostics_deterministically(&mut candidate_universe.diagnostics.entries);
        candidate_universe
    }

    fn manifest_candidate_files_for_repository_with_attribution(
        &self,
        repository_id: &str,
        root: &Path,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
    ) -> Option<ManifestCandidateFilesBuild> {
        let db_path = resolve_provenance_db_path(root).ok()?;
        if !db_path.exists() {
            return None;
        }

        let storage = Storage::new(db_path);
        let freshness_started_at = Instant::now();
        let validated_snapshot = latest_validated_manifest_snapshot(
            &storage,
            repository_id,
            root,
            Some(&self.validated_manifest_candidate_cache),
        )?;
        let freshness_validation_elapsed_us = elapsed_us(freshness_started_at);
        let candidate_intake_started_at = Instant::now();
        let root_ignore_matcher = build_root_ignore_matcher(root);
        let mut candidates = Vec::new();
        for digest in validated_snapshot.digests {
            let path = digest.path;
            if should_ignore_runtime_path(root, &path, Some(&root_ignore_matcher)) {
                continue;
            }
            let rel_path = normalize_repository_relative_path(root, &path);

            if let Some(language) = filters.language {
                if !language.matches_path(&path) {
                    continue;
                }
            }
            if let Some(path_regex) = &query.path_regex {
                if !path_regex.is_match(&rel_path) {
                    continue;
                }
            }

            candidates.push((rel_path, path));
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        candidates.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        Some(ManifestCandidateFilesBuild {
            snapshot_id: validated_snapshot.snapshot_id,
            candidates,
            candidate_intake_elapsed_us: elapsed_us(candidate_intake_started_at),
            freshness_validation_elapsed_us,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    fn search_path_witness_recall_with_filters(
        &self,
        query_text: &str,
        filters: &SearchFilters,
        limit: usize,
        intent: &HybridRankingIntent,
    ) -> FriggResult<SearchExecutionOutput> {
        if limit == 0 || !intent.wants_path_witness_recall() {
            return Ok(SearchExecutionOutput::default());
        }

        let normalized_filters = normalize_search_filters(filters.clone())?;
        let empty_query = SearchTextQuery {
            query: String::new(),
            path_regex: None,
            limit,
        };
        let candidate_universe = self.build_candidate_universe(&empty_query, &normalized_filters);
        self.search_path_witness_recall_in_universe(
            query_text,
            &candidate_universe,
            &normalized_filters,
            limit,
            intent,
        )
    }

    fn search_path_witness_recall_in_universe(
        &self,
        query_text: &str,
        candidate_universe: &SearchCandidateUniverse,
        filters: &NormalizedSearchFilters,
        limit: usize,
        intent: &HybridRankingIntent,
    ) -> FriggResult<SearchExecutionOutput> {
        let frontier = policy::plan_path_witness_frontier(intent, limit);
        let top_k = frontier.top_k;
        let materialized_limit = frontier.materialized_limit;
        let query_context = HybridPathWitnessQueryContext::from_query_text(query_text);
        let mut scored = Vec::<PathWitnessCandidate>::with_capacity(top_k);
        let base_repositories = candidate_universe
            .repositories
            .iter()
            .map(|repository| (repository.repository_id.clone(), repository))
            .collect::<BTreeMap<_, _>>();
        let candidate_universe =
            self.candidate_universe_with_hidden_workflows(candidate_universe, filters, intent);
        for repository in &candidate_universe.repositories {
            let repository_candidates = self
                .projected_path_witness_candidates_for_repository(
                    repository,
                    base_repositories.get(&repository.repository_id).copied(),
                    intent,
                    &query_context,
                )
                .unwrap_or_else(|| {
                    repository
                        .candidates
                        .iter()
                        .filter_map(|candidate| {
                            let score = hybrid_path_witness_recall_score(
                                &candidate.relative_path,
                                intent,
                                &query_context,
                            )?;
                            Some(PathWitnessCandidate {
                                score,
                                repository_id: repository.repository_id.clone(),
                                rel_path: candidate.relative_path.clone(),
                                path: candidate.absolute_path.clone(),
                                witness_provenance_ids: Vec::new(),
                            })
                        })
                        .collect::<Vec<_>>()
                });
            for candidate in repository_candidates {
                let insert_at = scored.partition_point(|probe| {
                    path_witness_candidate_order(probe, &candidate).is_lt()
                });
                if insert_at >= top_k {
                    continue;
                }

                scored.insert(insert_at, candidate);
                if scored.len() > top_k {
                    scored.pop();
                }
            }
        }

        let mut matches = Vec::with_capacity(materialized_limit);
        for candidate in scored.into_iter().take(materialized_limit) {
            let PathWitnessCandidate {
                score,
                repository_id,
                rel_path,
                path,
                witness_provenance_ids,
            } = candidate;
            let projected_anchor = base_repositories
                .get(&repository_id)
                .and_then(|repository| {
                    self.projection_store_service
                        .best_path_witness_anchor_for_repository(
                            repository,
                            &rel_path,
                            &query_context,
                        )
                });
            let (line, excerpt) = projected_anchor
                .or_else(|| best_path_witness_anchor_in_file(&rel_path, &path, &query_context))
                .unwrap_or_else(|| (1, rel_path.clone()));
            matches.push(TextMatch {
                repository_id,
                path: rel_path,
                line,
                column: 1,
                excerpt,
                witness_score_hint_millis: Some(path_witness_score_hint_millis(score)),
                witness_provenance_ids: (!witness_provenance_ids.is_empty())
                    .then_some(witness_provenance_ids),
            });
        }

        Ok(SearchExecutionOutput {
            total_matches: matches.len(),
            matches,
            diagnostics: candidate_universe.diagnostics,
        })
    }

    fn projected_path_witness_candidates_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        base_repository: Option<&RepositoryCandidateUniverse>,
        intent: &HybridRankingIntent,
        query_context: &HybridPathWitnessQueryContext,
    ) -> Option<Vec<PathWitnessCandidate>> {
        self.projection_store_service
            .projected_path_witness_candidates_for_repository(
                repository,
                base_repository,
                intent,
                query_context,
            )
    }

    fn build_overlay_aware_path_witness_seed_universe(
        &self,
        candidate_universe: &SearchCandidateUniverse,
        filters: &NormalizedSearchFilters,
        intent: &HybridRankingIntent,
        query_context: &HybridPathWitnessQueryContext,
        lexical_limit: usize,
    ) -> Option<SearchCandidateUniverse> {
        let per_repository_limit = lexical_limit.saturating_div(2).saturating_add(4).max(10);
        let overlay_reserve = overlay_seed_reserve_slots(intent, per_repository_limit);
        let expanded_universe =
            self.candidate_universe_with_hidden_workflows(candidate_universe, filters, intent);
        let repositories = expanded_universe
            .repositories
            .iter()
            .filter_map(|repository| {
                let base_repository = candidate_universe
                    .repositories
                    .iter()
                    .find(|candidate| candidate.repository_id == repository.repository_id);
                let overlay_boosts_by_path =
                    self.projection_store_service.overlay_boosts_for_repository(
                        repository,
                        base_repository,
                        intent,
                        query_context,
                    );
                let mut scored = repository
                    .candidates
                    .iter()
                    .filter_map(|candidate| {
                        let base_score = hybrid_path_witness_recall_score(
                            &candidate.relative_path,
                            intent,
                            query_context,
                        );
                        let overlay_boost = overlay_boosts_by_path
                            .get(&candidate.relative_path)
                            .cloned()
                            .unwrap_or_default();
                        let score = base_score
                            .map(|score| score + overlay_boost.bonus_score())
                            .or_else(|| {
                                (overlay_boost.bonus_millis > 0)
                                    .then_some(overlay_boost.bonus_score())
                            })?;
                        Some((score, overlay_boost.bonus_millis > 0, candidate))
                    })
                    .collect::<Vec<_>>();
                if scored.is_empty() {
                    return None;
                }

                scored.sort_by(|left, right| {
                    right
                        .0
                        .total_cmp(&left.0)
                        .then_with(|| right.1.cmp(&left.1))
                        .then_with(|| left.2.relative_path.cmp(&right.2.relative_path))
                        .then_with(|| left.2.absolute_path.cmp(&right.2.absolute_path))
                });
                let mut candidates = Vec::<SearchCandidateFile>::new();
                let mut selected_paths = std::collections::BTreeSet::<String>::new();
                let base_take = per_repository_limit.saturating_sub(overlay_reserve);
                for (_, _, candidate) in scored.iter().take(base_take) {
                    selected_paths.insert(candidate.relative_path.clone());
                    candidates.push(SearchCandidateFile {
                        relative_path: candidate.relative_path.clone(),
                        absolute_path: candidate.absolute_path.clone(),
                    });
                }
                if overlay_reserve > 0 {
                    for (_, has_overlay, candidate) in &scored {
                        if !has_overlay || !selected_paths.insert(candidate.relative_path.clone()) {
                            continue;
                        }
                        candidates.push(SearchCandidateFile {
                            relative_path: candidate.relative_path.clone(),
                            absolute_path: candidate.absolute_path.clone(),
                        });
                        if candidates.len() >= per_repository_limit {
                            break;
                        }
                    }
                }
                if candidates.len() < per_repository_limit {
                    for (_, _, candidate) in scored {
                        if !selected_paths.insert(candidate.relative_path.clone()) {
                            continue;
                        }
                        candidates.push(SearchCandidateFile {
                            relative_path: candidate.relative_path.clone(),
                            absolute_path: candidate.absolute_path.clone(),
                        });
                        if candidates.len() >= per_repository_limit {
                            break;
                        }
                    }
                }
                Some(RepositoryCandidateUniverse {
                    repository_id: repository.repository_id.clone(),
                    root: repository.root.clone(),
                    snapshot_id: repository.snapshot_id.clone(),
                    candidates,
                })
            })
            .collect::<Vec<_>>();
        if repositories.is_empty() {
            return None;
        }

        Some(SearchCandidateUniverse {
            repositories,
            diagnostics: expanded_universe.diagnostics,
        })
    }
}

#[derive(Debug)]
pub(super) struct PathWitnessCandidate {
    score: f32,
    repository_id: String,
    rel_path: String,
    path: PathBuf,
    witness_provenance_ids: Vec<String>,
}

fn overlay_seed_reserve_slots(intent: &HybridRankingIntent, per_repository_limit: usize) -> usize {
    let mut reserve = 0;
    if intent.wants_runtime_witnesses {
        reserve += 1;
    }
    if intent.wants_tests || intent.wants_test_witness_recall {
        reserve += 1;
    }
    if intent.wants_entrypoint_build_flow
        || intent.wants_runtime_config_artifacts
        || intent.wants_ci_workflow_witnesses
        || intent.wants_scripts_ops_witnesses
    {
        reserve += 1;
    }

    reserve.min(per_repository_limit.saturating_sub(1)).min(2)
}

fn path_witness_score_hint_millis(score: f32) -> u32 {
    let millis = score.max(0.0).mul_add(1000.0, 0.0).round();
    if !millis.is_finite() {
        return u32::MAX;
    }
    millis.clamp(0.0, u32::MAX as f32) as u32
}

fn path_witness_candidate_order(
    left: &PathWitnessCandidate,
    right: &PathWitnessCandidate,
) -> Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.repository_id.cmp(&right.repository_id))
        .then_with(|| left.rel_path.cmp(&right.rel_path))
        .then_with(|| left.path.cmp(&right.path))
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
