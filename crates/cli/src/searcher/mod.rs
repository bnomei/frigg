use std::borrow::Cow;
use std::fs;
use std::path::Path;

mod candidates;
mod graph_channel;
mod hybrid_execution;
mod intent;
mod laravel;
mod lexical_channel;
mod lexical_recall;
mod ordering;
mod query_terms;
mod ranker;
mod regex_support;
mod reranker;
mod scan_engine;
mod semantic;
mod surfaces;

use crate::domain::{FriggError, FriggResult, model::TextMatch};
use crate::indexer::FileMetadataDigest;
use crate::language_support::{LanguageCapability, SymbolLanguage, parse_supported_language};
use crate::manifest_validation::validate_manifest_digests_for_root;
use crate::playbooks::scrub_playbook_metadata_header;
use crate::settings::{FriggConfig, SemanticRuntimeCredentials};
use crate::storage::{Storage, resolve_provenance_db_path};
use aho_corasick::AhoCorasick;
use candidates::{
    hard_excluded_runtime_path, hidden_workflow_candidates_for_repository, merge_candidate_files,
    normalize_repository_relative_path, walk_candidate_files_for_repository,
};
use graph_channel::search_graph_channel_hits;
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
    best_path_witness_excerpt, build_hybrid_lexical_hits_with_intent,
    hybrid_canonical_match_multiplier, hybrid_path_has_exact_stem_match,
    hybrid_path_quality_multiplier_with_intent, hybrid_path_witness_recall_score,
    merge_hybrid_lexical_search_output, merge_hybrid_path_witness_recall_output, semantic_excerpt,
};
#[cfg(test)]
use lexical_channel::{build_hybrid_lexical_hits, build_hybrid_lexical_hits_for_query};
use lexical_recall::{build_hybrid_lexical_recall_regex, hybrid_lexical_recall_tokens};
use ordering::{
    retain_bounded_match, sort_matches_deterministically,
    sort_search_diagnostics_deterministically, text_match_candidate_order,
};
use query_terms::{
    hybrid_excerpt_has_build_flow_anchor, hybrid_excerpt_has_exact_identifier_anchor,
    hybrid_excerpt_has_test_double_anchor, hybrid_identifier_tokens, hybrid_overlap_count,
    hybrid_path_overlap_count, hybrid_path_overlap_tokens, hybrid_query_exact_terms,
    hybrid_query_overlap_terms, path_has_exact_query_term_match,
};
use ranker::blend_hybrid_evidence;
pub use ranker::rank_hybrid_evidence;
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
    is_repo_metadata_path, is_runtime_config_artifact_path, is_scripts_ops_path,
    is_test_harness_path, is_test_support_path,
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HybridDocumentRef {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HybridChannelHit {
    pub document: HybridDocumentRef,
    pub raw_score: f32,
    pub excerpt: String,
    pub provenance_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HybridChannel {
    Lexical,
    Graph,
    Semantic,
}

impl HybridChannel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lexical => "lexical",
            Self::Graph => "graph",
            Self::Semantic => "semantic",
        }
    }
}

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
    pub diagnostics: SearchExecutionDiagnostics,
    pub note: HybridExecutionNote,
}

#[derive(Debug, Clone, PartialEq)]
pub struct HybridRankedEvidence {
    pub document: HybridDocumentRef,
    pub excerpt: String,
    pub blended_score: f32,
    pub lexical_score: f32,
    pub graph_score: f32,
    pub semantic_score: f32,
    pub lexical_sources: Vec<String>,
    pub graph_sources: Vec<String>,
    pub semantic_sources: Vec<String>,
}

fn rank_hybrid_evidence_for_query(
    lexical_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
    limit: usize,
    query_text: &str,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let ranked = blend_hybrid_evidence(lexical_hits, graph_hits, semantic_hits, weights)?;
    Ok(diversify_hybrid_ranked_evidence(ranked, limit, query_text))
}

#[derive(Debug, Clone, Default)]
struct NormalizedSearchFilters {
    repository_id: Option<String>,
    language: Option<SymbolLanguage>,
}

