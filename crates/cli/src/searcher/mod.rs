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
mod path_witness_projection;
mod query_terms;
mod ranker;
mod regex_support;
mod reranker;
mod scan_engine;
mod semantic;
mod surfaces;

use crate::domain::{
    ChannelDiagnostic, ChannelHealth, ChannelHealthStatus, ChannelResult, ChannelStats,
    EvidenceAnchor, EvidenceChannel, EvidenceDocumentRef, EvidenceHit, FriggError, FriggResult,
    model::TextMatch,
};
use crate::indexer::PhpDeclarationRelation;
use crate::languages::{
    BladeSourceEvidence, LanguageCapability, PhpSourceEvidence, SymbolLanguage,
    parse_supported_language,
};
pub use crate::manifest_validation::ValidatedManifestCandidateCache;
use crate::manifest_validation::latest_validated_manifest_snapshot;
use crate::settings::{FriggConfig, SemanticRuntimeCredentials};
use crate::storage::{
    ManifestEntry, PathWitnessProjectionRecord, Storage, resolve_provenance_db_path,
};
use crate::text_sanitization::scrub_leading_html_comment;
use crate::workspace_ignores::{build_root_ignore_matcher, should_ignore_runtime_path};
use aho_corasick::AhoCorasick;
use attribution::elapsed_us;
pub use attribution::{SearchStageAttribution, SearchStageSample};
use candidates::{
    hidden_workflow_candidates_for_repository, merge_candidate_files,
    normalize_repository_relative_path, walk_candidate_files_for_repository,
};
use graph_channel::{HybridGraphArtifact, HybridGraphArtifactCacheKey, search_graph_channel_hits};
use intent::HybridRankingIntent;
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
    hybrid_canonical_match_multiplier, hybrid_path_has_exact_stem_match,
    hybrid_path_quality_multiplier_with_intent, hybrid_path_witness_recall_score,
    hybrid_path_witness_recall_score_for_projection, merge_hybrid_lexical_search_output,
    semantic_excerpt,
};
#[cfg(test)]
use lexical_channel::{build_hybrid_lexical_hits, build_hybrid_lexical_hits_for_query};
use lexical_recall::{build_hybrid_lexical_recall_regex, hybrid_lexical_recall_tokens};
use ordering::{
    retain_bounded_match, sort_matches_deterministically,
    sort_search_diagnostics_deterministically, text_match_candidate_order,
};
use path_witness_projection::{
    StoredPathWitnessProjection, build_path_witness_projection_record,
    decode_path_witness_projection_record,
};
use query_terms::{
    hybrid_excerpt_has_build_flow_anchor, hybrid_excerpt_has_exact_identifier_anchor,
    hybrid_excerpt_has_test_double_anchor, hybrid_identifier_tokens, hybrid_overlap_count,
    hybrid_path_overlap_count, hybrid_path_overlap_tokens, hybrid_query_exact_terms,
    hybrid_query_overlap_terms, hybrid_specific_witness_query_terms,
    path_has_exact_query_term_match,
};
pub use ranker::rank_hybrid_evidence;
use ranker::{blend_hybrid_evidence, group_hybrid_ranked_evidence, rank_lexical_hybrid_hits};
use regex::Regex;
pub use regex_support::{RegexSearchError, compile_safe_regex};
use regex_support::{build_regex_prefilter_plan, regex_error_to_frigg_error};
use reranker::diversify_hybrid_ranked_evidence;
use semantic::{
    RuntimeSemanticQueryEmbeddingExecutor, SemanticRuntimeQueryEmbeddingExecutor,
    retain_semantic_hits_for_query, search_semantic_channel_hits,
};
use surfaces::{
    HybridSourceClass, hybrid_source_class, is_bench_support_path, is_ci_workflow_path,
    is_cli_test_support_path, is_entrypoint_build_workflow_path, is_entrypoint_reference_doc_path,
    is_entrypoint_runtime_path, is_example_support_path, is_frontend_runtime_noise_path,
    is_generic_runtime_witness_doc_path, is_loose_python_test_module_path,
    is_navigation_reference_doc_path, is_navigation_runtime_path, is_non_code_test_doc_path,
    is_python_entrypoint_runtime_path, is_python_runtime_config_path, is_python_test_witness_path,
    is_repo_metadata_path, is_runtime_config_artifact_path, is_rust_workspace_config_path,
    is_scripts_ops_path, is_test_harness_path, is_test_support_path,
};

#[derive(Debug, Clone)]
pub struct SearchTextQuery {
    pub query: String,
    pub path_regex: Option<Regex>,
    pub limit: usize,
}