pub struct TextSearcher {
    config: FriggConfig,
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
        Self { config }
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
        scan_engine::search_with_streaming_lines(self, query, filters, match_columns)
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
        scan_engine::search_with_matcher(self, query, filters, file_may_match, match_columns)
    }
}

fn should_scrub_playbook_metadata(path: &str) -> bool {
    path.starts_with("playbooks/") && path.ends_with(".md")
}

fn scrub_search_content<'a>(path: &str, content: &'a str) -> Cow<'a, str> {
    if should_scrub_playbook_metadata(path) {
        return scrub_playbook_metadata_header(content);
    }

    Cow::Borrowed(content)
}

impl TextSearcher {
    fn candidate_files_for_repository(
        &self,
        repository_id: &str,
        root: &Path,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
        diagnostics: &mut SearchExecutionDiagnostics,
    ) -> Vec<(String, std::path::PathBuf)> {
        self.manifest_candidate_files_for_repository(repository_id, root, query, filters)
            .unwrap_or_else(|| {
                walk_candidate_files_for_repository(
                    repository_id,
                    root,
                    query,
                    filters,
                    diagnostics,
                )
            })
    }

    fn manifest_candidate_files_for_repository(
        &self,
        repository_id: &str,
        root: &Path,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
    ) -> Option<Vec<(String, std::path::PathBuf)>> {
        let db_path = resolve_provenance_db_path(root).ok()?;
        if !db_path.exists() {
            return None;
        }

        let storage = Storage::new(db_path);
        let latest = storage
            .load_latest_manifest_for_repository(repository_id)
            .ok()??;
        let snapshot_digests = latest
            .entries
            .into_iter()
            .map(|entry| FileMetadataDigest {
                path: entry.path.into(),
                size_bytes: entry.size_bytes,
                mtime_ns: entry.mtime_ns,
            })
            .collect::<Vec<_>>();
        let validated_digests = validate_manifest_digests_for_root(root, &snapshot_digests)?;
        let mut candidates = Vec::new();
        for digest in validated_digests {
            let path = digest.path;
            if hard_excluded_runtime_path(root, &path) {
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
        Some(candidates)
    }

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
        let mut diagnostics = SearchExecutionDiagnostics::default();
        let mut repositories = self.config.repositories();
        repositories.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.root_path.cmp(&right.root_path))
        });

        let mut scored = Vec::<(f32, String, String, std::path::PathBuf)>::new();
        for repo in repositories {
            if normalized_filters
                .repository_id
                .as_ref()
                .is_some_and(|repository_id| repository_id != &repo.repository_id.0)
            {
                continue;
            }

            let repository_id = repo.repository_id.0.clone();
            let root = Path::new(&repo.root_path);
            let mut candidates = self.candidate_files_for_repository(
                &repository_id,
                root,
                &empty_query,
                &normalized_filters,
                &mut diagnostics,
            );
            merge_candidate_files(
                &mut candidates,
                hidden_workflow_candidates_for_repository(
                    root,
                    &normalized_filters,
                    intent,
                    &mut diagnostics,
                ),
            );

            for (rel_path, path) in candidates {
                let Some(score) = hybrid_path_witness_recall_score(&rel_path, intent, query_text)
                else {
                    continue;
                };
                scored.push((score, repository_id.clone(), rel_path, path));
            }
        }

        scored.sort_by(|left, right| {
            right
                .0
                .total_cmp(&left.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(left.3.cmp(&right.3))
        });
        scored.truncate(limit.max(24));

        let mut matches = Vec::new();
        for (_, repository_id, rel_path, path) in scored {
            let excerpt = fs::read_to_string(&path)
                .ok()
                .and_then(|content| best_path_witness_excerpt(&rel_path, &content, query_text))
                .unwrap_or_else(|| rel_path.clone());
            matches.push(TextMatch {
                repository_id,
                path: rel_path.clone(),
                line: 1,
                column: 1,
                excerpt,
            });
        }

        Ok(SearchExecutionOutput {
            total_matches: matches.len(),
            matches,
            diagnostics,
        })
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
mod tests {
    use std::env;
    use std::fs;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
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
        HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridRankingIntent,
        HybridSemanticStatus, HybridSourceClass, MAX_REGEX_ALTERNATIONS, MAX_REGEX_GROUPS,
        MAX_REGEX_PATTERN_BYTES, MAX_REGEX_QUANTIFIERS, RegexSearchError, SearchDiagnosticKind,
        SearchFilters, SearchHybridQuery, SearchTextQuery, SemanticRuntimeQueryEmbeddingExecutor,
        TextSearcher, build_hybrid_lexical_hits, build_hybrid_lexical_hits_for_query,
        build_hybrid_lexical_recall_regex, build_regex_prefilter_plan, compile_safe_regex,
        hybrid_lexical_recall_tokens, hybrid_source_class, normalize_search_filters,
        rank_hybrid_evidence, rank_hybrid_evidence_for_query,
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
            &[("src/lib.rs", "needle 1\n"), ("src/lib.php", "needle 2\n")],
        )?;
        prepare_workspace(
            &root_b,
            &[
                ("src/main.rs", "needle 9\n"),
                ("src/main.php", "needle 10\n"),
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

        let unsupported_language = searcher.search_regex_with_filters(
            SearchTextQuery {
                query: r"needle".to_owned(),
                path_regex: None,
                limit: 10,
            },
            SearchFilters {
                repository_id: None,
                language: Some("typescript".to_owned()),
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
    fn hybrid_ranking_query_aware_lexical_hits_promote_public_docs_witnesses() -> FriggResult<()> {
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
        assert_eq!(ranked[0].document.path, "contracts/errors.md");
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
            document: HybridDocumentRef {
                repository_id: "repo-001".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            raw_score,
            excerpt: excerpt.to_owned(),
            provenance_id: format!("lexical::{path}"),
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
    fn hybrid_ranking_blends_lexical_graph_and_semantic_channels() -> FriggResult<()> {
        let lexical = vec![
            hybrid_hit("repo-001", "src/a.rs", 10.0, "lex-a"),
            hybrid_hit("repo-001", "src/b.rs", 8.0, "lex-b"),
        ];
        let graph = vec![
            hybrid_hit("repo-001", "src/b.rs", 5.0, "graph-b"),
            hybrid_hit("repo-001", "src/c.rs", 4.0, "graph-c"),
        ];
        let semantic = vec![
            hybrid_hit("repo-001", "src/c.rs", 0.9, "sem-c"),
            hybrid_hit("repo-001", "src/a.rs", 0.2, "sem-a"),
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
    fn hybrid_ranking_respects_configured_channel_weights() -> FriggResult<()> {
        let lexical = vec![
            hybrid_hit("repo-001", "src/a.rs", 10.0, "lex-a"),
            hybrid_hit("repo-001", "src/b.rs", 8.0, "lex-b"),
        ];
        let graph = vec![
            hybrid_hit("repo-001", "src/b.rs", 5.0, "graph-b"),
            hybrid_hit("repo-001", "src/c.rs", 4.0, "graph-c"),
        ];
        let semantic = vec![
            hybrid_hit("repo-001", "src/c.rs", 0.9, "sem-c"),
            hybrid_hit("repo-001", "src/a.rs", 0.2, "sem-a"),
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
        let graph = vec![hybrid_hit("repo-001", "src/c.rs", 1.0, "graph-c")];
        let semantic = vec![hybrid_hit("repo-001", "src/c.rs", 1.0, "sem-c")];

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
            Self { result: Ok(vector) }
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
        prepare_workspace(
            &root,
            &[
                ("src/b.rs", "pub fn b() { let _ = \"needle\"; }\n"),
                ("src/z.rs", "pub fn z() { let _ = \"needle\"; }\n"),
            ],
        )?;
        seed_semantic_embeddings(
            &root,
            "repo-001",
            "snapshot-001",
            &[
                semantic_record("repo-001", "snapshot-001", "src/b.rs", 0, vec![1.0, 0.0]),
                semantic_record("repo-001", "snapshot-001", "src/z.rs", 0, vec![0.0, 1.0]),
            ],
        )?;

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
            embedding,
        }
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
        HybridChannelHit {
            document: HybridDocumentRef {
                repository_id: repository_id.to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            raw_score,
            excerpt: format!("excerpt for {path}"),
            provenance_id: provenance_id.to_owned(),
        }
    }
}