#[derive(Debug, Clone, Default)]
pub struct SearchFilters {
    pub repository_id: Option<String>,
    pub language: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SearchDiagnosticKind {
    Walk,
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchDiagnostic {
    pub repository_id: String,
    pub path: Option<String>,
    pub kind: SearchDiagnosticKind,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchExecutionDiagnostics {
    pub entries: Vec<SearchDiagnostic>,
}

impl SearchExecutionDiagnostics {
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    pub fn count_by_kind(&self, kind: SearchDiagnosticKind) -> usize {
        self.entries
            .iter()
            .filter(|diagnostic| diagnostic.kind == kind)
            .count()
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchExecutionOutput {
    pub total_matches: usize,
    pub matches: Vec<TextMatch>,
    pub diagnostics: SearchExecutionDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SearchCandidateFile {
    relative_path: String,
    absolute_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct RepositoryCandidateUniverse {
    repository_id: String,
    root: PathBuf,
    snapshot_id: Option<String>,
    candidates: Vec<SearchCandidateFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SearchCandidateUniverse {
    repositories: Vec<RepositoryCandidateUniverse>,
    diagnostics: SearchExecutionDiagnostics,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct SearchCandidateUniverseBuild {
    universe: SearchCandidateUniverse,
    repository_count: usize,
    candidate_count: usize,
    manifest_backed_repository_count: usize,
    candidate_intake_elapsed_us: u64,
    freshness_validation_elapsed_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ManifestCandidateFilesBuild {
    snapshot_id: String,
    candidates: Vec<(String, PathBuf)>,
    candidate_intake_elapsed_us: u64,
    freshness_validation_elapsed_us: u64,
}

pub type HybridDocumentRef = EvidenceDocumentRef;
pub type HybridChannelHit = EvidenceHit;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HybridChannelWeights {
    pub lexical: f32,
    pub graph: f32,
    pub semantic: f32,
}

impl Default for HybridChannelWeights {
    fn default() -> Self {
        Self {
            lexical: 0.5,
            graph: 0.3,
            semantic: 0.2,
        }
    }
}

impl HybridChannelWeights {
    pub fn validate(self) -> FriggResult<Self> {
        if self.lexical < 0.0 || self.graph < 0.0 || self.semantic < 0.0 {
            return Err(FriggError::InvalidInput(
                "hybrid channel weights must be >= 0".to_owned(),
            ));
        }
        if self.lexical == 0.0 && self.graph == 0.0 && self.semantic == 0.0 {
            return Err(FriggError::InvalidInput(
                "hybrid channel weights must include at least one non-zero channel".to_owned(),
            ));
        }

        Ok(self)
    }
}

#[derive(Debug, Clone)]
pub struct SearchHybridQuery {
    pub query: String,
    pub limit: usize,
    pub weights: HybridChannelWeights,
    pub semantic: Option<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HybridSemanticStatus {
    Disabled,
    Unavailable,
    Ok,
    Degraded,
}

impl HybridSemanticStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
            Self::Ok => "ok",
            Self::Degraded => "degraded",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HybridExecutionNote {
    pub semantic_requested: bool,
    pub semantic_enabled: bool,
    pub semantic_status: HybridSemanticStatus,
    pub semantic_reason: Option<String>,
    pub semantic_candidate_count: usize,
    pub semantic_hit_count: usize,
    pub semantic_match_count: usize,
}

impl Default for HybridExecutionNote {
    fn default() -> Self {
        Self {
            semantic_requested: false,
            semantic_enabled: false,
            semantic_status: HybridSemanticStatus::Disabled,
            semantic_reason: None,
            semantic_candidate_count: 0,
            semantic_hit_count: 0,
            semantic_match_count: 0,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SearchHybridExecutionOutput {
    pub matches: Vec<HybridRankedEvidence>,
    pub ranked_anchors: Vec<HybridRankedEvidence>,
    pub diagnostics: SearchExecutionDiagnostics,
    pub channel_results: Vec<ChannelResult>,
    pub note: HybridExecutionNote,
    pub stage_attribution: Option<SearchStageAttribution>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HybridRankedEvidence {
    pub document: HybridDocumentRef,
    pub anchor: EvidenceAnchor,
    pub excerpt: String,
    pub blended_score: f32,
    pub lexical_score: f32,
    pub graph_score: f32,
    pub semantic_score: f32,
    pub lexical_sources: Vec<String>,
    pub graph_sources: Vec<String>,
    pub semantic_sources: Vec<String>,
}

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

fn search_diagnostics_to_channel_diagnostics(
    diagnostics: &SearchExecutionDiagnostics,
) -> Vec<ChannelDiagnostic> {
    diagnostics
        .entries
        .iter()
        .map(|entry| ChannelDiagnostic {
            code: match entry.kind {
                SearchDiagnosticKind::Walk => "walk".to_owned(),
                SearchDiagnosticKind::Read => "read".to_owned(),
            },
            message: entry.message.clone(),
        })
        .collect()
}

fn empty_channel_result(
    channel: EvidenceChannel,
    status: ChannelHealthStatus,
    reason: Option<String>,
) -> ChannelResult {
    ChannelResult::new(
        channel,
        Vec::new(),
        ChannelHealth::new(status, reason),
        Vec::new(),
        ChannelStats::default(),
    )
}

fn channel_result_by_channel(
    channel_results: &[ChannelResult],
    channel: EvidenceChannel,
) -> Option<&ChannelResult> {
    channel_results
        .iter()
        .find(|result| result.channel == channel)
}

fn hybrid_semantic_status_from_channel_health(status: ChannelHealthStatus) -> HybridSemanticStatus {
    match status {
        ChannelHealthStatus::Disabled | ChannelHealthStatus::Filtered => {
            HybridSemanticStatus::Disabled
        }
        ChannelHealthStatus::Unavailable => HybridSemanticStatus::Unavailable,
        ChannelHealthStatus::Ok => HybridSemanticStatus::Ok,
        ChannelHealthStatus::Degraded => HybridSemanticStatus::Degraded,
    }
}

fn hybrid_execution_note_from_channel_results(
    query_semantic: Option<bool>,
    semantic_runtime_enabled: bool,
    channel_results: &[ChannelResult],
) -> HybridExecutionNote {
    let semantic = channel_result_by_channel(channel_results, EvidenceChannel::Semantic);
    let semantic_requested = query_semantic.unwrap_or(semantic_runtime_enabled);
    let semantic_status = semantic
        .map(|result| hybrid_semantic_status_from_channel_health(result.health.status))
        .unwrap_or(HybridSemanticStatus::Disabled);
    let semantic_reason = semantic.and_then(|result| result.health.reason.clone());
    let semantic_candidate_count = semantic.map_or(0, |result| result.stats.candidate_count);
    let semantic_hit_count = semantic.map_or(0, |result| result.stats.hit_count);
    let semantic_match_count = semantic.map_or(0, |result| result.stats.match_count);

    HybridExecutionNote {
        semantic_requested,
        semantic_enabled: semantic_match_count > 0,
        semantic_status,
        semantic_reason,
        semantic_candidate_count,
        semantic_hit_count,
        semantic_match_count,
    }
}

fn match_count_for_hits(matches: &[HybridRankedEvidence], hits: &[HybridChannelHit]) -> usize {
    use std::collections::BTreeSet;

    if matches.is_empty() || hits.is_empty() {
        return 0;
    }

    let matched_documents = matches
        .iter()
        .map(|entry| (&entry.document.repository_id, &entry.document.path))
        .collect::<BTreeSet<_>>();
    hits.iter()
        .map(|hit| (&hit.document.repository_id, &hit.document.path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .filter(|document| matched_documents.contains(document))
        .count()
}

#[derive(Debug, Clone, Default)]
struct NormalizedSearchFilters {
    repository_id: Option<String>,
    language: Option<SymbolLanguage>,
}

pub struct TextSearcher {
    config: FriggConfig,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    hybrid_path_witness_projection_cache: Arc<
        RwLock<
            BTreeMap<HybridPathWitnessProjectionCacheKey, Arc<Vec<StoredPathWitnessProjection>>>,
        >,
    >,
    hybrid_graph_file_analysis_cache:
        Arc<RwLock<BTreeMap<HybridGraphFileAnalysisCacheKey, Arc<HybridGraphFileAnalysis>>>>,
    hybrid_graph_artifact_cache:
        Arc<RwLock<BTreeMap<HybridGraphArtifactCacheKey, Arc<HybridGraphArtifact>>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HybridPathWitnessProjectionCacheKey {
    repository_id: String,
    root: PathBuf,
    snapshot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct HybridGraphFileAnalysisCacheKey {
    path: PathBuf,
    modified_unix_nanos: u128,
    size_bytes: u64,
}

#[derive(Debug, Clone, Default)]
struct HybridGraphFileAnalysis {
    symbols: Vec<crate::indexer::SymbolDefinition>,
    php_declaration_relations: Option<Vec<PhpDeclarationRelation>>,
    php_evidence: Option<PhpSourceEvidence>,
    blade_evidence: Option<BladeSourceEvidence>,
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
            hybrid_path_witness_projection_cache: Arc::new(RwLock::new(BTreeMap::new())),
            hybrid_graph_file_analysis_cache: Arc::new(RwLock::new(BTreeMap::new())),
            hybrid_graph_artifact_cache: Arc::new(RwLock::new(BTreeMap::new())),
        }
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
        let widen_surface_witness_pool = intent.wants_laravel_ui_witnesses
            || intent.wants_test_witness_recall
            || intent.wants_entrypoint_build_flow;
        let top_k = if widen_surface_witness_pool {
            limit.saturating_mul(4).max(32)
        } else {
            limit.saturating_mul(2).max(16)
        };
        let materialized_limit = if widen_surface_witness_pool {
            limit.saturating_mul(2).max(20).min(top_k)
        } else {
            limit.saturating_add(2).max(8).min(top_k)
        };
        let query_context = HybridPathWitnessQueryContext::new(query_text);
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
                repository_id,
                rel_path,
                path,
                ..
            } = candidate;
            let (line, excerpt) =
                best_path_witness_anchor_in_file(&rel_path, &path, &query_context)
                    .unwrap_or_else(|| (1, rel_path.clone()));
            matches.push(TextMatch {
                repository_id,
                path: rel_path,
                line,
                column: 1,
                excerpt,
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
        let base_repository = base_repository?;
        let snapshot_id = base_repository.snapshot_id.as_deref()?;
        let projections = self
            .load_or_build_path_witness_projections_for_repository(base_repository, snapshot_id)?;
        let base_candidates_by_path = base_repository
            .candidates
            .iter()
            .map(|candidate| {
                (
                    candidate.relative_path.clone(),
                    candidate.absolute_path.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let projections_by_path = projections
            .iter()
            .map(|projection| (projection.path.clone(), projection))
            .collect::<BTreeMap<_, _>>();
        if base_candidates_by_path
            .keys()
            .any(|path| !projections_by_path.contains_key(path))
        {
            return None;
        }

        let mut scored = Vec::new();
        for (rel_path, path) in &base_candidates_by_path {
            let projection = projections_by_path.get(rel_path)?;
            let Some(score) = hybrid_path_witness_recall_score_for_projection(
                rel_path,
                projection,
                intent,
                query_context,
            ) else {
                continue;
            };
            scored.push(PathWitnessCandidate {
                score,
                repository_id: repository.repository_id.clone(),
                rel_path: rel_path.clone(),
                path: path.clone(),
            });
        }

        for candidate in &repository.candidates {
            if base_candidates_by_path.contains_key(&candidate.relative_path) {
                continue;
            }
            let Some(score) =
                hybrid_path_witness_recall_score(&candidate.relative_path, intent, query_context)
            else {
                continue;
            };
            scored.push(PathWitnessCandidate {
                score,
                repository_id: repository.repository_id.clone(),
                rel_path: candidate.relative_path.clone(),
                path: candidate.absolute_path.clone(),
            });
        }

        Some(scored)
    }

    fn load_or_build_path_witness_projections_for_repository(
        &self,
        repository: &RepositoryCandidateUniverse,
        snapshot_id: &str,
    ) -> Option<Arc<Vec<StoredPathWitnessProjection>>> {
        let cache_key = HybridPathWitnessProjectionCacheKey {
            repository_id: repository.repository_id.clone(),
            root: repository.root.clone(),
            snapshot_id: snapshot_id.to_owned(),
        };
        if let Some(cached) = self
            .hybrid_path_witness_projection_cache
            .read()
            .ok()?
            .get(&cache_key)
            .cloned()
        {
            return Some(cached);
        }

        let db_path = resolve_provenance_db_path(&repository.root).ok()?;
        if !db_path.exists() {
            return None;
        }

        let storage = Storage::new(db_path);
        let manifest_entries = storage.load_manifest_for_snapshot(snapshot_id).ok()?;
        if manifest_entries.is_empty() {
            return None;
        }
        let expected_paths = manifest_entries
            .iter()
            .map(|entry| {
                normalize_repository_relative_path(&repository.root, Path::new(&entry.path))
            })
            .collect::<Vec<_>>();

        let mut rows = storage
            .load_path_witness_projections_for_repository_snapshot(
                &repository.repository_id,
                snapshot_id,
            )
            .ok()?;
        let has_expected_rows = rows.len() == expected_paths.len()
            && rows
                .iter()
                .map(|row| row.path.as_str())
                .eq(expected_paths.iter().map(String::as_str));
        if !has_expected_rows {
            rows = self.build_path_witness_projection_records(
                &repository.repository_id,
                snapshot_id,
                &repository.root,
                &manifest_entries,
            )?;
            storage
                .replace_path_witness_projections_for_repository_snapshot(
                    &repository.repository_id,
                    snapshot_id,
                    &rows,
                )
                .ok()?;
        }

        let projections = Arc::new(self.decode_path_witness_projection_records(&rows)?);
        self.hybrid_path_witness_projection_cache
            .write()
            .ok()?
            .insert(cache_key, Arc::clone(&projections));
        Some(projections)
    }

    fn build_path_witness_projection_records(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        root: &Path,
        manifest_entries: &[ManifestEntry],
    ) -> Option<Vec<PathWitnessProjectionRecord>> {
        let mut rows = manifest_entries
            .iter()
            .map(|entry| {
                let relative_path =
                    normalize_repository_relative_path(root, Path::new(&entry.path));
                build_path_witness_projection_record(repository_id, snapshot_id, &relative_path)
                    .ok()
            })
            .collect::<Option<Vec<_>>>()?;
        rows.sort_by(|left, right| left.path.cmp(&right.path));
        rows.dedup_by(|left, right| left.path == right.path);
        Some(rows)
    }

    fn decode_path_witness_projection_records(
        &self,
        rows: &[PathWitnessProjectionRecord],
    ) -> Option<Vec<StoredPathWitnessProjection>> {
        rows.iter()
            .map(|row| decode_path_witness_projection_record(row).ok())
            .collect()
    }
}

#[derive(Debug)]
struct PathWitnessCandidate {
    score: f32,
    repository_id: String,
    rel_path: String,
    path: PathBuf,
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
mod tests {
    use std::env;
    use std::fs;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, RwLock};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::domain::{FriggError, FriggResult, model::TextMatch};
    use crate::settings::{
        FriggConfig, SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider,
    };
    use crate::storage::{
        ManifestEntry, SemanticChunkEmbeddingRecord, Storage, ensure_provenance_db_parent_dir,
        resolve_provenance_db_path,
    };
    use regex::Regex;

    use crate::searcher::{
        HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridPathWitnessQueryContext,
        HybridRankingIntent, HybridSemanticStatus, HybridSourceClass, MAX_REGEX_ALTERNATIONS,
        MAX_REGEX_GROUPS, MAX_REGEX_PATTERN_BYTES, MAX_REGEX_QUANTIFIERS, RegexSearchError,
        SearchDiagnosticKind, SearchFilters, SearchHybridQuery, SearchTextQuery,
        SemanticRuntimeQueryEmbeddingExecutor, StoredPathWitnessProjection, TextSearcher,
        ValidatedManifestCandidateCache, build_hybrid_lexical_hits,
        build_hybrid_lexical_hits_for_query, build_hybrid_lexical_recall_regex,
        build_regex_prefilter_plan, compile_safe_regex, hybrid_lexical_recall_tokens,
        hybrid_path_witness_recall_score_for_projection, hybrid_source_class,
        normalize_search_filters, rank_hybrid_evidence, rank_hybrid_evidence_for_query,
    };

    #[test]
    fn literal_search_returns_sorted_deterministic_matches() -> FriggResult<()> {
        let root_a = temp_workspace_root("literal-search-sort-a");
        let root_b = temp_workspace_root("literal-search-sort-b");
        prepare_workspace(
            &root_a,
            &[
                ("zeta.txt", "needle zeta\n"),
                ("alpha.txt", "needle alpha\nnext needle\n"),
            ],
        )?;
        prepare_workspace(&root_b, &[("beta.txt", "beta needle\n")])?;

        let config = FriggConfig::from_workspace_roots(vec![root_b.clone(), root_a.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 100,
        };

        let first = searcher.search(query.clone())?;
        let second = searcher.search(query)?;
        assert_eq!(first, second);
        assert_eq!(
            first,
            vec![
                text_match("repo-001", "beta.txt", 1, 6, "beta needle"),
                text_match("repo-002", "alpha.txt", 1, 1, "needle alpha"),
                text_match("repo-002", "alpha.txt", 2, 6, "next needle"),
                text_match("repo-002", "zeta.txt", 1, 1, "needle zeta"),
            ]
        );

        cleanup_workspace(&root_a);
        cleanup_workspace(&root_b);
        Ok(())
    }

    #[test]
    fn literal_search_walk_fallback_respects_gitignored_contract_artifacts() -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-gitignored-contracts");
        prepare_workspace(&root, &[("contracts/errors.md", "invalid_params\n")])?;
        fs::write(root.join(".gitignore"), "contracts\n").map_err(FriggError::Io)?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let matches = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "invalid_params".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters::default(),
        )?;

        assert!(
            matches
                .iter()
                .all(|entry| entry.path != "contracts/errors.md"),
            "walk fallback should respect gitignored contract artifacts"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_scrubs_generic_markdown_leading_comment_metadata() -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-markdown-leading-comment");
        prepare_workspace(
            &root,
            &[(
                "docs/guide.md",
                "<!-- hidden metadata secret-token -->\n# Guide\npublic content\n",
            )],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let hidden = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "secret-token".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters::default(),
        )?;
        assert!(
            hidden.is_empty(),
            "leading markdown comment metadata should not pollute literal search: {:?}",
            hidden
        );

        let public = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "public".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters::default(),
        )?;
        assert_eq!(public.len(), 1);
        assert_eq!(public[0].path, "docs/guide.md");
        assert_eq!(public[0].line, 3);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_walk_fallback_excludes_target_artifacts_without_gitignore() -> FriggResult<()>
    {
        let root = temp_workspace_root("literal-search-target-exclusion");
        prepare_workspace(
            &root,
            &[
                ("src/main.rs", "needle\n"),
                ("target/debug/app", "needle\n"),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let matches = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters::default(),
        )?;

        assert!(
            matches
                .iter()
                .all(|entry| !entry.path.starts_with("target/")),
            "walk fallback must not search target artifacts: {matches:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_walk_fallback_respects_root_ignore_file_for_auxiliary_trees()
    -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-root-ignore");
        prepare_workspace(
            &root,
            &[
                ("src/main.rs", "needle main\n"),
                ("auxiliary/embedded-repo/src/lib.rs", "needle auxiliary\n"),
            ],
        )?;
        fs::write(root.join(".ignore"), "auxiliary/\n").map_err(FriggError::Io)?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let matches = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters::default(),
        )?;

        assert_eq!(
            matches,
            vec![text_match("repo-001", "src/main.rs", 1, 1, "needle main")]
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_applies_path_regex_filter() -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-path-filter");
        prepare_workspace(
            &root,
            &[
                ("src/lib.rs", "needle here\n"),
                ("README.md", "needle docs\n"),
            ],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*\.rs$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 100,
        };

        let matches = searcher.search(query)?;
        assert_eq!(
            matches,
            vec![text_match("repo-001", "src/lib.rs", 1, 1, "needle here")]
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_applies_repository_filter_and_limit_after_sorting() -> FriggResult<()> {
        let root_a = temp_workspace_root("literal-search-repo-filter-a");
        let root_b = temp_workspace_root("literal-search-repo-filter-b");
        prepare_workspace(&root_a, &[("a.txt", "needle a\nneedle aa\n")])?;
        prepare_workspace(&root_b, &[("b.txt", "needle b\nneedle bb\n")])?;

        let config = FriggConfig::from_workspace_roots(vec![root_a.clone(), root_b.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 1,
        };

        let matches = searcher.search_literal(query, Some("repo-002"))?;
        assert_eq!(
            matches,
            vec![text_match("repo-002", "b.txt", 1, 1, "needle b")]
        );

        cleanup_workspace(&root_a);
        cleanup_workspace(&root_b);
        Ok(())
    }

    #[test]
    fn literal_search_small_limit_matches_sorted_prefix_of_full_results() -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-small-limit-prefix");
        prepare_workspace(
            &root,
            &[
                ("z.txt", "needle zeta\n"),
                ("a.txt", "needle alpha\nneedle again\n"),
                ("nested/b.txt", "prefix needle\nneedle suffix\n"),
            ],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let full_query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 100,
        };
        let limited_query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 3,
        };

        let full = searcher.search_literal_with_filters(full_query, SearchFilters::default())?;
        let first_limited = searcher
            .search_literal_with_filters(limited_query.clone(), SearchFilters::default())?;
        let second_limited =
            searcher.search_literal_with_filters(limited_query, SearchFilters::default())?;

        assert_eq!(first_limited, second_limited);
        assert_eq!(
            first_limited,
            full.into_iter().take(3).collect::<Vec<_>>(),
            "limited search should match deterministic sorted prefix"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn diagnostics_literal_search_reports_read_failures_deterministically() -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-diagnostics-read-failure");
        fs::create_dir_all(root.join("src")).map_err(FriggError::Io)?;
        fs::write(
            root.join("src/good.rs"),
            "pub fn hotspot() { let _ = \"needle_hotspot\"; }\n",
        )
        .map_err(FriggError::Io)?;
        fs::write(root.join("src/bad.rs"), [0xff, b'\n']).map_err(FriggError::Io)?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: "needle_hotspot".to_owned(),
            path_regex: None,
            limit: 20,
        };

        let first = searcher
            .search_literal_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
        let second =
            searcher.search_literal_with_filters_diagnostics(query, SearchFilters::default())?;

        assert_eq!(first.matches, second.matches);
        assert_eq!(first.matches.len(), 1);
        assert_eq!(first.matches[0].repository_id, "repo-001");
        assert_eq!(first.matches[0].path, "src/good.rs");

        assert_eq!(first.diagnostics.entries, second.diagnostics.entries);
        assert_eq!(first.diagnostics.total_count(), 1);
        assert_eq!(
            first.diagnostics.count_by_kind(SearchDiagnosticKind::Read),
            1
        );
        assert_eq!(
            first.diagnostics.count_by_kind(SearchDiagnosticKind::Walk),
            0
        );
        assert_eq!(first.diagnostics.entries[0].repository_id, "repo-001");
        assert_eq!(
            first.diagnostics.entries[0].path.as_deref(),
            Some("src/bad.rs")
        );
        assert_eq!(
            first.diagnostics.entries[0].kind,
            SearchDiagnosticKind::Read
        );
        assert!(
            !first.diagnostics.entries[0].message.is_empty(),
            "diagnostic message should be populated for read failures"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_reuses_validated_manifest_candidates_across_repeated_queries()
    -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-manifest-cache-hit");
        prepare_workspace(
            &root,
            &[("src/lib.rs", "pub fn cached() { let _ = \"needle\"; }\n")],
        )?;
        seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/lib.rs"])?;

        let cache = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
        let searcher = TextSearcher::with_validated_manifest_candidate_cache(
            FriggConfig::from_workspace_roots(vec![root.clone()])?,
            Arc::clone(&cache),
        );
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 10,
        };

        let first = searcher
            .search_literal_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
        let second =
            searcher.search_literal_with_filters_diagnostics(query, SearchFilters::default())?;

        assert_eq!(first.matches, second.matches);
        assert_eq!(first.matches.len(), 1);
        let stats = cache
            .read()
            .expect("validated manifest candidate cache should not be poisoned")
            .stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 1);
        assert_eq!(stats.dirty_bypasses, 0);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_dirty_validated_manifest_cache_falls_back_to_walk_for_new_files()
    -> FriggResult<()> {
        let root = temp_workspace_root("literal-search-manifest-cache-dirty");
        prepare_workspace(&root, &[("src/lib.rs", "pub fn cached() {}\n")])?;
        seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/lib.rs"])?;

        let cache = Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()));
        let searcher = TextSearcher::with_validated_manifest_candidate_cache(
            FriggConfig::from_workspace_roots(vec![root.clone()])?,
            Arc::clone(&cache),
        );
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 10,
        };

        let first = searcher
            .search_literal_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
        assert_eq!(first.matches.len(), 0);

        prepare_workspace(
            &root,
            &[("src/new.rs", "pub fn fresh() { let _ = \"needle\"; }\n")],
        )?;
        cache
            .write()
            .expect("validated manifest candidate cache should not be poisoned")
            .mark_dirty_root(&root);

        let second =
            searcher.search_literal_with_filters_diagnostics(query, SearchFilters::default())?;

        assert_eq!(second.matches.len(), 1);
        assert_eq!(second.matches[0].path, "src/new.rs");
        let stats = cache
            .read()
            .expect("validated manifest candidate cache should not be poisoned")
            .stats();
        assert_eq!(stats.misses, 1);
        assert_eq!(stats.hits, 0);
        assert_eq!(stats.dirty_bypasses, 1);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn candidate_discovery_prefers_manifest_snapshot_across_search_modes() -> FriggResult<()> {
        let root = temp_workspace_root("candidate-discovery-prefers-manifest");
        prepare_workspace(
            &root,
            &[
                ("src/indexed.rs", "needle indexed\n"),
                ("src/live_only.rs", "needle live-only\n"),
            ],
        )?;
        seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);

        let literal = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "needle".to_owned(),
                path_regex: None,
                limit: 20,
            },
            SearchFilters::default(),
        )?;
        assert_eq!(
            literal,
            vec![text_match(
                "repo-001",
                "src/indexed.rs",
                1,
                1,
                "needle indexed"
            )]
        );

        let regex = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle\s+\w+".to_owned(),
                path_regex: None,
                limit: 20,
            },
            SearchFilters::default(),
        )?;
        assert_eq!(
            regex,
            vec![text_match(
                "repo-001",
                "src/indexed.rs",
                1,
                1,
                "needle indexed"
            )]
        );

        let hybrid = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 20,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;
        assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
        assert_eq!(hybrid.matches.len(), 1);
        assert_eq!(hybrid.matches[0].document.path, "src/indexed.rs");

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn candidate_discovery_manifest_snapshot_respects_root_ignore_file() -> FriggResult<()> {
        let root = temp_workspace_root("candidate-discovery-manifest-ignore");
        prepare_workspace(
            &root,
            &[
                ("src/indexed.rs", "needle indexed\n"),
                ("auxiliary/embedded-repo/src/lib.rs", "needle auxiliary\n"),
            ],
        )?;
        fs::write(root.join(".ignore"), "auxiliary/\n").map_err(FriggError::Io)?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-001",
            &["src/indexed.rs", "auxiliary/embedded-repo/src/lib.rs"],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);

        let literal = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "needle".to_owned(),
                path_regex: None,
                limit: 20,
            },
            SearchFilters::default(),
        )?;
        assert_eq!(
            literal,
            vec![text_match(
                "repo-001",
                "src/indexed.rs",
                1,
                1,
                "needle indexed"
            )]
        );

        let hybrid = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 20,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;
        assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
        assert_eq!(hybrid.matches.len(), 1);
        assert_eq!(hybrid.matches[0].document.path, "src/indexed.rs");

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_supplements_manifest_with_hidden_workflows() -> FriggResult<()> {
        let root = temp_workspace_root("candidate-discovery-hidden-workflow-supplement");
        prepare_workspace(
            &root,
            &[
                (
                    "src-tauri/src/main.rs",
                    "fn main() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
                ),
                (
                    "src-tauri/src/lib.rs",
                    "pub fn run() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
                ),
                (
                    "src-tauri/src/proxy/config.rs",
                    "pub struct ProxyConfig;\n\
                     impl ProxyConfig { pub fn load() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/modules/config.rs",
                    "pub struct ModuleConfig;\n\
                     impl ModuleConfig { pub fn load() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/models/config.rs",
                    "pub struct AppConfig;\n\
                     impl AppConfig { pub fn load() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/proxy/proxy_pool.rs",
                    "pub struct ProxyPool;\n\
                     impl ProxyPool { pub fn runner() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/commands/security.rs",
                    "pub fn security_command_runner() {}\n",
                ),
                ("src-tauri/build.rs", "fn main() { tauri_build::build() }\n"),
                (
                    ".github/workflows/deploy-pages.yml",
                    "name: Deploy static content to Pages\n\
                     jobs:\n\
                       deploy:\n\
                         steps:\n\
                           - name: Deploy to GitHub Pages\n",
                ),
                (
                    ".github/workflows/release.yml",
                    "name: Release\n\
                     jobs:\n\
                       build-tauri:\n\
                         steps:\n\
                           - name: Build the app\n",
                ),
            ],
        )?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                "src-tauri/src/main.rs",
                "src-tauri/src/lib.rs",
                "src-tauri/src/proxy/config.rs",
                "src-tauri/src/modules/config.rs",
                "src-tauri/src/models/config.rs",
                "src-tauri/src/proxy/proxy_pool.rs",
                "src-tauri/src/commands/security.rs",
                "src-tauri/build.rs",
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap build flow command runner main config".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().take(8).any(|path| {
                matches!(
                    *path,
                    ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
                )
            }),
            "manifest-backed path recall should still surface hidden GitHub workflow build configs in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_keeps_hidden_ci_workflows_for_entrypoint_build_config_queries()
    -> FriggResult<()> {
        let root = temp_workspace_root("candidate-discovery-hidden-ci-workflow-supplement");
        prepare_workspace(
            &root,
            &[
                (
                    "src/bin/tool/main.rs",
                    "mod app;\nfn main() { app::run(); }\n",
                ),
                ("src/bin/tool/app.rs", "pub fn run() {}\n"),
                (
                    ".github/workflows/CICD.yml",
                    "name: CI\njobs:\n  test:\n    steps:\n      - run: cargo test\n",
                ),
                (
                    ".github/workflows/require-changelog-for-PRs.yml",
                    "name: Require changelog\njobs:\n  changelog:\n    steps:\n      - run: ./scripts/check-changelog.sh\n",
                ),
            ],
        )?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-001",
            &["src/bin/tool/main.rs", "src/bin/tool/app.rs"],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "entry point bootstrap build flow command runner main config cargo github workflow cicd require changelog"
                        .to_owned(),
                limit: 11,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().take(11).any(|path| {
                matches!(
                    *path,
                    ".github/workflows/CICD.yml"
                        | ".github/workflows/require-changelog-for-PRs.yml"
                )
            }),
            "entrypoint build-config queries should retain generic hidden CI workflows in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn candidate_discovery_rebuilds_after_stale_manifest_snapshot() -> FriggResult<()> {
        let root = temp_workspace_root("candidate-discovery-stale-manifest");
        prepare_workspace(
            &root,
            &[
                ("src/indexed.rs", "needle indexed\n"),
                ("src/live_only.rs", "needle live-only\n"),
            ],
        )?;
        seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/indexed.rs"])?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);

        let first = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "needle".to_owned(),
                path_regex: None,
                limit: 20,
            },
            SearchFilters::default(),
        )?;
        assert_eq!(
            first,
            vec![text_match(
                "repo-001",
                "src/indexed.rs",
                1,
                1,
                "needle indexed"
            )]
        );

        rewrite_file_with_new_mtime(&root.join("src/indexed.rs"), "changed\n")?;

        let literal = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "needle".to_owned(),
                path_regex: None,
                limit: 20,
            },
            SearchFilters::default(),
        )?;
        assert_eq!(
            literal,
            vec![text_match(
                "repo-001",
                "src/live_only.rs",
                1,
                1,
                "needle live-only"
            )]
        );

        let hybrid = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 20,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;
        assert_eq!(hybrid.note.semantic_status, HybridSemanticStatus::Disabled);
        assert_eq!(hybrid.matches.len(), 1);
        assert_eq!(hybrid.matches[0].document.path, "src/live_only.rs");

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn candidate_discovery_falls_back_to_repository_walk_without_manifest() -> FriggResult<()> {
        let root = temp_workspace_root("candidate-discovery-fallback-walk");
        prepare_workspace(
            &root,
            &[
                ("src/indexed.rs", "needle indexed\n"),
                ("src/live_only.rs", "needle live-only\n"),
            ],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let matches = searcher.search_literal_with_filters(
            SearchTextQuery {
                query: "needle".to_owned(),
                path_regex: None,
                limit: 20,
            },
            SearchFilters::default(),
        )?;

        assert_eq!(
            matches,
            vec![
                text_match("repo-001", "src/indexed.rs", 1, 1, "needle indexed"),
                text_match("repo-001", "src/live_only.rs", 1, 1, "needle live-only"),
            ]
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn literal_search_low_limit_large_corpus_matches_sorted_prefix_deterministically()
    -> FriggResult<()> {
        const FILE_COUNT: usize = 96;
        const LIMIT: usize = 5;

        let root = temp_workspace_root("literal-search-large-corpus-low-limit");
        fs::create_dir_all(root.join("src/nested")).map_err(FriggError::Io)?;
        for file_idx in 0..FILE_COUNT {
            let relative = if file_idx % 2 == 0 {
                format!("src/file_{file_idx:03}.rs")
            } else {
                format!("src/nested/file_{file_idx:03}.rs")
            };
            let mut lines = Vec::with_capacity(40);
            lines.push(format!(
                "// deterministic large-corpus fixture file={file_idx:03}"
            ));
            for line_idx in 0..36 {
                if line_idx % 4 == 0 {
                    lines.push(format!(
                        "let hotspot_{line_idx:03} = \"needle_hotspot {file_idx} {line_idx}\";"
                    ));
                } else {
                    lines.push(format!(
                        "let filler_{line_idx:03} = {};",
                        file_idx + line_idx
                    ));
                }
            }
            fs::write(root.join(relative), lines.join("\n")).map_err(FriggError::Io)?;
        }

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let full_query = SearchTextQuery {
            query: "needle_hotspot".to_owned(),
            path_regex: None,
            limit: 10_000,
        };
        let limited_query = SearchTextQuery {
            query: "needle_hotspot".to_owned(),
            path_regex: None,
            limit: LIMIT,
        };

        let full = searcher.search_literal_with_filters(full_query, SearchFilters::default())?;
        let first_limited = searcher
            .search_literal_with_filters(limited_query.clone(), SearchFilters::default())?;
        let second_limited =
            searcher.search_literal_with_filters(limited_query, SearchFilters::default())?;

        assert_eq!(first_limited.len(), LIMIT);
        assert_eq!(first_limited, second_limited);
        assert_eq!(
            first_limited,
            full.into_iter().take(LIMIT).collect::<Vec<_>>(),
            "low-limit search should stay equal to deterministic sorted prefix on large corpus"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn regex_search_returns_sorted_deterministic_matches() -> FriggResult<()> {
        let root_a = temp_workspace_root("regex-search-sort-a");
        let root_b = temp_workspace_root("regex-search-sort-b");
        prepare_workspace(
            &root_a,
            &[
                ("zeta.txt", "needle zeta\n"),
                ("alpha.txt", "needle alpha\nnext needle\n"),
            ],
        )?;
        prepare_workspace(&root_b, &[("beta.txt", "beta needle\n")])?;

        let config = FriggConfig::from_workspace_roots(vec![root_b.clone(), root_a.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: r"needle\s+\w+".to_owned(),
            path_regex: None,
            limit: 100,
        };

        let first = searcher.search_regex(query.clone(), None)?;
        let second = searcher.search_regex(query, None)?;
        assert_eq!(first, second);
        assert_eq!(
            first,
            vec![
                text_match("repo-002", "alpha.txt", 1, 1, "needle alpha"),
                text_match("repo-002", "zeta.txt", 1, 1, "needle zeta"),
            ]
        );

        cleanup_workspace(&root_a);
        cleanup_workspace(&root_b);
        Ok(())
    }

    #[test]
    fn regex_search_applies_repository_and_path_filters() -> FriggResult<()> {
        let root_a = temp_workspace_root("regex-search-filter-a");
        let root_b = temp_workspace_root("regex-search-filter-b");
        prepare_workspace(
            &root_a,
            &[
                ("src/lib.rs", "needle 123\n"),
                ("README.md", "needle docs\n"),
            ],
        )?;
        prepare_workspace(
            &root_b,
            &[
                ("src/main.rs", "needle 999\n"),
                ("README.md", "needle docs\n"),
            ],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root_a.clone(), root_b.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*\.rs$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 10,
        };

        let matches = searcher.search_regex(query, Some("repo-002"))?;
        assert_eq!(
            matches,
            vec![text_match("repo-002", "src/main.rs", 1, 1, "needle 999")]
        );

        cleanup_workspace(&root_a);
        cleanup_workspace(&root_b);
        Ok(())
    }

    #[test]
    fn regex_prefilter_plan_extracts_required_literals_for_safe_patterns() {
        let plan = build_regex_prefilter_plan(r"needle\s+\d+")
            .expect("safe regex pattern should produce a deterministic prefilter plan");
        assert_eq!(plan.required_literals(), vec!["needle"]);
        assert!(plan.file_may_match("prefix needle 42 suffix"));
        assert!(!plan.file_may_match("prefix token 42 suffix"));
    }

    #[test]
    fn regex_prefilter_plan_falls_back_for_unsupported_constructs() {
        assert!(build_regex_prefilter_plan(r"(needle|token)\s+\d+").is_none());
    }

    #[test]
    fn regex_prefilter_matches_unfiltered_baseline_without_false_negatives() -> FriggResult<()> {
        let root = temp_workspace_root("regex-prefilter-baseline-equivalence");
        prepare_workspace(
            &root,
            &[
                ("src/a.rs", "needle 100\nneedle words\n"),
                ("src/b.rs", "prefix needle 300 suffix\n"),
                ("src/c.rs", "completely unrelated\n"),
                ("src/nested/d.rs", "needle 101\nneedle 102\n"),
                ("README.md", "needle 999\n"),
            ],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*\.rs$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 3,
        };

        assert!(
            build_regex_prefilter_plan(&query.query).is_some(),
            "expected prefilter plan for deterministic baseline comparison"
        );

        let accelerated = searcher
            .search_regex_with_filters_diagnostics(query.clone(), SearchFilters::default())?;
        let accelerated_again = searcher
            .search_regex_with_filters_diagnostics(query.clone(), SearchFilters::default())?;

        let matcher = compile_safe_regex(&query.query)
            .map_err(|err| FriggError::InvalidInput(format!("test regex compile failed: {err}")))?;
        let normalized_filters = normalize_search_filters(SearchFilters::default())?;
        let baseline = searcher.search_with_matcher(
            &query,
            &normalized_filters,
            |_| true,
            |line, columns| {
                columns.clear();
                columns.extend(matcher.find_iter(line).map(|mat| mat.start() + 1));
            },
        )?;

        assert_eq!(accelerated.matches, accelerated_again.matches);
        assert_eq!(accelerated.matches, baseline.matches);
        assert_eq!(
            accelerated.diagnostics.entries,
            baseline.diagnostics.entries
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn regex_prefilter_keeps_repeated_literal_quantifier_matches() -> FriggResult<()> {
        let root = temp_workspace_root("regex-prefilter-repeated-quantifier");
        prepare_workspace(
            &root,
            &[("src/lib.rs", "abbc\nabc\n"), ("src/other.rs", "noise\n")],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: r"ab{2}c".to_owned(),
            path_regex: None,
            limit: 20,
        };

        let matches = searcher.search_regex_with_filters(query, SearchFilters::default())?;
        assert_eq!(
            matches,
            vec![text_match("repo-001", "src/lib.rs", 1, 1, "abbc")]
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn regex_search_rejects_invalid_pattern_with_typed_error() -> FriggResult<()> {
        let compile_error = compile_safe_regex("(unterminated");
        assert!(matches!(
            compile_error,
            Err(RegexSearchError::InvalidRegex(_))
        ));

        let root = temp_workspace_root("regex-search-invalid");
        prepare_workspace(&root, &[("a.txt", "text\n")])?;
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: "(unterminated".to_owned(),
            path_regex: None,
            limit: 10,
        };

        let search_error = searcher
            .search_regex(query, None)
            .expect_err("invalid regex pattern should fail");
        let error_message = search_error.to_string();
        assert!(
            error_message.contains("regex_invalid_pattern"),
            "unexpected regex invalid error: {error_message}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn regex_search_rejects_abusive_pattern_length_with_typed_error() {
        let abusive = "a".repeat(MAX_REGEX_PATTERN_BYTES + 1);
        let result = compile_safe_regex(&abusive);
        assert!(matches!(
            result,
            Err(RegexSearchError::PatternTooLong { .. })
        ));
    }

    #[test]
    fn security_regex_search_rejects_abusive_alternations_with_typed_error() {
        let terms = (0..(MAX_REGEX_ALTERNATIONS + 2))
            .map(|index| format!("term{index}"))
            .collect::<Vec<_>>();
        let abusive = terms.join("|");
        let result = compile_safe_regex(&abusive);
        assert!(matches!(
            result,
            Err(RegexSearchError::TooManyAlternations { .. })
        ));
    }

    #[test]
    fn security_regex_search_rejects_abusive_groups_with_typed_error() {
        let abusive = "(needle)".repeat(MAX_REGEX_GROUPS + 1);
        let result = compile_safe_regex(&abusive);
        assert!(matches!(
            result,
            Err(RegexSearchError::TooManyGroups { .. })
        ));
    }

    #[test]
    fn security_regex_search_rejects_abusive_quantifiers_with_typed_error() {
        let abusive = "needle+".repeat(MAX_REGEX_QUANTIFIERS + 1);
        let result = compile_safe_regex(&abusive);
        assert!(matches!(
            result,
            Err(RegexSearchError::TooManyQuantifiers { .. })
        ));
    }

    #[test]
    fn security_regex_search_maps_abuse_to_typed_invalid_input_error() -> FriggResult<()> {
        let root = temp_workspace_root("security-regex-abuse");
        prepare_workspace(&root, &[("src/lib.rs", "needle 1\n")])?;
        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let abusive = "needle+".repeat(MAX_REGEX_QUANTIFIERS + 1);
        let query = SearchTextQuery {
            query: abusive,
            path_regex: None,
            limit: 5,
        };

        let error = searcher
            .search_regex(query, None)
            .expect_err("abusive regex should fail with typed invalid-input error");
        assert!(
            error.to_string().contains("regex_too_many_quantifiers"),
            "unexpected abuse regex error: {error}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn ordering_literal_search_repeated_runs_are_identical() -> FriggResult<()> {
        let root_a = temp_workspace_root("ordering-literal-a");
        let root_b = temp_workspace_root("ordering-literal-b");
        prepare_workspace(
            &root_a,
            &[("z.txt", "needle z\n"), ("a.txt", "x needle\ny needle\n")],
        )?;
        prepare_workspace(&root_b, &[("b.txt", "needle b\n")])?;

        let config = FriggConfig::from_workspace_roots(vec![root_b.clone(), root_a.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: "needle".to_owned(),
            path_regex: None,
            limit: 100,
        };

        let first =
            searcher.search_literal_with_filters(query.clone(), SearchFilters::default())?;
        let second = searcher.search_literal_with_filters(query, SearchFilters::default())?;
        assert_eq!(first, second);

        cleanup_workspace(&root_a);
        cleanup_workspace(&root_b);
        Ok(())
    }

    #[test]
    fn ordering_regex_search_repeated_runs_are_identical() -> FriggResult<()> {
        let root = temp_workspace_root("ordering-regex");
        prepare_workspace(
            &root,
            &[
                ("src/lib.rs", "needle 1\nneedle 2\n"),
                ("README.md", "needle docs\n"),
            ],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: None,
            limit: 100,
        };

        let first = searcher.search_regex_with_filters(query.clone(), SearchFilters::default())?;
        let second = searcher.search_regex_with_filters(query, SearchFilters::default())?;
        assert_eq!(first, second);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn ordering_filter_normalization_applies_repo_path_and_language() -> FriggResult<()> {
        let root_a = temp_workspace_root("ordering-filters-a");
        let root_b = temp_workspace_root("ordering-filters-b");
        prepare_workspace(
            &root_a,
            &[
                ("src/lib.rs", "needle 1\n"),
                ("src/lib.php", "needle 2\n"),
                ("src/lib.tsx", "needle 3\n"),
                ("src/lib.py", "needle 4\n"),
                ("src/lib.go", "needle 5\n"),
                ("src/lib.kts", "needle 6\n"),
                ("src/lib.lua", "needle 7\n"),
                ("src/lib.roc", "needle 8\n"),
                ("src/lib.nims", "needle 14\n"),
            ],
        )?;
        prepare_workspace(
            &root_b,
            &[
                ("src/main.rs", "needle 9\n"),
                ("src/main.php", "needle 10\n"),
                ("src/main.ts", "needle 11\n"),
                ("src/main.py", "needle 12\n"),
                ("src/main.go", "needle 13\n"),
                ("src/main.kt", "needle 15\n"),
                ("src/main.lua", "needle 16\n"),
                ("src/main.roc", "needle 17\n"),
                ("src/main.nim", "needle 18\n"),
            ],
        )?;

        let config = FriggConfig::from_workspace_roots(vec![root_a.clone(), root_b.clone()])?;
        let searcher = TextSearcher::new(config);
        let query = SearchTextQuery {
            query: r"needle\s+\d+".to_owned(),
            path_regex: Some(Regex::new(r"^src/.*$").map_err(|err| {
                FriggError::InvalidInput(format!("invalid test path regex: {err}"))
            })?),
            limit: 100,
        };

        let matches = searcher.search_regex_with_filters(
            query,
            SearchFilters {
                repository_id: Some("  repo-002  ".to_owned()),
                language: Some("  RS ".to_owned()),
            },
        )?;
        assert_eq!(
            matches,
            vec![text_match("repo-002", "src/main.rs", 1, 1, "needle 9")]
        );

        let typescript_matches = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("tsx".to_owned()),
            },
        )?;
        assert_eq!(
            typescript_matches,
            vec![
                text_match("repo-001", "src/lib.tsx", 1, 1, "needle 3"),
                text_match("repo-002", "src/main.ts", 1, 1, "needle 11"),
            ]
        );

        let python_matches = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("py".to_owned()),
            },
        )?;
        assert_eq!(
            python_matches,
            vec![
                text_match("repo-001", "src/lib.py", 1, 1, "needle 4"),
                text_match("repo-002", "src/main.py", 1, 1, "needle 12"),
            ]
        );

        let go_matches = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("golang".to_owned()),
            },
        )?;
        assert_eq!(
            go_matches,
            vec![
                text_match("repo-001", "src/lib.go", 1, 1, "needle 5"),
                text_match("repo-002", "src/main.go", 1, 1, "needle 13"),
            ]
        );

        let kotlin_matches = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("kt".to_owned()),
            },
        )?;
        assert_eq!(
            kotlin_matches,
            vec![
                text_match("repo-001", "src/lib.kts", 1, 1, "needle 6"),
                text_match("repo-002", "src/main.kt", 1, 1, "needle 15"),
            ]
        );

        let lua_matches = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("lua".to_owned()),
            },
        )?;
        assert_eq!(
            lua_matches,
            vec![
                text_match("repo-001", "src/lib.lua", 1, 1, "needle 7"),
                text_match("repo-002", "src/main.lua", 1, 1, "needle 16"),
            ]
        );

        let roc_matches = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("roc".to_owned()),
            },
        )?;
        assert_eq!(
            roc_matches,
            vec![
                text_match("repo-001", "src/lib.roc", 1, 1, "needle 8"),
                text_match("repo-002", "src/main.roc", 1, 1, "needle 17"),
            ]
        );

        let nim_matches = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("nim".to_owned()),
            },
        )?;
        assert_eq!(
            nim_matches,
            vec![
                text_match("repo-001", "src/lib.nims", 1, 1, "needle 14"),
                text_match("repo-002", "src/main.nim", 1, 1, "needle 18"),
            ]
        );

        let unsupported_language = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("java".to_owned()),
            },
        );
        let err = unsupported_language.expect_err("unsupported language filter should fail");
        assert!(
            err.to_string().contains("unsupported language filter"),
            "unexpected unsupported-language error: {err}"
        );

        cleanup_workspace(&root_a);
        cleanup_workspace(&root_b);
        Ok(())
    }

    #[test]
    fn hybrid_lexical_recall_regex_is_deterministic_for_multi_term_queries() {
        let pattern =
            build_hybrid_lexical_recall_regex("semantic runtime strict failure note metadata")
                .expect("multi-token query should emit lexical recall regex");
        assert_eq!(
            pattern,
            r"(?i)\b(?:semantic|runtime|strict|failure|note|metadata)\b"
        );

        assert!(
            build_hybrid_lexical_recall_regex("abc xyz").is_none(),
            "short tokens should not enable lexical recall expansion"
        );
    }

    #[test]
    fn hybrid_lexical_recall_tokens_support_snake_case_terms() {
        assert_eq!(
            hybrid_lexical_recall_tokens("strict semantic failure unavailable semantic_status"),
            vec![
                "strict".to_owned(),
                "semantic".to_owned(),
                "failure".to_owned(),
                "unavailable".to_owned(),
                "semantic_status".to_owned(),
            ]
        );
    }

    #[test]
    fn hybrid_ranking_lexical_hits_prefer_source_paths_over_playbooks() -> FriggResult<()> {
        let lexical = build_hybrid_lexical_hits(&[
            text_match(
                "repo-001",
                "playbooks/hybrid.md",
                1,
                1,
                "semantic runtime metadata",
            ),
            text_match("repo-001", "src/lib.rs", 1, 1, "semantic runtime metadata"),
        ]);
        let ranked = rank_hybrid_evidence(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            10,
        )?;

        assert_eq!(ranked.len(), 2);
        assert_eq!(ranked[0].document.path, "src/lib.rs");
        assert_eq!(ranked[1].document.path, "playbooks/hybrid.md");
        Ok(())
    }

    #[test]
    fn hybrid_ranking_query_aware_lexical_hits_keep_public_docs_visible_with_runtime_and_tests()
    -> FriggResult<()> {
        let query = "trace invalid_params typed error from public docs to runtime helper and tests";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "contracts/errors.md",
                    1,
                    1,
                    "invalid_params maps to -32602",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/mcp/server.rs",
                    1,
                    1,
                    "fn invalid_params_error() -> JsonRpcError",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/tests/tool_handlers.rs",
                    1,
                    1,
                    "invalid_params typed failure coverage",
                ),
            ],
            query,
        );
        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            3,
            query,
        )?;

        assert_eq!(ranked.len(), 3);
        assert!(
            ranked
                .iter()
                .any(|entry| entry.document.path == "contracts/errors.md"),
            "public docs witness should remain in the ranked set"
        );
        assert!(
            ranked
                .iter()
                .any(|entry| entry.document.path == "crates/cli/src/mcp/server.rs"),
            "runtime witness should remain in the ranked set"
        );
        assert!(
            ranked
                .iter()
                .any(|entry| entry.document.path == "crates/cli/tests/tool_handlers.rs"),
            "test witness should remain in the ranked set"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_http_auth_queries_demote_repo_metadata_noise() -> FriggResult<()> {
        let query = "where is the optional HTTP MCP auth token declared enforced and documented";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "Cargo.lock",
                    1,
                    1,
                    "source = \"registry+https://github.com/rust-lang/crates.io-index\"",
                ),
                text_match(
                    "repo-001",
                    "README.md",
                    1,
                    1,
                    "POST /mcp --mcp-http-auth-token FRIGG_MCP_HTTP_AUTH_TOKEN",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/main.rs",
                    1,
                    1,
                    "mcp_http_auth_token bearer_auth_middleware serve_http",
                ),
            ],
            query,
        );
        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            3,
            query,
        )?;

        assert_eq!(ranked[0].document.path, "crates/cli/src/main.rs");
        assert_eq!(ranked[1].document.path, "README.md");
        assert_eq!(ranked[2].document.path, "Cargo.lock");
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_auth_queries_keep_runtime_and_readme_witnesses() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-auth-runtime-readme");
        prepare_workspace(
            &root,
            &[
                (
                    "README.md",
                    "POST /mcp --mcp-http-auth-token FRIGG_MCP_HTTP_AUTH_TOKEN\n\
                     keep --mcp-http-auth-token set or use the FRIGG_MCP_HTTP_AUTH_TOKEN env var\n",
                ),
                (
                    "crates/cli/src/main.rs",
                    "mcp_http_auth_token: Option<String>\n\
                     env = \"FRIGG_MCP_HTTP_AUTH_TOKEN\"\n\
                     bearer_auth_middleware\n\
                     serve_http\n",
                ),
                (
                    "contracts/errors.md",
                    "## MCP payload guidance\n\
                     invalid_params payload guidance\n",
                ),
                (
                    "benchmarks/mcp-tools.md",
                    "# MCP Tool Benchmark Methodology\n\
                     benchmark notes for MCP tools\n",
                ),
                (
                    "crates/cli/tests/security.rs",
                    "fn auth_token_marker() { let marker = \"auth token\"; }\n",
                ),
                ("crates/cli/src/lib.rs", "pub mod domain;\n"),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "crates/cli/src/main.rs",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record("repo-001", "snapshot-001", "README.md", 0, vec![0.82, 0.0]),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "contracts/errors.md",
                    0,
                    vec![0.76, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "benchmarks/mcp-tools.md",
                    0,
                    vec![0.71, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "crates/cli/tests/security.rs",
                    0,
                    vec![0.36, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "crates/cli/src/lib.rs",
                    0,
                    vec![0.62, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "where is the optional HTTP MCP auth token declared enforced and documented"
                    .to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
        assert!(output.note.semantic_enabled);

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.contains(&"crates/cli/src/main.rs"),
            "runtime auth witness should remain visible under semantic-ok ranking: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.contains(&"README.md"),
            "README auth witness should remain visible when the query explicitly asks where behavior is documented: {ranked_paths:?}"
        );
        let readme_position = output
            .matches
            .iter()
            .position(|entry| entry.document.path == "README.md")
            .expect("README witness position should be present");
        let benchmark_position = output
            .matches
            .iter()
            .position(|entry| entry.document.path == "benchmarks/mcp-tools.md");
        assert!(
            benchmark_position.is_none() || Some(readme_position) < benchmark_position,
            "README auth docs should outrank benchmark docs for auth-entrypoint queries: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_entrypoint_queries_choose_build_anchor_excerpt() -> FriggResult<()> {
        let query = "where the app starts and builds the pipeline runner";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "src/main.rs",
                    1081,
                    26,
                    "let mut runner = build_pipeline_runner(&self.config);",
                ),
                text_match(
                    "repo-001",
                    "src/main.rs",
                    1453,
                    5,
                    "runner: &PipelineRunner,",
                ),
                text_match(
                    "repo-001",
                    "src/runner.rs",
                    1216,
                    12,
                    "struct FakeInProcessExecutor {",
                ),
            ],
            query,
        );
        let main_hit = lexical
            .iter()
            .find(|hit| hit.document.path == "src/main.rs")
            .expect("main.rs lexical hit should exist");

        assert!(
            main_hit.excerpt.contains("build_pipeline_runner"),
            "entrypoint/build-flow queries should keep the strongest build anchor excerpt for main.rs, got {:?}",
            main_hit.excerpt
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_entrypoint_queries_promote_main_over_runner_helpers() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-entrypoint-build-flow");
        prepare_workspace(
            &root,
            &[
                (
                    "src/main.rs",
                    "fn main() {\n\
                     let config = AppConfig::load();\n\
                     let mut runner = build_pipeline_runner(&config);\n\
                     run_pipeline(&mut runner);\n\
                     }\n\
                     fn build_pipeline_runner(config: &AppConfig) -> PipelineRunner {\n\
                     PipelineRunner::new(config.clone())\n\
                     }\n",
                ),
                (
                    "src/runner.rs",
                    "pub struct PipelineRunner;\n\
                     struct FakeInProcessExecutor;\n\
                     impl PipelineRunner {\n\
                     pub fn new(_config: AppConfig) -> Self { Self }\n\
                     }\n",
                ),
                (
                    "tests/pipeline_runner_contract.rs",
                    "#[test]\n\
                     fn contract() { let runner = PipelineRunner::default(); }\n",
                ),
                (
                    "specs/01-pipeline-runner/design.md",
                    "# Design\n\
                     The pipeline runner boots from the app startup flow.\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/main.rs",
                    0,
                    vec![0.92, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/runner.rs",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "tests/pipeline_runner_contract.rs",
                    0,
                    vec![0.72, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "specs/01-pipeline-runner/design.md",
                    0,
                    vec![0.95, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "where the app starts and builds the pipeline runner".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
        assert_eq!(output.matches[0].document.path, "src/main.rs");
        assert!(
            output.matches[0].excerpt.contains("build_pipeline_runner"),
            "top entrypoint/build-flow witness should surface the build anchor excerpt, got {:?}",
            output.matches[0].excerpt
        );
        assert!(
            output
                .matches
                .iter()
                .any(|entry| entry.document.path == "src/runner.rs"),
            "runner helper should remain available as a secondary witness"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_symbol_plus_entrypoint_queries_keep_runner_family_above_semantic_tail()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-entrypoint-symbol-tail");
        prepare_workspace(
            &root,
            &[
                (
                    "src/main.rs",
                    "fn main() {\n\
                     let config = load_config();\n\
                     let mut runner = build_pipeline_runner(&config);\n\
                     run_pipeline(&mut runner);\n\
                     }\n",
                ),
                (
                    "src/runner.rs",
                    "pub struct PipelineRunner;\n\
                     impl PipelineRunner {\n\
                     pub fn new() -> Self { Self }\n\
                     }\n",
                ),
                ("src/replay.rs", "pub fn bootstrap_replay() {}\n"),
                (
                    "src/stt_google_tool.rs",
                    "pub fn bootstrap_google_tool() {}\n",
                ),
                ("src/config.rs", "pub fn bootstrap_config() {}\n"),
                ("src/lib.rs", "pub fn bootstrap_runtime() {}\n"),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record("repo-001", "snapshot-001", "src/main.rs", 0, vec![1.0, 0.0]),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/runner.rs",
                    0,
                    vec![0.82, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/replay.rs",
                    0,
                    vec![0.96, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/stt_google_tool.rs",
                    0,
                    vec![0.95, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/config.rs",
                    0,
                    vec![0.94, 0.0],
                ),
                semantic_record("repo-001", "snapshot-001", "src/lib.rs", 0, vec![0.93, 0.0]),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "build_pipeline_runner entry point bootstrap".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        let runner_position = output
            .matches
            .iter()
            .position(|entry| entry.document.path == "src/runner.rs")
            .expect("runner witness should remain in the ranked set");
        let replay_position = output
            .matches
            .iter()
            .position(|entry| entry.document.path == "src/replay.rs");
        let stt_position = output
            .matches
            .iter()
            .position(|entry| entry.document.path == "src/stt_google_tool.rs");

        assert_eq!(output.matches[0].document.path, "src/main.rs");
        assert!(
            replay_position.is_none() || runner_position < replay_position.unwrap(),
            "runner witness should outrank replay semantic tail for mixed symbol-plus-entrypoint queries: {:?}",
            output.matches
        );
        assert!(
            stt_position.is_none() || runner_position < stt_position.unwrap(),
            "runner witness should outrank unrelated semantic tail for mixed symbol-plus-entrypoint queries: {:?}",
            output.matches
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_python_entrypoint_queries_keep_python_witnesses_above_frontend_noise()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-python-entrypoints-vs-frontend-noise");
        prepare_workspace(
            &root,
            &[
                (
                    "classic/original_autogpt/autogpt/app/main.py",
                    "from autogpt.app.cli import run_cli\n\
                     run_cli()\n",
                ),
                (
                    "autogpt_platform/backend/backend/app.py",
                    "from fastapi import FastAPI\n\
                     application = FastAPI()\n",
                ),
                (
                    "autogpt_platform/backend/backend/copilot/executor/__main__.py",
                    "from backend.copilot.executor.processor import Processor\n\
                     Processor().run()\n",
                ),
                (
                    "autogpt_platform/backend/pyproject.toml",
                    "[project]\n\
                     name = \"autogpt-backend\"\n\
                     [project.scripts]\n\
                     backend = \"backend.app:app\"\n",
                ),
                (
                    "classic/benchmark/tests/test_benchmark_workflow.py",
                    "def verify_graph_shape() -> None:\n\
                     assert True\n",
                ),
                (
                    "autogpt_platform/frontend/src/components/renderers/InputRenderer/docs/HEIRARCHY.md",
                    "# Hierarchy\n\
                     app startup cli main renderer bootstrap guide\n",
                ),
                (
                    "autogpt_platform/frontend/CONTRIBUTING.md",
                    "# Frontend contributing\n\
                     app startup cli main contributor notes\n",
                ),
                (
                    "docs/platform/advanced_setup.md",
                    "# Advanced setup\n\
                     bootstrap app startup cli main platform setup\n",
                ),
                (
                    "classic/benchmark/frontend/package.json",
                    "{\n\
                     \"name\": \"frontend-benchmark\",\n\
                     \"main\": \"index.js\"\n\
                     }\n",
                ),
                (
                    "autogpt_platform/frontend/src/app/api/openapi.json",
                    "{\n\
                     \"openapi\": \"3.1.0\",\n\
                     \"info\": {\"title\": \"frontend app main api\"}\n\
                     }\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/frontend/src/components/renderers/InputRenderer/docs/HEIRARCHY.md",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "docs/platform/advanced_setup.md",
                    0,
                    vec![0.99, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/frontend/src/app/api/openapi.json",
                    0,
                    vec![0.97, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/frontend/CONTRIBUTING.md",
                    0,
                    vec![0.95, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "classic/benchmark/frontend/package.json",
                    0,
                    vec![0.93, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "classic/original_autogpt/autogpt/app/main.py",
                    0,
                    vec![0.78, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/backend/backend/app.py",
                    0,
                    vec![0.76, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/backend/backend/copilot/executor/__main__.py",
                    0,
                    vec![0.74, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/backend/pyproject.toml",
                    0,
                    vec![0.70, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "classic/benchmark/tests/test_benchmark_workflow.py",
                    0,
                    vec![0.68, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap app startup cli main".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths
                .iter()
                .take(5)
                .any(|path| *path == "classic/original_autogpt/autogpt/app/main.py"),
            "python main entrypoint should appear in the top witness set: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(5)
                .any(|path| *path == "autogpt_platform/backend/backend/app.py"),
            "python app runtime witness should appear in the top witness set: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(5)
                .any(|path| *path == "autogpt_platform/backend/pyproject.toml"),
            "python runtime config should appear in the top witness set: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(5)
                .any(|path| *path == "classic/benchmark/tests/test_benchmark_workflow.py"),
            "python tests should remain visible in the top witness set: {ranked_paths:?}"
        );

        let main_position = ranked_paths
            .iter()
            .position(|path| *path == "classic/original_autogpt/autogpt/app/main.py")
            .expect("python main entrypoint should be ranked");
        let app_position = ranked_paths
            .iter()
            .position(|path| *path == "autogpt_platform/backend/backend/app.py")
            .expect("python app witness should be ranked");
        if let Some(openapi_position) = ranked_paths
            .iter()
            .position(|path| *path == "autogpt_platform/frontend/src/app/api/openapi.json")
        {
            assert!(
                main_position < openapi_position,
                "python main entrypoint should outrank frontend openapi noise: {ranked_paths:?}"
            );
        }
        if let Some(frontend_doc_position) = ranked_paths.iter().position(|path| {
            *path == "autogpt_platform/frontend/src/components/renderers/InputRenderer/docs/HEIRARCHY.md"
        }) {
            assert!(
                app_position < frontend_doc_position,
                "python app witness should outrank frontend hierarchy docs: {ranked_paths:?}"
            );
        }

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_python_entrypoint_queries_prefer_canonical_entrypoints_over_backend_modules()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-python-entrypoints-vs-backend-modules");
        let mut files = vec![
            (
                "classic/original_autogpt/autogpt/app/main.py".to_owned(),
                "from autogpt.app.cli import run_cli\nrun_cli()\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/app.py".to_owned(),
                "from fastapi import FastAPI\napplication = FastAPI()\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/copilot/executor/__main__.py".to_owned(),
                "from backend.copilot.executor.processor import Processor\nProcessor().run()\n"
                    .to_owned(),
            ),
            (
                "autogpt_platform/backend/pyproject.toml".to_owned(),
                "[project]\nname = \"autogpt-backend\"\n[project.scripts]\nbackend = \"backend.app:app\"\n"
                    .to_owned(),
            ),
            (
                "autogpt_platform/autogpt_libs/pyproject.toml".to_owned(),
                "[project]\nname = \"autogpt-libs\"\n".to_owned(),
            ),
            (
                "classic/original_autogpt/pyproject.toml".to_owned(),
                "[project]\nname = \"classic-autogpt\"\n".to_owned(),
            ),
            (
                "classic/benchmark/tests/test_benchmark_workflow.py".to_owned(),
                "def verify_graph_shape() -> None:\n    assert True\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/blocks/twitter/tweets/manage.py".to_owned(),
                "def main() -> None:\n    return None\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/cli.py".to_owned(),
                "def main() -> None:\n    return None\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/blocks/notion/read_database.py".to_owned(),
                "def read_database() -> dict:\n    return {\"status\": \"ok\"}\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/api/features/mcp/test_routes.py".to_owned(),
                "def test_routes_health() -> None:\n    assert True\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/copilot/executor/processor.py".to_owned(),
                "class Processor:\n    def run(self) -> None:\n        pass\n".to_owned(),
            ),
            (
                "autogpt_platform/backend/backend/blocks/data_manipulation.py".to_owned(),
                "def transform_records() -> None:\n    return None\n".to_owned(),
            ),
        ];
        for index in 0..40 {
            files.push((
                format!("autogpt_platform/backend/backend/blocks/generated/noise_{index}.py"),
                format!("def generated_module_{index}() -> None:\n    return None\n"),
            ));
        }
        let file_refs = files
            .iter()
            .map(|(path, content)| (path.as_str(), content.as_str()))
            .collect::<Vec<_>>();
        prepare_workspace(&root, &file_refs)?;

        let mut semantic_records = vec![
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/pyproject.toml",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/autogpt_libs/pyproject.toml",
                0,
                vec![0.995, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "classic/original_autogpt/pyproject.toml",
                0,
                vec![0.992, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/blocks/notion/read_database.py",
                0,
                vec![0.99, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/blocks/twitter/tweets/manage.py",
                0,
                vec![0.985, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/api/features/mcp/test_routes.py",
                0,
                vec![0.98, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/cli.py",
                0,
                vec![0.975, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/copilot/executor/processor.py",
                0,
                vec![0.97, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/blocks/data_manipulation.py",
                0,
                vec![0.96, 0.0],
            ),
        ];
        for index in 0..40 {
            semantic_records.push(semantic_record(
                "repo-001",
                "snapshot-001",
                &format!("autogpt_platform/backend/backend/blocks/generated/noise_{index}.py"),
                0,
                vec![0.95 - (index as f32 * 0.002), 0.0],
            ));
        }
        semantic_records.extend([
            semantic_record(
                "repo-001",
                "snapshot-001",
                "classic/original_autogpt/autogpt/app/main.py",
                0,
                vec![0.78, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/app.py",
                0,
                vec![0.76, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "autogpt_platform/backend/backend/copilot/executor/__main__.py",
                0,
                vec![0.74, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "classic/benchmark/tests/test_benchmark_workflow.py",
                0,
                vec![0.72, 0.0],
            ),
        ]);
        seed_semantic_embeddings(&root, "repo-001", "snapshot-001", &semantic_records)?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        config.max_search_results = 8;
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap app startup cli main config tests benchmark workflow"
                    .to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths
                .iter()
                .take(8)
                .any(|path| *path == "classic/original_autogpt/autogpt/app/main.py"),
            "main.py should remain visible via path-shaped witness recall even without content overlap: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(8)
                .any(|path| *path == "autogpt_platform/backend/backend/app.py"),
            "app.py should remain visible via path-shaped witness recall even without content overlap: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.iter().take(8).any(
                |path| *path == "autogpt_platform/backend/backend/copilot/executor/__main__.py"
            ),
            "__main__.py should remain visible via path-shaped witness recall even without content overlap: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(8)
                .any(|path| *path == "classic/benchmark/tests/test_benchmark_workflow.py"),
            "python tests should remain visible in the crowded anchored witness set: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_python_config_queries_prefer_runtime_manifests_over_readmes()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-python-config-vs-readmes");
        prepare_workspace(
            &root,
            &[
                (
                    "README.md",
                    "# Setup\nconfig setup pyproject installation guide\n",
                ),
                (
                    "docs/setup.md",
                    "# Platform setup\nconfig setup pyproject walkthrough\n",
                ),
                (
                    "autogpt_platform/backend/pyproject.toml",
                    "[project]\nname = \"autogpt-backend\"\n",
                ),
                (
                    "classic/original_autogpt/setup.py",
                    "from setuptools import setup\nsetup(name=\"classic-autogpt\")\n",
                ),
                (
                    "autogpt_platform/frontend/package.json",
                    "{\n  \"name\": \"frontend\"\n}\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record("repo-001", "snapshot-001", "README.md", 0, vec![1.0, 0.0]),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "docs/setup.md",
                    0,
                    vec![0.98, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/frontend/package.json",
                    0,
                    vec![0.96, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/backend/pyproject.toml",
                    0,
                    vec![0.82, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "classic/original_autogpt/setup.py",
                    0,
                    vec![0.8, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        config.max_search_results = 5;
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "config setup pyproject".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        let backend_pyproject_position = ranked_paths
            .iter()
            .position(|path| *path == "autogpt_platform/backend/pyproject.toml")
            .expect("pyproject witness should be ranked");
        let readme_position = ranked_paths
            .iter()
            .position(|path| *path == "README.md")
            .expect("README noise should still be ranked");

        assert!(
            ranked_paths
                .iter()
                .take(3)
                .any(|path| *path == "autogpt_platform/backend/pyproject.toml"),
            "runtime manifest should appear near the top for focused config queries: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(4)
                .any(|path| *path == "classic/original_autogpt/setup.py"),
            "setup.py witness should remain visible for focused config queries: {ranked_paths:?}"
        );
        assert!(
            backend_pyproject_position < readme_position,
            "runtime manifest should outrank README drift for focused config queries: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_python_test_queries_prefer_backend_tests_over_frontend_docs()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-python-tests-vs-frontend-docs");
        prepare_workspace(
            &root,
            &[
                (
                    "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                    "def test_e2e_auth_flow() -> None:\n    assert True\n",
                ),
                (
                    "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                    "def build_test_helpers() -> None:\n    return None\n",
                ),
                (
                    "autogpt_platform/frontend/src/tests/CLAUDE.md",
                    "# Frontend tests\ntests e2e helpers guidance\n",
                ),
                (
                    "autogpt_platform/frontend/CLAUDE.md",
                    "# Frontend guide\ntests e2e helpers overview\n",
                ),
                (
                    "docs/testing.md",
                    "# Testing guide\ntests e2e helpers reference\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/frontend/src/tests/CLAUDE.md",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/frontend/CLAUDE.md",
                    0,
                    vec![0.98, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "docs/testing.md",
                    0,
                    vec![0.95, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                    0,
                    vec![0.82, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                    0,
                    vec![0.80, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        config.max_search_results = 5;
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests e2e helpers".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        let backend_test_position = ranked_paths
            .iter()
            .position(|path| *path == "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py")
            .expect("backend test witness should be ranked");
        let frontend_doc_position = ranked_paths
            .iter()
            .position(|path| *path == "autogpt_platform/frontend/src/tests/CLAUDE.md")
            .expect("frontend doc noise should still be ranked");

        assert!(
            ranked_paths
                .iter()
                .take(3)
                .any(|path| *path == "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py"),
            "backend test witness should appear near the top for focused tests queries: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(4)
                .any(|path| *path == "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py"),
            "test helper witness should remain visible for focused tests queries: {ranked_paths:?}"
        );
        assert!(
            backend_test_position < frontend_doc_position,
            "backend test witness should outrank frontend test docs: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_python_runtime_entrypoint_test_queries_keep_packet_backend_tests_visible()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-python-runtime-entrypoints-packet-tests");
        prepare_workspace(
            &root,
            &[
                (
                    "autogpt_platform/backend/backend/api/test_helpers.py",
                    "def build_test_helpers() -> None:\n    return None\n",
                ),
                (
                    "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
                    "def test_e2e_auth_flow() -> None:\n    assert True\n",
                ),
                (
                    "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
                    "def load_test_helper_graph() -> None:\n    return None\n",
                ),
                (
                    "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
                    "def test_server_bootstrap() -> None:\n    assert True\n",
                ),
                (
                    "autogpt_platform/backend/pyproject.toml",
                    "[project]\nname = \"autogpt-backend\"\n[project.scripts]\nbackend = \"backend.app:app\"\n",
                ),
                (
                    "autogpt_platform/autogpt_libs/pyproject.toml",
                    "[project]\nname = \"autogpt-libs\"\n",
                ),
                (
                    "classic/original_autogpt/autogpt/app/setup.py",
                    "from setuptools import setup\nsetup(name=\"classic-autogpt-app\")\n",
                ),
                (
                    "classic/benchmark/pyproject.toml",
                    "[project]\nname = \"agbenchmark\"\n",
                ),
                (
                    "classic/forge/pyproject.toml",
                    "[project]\nname = \"forge\"\n",
                ),
                (
                    "classic/original_autogpt/setup.py",
                    "from setuptools import setup\nsetup(name=\"classic-autogpt\")\n",
                ),
                (
                    "classic/original_autogpt/pyproject.toml",
                    "[project]\nname = \"classic-autogpt\"\n",
                ),
                (
                    "autogpt_platform/backend/test/sdk/conftest.py",
                    "def pytest_configure() -> None:\n    return None\n",
                ),
                (
                    "classic/original_autogpt/tests/unit/test_config.py",
                    "def test_runtime_config() -> None:\n    assert True\n",
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        config.max_search_results = 16;
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests fixtures integration helpers e2e config setup pyproject".to_owned(),
                limit: 16,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        let required_witnesses = [
            "autogpt_platform/backend/backend/api/test_helpers.py",
            "autogpt_platform/backend/backend/blocks/mcp/test_e2e.py",
            "autogpt_platform/backend/backend/blocks/mcp/test_helpers.py",
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
        ];

        let first_required_position = ranked_paths
            .iter()
            .position(|path| required_witnesses.iter().any(|required| required == path))
            .expect("at least one packet test witness should be ranked");
        let classic_test_config_position = ranked_paths
            .iter()
            .position(|path| *path == "classic/original_autogpt/tests/unit/test_config.py")
            .expect("classic test-config noise should still be ranked");

        assert!(
            ranked_paths
                .iter()
                .take(12)
                .any(|path| required_witnesses.iter().any(|required| required == path)),
            "at least one required packet test witness should stay visible under runtime-config crowding: {ranked_paths:?}"
        );
        assert!(
            first_required_position < classic_test_config_position,
            "packet backend test witnesses should outrank generic config-heavy test noise: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_rust_config_queries_rescue_cargo_manifests_from_path_witness_recall()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-config-path-witness");
        prepare_workspace(
            &root,
            &[
                (
                    "crates/ruff/src/commands/config.rs",
                    "pub fn config_command() { let _ = \"config cargo\"; }\n",
                ),
                ("crates/ruff/Cargo.toml", "[package]\nname = \"ruff\"\n"),
                (
                    "README.md",
                    "# Config guide\nconfig cargo setup walkthrough\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "config cargo".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        let cargo_position = ranked_paths
            .iter()
            .position(|path| *path == "crates/ruff/Cargo.toml")
            .expect("Cargo.toml witness should be ranked");
        let readme_position = ranked_paths
            .iter()
            .position(|path| *path == "README.md")
            .expect("README noise should still be ranked");

        assert!(
            cargo_position < readme_position,
            "Cargo.toml should outrank README drift for `config cargo` queries: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(3)
                .any(|path| *path == "crates/ruff/Cargo.toml"),
            "Cargo.toml should land near the top via config-artifact path recall: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_rust_workspace_config_queries_prefer_root_rust_configs_over_nested_pyprojects()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-workspace-config-vs-pyproject");
        let mut files = vec![
            (
                "Cargo.toml".to_owned(),
                "[workspace]\nmembers = [\"crates/*\"]\n".to_owned(),
            ),
            (
                "Cargo.lock".to_owned(),
                "[[package]]\nname = \"ruff\"\n".to_owned(),
            ),
            (
                ".cargo/config.toml".to_owned(),
                "[build]\ntarget-dir = \"target\"\n".to_owned(),
            ),
            (
                "rust-toolchain.toml".to_owned(),
                "[toolchain]\nchannel = \"stable\"\n".to_owned(),
            ),
            ("rustfmt.toml".to_owned(), "edition = \"2021\"\n".to_owned()),
            ("clippy.toml".to_owned(), "msrv = \"1.80\"\n".to_owned()),
        ];
        files.extend((0..8).map(|index| {
            (
                format!("crates/noise_{index:02}/pyproject.toml"),
                "[tool.pytest.ini_options]\naddopts = \"-q\"\n".to_owned(),
            )
        }));
        let file_refs = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect::<Vec<_>>();
        prepare_workspace(&root, &file_refs)?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "workspace cargo toolchain config cargo lock".to_owned(),
                limit: 9,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        let first_rust_config = ranked_paths
            .iter()
            .position(|path| {
                matches!(
                    *path,
                    "Cargo.toml"
                        | "Cargo.lock"
                        | ".cargo/config.toml"
                        | "rust-toolchain.toml"
                        | "rustfmt.toml"
                        | "clippy.toml"
                )
            })
            .expect("a rust workspace config witness should be ranked");
        let first_pyproject = ranked_paths
            .iter()
            .position(|path| path.ends_with("pyproject.toml"))
            .expect("pyproject noise should still be ranked");

        assert!(
            first_rust_config < first_pyproject,
            "rust workspace config should outrank nested pyproject noise: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.iter().take(5).any(|path| {
                matches!(
                    *path,
                    "Cargo.toml"
                        | "Cargo.lock"
                        | ".cargo/config.toml"
                        | "rust-toolchain.toml"
                        | "rustfmt.toml"
                        | "clippy.toml"
                )
            }),
            "a rust workspace config witness should land near the top: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_examples_queries_keep_examples_and_benches_visible_over_test_noise()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-examples-benches");
        prepare_workspace(
            &root,
            &[
                (
                    "crates/ruff/tests/cli/main.rs",
                    "tests examples fixtures integration benchmark\n",
                ),
                (
                    "crates/ruff/tests/cli/lint.rs",
                    "tests examples fixtures integration benchmark\n",
                ),
                (
                    "crates/ruff_annotate_snippets/tests/examples.rs",
                    "tests examples fixtures integration benchmark\n",
                ),
                (
                    "crates/ruff_annotate_snippets/examples/expected_type.rs",
                    "pub fn demo_example() {}\n",
                ),
                (
                    "crates/ruff_benchmark/benches/formatter.rs",
                    "pub fn bench_formatter() {}\n",
                ),
                (
                    "crates/ruff_benchmark/benches/ty.rs",
                    "pub fn bench_ty() {}\n",
                ),
                (
                    "crates/ruff/src/cache.rs",
                    "tests examples fixtures integration benchmark\n",
                ),
                (
                    "docs/examples.md",
                    "# Examples\ntests examples fixtures integration benchmark\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests examples fixtures integration benchmark".to_owned(),
                limit: 6,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().take(3).any(|path| matches!(
                *path,
                "crates/ruff_annotate_snippets/examples/expected_type.rs"
                    | "crates/ruff_benchmark/benches/formatter.rs"
            )),
            "an examples-or-benches witness should land near the top: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_rust_tests_queries_keep_required_tests_visible_under_examples_and_benches_crowding()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-tests-vs-examples-benches-crowding");
        prepare_workspace(
            &root,
            &[
                (
                    "crates/ruff/tests/analyze_graph.rs",
                    "mod analyze_graph {}\n",
                ),
                (
                    "crates/ruff/tests/cli/analyze_graph.rs",
                    "mod cli_analyze_graph {}\n",
                ),
                ("crates/ruff/tests/cli/format.rs", "mod cli_format {}\n"),
                ("crates/ruff/tests/cli/lint.rs", "mod cli_lint {}\n"),
                ("crates/ruff/tests/cli/main.rs", "mod cli_main {}\n"),
                ("crates/ruff/tests/config.rs", "mod config_test {}\n"),
                (
                    "crates/ruff_annotate_snippets/examples/footer.rs",
                    "Level::Error.title(\"mismatched types\").footer(Level::Note.title(\"expected type\"));\n",
                ),
                (
                    "crates/ruff_annotate_snippets/examples/footer.svg",
                    "<svg><text>expected type</text><text>footer</text></svg>\n",
                ),
                (
                    "crates/ruff_python_formatter/tests/fixtures.rs",
                    "fn black_compatibility() { format_range(); }\n",
                ),
                (
                    "crates/ruff_benchmark/benches/linter.rs",
                    "fn benchmark_linter() { criterion_group!(benches); }\n",
                ),
                (
                    "crates/ruff_benchmark/benches/ty.rs",
                    "fn benchmark_ty() { criterion_group!(benches); }\n",
                ),
                (
                    "crates/ruff_benchmark/benches/ty_walltime.rs",
                    "fn benchmark_ty_walltime() { criterion_group!(benches); }\n",
                ),
                (
                    "crates/ruff_annotate_snippets/examples/expected_type.rs",
                    "Level::Note.title(\"expected type\");\n",
                ),
                (
                    "crates/ruff_python_parser/tests/fixtures.rs",
                    "fn parse_fixture() { parse_module(\"x = 1\"); }\n",
                ),
                (
                    "crates/ruff_annotate_snippets/examples/expected_type.svg",
                    "<svg><text>expected type</text></svg>\n",
                ),
                (
                    "crates/ruff_annotate_snippets/tests/examples.rs",
                    "fn examples_snapshot() { assert_snapshot!(); }\n",
                ),
                (
                    "crates/ruff_benchmark/benches/formatter.rs",
                    "fn benchmark_formatter() { criterion_group!(benches); }\n",
                ),
                (
                    "crates/ruff_benchmark/benches/lexer.rs",
                    "fn benchmark_lexer() { criterion_group!(benches); }\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests fixtures integration analyze graph entrypoint".to_owned(),
                limit: 12,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.iter().take(12).any(|path| {
                matches!(
                    *path,
                    "crates/ruff/tests/analyze_graph.rs"
                        | "crates/ruff/tests/cli/analyze_graph.rs"
                        | "crates/ruff/tests/cli/format.rs"
                        | "crates/ruff/tests/cli/lint.rs"
                        | "crates/ruff/tests/cli/main.rs"
                        | "crates/ruff/tests/config.rs"
                )
            }),
            "a required Rust test witness should remain visible under example/bench crowding: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_tests_queries_keep_cli_runtime_witnesses_visible_over_bounded_doc_noise()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-cli-runtime-test-witnesses");
        let mut files = (0..24)
            .map(|index| {
                (
                    format!("docs/alpha-{index:02}.md"),
                    format!(
                        "# Alpha {index}\ntests examples fixtures integration benchmark latest tool\n"
                    ),
                )
            })
            .collect::<Vec<_>>();
        files.extend([
            (
                "docs/cli/latest.md".to_owned(),
                "# `mise latest`\nGets the latest available version for a plugin\n".to_owned(),
            ),
            (
                "docs/cli/tool.md".to_owned(),
                "# `mise tool`\nGets information about a tool\n".to_owned(),
            ),
            (
                "src/cli/latest.rs".to_owned(),
                "/// Gets the latest available version for a plugin\npub struct Latest;\n"
                    .to_owned(),
            ),
            (
                "src/cli/test_tool.rs".to_owned(),
                "/// Test a tool installs and executes\npub struct TestTool;\n".to_owned(),
            ),
            (
                "src/test.rs".to_owned(),
                "pub fn init_test_env() { let _ = \"tests fixtures integration\"; }\n".to_owned(),
            ),
        ]);
        let file_refs = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect::<Vec<_>>();
        prepare_workspace(&root, &file_refs)?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests examples fixtures integration benchmark latest tool".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.contains(&"src/cli/latest.rs"),
            "path witness recall should keep the CLI runtime witness visible in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_rust_mixed_tests_queries_keep_bench_witnesses_visible_under_test_crowding()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-mixed-tests-vs-benches");
        let mut files = (0..14)
            .map(|index| {
                (
                    format!("crates/biome_cli/tests/cases/noise_{index:02}.rs"),
                    "tests fixtures integration assist biome json css analyzer\n".to_owned(),
                )
            })
            .collect::<Vec<_>>();
        files.extend([
            (
                "crates/biome_cli/tests/cases/assist.rs".to_owned(),
                "tests fixtures integration assist biome json css analyzer\n".to_owned(),
            ),
            (
                "crates/biome_service/tests/fixtures/basic/biome.jsonc".to_owned(),
                "{ \"tests\": \"fixtures integration assist biome json css analyzer\" }\n"
                    .to_owned(),
            ),
            (
                "crates/biome_configuration/benches/biome_json.rs".to_owned(),
                "pub fn bench_biome_json() {}\n".to_owned(),
            ),
            (
                "crates/biome_css_analyze/benches/css_analyzer.rs".to_owned(),
                "pub fn bench_css_analyzer() {}\n".to_owned(),
            ),
            (
                "benchmark/biome.json".to_owned(),
                "{ \"benchmark\": true, \"biome\": \"json\" }\n".to_owned(),
            ),
        ]);
        let file_refs = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect::<Vec<_>>();
        prepare_workspace(&root, &file_refs)?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests fixtures integration assist biome json examples benches benchmark css analyzer"
                    .to_owned(),
                limit: 12,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().take(12).any(|path| {
                matches!(
                    *path,
                    "crates/biome_configuration/benches/biome_json.rs"
                        | "crates/biome_css_analyze/benches/css_analyzer.rs"
                )
            }),
            "a bench witness should remain visible for mixed rust tests queries: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_rust_mixed_examples_queries_keep_test_witnesses_visible_under_bench_crowding()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-mixed-benches-vs-tests");
        let mut files = (0..14)
            .map(|index| {
                (
                    format!("crates/biome_package/benches/noise_{index:02}.rs"),
                    "examples benches benchmark biome json css analyzer\n".to_owned(),
                )
            })
            .collect::<Vec<_>>();
        files.extend([
            (
                "crates/biome_configuration/benches/biome_json.rs".to_owned(),
                "pub fn bench_biome_json() {}\n".to_owned(),
            ),
            (
                "crates/biome_css_analyze/benches/css_analyzer.rs".to_owned(),
                "pub fn bench_css_analyzer() {}\n".to_owned(),
            ),
            (
                "crates/biome_cli/tests/cases/assist.rs".to_owned(),
                "assert_cli_snapshot();\n".to_owned(),
            ),
            (
                "crates/biome_cli/tests/cases/configuration.rs".to_owned(),
                "assert_cli_snapshot();\n".to_owned(),
            ),
        ]);
        let file_refs = files
            .iter()
            .map(|(path, contents)| (path.as_str(), contents.as_str()))
            .collect::<Vec<_>>();
        prepare_workspace(&root, &file_refs)?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "examples benches benchmark biome json css analyzer tests assist".to_owned(),
                limit: 12,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths
                .iter()
                .take(12)
                .any(|path| *path == "crates/biome_cli/tests/cases/assist.rs"),
            "a targeted test witness should remain visible for mixed rust examples queries: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_laravel_ui_queries_surface_livewire_and_blade_witnesses()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-laravel-ui-witnesses");
        prepare_workspace(
            &root,
            &[
                ("app/Livewire/Dashboard.php", "<?php\nclass Dashboard {}\n"),
                (
                    "app/Livewire/ActivityMonitor.php",
                    "<?php\nclass ActivityMonitor {}\n",
                ),
                (
                    "resources/views/livewire/subscription/show.blade.php",
                    "<div>subscription</div>\n",
                ),
                (
                    "resources/views/livewire/dashboard.blade.php",
                    "<div>dashboard</div>\n",
                ),
                (
                    "resources/views/layouts/simple.blade.php",
                    "<x-layouts.simple />\n",
                ),
                (
                    "resources/views/layouts/app.blade.php",
                    "<x-app-layout />\n",
                ),
                ("resources/views/components/navbar.blade.php", "<nav />\n"),
                (
                    "resources/views/components/applications/advanced.blade.php",
                    "<x-applications.advanced />\n",
                ),
                (
                    "resources/views/auth/verify-email.blade.php",
                    "<x-auth.verify-email />\n",
                ),
                ("TECH_STACK.md", "# Tech Stack\nLaravel Livewire Flux\n"),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/livewire/subscription/show.blade.php",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/layouts/simple.blade.php",
                    0,
                    vec![0.99, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/components/navbar.blade.php",
                    0,
                    vec![0.985, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/livewire/dashboard.blade.php",
                    0,
                    vec![0.98, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/layouts/app.blade.php",
                    0,
                    vec![0.97, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "TECH_STACK.md",
                    0,
                    vec![0.965, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/Livewire/Dashboard.php",
                    0,
                    vec![0.90, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/Livewire/ActivityMonitor.php",
                    0,
                    vec![0.89, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/components/applications/advanced.blade.php",
                    0,
                    vec![0.87, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/auth/verify-email.blade.php",
                    0,
                    vec![0.86, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "blade livewire flux component view slot section".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths
                .iter()
                .any(|path| *path == "app/Livewire/Dashboard.php"
                    || *path == "app/Livewire/ActivityMonitor.php"),
            "Laravel UI ranking should keep a Livewire component witness in top-k: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.iter().any(|path| {
                *path == "resources/views/components/applications/advanced.blade.php"
                    || *path == "resources/views/auth/verify-email.blade.php"
            }),
            "Laravel UI ranking should keep a Blade view witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_laravel_ui_queries_avoid_unit3d_component_only_collapse() -> FriggResult<()> {
        let query = "blade livewire flux component view slot section";
        let make_hit = |path: &str, raw_score: f32, excerpt: &str| HybridChannelHit {
            channel: crate::domain::EvidenceChannel::LexicalManifest,
            document: HybridDocumentRef {
                repository_id: "repo-001".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            anchor: crate::domain::EvidenceAnchor::new(
                crate::domain::EvidenceAnchorKind::TextSpan,
                1,
                1,
                1,
                1,
            ),
            raw_score,
            excerpt: excerpt.to_owned(),
            provenance_ids: vec![format!("lexical::{path}")],
        };
        let lexical = vec![
            make_hit(
                "resources/views/components/forum/post.blade.php",
                1.00,
                "@props(['post'])\n<article class=\"post\" x-data>\n",
            ),
            make_hit(
                "resources/views/components/torrent/row.blade.php",
                0.99,
                "@props(['torrent'])\n<tr data-torrent-id=\"1\">\n",
            ),
            make_hit(
                "resources/views/components/forum/topic-listing.blade.php",
                0.98,
                "<section class=\"topic-listing\"></section>\n",
            ),
            make_hit(
                "resources/views/components/torrent/comment-listing.blade.php",
                0.97,
                "<section class=\"comment-listing\"></section>\n",
            ),
            make_hit(
                "resources/views/components/tv/card.blade.php",
                0.96,
                "<x-card><x-slot:title>TV</x-slot:title></x-card>\n",
            ),
            make_hit(
                "resources/views/components/forum/subforum-listing.blade.php",
                0.95,
                "<section class=\"subforum-listing\"></section>\n",
            ),
            make_hit(
                "resources/views/components/user-tag.blade.php",
                0.94,
                "<x-user-tag />\n",
            ),
            make_hit(
                "resources/views/components/playlist/card.blade.php",
                0.93,
                "<x-card><x-slot:title>Playlist</x-slot:title></x-card>\n",
            ),
            make_hit(
                "resources/views/Staff/announce/index.blade.php",
                0.91,
                "@section('main')\n    @livewire('announce-search')\n@endsection\n",
            ),
            make_hit(
                "resources/views/Staff/application/index.blade.php",
                0.90,
                "@section('main')\n    @livewire('application-search')\n@endsection\n",
            ),
            make_hit(
                "resources/views/livewire/announce-search.blade.php",
                0.89,
                "<section class=\"panelV2\">\n    <input wire:model.live=\"torrentId\" />\n</section>\n",
            ),
            make_hit(
                "resources/views/livewire/apikey-search.blade.php",
                0.88,
                "<section class=\"panelV2\">\n    <input wire:model.live=\"apikey\" />\n</section>\n",
            ),
            make_hit(
                "app/Http/Livewire/AnnounceSearch.php",
                0.87,
                "<?php class AnnounceSearch extends Component {}\n",
            ),
        ];

        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            8,
            query,
        )?;
        let ranked_paths = ranked
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths
                .iter()
                .any(|path| path.starts_with("resources/views/Staff/")),
            "Laravel UI ranking should keep a non-component Blade view witness in top-k: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .any(|path| path.starts_with("resources/views/livewire/")),
            "Laravel UI ranking should keep a Livewire Blade view witness in top-k: {ranked_paths:?}"
        );

        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_laravel_route_queries_surface_route_witnesses() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-laravel-route-witnesses");
        prepare_workspace(
            &root,
            &[
                ("composer.lock", "{\n  \"packages\": []\n}\n"),
                (
                    "tests/Feature/TrustHostsMiddlewareTest.php",
                    "<?php\nclass TrustHostsMiddlewareTest {}\n",
                ),
                (
                    "tests/Feature/CommandInjectionSecurityTest.php",
                    "<?php\nclass CommandInjectionSecurityTest {}\n",
                ),
                (
                    "app/Providers/FortifyServiceProvider.php",
                    "<?php\nclass FortifyServiceProvider {}\n",
                ),
                (
                    "app/Providers/RouteServiceProvider.php",
                    "<?php\nclass RouteServiceProvider {}\n",
                ),
                (
                    "app/Providers/ConfigurationServiceProvider.php",
                    "<?php\nclass ConfigurationServiceProvider {}\n",
                ),
                ("routes/web.php", "<?php\nRoute::get('/', fn () => 'ok');\n"),
                (
                    "routes/api.php",
                    "<?php\nRoute::get('/api', fn () => 'ok');\n",
                ),
                (
                    "routes/webhooks.php",
                    "<?php\nRoute::post('/webhooks', fn () => 'ok');\n",
                ),
                ("bootstrap/app.php", "<?php\nreturn 'bootstrap';\n"),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "tests/Feature/CommandInjectionSecurityTest.php",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "tests/Feature/TrustHostsMiddlewareTest.php",
                    0,
                    vec![0.95, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/Providers/FortifyServiceProvider.php",
                    0,
                    vec![0.92, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/Providers/RouteServiceProvider.php",
                    0,
                    vec![0.90, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/Providers/ConfigurationServiceProvider.php",
                    0,
                    vec![0.89, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "composer.lock",
                    0,
                    vec![0.87, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "routes/web.php",
                    0,
                    vec![0.82, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "routes/api.php",
                    0,
                    vec![0.81, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "routes/webhooks.php",
                    0,
                    vec![0.80, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "bootstrap/app.php",
                    0,
                    vec![0.79, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "bootstrap providers routes middleware app entrypoint".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().any(|path| matches!(
                *path,
                "routes/web.php" | "routes/api.php" | "routes/webhooks.php"
            )),
            "Laravel route ranking should keep a routes witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_laravel_linkstack_queries_recover_layouts_and_blade_views()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-laravel-linkstack-layouts-and-views");
        prepare_workspace(
            &root,
            &[
                (
                    "app/View/Components/AppLayout.php",
                    "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass AppLayout extends Component {\n    public function render() {\n        return view('layouts.app');\n    }\n}\n",
                ),
                (
                    "app/View/Components/GuestLayout.php",
                    "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass GuestLayout extends Component {\n    public function render() {\n        return view('layouts.guest');\n    }\n}\n",
                ),
                (
                    "app/View/Components/Modal.php",
                    "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass Modal extends Component {\n    public function render() {\n        return view('components.modal');\n    }\n}\n",
                ),
                (
                    "app/View/Components/PageItemDisplay.php",
                    "<?php\nnamespace App\\View\\Components;\nuse Illuminate\\View\\Component;\nclass PageItemDisplay extends Component {\n    public function render() {\n        return view('components.page-item-display');\n    }\n}\n",
                ),
                (
                    "app/Models/Page.php",
                    "<?php\nnamespace App\\Models;\nclass Page {}\n",
                ),
                (
                    "resources/views/components/finishing.blade.php",
                    "<x-alert>\n<x-slot name=\"title\">blade layout component slot section render page navigation</x-slot>\n<div>blade layout component slot section render page navigation blade layout component slot section render page navigation</div>\n</x-alert>\n",
                ),
                (
                    "resources/views/components/alert.blade.php",
                    "<div class=\"alert\">blade component layout slot section view render</div>\n",
                ),
                (
                    "resources/views/components/auth-card.blade.php",
                    "<section class=\"auth-card\">blade component layout slot section view render</section>\n",
                ),
                (
                    "resources/views/layouts/app.blade.php",
                    "@include('layouts.analytics')\n@include('layouts.navigation')\n<header>{{ $header }}</header>\n<main>{{ $slot }}</main>\n",
                ),
                (
                    "resources/views/layouts/guest.blade.php",
                    "<main class=\"guest-layout\">{{ $slot }}</main>\n",
                ),
                (
                    "resources/views/layouts/analytics.blade.php",
                    "<script>window.analytics = true;</script>\n",
                ),
                (
                    "resources/views/layouts/navigation.blade.php",
                    "<nav class=\"main-nav\">navigation</nav>\n",
                ),
                (
                    "resources/views/auth/forgot-password.blade.php",
                    "<x-guest-layout>\n@include('layouts.lang')\n<x-auth-card>\n<x-slot name=\"logo\"></x-slot>\n@section('content') blade component layout slot section view render @endsection\n</x-auth-card>\n</x-guest-layout>\n",
                ),
                (
                    "resources/views/auth/login.blade.php",
                    "<x-guest-layout>\n<x-auth-card>\n<x-slot name=\"logo\"></x-slot>\n@section('content') blade component layout slot section view render @endsection\n</x-auth-card>\n</x-guest-layout>\n",
                ),
                (
                    "resources/views/admin/linktype/index.blade.php",
                    "@extends('layouts.app')\n@section('content')\n<a href=\"/admin/linktype/create\">blade component layout slot section view render</a>\n@endsection\n",
                ),
                (
                    "TECH_STACK.md",
                    "Blade Laravel view component layout reference.\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/components/finishing.blade.php",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/Models/Page.php",
                    0,
                    vec![0.99, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/View/Components/AppLayout.php",
                    0,
                    vec![0.98, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/View/Components/GuestLayout.php",
                    0,
                    vec![0.97, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/View/Components/PageItemDisplay.php",
                    0,
                    vec![0.96, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/components/alert.blade.php",
                    0,
                    vec![0.95, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "app/View/Components/Modal.php",
                    0,
                    vec![0.94, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/components/auth-card.blade.php",
                    0,
                    vec![0.93, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/auth/forgot-password.blade.php",
                    0,
                    vec![0.92, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/auth/login.blade.php",
                    0,
                    vec![0.91, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/admin/linktype/index.blade.php",
                    0,
                    vec![0.90, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/layouts/app.blade.php",
                    0,
                    vec![0.89, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/layouts/guest.blade.php",
                    0,
                    vec![0.88, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "resources/views/layouts/analytics.blade.php",
                    0,
                    vec![0.87, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "TECH_STACK.md",
                    0,
                    vec![0.86, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let layout_output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "blade layout component slot section render page navigation".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &executor,
        )?;
        let layout_paths = layout_output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            layout_paths.iter().any(|path| matches!(
                *path,
                "resources/views/layouts/app.blade.php"
                    | "resources/views/layouts/guest.blade.php"
                    | "resources/views/layouts/analytics.blade.php"
            )),
            "Laravel UI ranking should keep a Blade layout witness in top-k under component-class pressure: {layout_paths:?}"
        );

        let blade_view_output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "blade component layout slot section view render".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &executor,
        )?;
        let blade_view_paths = blade_view_output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            blade_view_paths.iter().any(|path| matches!(
                *path,
                "resources/views/auth/forgot-password.blade.php"
                    | "resources/views/auth/login.blade.php"
                    | "resources/views/admin/linktype/index.blade.php"
            )),
            "Laravel UI ranking should keep a concrete Blade page witness in top-k under component-class pressure: {blade_view_paths:?}"
        );
        assert!(
            blade_view_paths.iter().any(|path| matches!(
                *path,
                "resources/views/components/alert.blade.php"
                    | "resources/views/components/auth-card.blade.php"
                    | "resources/views/components/finishing.blade.php"
            )),
            "Laravel UI ranking should still keep a Blade component witness in top-k: {blade_view_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_materializes_manifest_projection_rows() -> FriggResult<()> {
        let root = temp_workspace_root("path-witness-projection-materialization");
        prepare_workspace(
            &root,
            &[
                (
                    "tests/CreatesApplication.php",
                    "<?php\n\ntrait CreatesApplication {}\n",
                ),
                (
                    "tests/DuskTestCase.php",
                    "<?php\n\nabstract class DuskTestCase {}\n",
                ),
            ],
        )?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-001",
            &["tests/CreatesApplication.php", "tests/DuskTestCase.php"],
        )?;

        let db_path = resolve_provenance_db_path(&root)?;
        let storage = Storage::new(db_path);
        assert!(
            storage
                .load_path_witness_projections_for_repository_snapshot("repo-001", "snapshot-001")?
                .is_empty(),
            "path witness projection rows should start empty before the first search"
        );

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let query = "tests fixtures integration tests createsapplication dusktestcase";
        let intent = HybridRankingIntent::from_query(query);
        let output = searcher.search_path_witness_recall_with_filters(
            query,
            &SearchFilters::default(),
            8,
            &intent,
        )?;

        assert_eq!(output.matches.len(), 2);

        let rows = storage
            .load_path_witness_projections_for_repository_snapshot("repo-001", "snapshot-001")?;
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].path, "tests/CreatesApplication.php");
        assert_eq!(rows[1].path, "tests/DuskTestCase.php");

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_reuses_snapshot_scoped_projection_cache() -> FriggResult<()> {
        let root = temp_workspace_root("path-witness-projection-cache-reuse");
        prepare_workspace(
            &root,
            &[
                (
                    "tests/CreatesApplication.php",
                    "<?php\n\ntrait CreatesApplication {}\n",
                ),
                (
                    "tests/DuskTestCase.php",
                    "<?php\n\nabstract class DuskTestCase {}\n",
                ),
            ],
        )?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-001",
            &["tests/CreatesApplication.php", "tests/DuskTestCase.php"],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        assert_eq!(
            searcher
                .hybrid_path_witness_projection_cache
                .read()
                .expect("path witness projection cache should not be poisoned")
                .len(),
            0
        );

        let query = "tests fixtures integration tests createsapplication dusktestcase";
        let intent = HybridRankingIntent::from_query(query);
        let first = searcher.search_path_witness_recall_with_filters(
            query,
            &SearchFilters::default(),
            8,
            &intent,
        )?;
        assert_eq!(
            searcher
                .hybrid_path_witness_projection_cache
                .read()
                .expect("path witness projection cache should not be poisoned")
                .len(),
            1
        );

        let second = searcher.search_path_witness_recall_with_filters(
            query,
            &SearchFilters::default(),
            8,
            &intent,
        )?;
        assert_eq!(first.matches, second.matches);
        assert_eq!(
            searcher
                .hybrid_path_witness_projection_cache
                .read()
                .expect("path witness projection cache should not be poisoned")
                .len(),
            1
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_prefers_exact_php_test_harness_excerpt() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-php-test-harness-excerpt");
        prepare_workspace(
            &root,
            &[
                (
                    "tests/CreatesApplication.php",
                    "<?php\n\ntrait CreatesApplication {}\n",
                ),
                (
                    "tests/DuskTestCase.php",
                    "<?php\n\nabstract class DuskTestCase {}\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let query = "tests fixtures integration tests createsapplication dusktestcase";
        let intent = HybridRankingIntent::from_query(query);
        let output = searcher.search_path_witness_recall_with_filters(
            query,
            &SearchFilters::default(),
            8,
            &intent,
        )?;

        let creates_application = output
            .matches
            .iter()
            .find(|entry| entry.path == "tests/CreatesApplication.php")
            .expect("CreatesApplication path witness should be returned");
        let dusk_test_case = output
            .matches
            .iter()
            .find(|entry| entry.path == "tests/DuskTestCase.php")
            .expect("DuskTestCase path witness should be returned");

        assert!(
            creates_application.excerpt.contains("CreatesApplication"),
            "path witness recall should choose the exact harness line, got {:?}",
            creates_application.excerpt
        );
        assert!(
            dusk_test_case.excerpt.contains("DuskTestCase"),
            "path witness recall should choose the exact harness line, got {:?}",
            dusk_test_case.excerpt
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_uses_live_entrypoint_detection_for_stale_typescript_projections()
    -> FriggResult<()> {
        let query = "entry point bootstrap server app cli router main";
        let intent = HybridRankingIntent::from_query(query);
        let query_context = HybridPathWitnessQueryContext::new(query);
        let mut stale_entrypoint =
            StoredPathWitnessProjection::from_path("packages/cli/src/server.ts");
        stale_entrypoint.flags.is_entrypoint_runtime = false;
        let competing_router = StoredPathWitnessProjection::from_path(
            "packages/@n8n/nodes-langchain/nodes/vendors/Anthropic/actions/router.ts",
        );

        let stale_score = hybrid_path_witness_recall_score_for_projection(
            "packages/cli/src/server.ts",
            &stale_entrypoint,
            &intent,
            &query_context,
        )
        .expect("live path detection should recover stale TypeScript entrypoint projections");
        let router_score = hybrid_path_witness_recall_score_for_projection(
            "packages/@n8n/nodes-langchain/nodes/vendors/Anthropic/actions/router.ts",
            &competing_router,
            &intent,
            &query_context,
        )
        .expect("router path should still receive a score from query overlap");

        assert!(
            stale_score > router_score,
            "canonical src/server.ts should outrank non-src router noise even when the stored projection is stale"
        );
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_uses_live_roc_entrypoint_detection_for_stale_projections()
    -> FriggResult<()> {
        let query = "entry point main app package platform runtime";
        let intent = HybridRankingIntent::from_query(query);
        let query_context = HybridPathWitnessQueryContext::new(query);
        let mut stale_entrypoint = StoredPathWitnessProjection::from_path("platform/main.roc");
        stale_entrypoint.flags.is_entrypoint_runtime = false;
        let competing_host_lib =
            StoredPathWitnessProjection::from_path("crates/roc_host/src/lib.rs");

        let stale_score = hybrid_path_witness_recall_score_for_projection(
            "platform/main.roc",
            &stale_entrypoint,
            &intent,
            &query_context,
        )
        .expect("live path detection should recover stale Roc platform entrypoints");
        let host_lib_score = hybrid_path_witness_recall_score_for_projection(
            "crates/roc_host/src/lib.rs",
            &competing_host_lib,
            &intent,
            &query_context,
        )
        .expect("host runtime libraries should still receive a score from query overlap");

        assert!(
            stale_score > host_lib_score,
            "platform/main.roc should outrank generic host runtime libraries even when the stored Roc projection is stale"
        );
        Ok(())
    }

    #[test]
    fn hybrid_path_witness_recall_uses_live_pytest_detection_for_stale_python_test_projections()
    -> FriggResult<()> {
        let query = "tests fixtures integration helpers e2e config setup pyproject";
        let intent = HybridRankingIntent::from_query(query);
        let query_context = HybridPathWitnessQueryContext::new(query);
        let mut stale_test = StoredPathWitnessProjection::from_path(
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
        );
        stale_test.flags.is_python_test_witness = false;
        stale_test.flags.is_test_support = false;
        stale_test.source_class = HybridSourceClass::Project;
        let generic_server =
            StoredPathWitnessProjection::from_path("autogpt_platform/backend/backend/server.py");

        let stale_score = hybrid_path_witness_recall_score_for_projection(
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
            &stale_test,
            &intent,
            &query_context,
        )
        .expect("live path detection should recover stale pytest projections");
        let generic_server_score = hybrid_path_witness_recall_score_for_projection(
            "autogpt_platform/backend/backend/server.py",
            &generic_server,
            &intent,
            &query_context,
        );

        assert!(
            stale_score > 0.0,
            "stale pytest projections should still receive a live witness score"
        );
        assert!(
            generic_server_score.is_none(),
            "non-test runtime helpers without query overlap should not be recalled for the packet query"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_cli_entrypoint_queries_prefer_cli_test_witnesses_over_runtime_noise()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-cli-test-witnesses");
        prepare_workspace(
            &root,
            &[
                (
                    "crates/ruff/src/commands/analyze_graph.rs",
                    "pub fn analyze_graph() { let _ = \"ruff analyze\"; }\n",
                ),
                (
                    "crates/ruff_linter/src/checkers/ast/analyze/expression.rs",
                    "pub fn analyze_expression() { let _ = \"ruff analyze\"; }\n",
                ),
                (
                    "crates/ruff_linter/src/checkers/ast/analyze/module.rs",
                    "pub fn analyze_module() { let _ = \"ruff analyze\"; }\n",
                ),
                (
                    "crates/ruff_linter/src/checkers/ast/analyze/suite.rs",
                    "pub fn analyze_suite() { let _ = \"ruff analyze\"; }\n",
                ),
                (
                    "crates/ruff_linter/src/lib.rs",
                    "pub fn lib_runtime() { let _ = \"ruff analyze\"; }\n",
                ),
                (
                    "crates/ruff_linter/resources/test/fixtures/isort/pyproject.toml",
                    "[tool.ruff]\nline-length = 88\n",
                ),
                (
                    "crates/ruff/tests/integration_test.rs",
                    "mod integration_test {}\n",
                ),
                (
                    ".github/workflows/ci.yaml",
                    "ruff analyze cli entrypoint workflow\n",
                ),
                (
                    "crates/ruff/tests/cli/analyze_graph.rs",
                    "mod cli_analyze_graph {}\n",
                ),
                ("crates/ruff/tests/cli/main.rs", "mod cli_main {}\n"),
                ("crates/ruff/tests/cli/format.rs", "mod cli_format {}\n"),
                ("crates/ruff/tests/cli/lint.rs", "mod cli_lint {}\n"),
                ("crates/ruff/tests/config.rs", "mod config_test {}\n"),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "ruff analyze ruff cli entrypoint".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths
                .iter()
                .take(4)
                .any(|path| *path == "crates/ruff/tests/cli/analyze_graph.rs"),
            "CLI analyze_graph test witness should land near the top for the saved query: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(5)
                .any(|path| *path == "crates/ruff/tests/cli/main.rs"),
            "secondary CLI test witness should remain visible near the top: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_ci_workflow_queries_surface_hidden_workflow_witnesses() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-ci-workflow-witnesses");
        prepare_workspace(
            &root,
            &[
                (
                    ".github/workflows/autofix.yml",
                    "name: autofix.ci\njobs:\n  autofix:\n    steps:\n      - run: cargo codegen\n",
                ),
                (
                    ".github/workflows/bench_cli.yml",
                    "name: Bench CLI\njobs:\n  bench:\n    steps:\n      - run: cargo bench\n",
                ),
                (
                    "crates/noise/src/github.rs",
                    "pub fn github_reporter() { let _ = \"github workflow autofix\"; }\n",
                ),
                (
                    "crates/noise/src/autofix.rs",
                    "pub fn autofix_runtime() { let _ = \"autofix runtime\"; }\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "github workflow autofix bench cli".to_owned(),
                limit: 4,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().take(4).any(|path| matches!(
                *path,
                ".github/workflows/autofix.yml" | ".github/workflows/bench_cli.yml"
            )),
            "CI workflow query should surface a hidden workflow witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_scripts_ops_queries_surface_script_and_justfile_witnesses() -> FriggResult<()>
    {
        let root = temp_workspace_root("hybrid-scripts-ops-witnesses");
        prepare_workspace(
            &root,
            &[
                ("justfile", "fmt:\n\tcargo fmt\n"),
                (
                    "scripts/print-changelog.sh",
                    "#!/usr/bin/env bash\necho changelog\n",
                ),
                (
                    "scripts/update-manifests.mjs",
                    "console.log('update manifests');\n",
                ),
                ("docs/changelog.md", "# Changelog\nupdate notes\n"),
                ("src/version.rs", "pub const VERSION: &str = \"1.0.0\";\n"),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "scripts justfile changelog manifests".to_owned(),
                limit: 4,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().take(4).any(|path| matches!(
                *path,
                "justfile" | "scripts/print-changelog.sh" | "scripts/update-manifests.mjs"
            )),
            "scripts/ops query should surface a concrete script witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_entrypoint_queries_surface_build_workflow_configs() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-entrypoint-build-workflows");
        prepare_workspace(
            &root,
            &[
                (
                    "src-tauri/src/main.rs",
                    "fn main() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
                ),
                (
                    "src-tauri/src/lib.rs",
                    "pub fn run() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
                ),
                (
                    "src-tauri/src/proxy/config.rs",
                    "pub struct ProxyConfig;\n\
                     impl ProxyConfig { pub fn load() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/modules/config.rs",
                    "pub struct ModuleConfig;\n\
                     impl ModuleConfig { pub fn load() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/models/config.rs",
                    "pub struct AppConfig;\n\
                     impl AppConfig { pub fn load() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/proxy/proxy_pool.rs",
                    "pub struct ProxyPool;\n\
                     impl ProxyPool { pub fn runner() -> Self { Self } }\n",
                ),
                (
                    "src-tauri/src/commands/security.rs",
                    "pub fn security_command_runner() {}\n",
                ),
                ("src-tauri/build.rs", "fn main() { tauri_build::build() }\n"),
                (
                    ".github/workflows/deploy-pages.yml",
                    "name: Deploy static content to Pages\n\
                     jobs:\n\
                       deploy:\n\
                         steps:\n\
                           - name: Deploy to GitHub Pages\n",
                ),
                (
                    ".github/workflows/release.yml",
                    "name: Release\n\
                     jobs:\n\
                       build-tauri:\n\
                         steps:\n\
                           - name: Build the app\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap build flow command runner main config".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths
                .iter()
                .take(8)
                .any(|path| *path == "src-tauri/src/main.rs"),
            "entrypoint runtime witness should remain visible near the top: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.iter().take(8).any(|path| {
                matches!(
                    *path,
                    ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
                )
            }),
            "entrypoint/build-flow queries should surface at least one GitHub workflow config witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_entrypoint_build_flow_queries_keep_runtime_entrypoints_visible_under_workflow_crowding()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-entrypoint-vs-workflow-crowding");
        prepare_workspace(
            &root,
            &[
                (
                    "crates/ruff/src/main.rs",
                    "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
                ),
                (
                    "crates/ruff_dev/src/main.rs",
                    "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
                ),
                (
                    "crates/ruff_python_formatter/src/main.rs",
                    "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
                ),
                (
                    "crates/ty/src/main.rs",
                    "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
                ),
                (
                    "crates/ty_completion_bench/src/main.rs",
                    "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
                ),
                (
                    ".github/workflows/build-binaries.yml",
                    "name: Build binaries\njobs:\n  build:\n    steps:\n      - run: cargo build --release --bin ruff\n",
                ),
                (
                    ".github/workflows/build-docker.yml",
                    "name: Build docker\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
                ),
                (
                    ".github/workflows/build-wasm.yml",
                    "name: Build wasm\njobs:\n  build:\n    steps:\n      - run: cargo build --target wasm32-unknown-unknown\n",
                ),
                (
                    ".github/workflows/publish-playground.yml",
                    "name: Publish playground\njobs:\n  publish:\n    steps:\n      - run: cargo run --bin playground\n",
                ),
                (
                    ".github/workflows/publish-ty-playground.yml",
                    "name: Publish ty playground\njobs:\n  publish:\n    steps:\n      - run: cargo run --bin ty-playground\n",
                ),
                (
                    ".github/workflows/release.yml",
                    "name: Release\njobs:\n  release:\n    steps:\n      - run: cargo build --release\n",
                ),
                (
                    ".github/workflows/publish-docs.yml",
                    "name: Publish docs\njobs:\n  publish:\n    steps:\n      - run: cargo doc --no-deps\n",
                ),
                (
                    ".github/workflows/publish-mirror.yml",
                    "name: Publish mirror\njobs:\n  publish:\n    steps:\n      - run: echo mirror\n",
                ),
                (
                    ".github/workflows/publish-pypi.yml",
                    "name: Publish pypi\njobs:\n  publish:\n    steps:\n      - run: maturin publish\n",
                ),
                (
                    ".github/workflows/publish-versions.yml",
                    "name: Publish versions\njobs:\n  publish:\n    steps:\n      - run: cargo metadata --format-version 1\n",
                ),
                (
                    ".github/workflows/publish-wasm.yml",
                    "name: Publish wasm\njobs:\n  publish:\n    steps:\n      - run: wasm-pack build\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap build flow command runner main".to_owned(),
                limit: 11,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.iter().take(11).any(|path| {
                matches!(
                    *path,
                    "crates/ruff/src/main.rs"
                        | "crates/ruff_dev/src/main.rs"
                        | "crates/ruff_python_formatter/src/main.rs"
                        | "crates/ty/src/main.rs"
                        | "crates/ty_completion_bench/src/main.rs"
                )
            }),
            "a runtime entrypoint witness should remain visible under workflow crowding: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(11)
                .any(|path| path.starts_with(".github/workflows/")),
            "workflow witnesses should remain visible for entrypoint build-flow queries: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_entrypoint_build_flow_queries_recover_bat_build_config_witnesses()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-bat-build-config-witnesses");
        prepare_workspace(
            &root,
            &[
                (
                    "Cargo.toml",
                    "[package]\nname = \"bat\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
                ),
                ("rustfmt.toml", "edition = \"2021\"\n"),
                (
                    ".github/workflows/CICD.yml",
                    "name: CICD\njobs:\n  build:\n    steps:\n      - run: cargo build --locked\n",
                ),
                (
                    ".github/workflows/require-changelog-for-PRs.yml",
                    "name: Require changelog\njobs:\n  check:\n    steps:\n      - run: ./tests/scripts/license-checks.sh\n",
                ),
                (
                    "src/lib.rs",
                    "pub fn run() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
                ),
                (
                    "src/bin/bat/main.rs",
                    "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
                ),
                ("src/bin/bat/app.rs", "pub fn build_app() {}\n"),
                ("src/bin/bat/assets.rs", "pub fn build_assets() {}\n"),
                ("src/bin/bat/clap_app.rs", "pub fn clap_app() {}\n"),
                (
                    "src/bin/bat/completions.rs",
                    "pub fn generate_completions() {}\n",
                ),
                ("src/bin/bat/config.rs", "pub fn load_bat_config() {}\n"),
                ("src/config.rs", "pub struct RuntimeConfig;\n"),
                ("tests/scripts/license-checks.sh", "#!/bin/sh\necho check\n"),
                (
                    "tests/examples/system_config/bat/config",
                    "--theme=\"TwoDark\"\n",
                ),
                (
                    "tests/syntax-tests/highlighted/Elixir/command.ex",
                    "defmodule Command do\nend\n",
                ),
                (
                    "tests/syntax-tests/highlighted/Go/main.go",
                    "package main\nfunc main() {}\n",
                ),
                (
                    "tests/syntax-tests/source/Elixir/command.ex",
                    "defmodule Command do\nend\n",
                ),
                (
                    "tests/syntax-tests/source/Go/main.go",
                    "package main\nfunc main() {}\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap build flow command runner main config cargo github workflow cicd require changelog".to_owned(),
                limit: 11,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.iter().take(11).any(|path| {
                matches!(
                    *path,
                    "Cargo.toml"
                        | ".github/workflows/CICD.yml"
                        | ".github/workflows/require-changelog-for-PRs.yml"
                )
            }),
            "build-config entrypoint queries should recover a Cargo/workflow witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_go_entrypoint_queries_surface_cmd_command_packages() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-go-cmd-entrypoint-witnesses");
        prepare_workspace(
            &root,
            &[
                ("cmd/frpc/main.go", "package main\nfunc main() {}\n"),
                ("cmd/frps/main.go", "package main\nfunc main() {}\n"),
                ("cmd/frps/root.go", "package frps\nfunc Execute() {}\n"),
                ("cmd/frps/verify.go", "package frps\nfunc Verify() {}\n"),
                ("cmd/frpc/sub/admin.go", "package sub\nfunc Admin() {}\n"),
                (
                    "cmd/frpc/sub/nathole.go",
                    "package sub\nfunc NatHole() {}\n",
                ),
                ("cmd/frpc/sub/proxy.go", "package sub\nfunc Proxy() {}\n"),
                ("cmd/frpc/sub/root.go", "package sub\nfunc Root() {}\n"),
                (
                    ".github/workflows/build-and-push-image.yml",
                    "name: build and push\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
                ),
                (
                    "pkg/config/legacy/server.go",
                    "package legacy\nfunc Server() {}\n",
                ),
                (
                    "pkg/config/v1/validation/server.go",
                    "package validation\nfunc Server() {}\n",
                ),
                (
                    "pkg/metrics/mem/server.go",
                    "package mem\nfunc Server() {}\n",
                ),
                (
                    "web/frpc/src/main.ts",
                    "export const mount = 'frontend main';\n",
                ),
                (
                    "web/frps/src/main.ts",
                    "export const mount = 'frontend main';\n",
                ),
                (
                    "web/frps/src/api/server.ts",
                    "export const api = 'server';\n",
                ),
                (
                    "web/frps/src/types/server.ts",
                    "export const server = 'type';\n",
                ),
                (
                    "test/e2e/mock/server/httpserver/server.go",
                    "package httpserver\nfunc Server() {}\n",
                ),
                (
                    "test/e2e/mock/server/streamserver/server.go",
                    "package streamserver\nfunc Server() {}\n",
                ),
                (
                    "test/e2e/legacy/basic/server.go",
                    "package basic\nfunc Server() {}\n",
                ),
                (
                    "test/e2e/legacy/plugin/server.go",
                    "package plugin\nfunc Server() {}\n",
                ),
                (
                    "test/e2e/v1/basic/server.go",
                    "package basic\nfunc Server() {}\n",
                ),
                (
                    "test/e2e/v1/plugin/server.go",
                    "package plugin\nfunc Server() {}\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap server api main cli command".to_owned(),
                limit: 14,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths
                .iter()
                .take(14)
                .any(|path| path.starts_with("cmd/")),
            "go entrypoint queries should recover a cmd/ command witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_roc_entrypoint_queries_prefer_platform_main_over_host_crates_noise()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-roc-platform-entrypoints");
        prepare_workspace(
            &root,
            &[
                (
                    "platform/main.roc",
                    "# entry point main app package platform runtime\nplatform \"cli\"\npackages {}\nprovides [main_for_host!]\n",
                ),
                ("platform/Arg.roc", "# platform arg runtime package\n"),
                ("platform/Cmd.roc", "# platform cmd runtime package\n"),
                ("platform/Host.roc", "# platform host runtime package\n"),
                (
                    "examples/command.roc",
                    "# example command package\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
                ),
                (
                    "crates/roc_host_bin/src/main.rs",
                    "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
                ),
                (
                    "crates/roc_host/src/lib.rs",
                    "pub fn host_runtime() { let _ = \"main app package runtime\"; }\n",
                ),
                (
                    "ci/rust_http_server/src/main.rs",
                    "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
                ),
                (
                    ".github/workflows/deploy-docs.yml",
                    "name: deploy docs\njobs:\n  deploy:\n    steps:\n      - run: cargo doc\n",
                ),
                (
                    ".github/workflows/test_latest_release.yml",
                    "name: test latest release\njobs:\n  test:\n    steps:\n      - run: cargo test\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point main app package platform runtime".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        let platform_main_rank = ranked_paths
            .iter()
            .position(|path| *path == "platform/main.roc")
            .expect("platform/main.roc should be ranked for Roc entrypoint queries");
        let host_lib_rank = ranked_paths
            .iter()
            .position(|path| *path == "crates/roc_host/src/lib.rs")
            .expect("host runtime lib.rs should be ranked as competing noise");

        assert!(
            platform_main_rank < 6,
            "platform/main.roc should stay visible near the top for Roc platform queries: {ranked_paths:?}"
        );
        assert!(
            platform_main_rank < host_lib_rank,
            "platform/main.roc should outrank generic host runtime library noise for Roc platform queries: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_go_package_queries_surface_pkg_test_witnesses() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-go-package-witnesses");
        prepare_workspace(
            &root,
            &[
                (
                    "client/http/controller.go",
                    "package http\nfunc Controller() {}\n",
                ),
                (
                    "client/http/controller_test.go",
                    "package http\nfunc TestController() {}\n",
                ),
                (
                    "client/config_manager.go",
                    "package client\nfunc ConfigManager() {}\n",
                ),
                (
                    "client/config_manager_test.go",
                    "package client\nfunc TestConfigManager() {}\n",
                ),
                (
                    "client/proxy/proxy_manager.go",
                    "package proxy\nfunc Manager() {}\n",
                ),
                (
                    "client/visitor/visitor_manager.go",
                    "package visitor\nfunc Manager() {}\n",
                ),
                (
                    "pkg/config/source/aggregator_test.go",
                    "package source\nfunc TestAggregator() {}\n",
                ),
                (
                    "pkg/config/source/base_source_test.go",
                    "package source\nfunc TestBaseSource() {}\n",
                ),
                (
                    "pkg/config/source/config_source_test.go",
                    "package source\nfunc TestConfigSource() {}\n",
                ),
                (
                    "pkg/auth/oidc_test.go",
                    "package auth\nfunc TestOIDC() {}\n",
                ),
                (
                    "pkg/config/load_test.go",
                    "package config\nfunc TestLoad() {}\n",
                ),
                (
                    "pkg/config/source/aggregator.go",
                    "package source\nfunc NewAggregator() {}\n",
                ),
                (
                    "pkg/config/source/base_source.go",
                    "package source\nfunc NewBaseSource() {}\n",
                ),
                (
                    "pkg/config/source/clone.go",
                    "package source\nfunc Clone() {}\n",
                ),
                ("pkg/config/flags.go", "package config\nfunc Flags() {}\n"),
                ("go.mod", "module github.com/example/frp\n"),
                ("go.sum", "github.com/example/dependency v1.0.0 h1:test\n"),
                ("web/frpc/tsconfig.json", "{ \"compilerOptions\": {} }\n"),
                ("web/frps/tsconfig.json", "{ \"compilerOptions\": {} }\n"),
                (
                    "web/frpc/src/main.ts",
                    "export const mount = 'frontend main';\n",
                ),
                (
                    "web/frps/src/main.ts",
                    "export const mount = 'frontend main';\n",
                ),
                ("package.sh", "#!/bin/sh\necho package\n"),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests packages internal library integration config manager controller"
                    .to_owned(),
                limit: 14,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.iter().take(14).any(|path| {
                matches!(
                    *path,
                    "pkg/config/source/aggregator_test.go"
                        | "pkg/config/source/base_source_test.go"
                        | "pkg/config/source/config_source_test.go"
                        | "pkg/auth/oidc_test.go"
                        | "pkg/config/load_test.go"
                        | "pkg/config/source/aggregator.go"
                        | "pkg/config/source/base_source.go"
                        | "pkg/config/source/clone.go"
                )
            }),
            "go package/test queries should recover a pkg/ witness in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_typescript_entrypoint_queries_keep_cli_entrypoints_visible_under_workflow_noise()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-typescript-entrypoints-vs-workflow-noise");
        prepare_workspace(
            &root,
            &[
                (
                    "packages/cli/src/server.ts",
                    "export function startServer() { return \"bootstrap server app\"; }\n",
                ),
                (
                    "packages/cli/src/index.ts",
                    "export { startServer } from \"./server\";\n",
                ),
                (
                    "packages/@n8n/node-cli/src/index.ts",
                    "export const runCli = \"cli bootstrap app\";\n",
                ),
                (
                    "packages/frontend/editor-ui/src/main.ts",
                    "export const mount = \"frontend browser app\";\n",
                ),
                (
                    "packages/@n8n/task-runner-python/src/main.py",
                    "ENTRYPOINT = 'entry point bootstrap server app cli router main'\n",
                ),
                (
                    "packages/@n8n/nodes-langchain/nodes/vendors/Anthropic/actions/router.ts",
                    "export const router = 'entry point bootstrap server app cli router main';\n",
                ),
                (
                    "packages/testing/playwright/tests/e2e/building-blocks/workflow-entry-points.spec.ts",
                    "test('entry point bootstrap server app cli router main');\n",
                ),
                (
                    "packages/testing/playwright/tests/e2e/capabilities/proxy-server.spec.ts",
                    "test('entry point bootstrap server app cli router main');\n",
                ),
                (
                    ".github/workflows/build-windows.yml",
                    "name: Build windows\njobs:\n  build:\n    steps:\n      - run: pnpm build\n",
                ),
                (
                    ".github/workflows/docker-build-push.yml",
                    "name: Docker build push\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
                ),
                (
                    ".github/workflows/docker-build-smoke.yml",
                    "name: Docker build smoke\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
                ),
                (
                    ".github/workflows/release-create-pr.yml",
                    "name: Release create pr\njobs:\n  release:\n    steps:\n      - run: pnpm release\n",
                ),
                (
                    ".github/workflows/release-merge-tag-to-branch.yml",
                    "name: Release merge tag to branch\njobs:\n  release:\n    steps:\n      - run: pnpm release\n",
                ),
                (
                    ".github/workflows/sec-publish-fix.yml",
                    "name: Security publish fix\njobs:\n  publish:\n    steps:\n      - run: pnpm publish\n",
                ),
                (
                    ".github/workflows/create-patch-release-branch.yml",
                    "name: Create patch release branch\njobs:\n  release:\n    steps:\n      - run: pnpm release\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap server app cli router main".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.iter().take(10).any(|path| {
                matches!(
                    *path,
                    "packages/cli/src/server.ts"
                        | "packages/cli/src/index.ts"
                        | "packages/@n8n/node-cli/src/index.ts"
                )
            }),
            "typescript runtime entrypoints should remain visible under workflow/test crowding: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_runtime_config_queries_keep_typescript_runtime_entrypoints_visible_under_test_noise()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-typescript-config-vs-test-noise");
        prepare_workspace(
            &root,
            &[
                ("package.json", "{ \"name\": \"supabase\" }\n"),
                (
                    "tsconfig.json",
                    "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
                ),
                (
                    ".github/workflows/ai-tests.yml",
                    "name: AI Unit Tests\njobs:\n  test:\n    steps:\n      - run: pnpm run test\n",
                ),
                (
                    "packages/ai-commands/src/sql/index.ts",
                    "export * from './functions'\n",
                ),
                (
                    "packages/pg-meta/src/index.ts",
                    "export { config } from './pg-meta-config'\n",
                ),
                (
                    "packages/pg-meta/test/config.test.ts",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "packages/pg-meta/test/functions.test.ts",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "packages/ai-commands/test/extensions.ts",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "packages/ai-commands/test/sql-util.ts",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "apps/studio/tests/config/router.test.tsx",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "apps/studio/tests/config/router.tsx",
                    "export const router = 'config package tsconfig github workflow ai tests';\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "config package tsconfig github workflow ai tests".to_owned(),
                limit: 14,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths
                .iter()
                .take(14)
                .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
            "runtime-config queries should keep a config artifact visible in top-k: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.iter().take(14).any(|path| {
                matches!(
                    *path,
                    "packages/ai-commands/src/sql/index.ts" | "packages/pg-meta/src/index.ts"
                )
            }),
            "runtime-config queries should still surface a runtime entrypoint sibling in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_entrypoint_queries_recover_typescript_config_artifacts_without_explicit_config_terms()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-typescript-entrypoints-with-config-siblings");
        prepare_workspace(
            &root,
            &[
                ("package.json", "{ \"name\": \"supabase\" }\n"),
                (
                    "tsconfig.json",
                    "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
                ),
                (
                    "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                    "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
                ),
                (
                    "packages/build-icons/src/main.mjs",
                    "export const build = 'entry point bootstrap server app cli router main';\n",
                ),
                (
                    "apps/studio/tests/config/router.tsx",
                    "export const router = 'entry point bootstrap server app cli router main';\n",
                ),
                (
                    ".github/workflows/braintrust-preview-scorers-deploy.yml",
                    "name: Deploy preview scorers\njobs:\n  deploy:\n    steps:\n      - run: pnpm deploy\n",
                ),
                (
                    ".github/workflows/publish_image.yml",
                    "name: Publish image\njobs:\n  publish:\n    steps:\n      - run: docker build .\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap server app cli router main".to_owned(),
                limit: 14,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.iter().take(14).any(|path| {
                *path
                    == "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
            }),
            "entrypoint queries should still surface the runtime entrypoint witness in top-k: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(14)
                .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
            "entrypoint queries should recover a config artifact sibling in top-k: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_typescript_config_queries_keep_root_manifests_and_runtime_entrypoints_visible()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-typescript-config-vs-test-crowding");
        prepare_workspace(
            &root,
            &[
                (
                    "package.json",
                    "{\n  \"scripts\": {\n    \"test:ui\": \"pnpm turbo run test --filter=ui\",\n    \"authorize-vercel-deploys\": \"tsx scripts/authorizeVercelDeploys.ts\"\n  }\n}\n",
                ),
                (
                    "tsconfig.json",
                    "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
                ),
                (
                    "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                    "export function createServerClient() { return \"supabase server\"; }\n",
                ),
                (
                    "apps/docs/generator/cli.ts",
                    "export async function runCli() { return \"docs cli\"; }\n",
                ),
                (
                    ".github/workflows/ai-tests.yml",
                    "name: AI tests\njobs:\n  test:\n    steps:\n      - run: pnpm test:ui\n",
                ),
                (
                    ".github/workflows/authorize-vercel-deploys.yml",
                    "name: Authorize vercel deploys\njobs:\n  release:\n    steps:\n      - run: pnpm authorize-vercel-deploys\n",
                ),
                (
                    "packages/pg-meta/test/config.test.ts",
                    "describe('config', () => test('package tsconfig github workflow ai tests', () => {}));\n",
                ),
                (
                    "packages/pg-meta/test/sql/studio/get-users-common.test.ts",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "apps/studio/tests/config/router.test.tsx",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "apps/studio/tests/config/router.tsx",
                    "export const router = 'config package tsconfig github workflow ai tests';\n",
                ),
                (
                    "apps/studio/tests/config/msw.test.ts",
                    "test('config package tsconfig github workflow ai tests');\n",
                ),
                (
                    "packages/ai-commands/test/extensions.ts",
                    "export const extensionTest = 'config package tsconfig github workflow ai tests';\n",
                ),
                (
                    "packages/ai-commands/test/sql-util.ts",
                    "export const sqlUtilTest = 'config package tsconfig github workflow ai tests';\n",
                ),
                (
                    "packages/build-icons/src/main.mjs",
                    "export const main = 'entry point bootstrap server app cli router main';\n",
                ),
                (
                    "examples/ai/image_search/image_search/main.py",
                    "ENTRYPOINT = 'entry point bootstrap server app cli router main'\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "config package tsconfig github workflow ai tests".to_owned(),
                limit: 14,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths
                .iter()
                .take(14)
                .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
            "typescript config queries should keep a root manifest visible under config-test crowding: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.iter().take(14).any(|path| {
                matches!(
                    *path,
                    "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
                        | "apps/docs/generator/cli.ts"
                )
            }),
            "typescript config queries should keep a runtime entrypoint visible under config-test crowding: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_typescript_entrypoint_queries_keep_root_manifests_visible_under_test_crowding()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-typescript-entrypoints-vs-config-tests");
        prepare_workspace(
            &root,
            &[
                (
                    "package.json",
                    "{\n  \"scripts\": {\n    \"build\": \"turbo run build\",\n    \"test:ui\": \"turbo run test --filter=ui\"\n  }\n}\n",
                ),
                (
                    "tsconfig.json",
                    "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
                ),
                (
                    "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                    "export function createServerClient() { return \"supabase server\"; }\n",
                ),
                (
                    "apps/docs/generator/cli.ts",
                    "export async function runCli() { return \"docs cli\"; }\n",
                ),
                (
                    "packages/build-icons/src/main.mjs",
                    "export const main = 'entry point bootstrap server app cli router main';\n",
                ),
                (
                    "apps/studio/tests/config/router.tsx",
                    "export const router = 'entry point bootstrap server app cli router main';\n",
                ),
                (
                    "apps/studio/tests/config/router.test.tsx",
                    "test('entry point bootstrap server app cli router main');\n",
                ),
                (
                    "packages/pg-meta/test/db/server.crt",
                    "entry point bootstrap server app cli router main\n",
                ),
                (
                    "packages/pg-meta/test/db/server.key",
                    "entry point bootstrap server app cli router main\n",
                ),
                (
                    "examples/ai/image_search/image_search/main.py",
                    "ENTRYPOINT = 'entry point bootstrap server app cli router main'\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap server app cli router main".to_owned(),
                limit: 14,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.iter().take(14).any(|path| {
                matches!(
                    *path,
                    "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
                        | "apps/docs/generator/cli.ts"
                )
            }),
            "typescript entrypoint queries should keep a runtime entrypoint visible: {ranked_paths:?}"
        );
        assert!(
            ranked_paths
                .iter()
                .take(14)
                .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
            "typescript entrypoint queries should keep a root manifest visible under test crowding: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_entrypoint_queries_surface_build_workflow_configs_with_semantic_runtime()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-rust-entrypoint-build-workflows-semantic");
        prepare_workspace(
            &root,
            &[
                (
                    "src-tauri/src/main.rs",
                    "fn main() {\n\
                     // entry point bootstrap build flow command runner main config\n\
                     let config = load_config();\n\
                     run_build_flow(config);\n\
                     }\n",
                ),
                (
                    "src-tauri/src/proxy/config.rs",
                    "pub struct ProxyConfig;\n// entry point bootstrap build flow command runner main config\n",
                ),
                (
                    "src-tauri/src/lib.rs",
                    "pub fn run() {\n// entry point bootstrap build flow command runner main config\n}\n",
                ),
                (
                    "src-tauri/src/modules/config.rs",
                    "pub struct ModuleConfig;\n// entry point bootstrap build flow command runner main config\n",
                ),
                (
                    "src-tauri/src/models/config.rs",
                    "pub struct ModelConfig;\n// entry point bootstrap build flow command runner main config\n",
                ),
                (
                    "src-tauri/src/proxy/proxy_pool.rs",
                    "pub struct ProxyPool;\n// entry point bootstrap build flow command runner main config\n",
                ),
                (
                    "src-tauri/build.rs",
                    "fn main() {\n tauri_build::build();\n}\n",
                ),
                (
                    "src-tauri/src/commands/security.rs",
                    "pub fn security_command() {\n// entry point bootstrap build flow command runner main config\n}\n",
                ),
                (
                    ".github/workflows/deploy-pages.yml",
                    "name: Deploy static content to Pages\n\
                     jobs:\n\
                       deploy:\n\
                         steps:\n\
                           - name: Upload artifact\n\
                             run: echo upload build artifacts\n\
                           - name: Deploy to GitHub Pages\n\
                             run: echo deploy release pages\n",
                ),
                (
                    ".github/workflows/release.yml",
                    "name: Release\n\
                     jobs:\n\
                       build-tauri:\n\
                         steps:\n\
                           - name: Build the app\n\
                             run: cargo build --release\n\
                           - name: Publish release artifacts\n\
                             run: echo publish release artifacts\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/src/main.rs",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/src/proxy/config.rs",
                    0,
                    vec![0.99, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/src/lib.rs",
                    0,
                    vec![0.98, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/src/modules/config.rs",
                    0,
                    vec![0.97, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/src/models/config.rs",
                    0,
                    vec![0.96, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/src/proxy/proxy_pool.rs",
                    0,
                    vec![0.95, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/build.rs",
                    0,
                    vec![0.94, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src-tauri/src/commands/security.rs",
                    0,
                    vec![0.93, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    ".github/workflows/release.yml",
                    0,
                    vec![0.82, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    ".github/workflows/deploy-pages.yml",
                    0,
                    vec![0.81, 0.0],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        config.max_search_results = 8;
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap build flow command runner main config".to_owned(),
                limit: 8,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);

        let ranked_paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            ranked_paths.iter().take(8).any(|path| {
                matches!(
                    *path,
                    ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
                )
            }),
            "entrypoint/build-flow queries should keep a workflow config witness visible even under semantic runtime pressure: {ranked_paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_lexical_recall_tokens_preserve_signal_order_for_tool_surface_queries() {
        let tokens = hybrid_lexical_recall_tokens(
            "which MCP tools are core versus extended and where is tool surface gating enforced in runtime docs and tests",
        );

        assert_eq!(
            tokens,
            vec![
                "tools", "core", "versus", "extended", "tool", "surface", "gating", "enforced",
                "runtime", "docs", "tests",
            ]
        );
    }

    #[test]
    fn hybrid_ranking_query_aware_lexical_hits_promote_tool_contract_docs_over_generic_readmes()
    -> FriggResult<()> {
        let query = "which MCP tools are core versus extended and where is tool surface gating enforced in runtime docs and tests";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "README.md",
                    1,
                    1,
                    "FRIGG_MCP_TOOL_SURFACE_PROFILE core extended tools list",
                ),
                text_match(
                    "repo-001",
                    "contracts/tools/v1/README.md",
                    1,
                    1,
                    "tool surface profile core extended_only tools/list",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/mcp/tool_surface.rs",
                    1,
                    1,
                    "ToolSurfaceProfile::Core ToolSurfaceProfile::Extended",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/tests/tool_surface_parity.rs",
                    1,
                    1,
                    "runtime_tool_surface_parity",
                ),
            ],
            query,
        );
        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            4,
            query,
        )?;

        assert!(
            ranked[0].document.path == "contracts/tools/v1/README.md"
                || ranked[1].document.path == "contracts/tools/v1/README.md",
            "tool contract docs should land at the top of the ranked set"
        );
        assert!(
            ranked
                .iter()
                .position(|entry| entry.document.path == "contracts/tools/v1/README.md")
                < ranked
                    .iter()
                    .position(|entry| entry.document.path == "README.md"),
            "tool contract docs should outrank the generic README for tool-surface queries"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_tool_surface_queries_prefer_mcp_runtime_surface_over_searcher_noise()
    -> FriggResult<()> {
        let query = "which MCP tools are core versus extended and where are tool surface types and runtime gating defined";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "contracts/tools/v1/README.md",
                    1,
                    1,
                    "tool surface profile core extended_only tools/list",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/mcp/tool_surface.rs",
                    1,
                    1,
                    "ToolSurfaceProfile::Core ToolSurfaceProfile::Extended",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/mcp/types.rs",
                    1,
                    1,
                    "tools/list tool metadata runtime response types",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/mcp/mod.rs",
                    1,
                    1,
                    "pub mod server pub mod types pub mod tool_surface",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/searcher/mod.rs",
                    1,
                    1,
                    "search_hybrid ranking intent tool surface docs",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/embeddings/mod.rs",
                    1,
                    1,
                    "embedding runtime provider",
                ),
            ],
            query,
        );
        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            6,
            query,
        )?;

        assert!(
            ranked
                .iter()
                .position(|entry| entry.document.path == "crates/cli/src/mcp/tool_surface.rs")
                < ranked
                    .iter()
                    .position(|entry| entry.document.path == "crates/cli/src/searcher/mod.rs"),
            "tool-surface runtime file should outrank searcher noise for MCP tool-surface queries"
        );
        assert!(
            ranked
                .iter()
                .position(|entry| entry.document.path == "crates/cli/src/mcp/types.rs")
                < ranked
                    .iter()
                    .position(|entry| entry.document.path == "crates/cli/src/embeddings/mod.rs"),
            "MCP runtime types should outrank unrelated embedding runtime files"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_mcp_http_startup_queries_prefer_http_runtime_entrypoint() -> FriggResult<()> {
        let query = "where does MCP HTTP startup happen and which runtime entrypoint wires the loopback HTTP server";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "crates/cli/src/main.rs",
                    1,
                    1,
                    "mcp http startup cli command wires runtime",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/http_runtime.rs",
                    1,
                    1,
                    "loopback http server startup runtime tool surface",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/embeddings/mod.rs",
                    1,
                    1,
                    "http client embedding provider runtime",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/searcher/mod.rs",
                    1,
                    1,
                    "runtime startup ranking path",
                ),
                text_match(
                    "repo-001",
                    "docs/overview.md",
                    1,
                    1,
                    "mcp http runtime overview",
                ),
            ],
            query,
        );
        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            5,
            query,
        )?;

        assert!(
            ranked[0].document.path == "crates/cli/src/http_runtime.rs"
                || ranked[1].document.path == "crates/cli/src/http_runtime.rs",
            "http_runtime.rs should land at the top for MCP HTTP startup queries"
        );
        assert!(
            ranked
                .iter()
                .position(|entry| entry.document.path == "crates/cli/src/http_runtime.rs")
                < ranked
                    .iter()
                    .position(|entry| entry.document.path == "crates/cli/src/embeddings/mod.rs"),
            "HTTP runtime entrypoint should outrank unrelated embedding runtime files"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_navigation_fallback_queries_promote_mcp_runtime_witnesses() -> FriggResult<()>
    {
        let query = "find EmbeddingProvider implementations and fallback when precise navigation data is missing";
        let ranked = rank_hybrid_evidence_for_query(
            &[
                hybrid_hit(
                    "repo-001",
                    "crates/cli/src/embeddings/mod.rs",
                    1.00,
                    "lex-impl-runtime",
                ),
                hybrid_hit(
                    "repo-001",
                    "crates/cli/src/searcher/mod.rs",
                    0.98,
                    "lex-searcher-runtime",
                ),
                hybrid_hit(
                    "repo-001",
                    "skills/frigg-mcp-search-navigation/references/navigation-fallbacks.md",
                    0.97,
                    "lex-nav-doc",
                ),
                hybrid_hit(
                    "repo-001",
                    "crates/cli/tests/tool_handlers.rs",
                    0.96,
                    "lex-tests",
                ),
                hybrid_hit(
                    "repo-001",
                    "contracts/tools/v1/README.md",
                    0.95,
                    "lex-tool-contract",
                ),
                hybrid_hit(
                    "repo-001",
                    "contracts/semantic.md",
                    0.94,
                    "lex-semantic-contract",
                ),
                hybrid_hit(
                    "repo-001",
                    "contracts/errors.md",
                    0.93,
                    "lex-error-contract",
                ),
                hybrid_hit(
                    "repo-001",
                    "crates/cli/src/indexer/mod.rs",
                    0.92,
                    "lex-indexer-runtime",
                ),
                hybrid_hit(
                    "repo-001",
                    "crates/cli/src/mcp/server.rs",
                    0.88,
                    "lex-mcp-server-runtime",
                ),
                hybrid_hit(
                    "repo-001",
                    "crates/cli/src/mcp/types.rs",
                    0.87,
                    "lex-mcp-types-runtime",
                ),
            ],
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            8,
            query,
        )?;

        let ranked_paths = ranked
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.contains(&"crates/cli/src/mcp/server.rs")
                || ranked_paths.contains(&"crates/cli/src/mcp/types.rs"),
            "navigation-fallback queries should surface at least one MCP runtime witness in top-k"
        );
        assert!(
            ranked
                .iter()
                .position(|entry| entry.document.path == "crates/cli/src/mcp/server.rs")
                < ranked.iter().position(|entry| {
                    entry.document.path
                        == "skills/frigg-mcp-search-navigation/references/navigation-fallbacks.md"
                }),
            "MCP runtime witness should outrank the secondary navigation reference doc"
        );

        Ok(())
    }

    #[test]
    fn hybrid_ranking_query_aware_lexical_hits_promote_benchmark_docs_for_replay_queries()
    -> FriggResult<()> {
        let query = "how does Frigg turn a multi-step suite playbook fixture into a deterministic trace artifact replay and citations";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "README.md",
                    1,
                    1,
                    "deterministic replay provenance auditing deep_search_replay",
                ),
                text_match(
                    "repo-001",
                    "benchmarks/deep-search.md",
                    1,
                    1,
                    "deterministic trace artifact replay citations playbook fixture benchmark",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/mcp/deep_search.rs",
                    1,
                    1,
                    "DeepSearchTraceArtifact deep_search_compose_citations",
                ),
            ],
            query,
        );
        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            3,
            query,
        )?;

        assert!(
            ranked
                .iter()
                .position(|entry| entry.document.path == "benchmarks/deep-search.md")
                < ranked
                    .iter()
                    .position(|entry| entry.document.path == "README.md"),
            "benchmark docs should outrank the generic README for replay/citation queries"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_query_aware_diversification_avoids_single_class_collapse() -> FriggResult<()>
    {
        let query = "trace invalid_params typed error from public docs to runtime helper and tests";
        let lexical = vec![
            hybrid_hit("repo-001", "crates/cli/src/a.rs", 1.00, "lex-runtime-a"),
            hybrid_hit("repo-001", "crates/cli/src/b.rs", 0.99, "lex-runtime-b"),
            hybrid_hit("repo-001", "contracts/errors.md", 0.98, "lex-docs"),
            hybrid_hit(
                "repo-001",
                "crates/cli/tests/tool_handlers.rs",
                0.97,
                "lex-tests",
            ),
        ];

        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            3,
            query,
        )?;
        let ranked_paths = ranked
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths
                .iter()
                .any(|path| matches!(hybrid_source_class(path), HybridSourceClass::Runtime)),
            "runtime witness should remain in top-k"
        );
        assert!(
            ranked_paths.contains(&"contracts/errors.md"),
            "docs witness should be promoted into top-k"
        );
        assert!(
            ranked_paths.contains(&"crates/cli/tests/tool_handlers.rs"),
            "test witness should be promoted into top-k"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_error_taxonomy_queries_prefer_exact_anchored_runtime_and_tests_over_auxiliary_noise()
    -> FriggResult<()> {
        let query =
            "invalid_params -32602 public error taxonomy docs contract runtime helper tests";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "docs/error-taxonomy.md",
                    1,
                    1,
                    "invalid_params maps to -32602",
                ),
                text_match(
                    "repo-001",
                    "src/runtime/jsonrpc/errors.rs",
                    1,
                    1,
                    "invalid_params runtime helper",
                ),
                text_match(
                    "repo-001",
                    "src/runtime/replay.rs",
                    1,
                    1,
                    "invalid_params replay helper",
                ),
                text_match(
                    "repo-001",
                    "tests/runtime_errors.rs",
                    1,
                    1,
                    "invalid_params tests coverage",
                ),
                text_match(
                    "repo-001",
                    "src/domain/error.rs",
                    1,
                    1,
                    "invalid_params internal domain error type",
                ),
                text_match(
                    "repo-001",
                    "src/main.rs",
                    1,
                    1,
                    "runtime helper tests invalid_params",
                ),
                text_match(
                    "repo-001",
                    "src/cli_runtime.rs",
                    1,
                    1,
                    "runtime helper tests invalid_params",
                ),
                text_match(
                    "repo-001",
                    "playbooks/error-contract-alignment.md",
                    1,
                    1,
                    "runtime helper tests invalid_params",
                ),
                text_match(
                    "repo-001",
                    "fixtures/scip/matrix-invalid-range.json",
                    1,
                    1,
                    "runtime helper tests invalid_params",
                ),
            ],
            query,
        );

        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            5,
            query,
        )?;
        let ranked_paths = ranked
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert!(
            ranked_paths.contains(&"src/runtime/jsonrpc/errors.rs")
                || ranked_paths.contains(&"src/runtime/replay.rs"),
            "exact-anchored runtime helpers should remain in top-k: {ranked_paths:?}"
        );
        assert!(
            ranked_paths.contains(&"tests/runtime_errors.rs"),
            "exact-anchored test witnesses should remain in top-k: {ranked_paths:?}"
        );
        assert!(
            !ranked_paths.contains(&"src/main.rs"),
            "generic runtime entrypoints should not outrank exact-anchored runtime helpers: {ranked_paths:?}"
        );
        assert!(
            !ranked_paths.contains(&"playbooks/error-contract-alignment.md"),
            "playbook self-reference should not outrank exact-anchored runtime witnesses: {ranked_paths:?}"
        );
        assert!(
            !ranked_paths.contains(&"fixtures/scip/matrix-invalid-range.json"),
            "fixtures should not outrank exact-anchored runtime witnesses: {ranked_paths:?}"
        );
        Ok(())
    }

    #[test]
    fn hybrid_ranking_shared_path_class_demotes_support_paths_under_crates_prefixes()
    -> FriggResult<()> {
        let query = "builder configuration";
        let lexical = build_hybrid_lexical_hits_for_query(
            &[
                text_match(
                    "repo-001",
                    "crates/cli/examples/server.rs",
                    1,
                    1,
                    "builder configuration builder configuration builder configuration",
                ),
                text_match(
                    "repo-001",
                    "crates/cli/src/builder.rs",
                    1,
                    1,
                    "builder configuration",
                ),
            ],
            query,
        );

        let ranked = rank_hybrid_evidence_for_query(
            &lexical,
            &[],
            &[],
            HybridChannelWeights {
                lexical: 1.0,
                graph: 0.0,
                semantic: 0.0,
            },
            2,
            query,
        )?;

        assert_eq!(ranked[0].document.path, "crates/cli/src/builder.rs");
        assert_eq!(ranked[1].document.path, "crates/cli/examples/server.rs");
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_channel_surfaces_docs_runtime_and_tests_witnesses() -> FriggResult<()>
    {
        let root = temp_workspace_root("hybrid-semantic-doc-runtime-tests");
        prepare_workspace(
            &root,
            &[
                (
                    "contracts/errors.md",
                    "invalid_params typed error public docs contract\n",
                ),
                (
                    "crates/cli/src/mcp/server.rs",
                    "invalid_params runtime helper\n",
                ),
                (
                    "crates/cli/tests/tool_handlers.rs",
                    "invalid_params tests coverage\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "contracts/errors.md",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "crates/cli/src/mcp/server.rs",
                    0,
                    vec![0.95, 0.05],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "crates/cli/tests/tool_handlers.rs",
                    0,
                    vec![0.90, 0.10],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "trace invalid_params typed error from public docs to runtime helper and tests"
                        .to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;
        let paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
        assert!(paths.contains(&"contracts/errors.md"));
        assert!(paths.contains(&"crates/cli/src/mcp/server.rs"));
        assert!(paths.contains(&"crates/cli/tests/tool_handlers.rs"));

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_ok_still_expands_lexical_recall_for_underfilled_queries()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-ok-lexical-recall");
        prepare_workspace(
            &root,
            &[
                (
                    "contracts/tools/v1/README.md",
                    "tool surface profile core extended_only tools/list contract\n",
                ),
                (
                    "crates/cli/src/mcp/tool_surface.rs",
                    "ToolSurfaceProfile::Core ToolSurfaceProfile::Extended runtime gating\n",
                ),
                (
                    "crates/cli/tests/tool_surface_parity.rs",
                    "runtime_tool_surface_parity tests\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "contracts/tools/v1/README.md",
                    0,
                    vec![0.0, 1.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "crates/cli/src/mcp/tool_surface.rs",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "crates/cli/tests/tool_surface_parity.rs",
                    0,
                    vec![0.95, 0.05],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "which MCP tools are core versus extended and where is tool surface gating enforced in runtime docs and tests"
                        .to_owned(),
                limit: 4,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;
        let paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
        assert_eq!(output.note.semantic_candidate_count, 3);
        assert_eq!(output.note.semantic_hit_count, 2);
        assert!(output.note.semantic_match_count >= 2);
        assert!(output.note.semantic_enabled);
        assert!(paths.contains(&"crates/cli/src/mcp/tool_surface.rs"));
        assert!(paths.contains(&"crates/cli/tests/tool_surface_parity.rs"));
        assert!(
            paths.contains(&"contracts/tools/v1/README.md"),
            "underfilled natural-language queries should still pull in tool-contract docs via lexical expansion when semantic retrieval is healthy; got {paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_hit_count_tracks_retained_documents_not_raw_chunks()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-retained-hit-count");
        prepare_workspace(
            &root,
            &[
                (
                    "src/relevant.rs",
                    "pub fn relevant() { let _ = \"needle\"; }\n",
                ),
                (
                    "src/secondary.rs",
                    "pub fn secondary() { let _ = \"needle\"; }\n",
                ),
                ("src/noisy.rs", "pub fn noisy() { let _ = \"needle\"; }\n"),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/relevant.rs",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/relevant.rs",
                    1,
                    vec![0.82, 0.02],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/secondary.rs",
                    0,
                    vec![0.69, 0.72],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/noisy.rs",
                    0,
                    vec![0.41, 0.91],
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
        assert_eq!(
            output.note.semantic_candidate_count, 4,
            "semantic_candidate_count should expose the broader raw semantic chunk pool"
        );
        assert_eq!(
            output.note.semantic_hit_count, 1,
            "semantic_hit_count should reflect retained semantic documents, not raw chunk count"
        );
        assert_eq!(output.matches[0].document.path, "src/relevant.rs");
        assert!(output.note.semantic_match_count >= 1);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_unavailable_without_corpus_still_expands_lexical_recall()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-unavailable-lexical-recall");
        prepare_workspace(
            &root,
            &[
                (
                    "src/config.rs",
                    "pub fn resolve_config_path() {\n\
                     let precedence = \"cli then env then file\";\n\
                     }\n",
                ),
                (
                    "src/main.rs",
                    "pub fn load_config() {\n\
                     let config_loaded = true;\n\
                     }\n",
                ),
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "Where is config loaded and what is the precedence?".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;
        let paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(
            output.note.semantic_status,
            HybridSemanticStatus::Unavailable
        );
        assert_eq!(output.note.semantic_hit_count, 0);
        assert_eq!(output.note.semantic_match_count, 0);
        assert!(!output.note.semantic_enabled);
        assert!(
            output
                .note
                .semantic_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("no semantic storage database")),
            "unavailable note should explain that no semantic storage database exists"
        );
        assert!(
            !output.matches.is_empty(),
            "semantic-unavailable hybrid search should still recover lexical matches when the semantic channel cannot run against a corpus"
        );
        assert!(paths.contains(&"src/config.rs"));
        assert!(paths.contains(&"src/main.rs"));

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_ok_empty_channel_when_active_index_is_filtered_out()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-ok-filtered-empty-channel");
        prepare_workspace(
            &root,
            &[("src/lib.rs", "pub fn rust_only() { let _ = \"needle\"; }\n")],
        )?;
        seed_manifest_snapshot(&root, "repo-001", "snapshot-001", &["src/lib.rs"])?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[semantic_record(
                "repo-001",
                "snapshot-001",
                "src/lib.rs",
                0,
                vec![1.0, 0.0],
            )],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters {
                repository_id: None,
                language: Some("php".to_owned()),
            },
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
        assert_eq!(output.note.semantic_hit_count, 0);
        assert_eq!(output.note.semantic_match_count, 0);
        assert!(!output.note.semantic_enabled);
        assert!(output.note.semantic_reason.is_none());
        assert!(
            output.matches.is_empty(),
            "language-filtered semantic search should be allowed to return an empty but healthy result set"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_channel_falls_back_to_older_snapshot_when_latest_manifest_lacks_embeddings()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-fallback-split-snapshot");
        prepare_workspace(
            &root,
            &[
                (
                    "src/current.rs",
                    "pub fn current() { let _ = \"semantic needle\"; }\n",
                ),
                (
                    "src/deleted.rs",
                    "pub fn deleted() { let _ = \"semantic needle\"; }\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/current.rs",
                    0,
                    vec![1.0, 0.0],
                ),
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/deleted.rs",
                    0,
                    vec![0.95, 0.05],
                ),
            ],
        )?;
        seed_manifest_snapshot(&root, "repo-001", "snapshot-002", &["src/current.rs"])?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "semantic needle".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;
        let paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Degraded);
        assert!(output.note.semantic_enabled);
        assert!(
            output
                .note
                .semantic_reason
                .as_deref()
                .is_some_and(
                    |reason| reason.contains("snapshot-002") && reason.contains("snapshot-001")
                ),
            "split-snapshot fallback should name both latest and fallback snapshots"
        );
        assert!(
            paths.contains(&"src/current.rs"),
            "current manifest path should remain visible under semantic fallback: {paths:?}"
        );
        assert!(
            !paths.contains(&"src/deleted.rs"),
            "paths removed from the latest manifest must not resurface from an older semantic snapshot: {paths:?}"
        );
        assert!(
            output.matches.iter().any(|entry| {
                entry.document.path == "src/current.rs" && entry.semantic_score > 0.0
            }),
            "fallback semantic snapshot should still contribute non-zero semantic score"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_disabled_expands_lexical_recall_for_multi_token_queries()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-disabled-lexical-recall");
        prepare_workspace(
            &root,
            &[
                (
                    "playbooks/hybrid-search-context-retrieval.md",
                    "semantic runtime strict failure note metadata\n",
                ),
                (
                    "src/lib.rs",
                    "pub fn strict_failure_note() {\n\
                     let semantic_status = \"strict_failure\";\n\
                     let semantic_reason = \"runtime metadata\";\n\
                     }\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "semantic runtime strict failure note metadata".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
        assert!(
            output
                .matches
                .iter()
                .any(|entry| entry.document.path == "src/lib.rs"),
            "tokenized lexical recall should include source evidence even when phrase-literal match is doc-only"
        );
        assert_eq!(
            output.matches[0].document.path, "src/lib.rs",
            "source evidence should outrank playbook self-reference in lexical-only fallback mode"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_disabled_literal_floor_recovers_snake_case_only_matches()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-disabled-literal-floor");
        prepare_workspace(
            &root,
            &[(
                "src/lib.rs",
                "pub fn strict_failure_note() {\n\
                 let semantic_status = \"strict_failure\";\n\
                 }\n",
            )],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "strict semantic failure".to_owned(),
                limit: 5,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
        assert!(
            !output.matches.is_empty(),
            "token literal floor should avoid empty degraded hybrid responses"
        );
        assert_eq!(output.matches[0].document.path, "src/lib.rs");

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_large_top_k_laravel_witness_queries_do_not_panic() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-large-topk-laravel-witness");
        prepare_workspace(
            &root,
            &[
                ("tests/CreatesApplication.php", "<?php\n"),
                ("tests/DuskTestCase.php", "<?php\n"),
                (
                    "resources/views/auth/confirm-password.blade.php",
                    "<div>confirm password</div>\n",
                ),
                (
                    "resources/views/components/applications/advanced.blade.php",
                    "<div>advanced</div>\n",
                ),
                ("app/Livewire/ActivityMonitor.php", "<?php\n"),
                ("app/Livewire/Dashboard.php", "<?php\n"),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "tests fixtures integration creates application dusk case resources views auth confirm auth forgot view components app livewire activity monitor dashboard".to_owned(),
                limit: 200,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials::default(),
            &PanicSemanticQueryEmbeddingExecutor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
        assert!(
            !output.matches.is_empty(),
            "large lexical top-k witness queries should still return results instead of panicking"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_blends_lexical_graph_and_semantic_channels() -> FriggResult<()> {
        let lexical = vec![
            hybrid_hit("repo-001", "src/a.rs", 10.0, "lex-a"),
            hybrid_hit("repo-001", "src/b.rs", 8.0, "lex-b"),
        ];
        let graph = vec![
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::GraphPrecise,
                "repo-001",
                "src/b.rs",
                5.0,
                "graph-b",
            ),
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::GraphPrecise,
                "repo-001",
                "src/c.rs",
                4.0,
                "graph-c",
            ),
        ];
        let semantic = vec![
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::Semantic,
                "repo-001",
                "src/c.rs",
                0.9,
                "sem-c",
            ),
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::Semantic,
                "repo-001",
                "src/a.rs",
                0.2,
                "sem-a",
            ),
        ];

        let ranked = rank_hybrid_evidence(
            &lexical,
            &graph,
            &semantic,
            HybridChannelWeights::default(),
            10,
        )?;
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].document.path, "src/b.rs");
        assert_eq!(ranked[1].document.path, "src/a.rs");
        assert_eq!(ranked[2].document.path, "src/c.rs");
        assert_eq!(ranked[0].lexical_sources, vec!["lex-b".to_owned()]);
        assert_eq!(ranked[0].graph_sources, vec!["graph-b".to_owned()]);
        assert_eq!(ranked[2].semantic_sources, vec!["sem-c".to_owned()]);

        Ok(())
    }

    #[test]
    fn graph_channel_falls_back_to_exact_stem_candidates_when_lexical_paths_have_no_symbols()
    -> FriggResult<()> {
        let root = temp_workspace_root("graph-channel-fallback-exact-stem");
        prepare_workspace(
            &root,
            &[
                (
                    "src/Handlers/OrderHandler.php",
                    "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
                ),
                (
                    "src/Listeners/OrderListener.php",
                    "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
                ),
                (
                    "docs/handlers.md",
                    "# Handlers\nOrderHandler handle listener overview.\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let normalized_filters = normalize_search_filters(SearchFilters::default())?;
        let candidate_universe = searcher.build_candidate_universe(
            &SearchTextQuery {
                query: String::new(),
                path_regex: None,
                limit: 5,
            },
            &normalized_filters,
        );
        let hits = super::graph_channel::search_graph_channel_hits(
            &searcher,
            "OrderHandler handle listener",
            &candidate_universe,
            &[TextMatch {
                repository_id: "repo-001".to_owned(),
                path: "docs/handlers.md".to_owned(),
                line: 1,
                column: 1,
                excerpt: "OrderHandler handle listener overview".to_owned(),
            }],
            5,
        )?;

        assert!(
            hits.iter()
                .any(|hit| hit.document.path == "src/Handlers/OrderHandler.php"),
            "graph fallback should recover the handler anchor from exact-stem candidates: {hits:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_graph_queries_reuse_snapshot_scoped_graph_artifacts() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-graph-artifact-cache-reuse");
        prepare_workspace(
            &root,
            &[
                (
                    "src/Handlers/OrderHandler.php",
                    "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
                ),
                (
                    "src/Listeners/OrderListener.php",
                    "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
                ),
                (
                    "docs/handlers.md",
                    "# Handlers\nOrder handler listener overview.\n",
                ),
            ],
        )?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                "src/Handlers/OrderHandler.php",
                "src/Listeners/OrderListener.php",
                "docs/handlers.md",
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        assert_eq!(
            searcher
                .hybrid_graph_artifact_cache
                .read()
                .expect("hybrid graph artifact cache should not be poisoned")
                .len(),
            0
        );

        let first = searcher.search_hybrid(SearchHybridQuery {
            query: "OrderHandler handle listener".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        })?;
        assert!(
            first
                .matches
                .iter()
                .any(|entry| entry.document.path == "src/Listeners/OrderListener.php"),
            "initial graph query should surface listener evidence: {:?}",
            first.matches
        );
        assert_eq!(
            searcher
                .hybrid_graph_artifact_cache
                .read()
                .expect("hybrid graph artifact cache should not be poisoned")
                .len(),
            1
        );

        let second = searcher.search_hybrid(SearchHybridQuery {
            query: "OrderHandler handle listener".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        })?;
        assert_eq!(first.matches, second.matches);
        assert_eq!(
            searcher
                .hybrid_graph_artifact_cache
                .read()
                .expect("hybrid graph artifact cache should not be poisoned")
                .len(),
            1
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_graph_artifact_cache_rebuilds_after_snapshot_change() -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-graph-artifact-cache-snapshot-change");
        prepare_workspace(
            &root,
            &[
                (
                    "src/Handlers/OrderHandler.php",
                    "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
                ),
                (
                    "src/Listeners/OrderListener.php",
                    "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
                ),
                (
                    "docs/handlers.md",
                    "# Handlers\nOrder handler listener overview.\n",
                ),
            ],
        )?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                "src/Handlers/OrderHandler.php",
                "src/Listeners/OrderListener.php",
                "docs/handlers.md",
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let first = searcher.search_hybrid(SearchHybridQuery {
            query: "OrderHandler handle listener".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        })?;
        assert!(
            first
                .matches
                .iter()
                .any(|entry| entry.document.path == "src/Listeners/OrderListener.php"),
            "baseline graph query should surface listener evidence: {:?}",
            first.matches
        );
        assert_eq!(
            searcher
                .hybrid_graph_artifact_cache
                .read()
                .expect("hybrid graph artifact cache should not be poisoned")
                .len(),
            1
        );

        prepare_workspace(
            &root,
            &[
                (
                    "src/Handlers/PaymentHandler.php",
                    "<?php\n\
                     namespace App\\Handlers;\n\
                     class PaymentHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
                ),
                (
                    "src/Listeners/PaymentListener.php",
                    "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\PaymentHandler;\n\
                     class PaymentListener {\n\
                         public function handlers(): array {\n\
                             return [[PaymentHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
                ),
            ],
        )?;
        seed_manifest_snapshot(
            &root,
            "repo-001",
            "snapshot-002",
            &[
                "src/Handlers/OrderHandler.php",
                "src/Listeners/OrderListener.php",
                "src/Handlers/PaymentHandler.php",
                "src/Listeners/PaymentListener.php",
                "docs/handlers.md",
            ],
        )?;

        let second = searcher.search_hybrid(SearchHybridQuery {
            query: "PaymentHandler handle listener".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        })?;
        let payment_listener = second
            .matches
            .iter()
            .find(|entry| entry.document.path == "src/Listeners/PaymentListener.php")
            .expect("snapshot change should rebuild graph artifact for the new payment listener");
        assert!(
            payment_listener.graph_score > 0.0,
            "rebuilt graph artifact should contribute graph evidence for new snapshot content: {:?}",
            second.matches
        );
        assert_eq!(
            searcher
                .hybrid_graph_artifact_cache
                .read()
                .expect("hybrid graph artifact cache should not be poisoned")
                .len(),
            1
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_graph_channel_seeds_from_canonical_runtime_paths_without_exact_symbol_terms()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-graph-canonical-path-seed");
        prepare_workspace(
            &root,
            &[
                (
                    "src/Handlers/OrderHandler.php",
                    "<?php\n\
                     namespace App\\Handlers;\n\
                     class OrderHandler {\n\
                         public function handle(): void {}\n\
                     }\n",
                ),
                (
                    "src/Listeners/OrderListener.php",
                    "<?php\n\
                     namespace App\\Listeners;\n\
                     use App\\Handlers\\OrderHandler;\n\
                     class OrderListener {\n\
                         public function handlers(): array {\n\
                             return [[OrderHandler::class, 'handle']];\n\
                         }\n\
                     }\n",
                ),
                (
                    "docs/handlers.md",
                    "# Handlers\nOrder listener wiring overview.\n",
                ),
            ],
        )?;

        let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
        let output = searcher.search_hybrid(SearchHybridQuery {
            query: "order listener wiring".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        })?;
        let handler = output
            .matches
            .iter()
            .find(|entry| entry.document.path == "src/Handlers/OrderHandler.php")
            .expect("canonical path-seeded graph search should surface the handler runtime file");

        assert!(
            handler.graph_score > 0.0,
            "graph channel should activate from canonical runtime path seeds even without exact symbol terms: {:?}",
            output.matches
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_respects_configured_channel_weights() -> FriggResult<()> {
        let lexical = vec![
            hybrid_hit("repo-001", "src/a.rs", 10.0, "lex-a"),
            hybrid_hit("repo-001", "src/b.rs", 8.0, "lex-b"),
        ];
        let graph = vec![
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::GraphPrecise,
                "repo-001",
                "src/b.rs",
                5.0,
                "graph-b",
            ),
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::GraphPrecise,
                "repo-001",
                "src/c.rs",
                4.0,
                "graph-c",
            ),
        ];
        let semantic = vec![
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::Semantic,
                "repo-001",
                "src/c.rs",
                0.9,
                "sem-c",
            ),
            hybrid_hit_with_channel(
                crate::domain::EvidenceChannel::Semantic,
                "repo-001",
                "src/a.rs",
                0.2,
                "sem-a",
            ),
        ];
        let weights = HybridChannelWeights {
            lexical: 0.2,
            graph: 0.2,
            semantic: 0.6,
        };

        let ranked = rank_hybrid_evidence(&lexical, &graph, &semantic, weights, 10)?;
        assert_eq!(ranked.len(), 3);
        assert_eq!(ranked[0].document.path, "src/c.rs");
        assert_eq!(ranked[1].document.path, "src/b.rs");
        assert_eq!(ranked[2].document.path, "src/a.rs");

        Ok(())
    }

    #[test]
    fn hybrid_ranking_is_deterministic_under_tied_scores() -> FriggResult<()> {
        let lexical = vec![
            hybrid_hit("repo-001", "src/b.rs", 1.0, "lex-b"),
            hybrid_hit("repo-001", "src/a.rs", 1.0, "lex-a"),
        ];
        let graph = vec![hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::GraphPrecise,
            "repo-001",
            "src/c.rs",
            1.0,
            "graph-c",
        )];
        let semantic = vec![hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::Semantic,
            "repo-001",
            "src/c.rs",
            1.0,
            "sem-c",
        )];

        let first = rank_hybrid_evidence(
            &lexical,
            &graph,
            &semantic,
            HybridChannelWeights::default(),
            10,
        )?;
        let reversed_lexical = lexical.into_iter().rev().collect::<Vec<_>>();
        let second = rank_hybrid_evidence(
            &reversed_lexical,
            &graph,
            &semantic,
            HybridChannelWeights::default(),
            10,
        )?;

        assert_eq!(first, second);
        assert_eq!(first[0].document.path, "src/a.rs");
        assert_eq!(first[1].document.path, "src/b.rs");
        assert_eq!(first[2].document.path, "src/c.rs");

        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_channel_blends_retrieval_when_enabled() -> FriggResult<()> {
        let (searcher, root) =
            semantic_hybrid_fixture("hybrid-semantic-enabled", semantic_runtime_enabled(false))?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![0.0, 1.0]);

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);
        assert!(output.note.semantic_hit_count > 0);
        assert!(output.note.semantic_match_count > 0);
        assert!(output.note.semantic_enabled);
        assert!(output.note.semantic_reason.is_none());
        assert!(
            output.matches.len() >= 2,
            "expected at least two hybrid matches from lexical + semantic fixture"
        );
        assert_eq!(
            output.matches[0].document.path, "src/z.rs",
            "semantic similarity should promote src/z.rs above lexical tie ordering"
        );
        assert!(
            output.matches[0].semantic_score > output.matches[1].semantic_score,
            "top-ranked semantic score should be strictly greater for promoted path"
        );
        assert!(
            output.matches[0]
                .semantic_sources
                .iter()
                .any(|source| source.starts_with("chunk-src_z.rs")),
            "semantic sources should include deterministic chunk provenance ids"
        );
        let semantic_match = output
            .matches
            .iter()
            .find(|matched| matched.document.path == "src/z.rs")
            .expect("semantic-promoted match should be present");
        assert_eq!(semantic_match.document.line, 2);
        assert_eq!(semantic_match.anchor.start_line, 2);
        assert_eq!(semantic_match.anchor.end_line, 2);
        let semantic_channel = output
            .channel_results
            .iter()
            .find(|result| result.channel == crate::domain::EvidenceChannel::Semantic)
            .expect("semantic channel result should be present");
        let semantic_hit = semantic_channel
            .hits
            .iter()
            .find(|hit| hit.document.path == "src/z.rs")
            .expect("semantic hit for src/z.rs should be present");
        assert_eq!(semantic_hit.anchor.start_line, 2);
        assert_eq!(semantic_hit.anchor.end_line, 3);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_channel_ignores_excluded_paths_and_non_active_models()
    -> FriggResult<()> {
        let root = temp_workspace_root("hybrid-semantic-filtered");
        prepare_workspace(
            &root,
            &[
                ("src/current.rs", "pub fn current() {}\n"),
                ("src/legacy.rs", "pub fn legacy() {}\n"),
                ("target/debug/app.rs", "pub fn target_artifact() {}\n"),
            ],
        )?;
        let mut legacy = semantic_record(
            "repo-001",
            "snapshot-001",
            "src/legacy.rs",
            0,
            vec![1.0, 0.0],
        );
        legacy.provider = "google".to_owned();
        legacy.model = "gemini-embedding-001".to_owned();
        let target = semantic_record(
            "repo-001",
            "snapshot-001",
            "target/debug/app.rs",
            0,
            vec![1.0, 0.0],
        );
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record(
                    "repo-001",
                    "snapshot-001",
                    "src/current.rs",
                    0,
                    vec![1.0, 0.0],
                ),
                legacy,
                target,
            ],
        )?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime_enabled(false);
        let searcher = TextSearcher::new(config);
        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "current symbol".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: Some("test-gemini-key".to_owned()),
            },
            &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
        )?;

        let paths = output
            .matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect::<Vec<_>>();
        assert!(
            paths.contains(&"src/current.rs"),
            "active-model semantic path should remain visible: {paths:?}"
        );
        assert!(
            !paths.contains(&"src/legacy.rs"),
            "rows for other provider/model combinations must be ignored: {paths:?}"
        );
        assert!(
            !paths.iter().any(|path| path.starts_with("target/")),
            "excluded runtime paths must not surface from semantic storage: {paths:?}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_channel_can_be_disabled_per_query_toggle() -> FriggResult<()> {
        let (searcher, root) = semantic_hybrid_fixture(
            "hybrid-semantic-toggle-off",
            semantic_runtime_enabled(false),
        )?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = PanicSemanticQueryEmbeddingExecutor;

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(false),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Disabled);
        assert_eq!(output.note.semantic_hit_count, 0);
        assert_eq!(output.note.semantic_match_count, 0);
        assert!(!output.note.semantic_enabled);
        assert_eq!(
            output.note.semantic_reason.as_deref(),
            Some("semantic channel disabled by request toggle")
        );
        assert!(
            output
                .matches
                .iter()
                .all(|evidence| evidence.semantic_score == 0.0),
            "semantic channel scores should be zero when semantic toggle is disabled"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_channel_degrades_on_provider_failure_non_strict() -> FriggResult<()>
    {
        let (searcher, root) =
            semantic_hybrid_fixture("hybrid-semantic-degraded", semantic_runtime_enabled(false))?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor =
            MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

        let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Degraded);
        assert_eq!(output.note.semantic_hit_count, 0);
        assert_eq!(output.note.semantic_match_count, 0);
        assert!(!output.note.semantic_enabled);
        assert!(
            output
                .note
                .semantic_reason
                .as_deref()
                .is_some_and(|reason| reason.contains("mock semantic provider unavailable")),
            "degraded note should include deterministic provider failure reason"
        );
        assert!(
            output
                .matches
                .iter()
                .all(|entry| entry.semantic_score == 0.0),
            "semantic scores should be zero when semantic channel degrades"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_channel_strict_mode_surfaces_strict_failure() -> FriggResult<()> {
        let (searcher, root) = semantic_hybrid_fixture(
            "hybrid-semantic-strict-failure",
            semantic_runtime_enabled(true),
        )?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor =
            MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

        let err = searcher
            .search_hybrid_with_filters_using_executor(
                SearchHybridQuery {
                    query: "needle".to_owned(),
                    limit: 10,
                    weights: HybridChannelWeights::default(),
                    semantic: Some(true),
                },
                SearchFilters::default(),
                &credentials,
                &semantic_executor,
            )
            .expect_err("strict semantic mode should fail on semantic provider errors");
        let err_message = err.to_string();
        assert!(
            err_message.contains("semantic_status=strict_failure"),
            "strict mode failure should carry deterministic strict status metadata: {err_message}"
        );
        assert!(
            err_message.contains("mock semantic provider unavailable"),
            "strict mode failure should include semantic channel failure reason: {err_message}"
        );

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_enabled_replay_is_deterministic() -> FriggResult<()> {
        let (searcher, root) = semantic_hybrid_fixture(
            "hybrid-semantic-enabled-deterministic-replay",
            semantic_runtime_enabled(false),
        )?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![0.0, 1.0]);

        let first = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;
        let second = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(first.matches, second.matches);
        assert_eq!(first.note, second.note);
        assert_eq!(first.diagnostics, second.diagnostics);
        assert_eq!(first.note.semantic_status, HybridSemanticStatus::Ok);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_degraded_replay_is_deterministic() -> FriggResult<()> {
        let (searcher, root) = semantic_hybrid_fixture(
            "hybrid-semantic-degraded-deterministic-replay",
            semantic_runtime_enabled(false),
        )?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor =
            MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

        let first = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;
        let second = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "needle".to_owned(),
                limit: 10,
                weights: HybridChannelWeights::default(),
                semantic: Some(true),
            },
            SearchFilters::default(),
            &credentials,
            &semantic_executor,
        )?;

        assert_eq!(first.matches, second.matches);
        assert_eq!(first.note, second.note);
        assert_eq!(first.diagnostics, second.diagnostics);
        assert_eq!(first.note.semantic_status, HybridSemanticStatus::Degraded);

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_ranking_semantic_strict_failure_replay_is_deterministic() -> FriggResult<()> {
        let (searcher, root) = semantic_hybrid_fixture(
            "hybrid-semantic-strict-deterministic-replay",
            semantic_runtime_enabled(true),
        )?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor =
            MockSemanticQueryEmbeddingExecutor::failure("mock semantic provider unavailable");

        let first = searcher
            .search_hybrid_with_filters_using_executor(
                SearchHybridQuery {
                    query: "needle".to_owned(),
                    limit: 10,
                    weights: HybridChannelWeights::default(),
                    semantic: Some(true),
                },
                SearchFilters::default(),
                &credentials,
                &semantic_executor,
            )
            .expect_err("strict semantic mode should fail deterministically");
        let second = searcher
            .search_hybrid_with_filters_using_executor(
                SearchHybridQuery {
                    query: "needle".to_owned(),
                    limit: 10,
                    weights: HybridChannelWeights::default(),
                    semantic: Some(true),
                },
                SearchFilters::default(),
                &credentials,
                &semantic_executor,
            )
            .expect_err("strict semantic mode should fail deterministically");

        assert_eq!(first.to_string(), second.to_string());
        assert!(first.to_string().contains("semantic_status=strict_failure"));

        cleanup_workspace(&root);
        Ok(())
    }

    #[test]
    fn hybrid_search_semantic_query_embedding_works_inside_existing_tokio_runtime()
    -> FriggResult<()> {
        let (searcher, root) = semantic_hybrid_fixture(
            "hybrid-semantic-inside-current-runtime",
            semantic_runtime_enabled(false),
        )?;
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let semantic_executor = MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");

        let output = runtime.block_on(async {
            searcher.search_hybrid_with_filters_using_executor(
                SearchHybridQuery {
                    query: "needle".to_owned(),
                    limit: 10,
                    weights: HybridChannelWeights::default(),
                    semantic: Some(true),
                },
                SearchFilters::default(),
                &credentials,
                &semantic_executor,
            )
        })?;

        assert!(
            !output.matches.is_empty(),
            "hybrid search inside an existing runtime should still return matches"
        );
        assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);

        cleanup_workspace(&root);
        Ok(())
    }

    #[derive(Debug, Clone)]
    struct MockSemanticQueryEmbeddingExecutor {
        result: Result<Vec<f32>, String>,
    }

    impl MockSemanticQueryEmbeddingExecutor {
        fn success(vector: Vec<f32>) -> Self {
            Self {
                result: Ok(pad_semantic_test_vector(vector)),
            }
        }

        fn failure(message: &str) -> Self {
            Self {
                result: Err(message.to_owned()),
            }
        }
    }

    impl SemanticRuntimeQueryEmbeddingExecutor for MockSemanticQueryEmbeddingExecutor {
        fn embed_query<'a>(
            &'a self,
            _provider: SemanticRuntimeProvider,
            _model: &'a str,
            _query: String,
        ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>> {
            let result = self.result.clone();
            Box::pin(async move {
                match result {
                    Ok(vector) => Ok(vector),
                    Err(message) => Err(FriggError::Internal(message)),
                }
            })
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct PanicSemanticQueryEmbeddingExecutor;

    impl SemanticRuntimeQueryEmbeddingExecutor for PanicSemanticQueryEmbeddingExecutor {
        fn embed_query<'a>(
            &'a self,
            _provider: SemanticRuntimeProvider,
            _model: &'a str,
            _query: String,
        ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>> {
            Box::pin(async move {
                panic!("semantic executor should not be called when semantic toggle is disabled")
            })
        }
    }

    fn semantic_hybrid_fixture(
        test_name: &str,
        semantic_runtime: SemanticRuntimeConfig,
    ) -> FriggResult<(TextSearcher, PathBuf)> {
        let root = temp_workspace_root(test_name);
        let semantic_b = semantic_record("repo-001", "snapshot-001", "src/b.rs", 0, vec![1.0, 0.0]);
        let mut semantic_z =
            semantic_record("repo-001", "snapshot-001", "src/z.rs", 0, vec![0.0, 1.0]);
        semantic_z.start_line = 2;
        semantic_z.end_line = 3;
        prepare_workspace(
            &root,
            &[
                ("src/b.rs", "pub fn b() { let _ = \"needle\"; }\n"),
                (
                    "src/z.rs",
                    "pub fn z() {\n    let _ = \"needle\";\n    let _ = \"semantic\";\n}\n",
                ),
            ],
        )?;
        seed_semantic_embeddings(&root, "repo-001", "snapshot-001", &[semantic_b, semantic_z])?;

        let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
        config.semantic_runtime = semantic_runtime;
        Ok((TextSearcher::new(config), root))
    }

    fn semantic_runtime_enabled(strict_mode: bool) -> SemanticRuntimeConfig {
        SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode,
        }
    }

    fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
        system_time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
    }

    fn seed_semantic_embeddings(
        workspace_root: &Path,
        repository_id: &str,
        snapshot_id: &str,
        records: &[SemanticChunkEmbeddingRecord],
    ) -> FriggResult<()> {
        let db_path = ensure_provenance_db_parent_dir(workspace_root)?;
        let resolved_db_path = resolve_provenance_db_path(workspace_root)?;
        assert_eq!(db_path, resolved_db_path);

        let storage = Storage::new(db_path);
        storage.initialize()?;

        let mut manifest_entries = records
            .iter()
            .map(|record| {
                let metadata = fs::metadata(workspace_root.join(&record.path))
                    .expect("semantic embedding manifest path should exist");
                ManifestEntry {
                    path: record.path.clone(),
                    sha256: format!("hash-{}", record.path),
                    size_bytes: metadata.len(),
                    mtime_ns: metadata.modified().ok().and_then(system_time_to_unix_nanos),
                }
            })
            .collect::<Vec<_>>();
        manifest_entries.sort_by(|left, right| left.path.cmp(&right.path));
        manifest_entries.dedup_by(|left, right| left.path == right.path);

        storage.upsert_manifest(repository_id, snapshot_id, &manifest_entries)?;
        storage.replace_semantic_embeddings_for_repository(repository_id, snapshot_id, records)?;

        Ok(())
    }

    fn seed_manifest_snapshot(
        workspace_root: &Path,
        repository_id: &str,
        snapshot_id: &str,
        paths: &[&str],
    ) -> FriggResult<()> {
        let db_path = ensure_provenance_db_parent_dir(workspace_root)?;
        let resolved_db_path = resolve_provenance_db_path(workspace_root)?;
        assert_eq!(db_path, resolved_db_path);

        let storage = Storage::new(db_path);
        storage.initialize()?;

        let mut manifest_entries = paths
            .iter()
            .map(|path| {
                let metadata = fs::metadata(workspace_root.join(path)).map_err(FriggError::Io)?;
                Ok(ManifestEntry {
                    path: (*path).to_owned(),
                    sha256: format!("hash-{path}"),
                    size_bytes: metadata.len(),
                    mtime_ns: metadata.modified().ok().and_then(system_time_to_unix_nanos),
                })
            })
            .collect::<FriggResult<Vec<_>>>()?;
        manifest_entries.sort_by(|left, right| left.path.cmp(&right.path));
        manifest_entries.dedup_by(|left, right| left.path == right.path);

        storage.upsert_manifest(repository_id, snapshot_id, &manifest_entries)?;
        Ok(())
    }

    fn semantic_record(
        repository_id: &str,
        snapshot_id: &str,
        path: &str,
        chunk_index: usize,
        embedding: Vec<f32>,
    ) -> SemanticChunkEmbeddingRecord {
        let language = Path::new(path)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| match extension {
                "php" => "php",
                "md" | "markdown" => "markdown",
                "json" => "json",
                _ => "rust",
            })
            .unwrap_or("rust")
            .to_owned();
        let path_slug = path.replace('/', "_");

        SemanticChunkEmbeddingRecord {
            chunk_id: format!("chunk-{path_slug}-{chunk_index}"),
            repository_id: repository_id.to_owned(),
            snapshot_id: snapshot_id.to_owned(),
            path: path.to_owned(),
            language,
            chunk_index,
            start_line: 1,
            end_line: 1,
            provider: "openai".to_owned(),
            model: "text-embedding-3-small".to_owned(),
            trace_id: Some("trace-semantic-test".to_owned()),
            content_hash_blake3: format!("hash-content-{path_slug}-{chunk_index}"),
            content_text: format!("semantic excerpt for {path}"),
            embedding: pad_semantic_test_vector(embedding),
        }
    }

    fn pad_semantic_test_vector(mut embedding: Vec<f32>) -> Vec<f32> {
        embedding.resize(crate::storage::DEFAULT_VECTOR_DIMENSIONS, 0.0);
        embedding
    }

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!(
            "frigg-search-{test_name}-{nonce}-{}",
            std::process::id()
        ))
    }

    fn prepare_workspace(root: &Path, files: &[(&str, &str)]) -> FriggResult<()> {
        fs::create_dir_all(root).map_err(FriggError::Io)?;
        for (relative_path, contents) in files {
            let path = root.join(relative_path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(FriggError::Io)?;
            }
            fs::write(path, contents).map_err(FriggError::Io)?;
        }

        Ok(())
    }

    fn cleanup_workspace(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }

    fn rewrite_file_with_new_mtime(path: &Path, contents: &str) -> FriggResult<()> {
        let before = fs::metadata(path)
            .ok()
            .and_then(|metadata| metadata.modified().ok())
            .and_then(system_time_to_unix_nanos);

        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(20));
            fs::write(path, contents).map_err(FriggError::Io)?;
            let after = fs::metadata(path)
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_to_unix_nanos);
            if after != before {
                return Ok(());
            }
        }

        Err(FriggError::Internal(
            "fixture file mtime did not advance after rewrite".to_owned(),
        ))
    }

    fn text_match(
        repository_id: &str,
        path: &str,
        line: usize,
        column: usize,
        excerpt: &str,
    ) -> TextMatch {
        TextMatch {
            repository_id: repository_id.to_owned(),
            path: path.to_owned(),
            line,
            column,
            excerpt: excerpt.to_owned(),
        }
    }

    fn hybrid_hit(
        repository_id: &str,
        path: &str,
        raw_score: f32,
        provenance_id: &str,
    ) -> HybridChannelHit {
        hybrid_hit_with_channel(
            crate::domain::EvidenceChannel::LexicalManifest,
            repository_id,
            path,
            raw_score,
            provenance_id,
        )
    }

    fn hybrid_hit_with_channel(
        channel: crate::domain::EvidenceChannel,
        repository_id: &str,
        path: &str,
        raw_score: f32,
        provenance_id: &str,
    ) -> HybridChannelHit {
        HybridChannelHit {
            channel,
            document: HybridDocumentRef {
                repository_id: repository_id.to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            anchor: crate::domain::EvidenceAnchor::new(
                crate::domain::EvidenceAnchorKind::TextSpan,
                1,
                1,
                1,
                1,
            ),
            raw_score,
            excerpt: format!("excerpt for {path}"),
            provenance_ids: vec![provenance_id.to_owned()],
        }
    }
}
