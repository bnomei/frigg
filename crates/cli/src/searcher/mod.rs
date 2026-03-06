use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::future::Future;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::pin::Pin;

use crate::domain::{FriggError, FriggResult, model::TextMatch};
use crate::embeddings::{
    EmbeddingProvider, EmbeddingPurpose, EmbeddingRequest, GoogleEmbeddingProvider,
    OpenAiEmbeddingProvider,
};
use crate::indexer::{FileMetadataDigest, SymbolLanguage};
use crate::manifest_validation::validate_manifest_digests_for_root;
use crate::playbooks::scrub_playbook_metadata_header;
use crate::settings::{FriggConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider};
use crate::storage::{SemanticChunkEmbeddingProjection, Storage, resolve_provenance_db_path};
use aho_corasick::AhoCorasick;
use ignore::WalkBuilder;
use regex::{Regex, RegexBuilder};
use thiserror::Error;

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
    Ok,
    Degraded,
}

impl HybridSemanticStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
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
}

impl Default for HybridExecutionNote {
    fn default() -> Self {
        Self {
            semantic_requested: false,
            semantic_enabled: false,
            semantic_status: HybridSemanticStatus::Disabled,
            semantic_reason: None,
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

#[derive(Debug, Clone)]
struct HybridScoreAccumulator {
    excerpt: String,
    lexical_score: f32,
    graph_score: f32,
    semantic_score: f32,
    lexical_sources: Vec<String>,
    graph_sources: Vec<String>,
    semantic_sources: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum HybridSourceClass {
    ErrorContracts,
    ToolContracts,
    BenchmarkDocs,
    Documentation,
    Readme,
    Runtime,
    Tests,
    Fixtures,
    Playbooks,
    Specs,
    Other,
}

#[derive(Debug, Clone, Copy, Default)]
struct HybridRankingIntent {
    wants_docs: bool,
    wants_runtime: bool,
    wants_tests: bool,
    wants_fixtures: bool,
    wants_benchmarks: bool,
    wants_readme: bool,
    wants_contracts: bool,
    wants_error_taxonomy: bool,
    wants_tool_contracts: bool,
    penalize_playbook_self_reference: bool,
}

impl HybridRankingIntent {
    fn from_query(query_text: &str) -> Self {
        let query = query_text.trim().to_ascii_lowercase();
        let has_any = |needles: &[&str]| needles.iter().any(|needle| query.contains(needle));

        let wants_docs = has_any(&[
            "docs",
            "documented",
            "documentation",
            "public docs",
            "contract",
            "contracts",
            "readme",
            "invalid_params",
            "error_code",
            "typed error",
            "citation",
            "citations",
        ]);
        let wants_tests = has_any(&[
            "test", "tests", "coverage", "assert", "parity", "canary", "replay",
        ]);
        let wants_fixtures = has_any(&[
            "fixture",
            "fixtures",
            "playbook",
            "playbooks",
            "replay",
            "trace artifact",
        ]);
        let wants_benchmarks =
            has_any(&[
                "benchmark",
                "benchmarks",
                "metric",
                "metrics",
                "acceptance metric",
                "acceptance metrics",
                "replayability",
                "deterministic replay",
            ]) || (has_any(&["deterministic", "replay", "suite", "fixture", "fixtures"])
                && has_any(&["trace artifact", "citation", "citations", "playbook"]));
        let wants_error_taxonomy = has_any(&[
            "invalid_params",
            "-32602",
            "error taxonomy",
            "unavailable",
            "strict_failure",
            "semantic_status",
            "semantic_reason",
        ]);
        let wants_tool_contracts = has_any(&[
            "search_hybrid",
            "semantic_status",
            "semantic_reason",
            "tool schema",
            "tool contract",
            "tool contracts",
            "tool surface",
            "tools/list",
            "mcp tool",
            "mcp tools",
            "core versus extended",
            "core vs extended",
            "extended_only",
        ]) || (has_any(&["mcp", "tool", "tools"])
            && has_any(&["core", "extended", "schema"]));

        Self {
            wants_docs,
            wants_runtime: true,
            wants_tests,
            wants_fixtures,
            wants_benchmarks,
            wants_readme: has_any(&["readme", "documented"]),
            wants_contracts: has_any(&[
                "contract",
                "contracts",
                "invalid_params",
                "error_code",
                "typed error",
                "unavailable",
                "strict_failure",
            ]),
            wants_error_taxonomy,
            wants_tool_contracts,
            penalize_playbook_self_reference: !has_any(&["playbook", "playbooks"]),
        }
    }

    fn wants_class(self, class: HybridSourceClass) -> bool {
        match class {
            HybridSourceClass::ErrorContracts => self.wants_error_taxonomy || self.wants_contracts,
            HybridSourceClass::ToolContracts => self.wants_tool_contracts || self.wants_contracts,
            HybridSourceClass::BenchmarkDocs => self.wants_benchmarks,
            HybridSourceClass::Documentation => self.wants_docs,
            HybridSourceClass::Readme => self.wants_readme,
            HybridSourceClass::Runtime => self.wants_runtime,
            HybridSourceClass::Tests => self.wants_tests,
            HybridSourceClass::Fixtures => self.wants_fixtures,
            HybridSourceClass::Playbooks => !self.penalize_playbook_self_reference,
            HybridSourceClass::Specs | HybridSourceClass::Other => false,
        }
    }
}

pub fn rank_hybrid_evidence(
    lexical_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
    limit: usize,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    if limit == 0 {
        return Ok(Vec::new());
    }

    let mut ranked = blend_hybrid_evidence(lexical_hits, graph_hits, semantic_hits, weights)?;
    ranked.truncate(limit);

    Ok(ranked)
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

fn blend_hybrid_evidence(
    lexical_hits: &[HybridChannelHit],
    graph_hits: &[HybridChannelHit],
    semantic_hits: &[HybridChannelHit],
    weights: HybridChannelWeights,
) -> FriggResult<Vec<HybridRankedEvidence>> {
    let weights = weights.validate()?;
    let mut by_document: BTreeMap<HybridDocumentRef, HybridScoreAccumulator> = BTreeMap::new();

    apply_hybrid_channel_hits(lexical_hits, HybridChannel::Lexical, &mut by_document);
    apply_hybrid_channel_hits(graph_hits, HybridChannel::Graph, &mut by_document);
    apply_hybrid_channel_hits(semantic_hits, HybridChannel::Semantic, &mut by_document);

    let mut ranked = by_document
        .into_iter()
        .map(|(document, state)| {
            let blended_score = (state.lexical_score * weights.lexical)
                + (state.graph_score * weights.graph)
                + (state.semantic_score * weights.semantic);
            HybridRankedEvidence {
                document,
                excerpt: state.excerpt,
                blended_score,
                lexical_score: state.lexical_score,
                graph_score: state.graph_score,
                semantic_score: state.semantic_score,
                lexical_sources: state.lexical_sources,
                graph_sources: state.graph_sources,
                semantic_sources: state.semantic_sources,
            }
        })
        .collect::<Vec<_>>();

    ranked.sort_by(|left, right| {
        right
            .blended_score
            .total_cmp(&left.blended_score)
            .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
            .then_with(|| right.graph_score.total_cmp(&left.graph_score))
            .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
            .then(left.document.cmp(&right.document))
            .then(left.excerpt.cmp(&right.excerpt))
    });

    Ok(ranked)
}

fn diversify_hybrid_ranked_evidence(
    ranked: Vec<HybridRankedEvidence>,
    limit: usize,
    query_text: &str,
) -> Vec<HybridRankedEvidence> {
    let intent = HybridRankingIntent::from_query(query_text);
    let mut seen_classes = BTreeMap::<HybridSourceClass, usize>::new();
    let mut remaining = ranked;
    let mut selected = Vec::with_capacity(limit.min(remaining.len()));

    while selected.len() < limit && !remaining.is_empty() {
        let best_index = remaining
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                hybrid_selection_score(left, &intent, &seen_classes)
                    .total_cmp(&hybrid_selection_score(right, &intent, &seen_classes))
                    .then_with(|| hybrid_ranked_evidence_order(right, left))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        let chosen = remaining.remove(best_index);
        let class = hybrid_source_class(&chosen.document.path);
        *seen_classes.entry(class).or_insert(0) += 1;
        selected.push(chosen);
    }

    selected
}

#[derive(Debug, Clone, Default)]
struct NormalizedSearchFilters {
    repository_id: Option<String>,
    language: Option<NormalizedLanguage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NormalizedLanguage {
    Rust,
    Php,
}

impl NormalizedLanguage {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "php" => Some(Self::Php),
            _ => None,
        }
    }

    fn matches_path(self, path: &Path) -> bool {
        match SymbolLanguage::from_path(path) {
            Some(SymbolLanguage::Rust) => self == Self::Rust,
            Some(SymbolLanguage::Php) => self == Self::Php,
            None => false,
        }
    }
}

pub struct TextSearcher {
    config: FriggConfig,
}

trait SemanticRuntimeQueryEmbeddingExecutor {
    fn embed_query<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        query: String,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>>;
}

#[derive(Debug, Default)]
struct RuntimeSemanticQueryEmbeddingExecutor {
    credentials: SemanticRuntimeCredentials,
}

impl RuntimeSemanticQueryEmbeddingExecutor {
    fn new(credentials: SemanticRuntimeCredentials) -> Self {
        Self { credentials }
    }
}

impl SemanticRuntimeQueryEmbeddingExecutor for RuntimeSemanticQueryEmbeddingExecutor {
    fn embed_query<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        query: String,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<f32>>> + Send + 'a>> {
        let model = model.trim().to_owned();
        let api_key = self
            .credentials
            .api_key_for(provider)
            .map(str::to_owned)
            .unwrap_or_default();
        Box::pin(async move {
            let request = EmbeddingRequest {
                model,
                input: vec![query],
                purpose: EmbeddingPurpose::Query,
                dimensions: None,
                trace_id: None,
            };
            let response = match provider {
                SemanticRuntimeProvider::OpenAi => {
                    let client = OpenAiEmbeddingProvider::new(api_key);
                    client.embed(request).await
                }
                SemanticRuntimeProvider::Google => {
                    let client = GoogleEmbeddingProvider::new(api_key);
                    client.embed(request).await
                }
            }
            .map_err(|err| {
                FriggError::Internal(format!(
                    "semantic query embedding provider call failed: {err}"
                ))
            })?;

            if response.vectors.len() != 1 {
                return Err(FriggError::Internal(format!(
                    "semantic query embedding response length mismatch: expected 1 vector, received {}",
                    response.vectors.len()
                )));
            }
            let vector = response
                .vectors
                .into_iter()
                .next()
                .map(|entry| entry.values);
            let Some(vector) = vector else {
                return Err(FriggError::Internal(
                    "semantic query embedding response did not include vector payload".to_owned(),
                ));
            };
            if vector.is_empty() {
                return Err(FriggError::Internal(
                    "semantic query embedding provider returned an empty vector".to_owned(),
                ));
            }
            if vector.iter().any(|value| !value.is_finite()) {
                return Err(FriggError::Internal(
                    "semantic query embedding provider returned non-finite vector values"
                        .to_owned(),
                ));
            }

            Ok(vector)
        })
    }
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
const HYBRID_SEMANTIC_CANDIDATE_POOL_MULTIPLIER: usize = 6;
const HYBRID_SEMANTIC_CANDIDATE_POOL_MIN: usize = 24;
const REGEX_TRIGRAM_BITMAP_BITS: usize = 1 << 16;
const REGEX_TRIGRAM_BITMAP_WORDS: usize = REGEX_TRIGRAM_BITMAP_BITS / 64;
const REGEX_TRIGRAM_HASH_MULTIPLIER: u32 = 0x9E37_79B1;

#[derive(Debug, Clone)]
struct RegexPrefilterPlan {
    checks: Vec<RegexPrefilterLiteralCheck>,
    needs_bitmap: bool,
}

#[derive(Debug, Clone)]
struct RegexPrefilterLiteralCheck {
    literal: String,
    trigram_hashes: Vec<usize>,
}

#[derive(Debug, Clone)]
struct TrigramBitmap {
    words: [u64; REGEX_TRIGRAM_BITMAP_WORDS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParsedRegexAtom {
    Literal(u8),
    NonLiteral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ParsedRegexQuantifier {
    min_repetitions: usize,
    exact_one: bool,
}

impl RegexPrefilterPlan {
    fn build(pattern: &str) -> Option<Self> {
        let literals = extract_required_regex_literals(pattern)?;
        let mut checks = Vec::with_capacity(literals.len());
        let mut needs_bitmap = false;

        for literal in literals {
            let trigram_hashes = literal_trigram_hashes(literal.as_bytes());
            if !trigram_hashes.is_empty() {
                needs_bitmap = true;
            }
            checks.push(RegexPrefilterLiteralCheck {
                literal,
                trigram_hashes,
            });
        }

        Some(Self {
            checks,
            needs_bitmap,
        })
    }

    fn file_may_match(&self, content: &str) -> bool {
        let bitmap = self
            .needs_bitmap
            .then(|| TrigramBitmap::from_bytes(content.as_bytes()));

        for check in &self.checks {
            if !check.trigram_hashes.is_empty() {
                if let Some(bitmap) = bitmap.as_ref() {
                    if check
                        .trigram_hashes
                        .iter()
                        .any(|&hash| !bitmap.contains(hash))
                    {
                        return false;
                    }
                }
            } else if !content.contains(check.literal.as_str()) {
                return false;
            }
        }

        true
    }

    #[cfg(test)]
    fn required_literals(&self) -> Vec<&str> {
        self.checks
            .iter()
            .map(|check| check.literal.as_str())
            .collect()
    }
}

impl TrigramBitmap {
    fn from_bytes(bytes: &[u8]) -> Self {
        let mut bitmap = Self {
            words: [0; REGEX_TRIGRAM_BITMAP_WORDS],
        };
        for window in bytes.windows(3) {
            bitmap.insert(trigram_hash(window[0], window[1], window[2]));
        }
        bitmap
    }

    fn insert(&mut self, hash: usize) {
        let word_index = hash / 64;
        let bit_index = hash % 64;
        self.words[word_index] |= 1_u64 << bit_index;
    }

    fn contains(&self, hash: usize) -> bool {
        let word_index = hash / 64;
        let bit_index = hash % 64;
        (self.words[word_index] & (1_u64 << bit_index)) != 0
    }
}

#[derive(Debug, Error)]
pub enum RegexSearchError {
    #[error("regex pattern must not be empty")]
    EmptyPattern,
    #[error("regex pattern length {actual} exceeds limit {max}")]
    PatternTooLong { actual: usize, max: usize },
    #[error("regex pattern alternation count {actual} exceeds limit {max}")]
    TooManyAlternations { actual: usize, max: usize },
    #[error("regex pattern group count {actual} exceeds limit {max}")]
    TooManyGroups { actual: usize, max: usize },
    #[error("regex pattern quantifier count {actual} exceeds limit {max}")]
    TooManyQuantifiers { actual: usize, max: usize },
    #[error("invalid regex: {0}")]
    InvalidRegex(#[from] regex::Error),
}

impl RegexSearchError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::EmptyPattern => "regex_empty_pattern",
            Self::PatternTooLong { .. } => "regex_pattern_too_long",
            Self::TooManyAlternations { .. } => "regex_too_many_alternations",
            Self::TooManyGroups { .. } => "regex_too_many_groups",
            Self::TooManyQuantifiers { .. } => "regex_too_many_quantifiers",
            Self::InvalidRegex(_) => "regex_invalid_pattern",
        }
    }
}

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
        let query_text = query.query.trim().to_owned();
        let ranking_intent = HybridRankingIntent::from_query(&query_text);
        if query_text.is_empty() {
            return Err(FriggError::InvalidInput(
                "hybrid search query must not be empty".to_owned(),
            ));
        }
        if query.limit == 0 {
            return Ok(SearchHybridExecutionOutput::default());
        }

        let lexical_limit = query.limit.max(self.config.max_search_results);
        let semantic_limit = query.limit.max(self.config.max_search_results);
        let mut lexical_output = self.search_literal_with_filters_diagnostics(
            SearchTextQuery {
                query: query_text.clone(),
                path_regex: None,
                limit: lexical_limit,
            },
            filters.clone(),
        )?;
        let graph_hits: Vec<HybridChannelHit> = Vec::new();

        let semantic_requested = query
            .semantic
            .unwrap_or(self.config.semantic_runtime.enabled);
        let strict_semantic = self.config.semantic_runtime.strict_mode;
        let (semantic_hits, note) = if matches!(query.semantic, Some(false)) {
            (
                Vec::new(),
                HybridExecutionNote {
                    semantic_requested,
                    semantic_enabled: false,
                    semantic_status: HybridSemanticStatus::Disabled,
                    semantic_reason: Some("semantic channel disabled by request toggle".to_owned()),
                },
            )
        } else if !self.config.semantic_runtime.enabled {
            (
                Vec::new(),
                HybridExecutionNote {
                    semantic_requested,
                    semantic_enabled: false,
                    semantic_status: HybridSemanticStatus::Disabled,
                    semantic_reason: Some(
                        "semantic runtime disabled in active configuration".to_owned(),
                    ),
                },
            )
        } else {
            match self.search_semantic_channel_hits(
                &query_text,
                &filters,
                semantic_limit,
                credentials,
                semantic_executor,
            ) {
                Ok(hits) => (
                    hits,
                    HybridExecutionNote {
                        semantic_requested,
                        semantic_enabled: true,
                        semantic_status: HybridSemanticStatus::Ok,
                        semantic_reason: None,
                    },
                ),
                Err(err) => {
                    if strict_semantic {
                        return Err(FriggError::Internal(format!(
                            "semantic_status=strict_failure: {err}"
                        )));
                    }
                    (
                        Vec::new(),
                        HybridExecutionNote {
                            semantic_requested,
                            semantic_enabled: false,
                            semantic_status: HybridSemanticStatus::Degraded,
                            semantic_reason: Some(err.to_string()),
                        },
                    )
                }
            }
        };

        let should_expand_lexical = lexical_output.matches.len() < query.limit
            && (note.semantic_status != HybridSemanticStatus::Ok
                || ranking_intent.wants_docs
                || ranking_intent.wants_contracts
                || ranking_intent.wants_error_taxonomy
                || ranking_intent.wants_tool_contracts
                || ranking_intent.wants_benchmarks);
        if should_expand_lexical {
            let recall_tokens = hybrid_lexical_recall_tokens(&query_text);

            // Exact token recall should land before broad regex expansion so
            // high-signal contract terms survive generic phrase misses.
            for token in recall_tokens
                .iter()
                .take(HYBRID_LEXICAL_RECALL_MAX_TOKENS)
                .cloned()
            {
                let expanded = self.search_literal_with_filters_diagnostics(
                    SearchTextQuery {
                        query: token,
                        path_regex: None,
                        limit: lexical_limit,
                    },
                    filters.clone(),
                )?;
                merge_hybrid_lexical_search_output(&mut lexical_output, expanded, lexical_limit);
                if lexical_output.matches.len() >= lexical_limit {
                    break;
                }
            }

            if lexical_output.matches.len() < lexical_limit {
                if let Some(token_regex) = build_hybrid_lexical_recall_regex(&query_text) {
                    let expanded = self.search_regex_with_filters_diagnostics(
                        SearchTextQuery {
                            query: token_regex,
                            path_regex: None,
                            limit: lexical_limit,
                        },
                        filters.clone(),
                    )?;
                    merge_hybrid_lexical_search_output(
                        &mut lexical_output,
                        expanded,
                        lexical_limit,
                    );
                }
            }
        }
        let lexical_hits =
            build_hybrid_lexical_hits_with_intent(&lexical_output.matches, &ranking_intent);

        let matches = rank_hybrid_evidence_for_query(
            &lexical_hits,
            &graph_hits,
            &semantic_hits,
            query.weights,
            query.limit,
            &query_text,
        )?;

        Ok(SearchHybridExecutionOutput {
            matches,
            diagnostics: lexical_output.diagnostics,
            note,
        })
    }

    fn search_semantic_channel_hits(
        &self,
        query_text: &str,
        filters: &SearchFilters,
        limit: usize,
        credentials: &SemanticRuntimeCredentials,
        semantic_executor: &dyn SemanticRuntimeQueryEmbeddingExecutor,
    ) -> FriggResult<Vec<HybridChannelHit>> {
        #[derive(Debug)]
        struct PendingSemanticHit {
            repository_id: String,
            snapshot_id: String,
            path: String,
            chunk_id: String,
            raw_score: f32,
        }

        self.config
            .semantic_runtime
            .validate_startup(credentials)
            .map_err(|err| {
                FriggError::InvalidInput(format!(
                    "semantic runtime validation failed code={}: {err}",
                    err.code()
                ))
            })?;

        let provider = self.config.semantic_runtime.provider.ok_or_else(|| {
            FriggError::Internal(
                "semantic runtime provider missing after successful startup validation".to_owned(),
            )
        })?;
        let model = self
            .config
            .semantic_runtime
            .normalized_model()
            .ok_or_else(|| {
                FriggError::Internal(
                    "semantic runtime model missing after successful startup validation".to_owned(),
                )
            })?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to build tokio runtime for semantic query embedding request: {err}"
                ))
            })?;
        let query_embedding = runtime.block_on(semantic_executor.embed_query(
            provider,
            model,
            query_text.to_owned(),
        ))?;
        if query_embedding.is_empty() {
            return Err(FriggError::Internal(
                "semantic query embedding provider returned an empty vector".to_owned(),
            ));
        }
        if query_embedding.iter().any(|value| !value.is_finite()) {
            return Err(FriggError::Internal(
                "semantic query embedding provider returned non-finite vector values".to_owned(),
            ));
        }

        let normalized_filters = normalize_search_filters(filters.clone())?;
        let ranking_intent = HybridRankingIntent::from_query(query_text);
        let mut repositories = self.config.repositories();
        repositories.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.root_path.cmp(&right.root_path))
        });

        let mut pending_hits = Vec::new();
        let mut db_paths_by_repository = BTreeMap::new();
        for repo in repositories {
            if normalized_filters
                .repository_id
                .as_ref()
                .is_some_and(|repository_id| repository_id != &repo.repository_id.0)
            {
                continue;
            }
            let repository_id = repo.repository_id.0;
            let root = Path::new(&repo.root_path);
            let db_path = resolve_provenance_db_path(root).map_err(|err| {
                FriggError::Internal(format!(
                    "semantic storage path resolution failed for repository '{repository_id}': {err}"
                ))
            })?;
            if !db_path.exists() {
                continue;
            }
            db_paths_by_repository.insert(repository_id.clone(), db_path.clone());

            let storage = Storage::new(db_path);
            let latest = storage
                .load_latest_manifest_for_repository(&repository_id)
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "semantic storage snapshot lookup failed for repository '{repository_id}': {err}"
                    ))
                })?;
            let Some(latest_snapshot) = latest else {
                continue;
            };
            let projections = storage
                .load_semantic_embedding_projections_for_repository_snapshot_model(
                    &repository_id,
                    &latest_snapshot.snapshot_id,
                    Some(provider.as_str()),
                    Some(model),
                )
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "semantic storage embedding projection load failed for repository '{repository_id}' snapshot '{}': {err}",
                        latest_snapshot.snapshot_id
                    ))
                })?;

            for projection in projections {
                if hard_excluded_runtime_path(root, Path::new(&projection.path)) {
                    continue;
                }
                if let Some(language) = normalized_filters.language {
                    if !language.matches_path(Path::new(&projection.path)) {
                        continue;
                    }
                }
                let score =
                    semantic_projection_score(&query_embedding, &projection, &repository_id)?
                        * hybrid_path_quality_multiplier_with_intent(
                            &projection.path,
                            &ranking_intent,
                        );
                if !score.is_finite() {
                    return Err(FriggError::Internal(format!(
                        "semantic similarity produced non-finite score for repository '{repository_id}' path '{}' chunk_id='{}'",
                        projection.path, projection.chunk_id
                    )));
                }

                pending_hits.push(PendingSemanticHit {
                    repository_id: repository_id.clone(),
                    snapshot_id: latest_snapshot.snapshot_id.clone(),
                    path: projection.path,
                    chunk_id: projection.chunk_id,
                    raw_score: score,
                });
            }
        }

        pending_hits.sort_by(|left, right| {
            right
                .raw_score
                .total_cmp(&left.raw_score)
                .then(left.repository_id.cmp(&right.repository_id))
                .then(left.path.cmp(&right.path))
                .then(left.chunk_id.cmp(&right.chunk_id))
        });
        let semantic_candidate_limit = limit
            .saturating_mul(HYBRID_SEMANTIC_CANDIDATE_POOL_MULTIPLIER)
            .max(HYBRID_SEMANTIC_CANDIDATE_POOL_MIN);
        pending_hits.truncate(semantic_candidate_limit);

        let mut chunk_texts_by_group = BTreeMap::new();
        for ((repository_id, snapshot_id), chunk_ids) in pending_hits.iter().fold(
            BTreeMap::<(String, String), Vec<String>>::new(),
            |mut grouped, hit| {
                grouped
                    .entry((hit.repository_id.clone(), hit.snapshot_id.clone()))
                    .or_default()
                    .push(hit.chunk_id.clone());
                grouped
            },
        ) {
            let Some(db_path) = db_paths_by_repository.get(&repository_id) else {
                continue;
            };
            let storage = Storage::new(db_path.clone());
            let texts = storage
                .load_semantic_chunk_texts_for_repository_snapshot(
                    &repository_id,
                    &snapshot_id,
                    &chunk_ids,
                )
                .map_err(|err| {
                    FriggError::Internal(format!(
                        "semantic storage chunk text load failed for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
                    ))
                })?;
            chunk_texts_by_group.insert((repository_id, snapshot_id), texts);
        }

        let semantic_hits = pending_hits
            .into_iter()
            .map(|hit| {
                let excerpt_source = chunk_texts_by_group
                    .get(&(hit.repository_id.clone(), hit.snapshot_id.clone()))
                    .and_then(|texts| texts.get(&hit.chunk_id))
                    .map(|text| semantic_excerpt(text, &hit.path))
                    .unwrap_or_else(|| semantic_excerpt("", &hit.path));
                HybridChannelHit {
                    document: HybridDocumentRef {
                        repository_id: hit.repository_id,
                        path: hit.path.clone(),
                        line: 1,
                        column: 1,
                    },
                    raw_score: hit.raw_score,
                    excerpt: excerpt_source,
                    provenance_id: hit.chunk_id,
                }
            })
            .collect::<Vec<_>>();

        Ok(semantic_hits)
    }

    fn search_with_streaming_lines<F>(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
        mut match_columns: F,
    ) -> FriggResult<SearchExecutionOutput>
    where
        F: FnMut(&str, &mut Vec<usize>),
    {
        let use_bounded_retention = query.limit <= BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD;
        let mut matches = if use_bounded_retention {
            Vec::with_capacity(query.limit)
        } else {
            Vec::new()
        };
        let mut diagnostics = SearchExecutionDiagnostics::default();
        let mut match_columns_buffer = Vec::new();
        let mut repositories = self.config.repositories();
        repositories.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.root_path.cmp(&right.root_path))
        });

        for repo in repositories {
            if filters
                .repository_id
                .as_ref()
                .is_some_and(|repository_id| repository_id != &repo.repository_id.0)
            {
                continue;
            }

            let repository_id = repo.repository_id.0.clone();
            let file_candidates = self.candidate_files_for_repository(
                &repository_id,
                Path::new(&repo.root_path),
                query,
                filters,
                &mut diagnostics,
            );

            for (rel_path, path) in file_candidates {
                if should_scrub_playbook_metadata(&rel_path) {
                    let content = match fs::read_to_string(&path) {
                        Ok(content) => content,
                        Err(err) => {
                            diagnostics.entries.push(SearchDiagnostic {
                                repository_id: repository_id.clone(),
                                path: Some(rel_path),
                                kind: SearchDiagnosticKind::Read,
                                message: err.to_string(),
                            });
                            continue;
                        }
                    };
                    let content = scrub_search_content(&rel_path, &content);

                    for (line_idx, line) in content.lines().enumerate() {
                        match_columns(line, &mut match_columns_buffer);
                        if match_columns_buffer.is_empty() {
                            continue;
                        }

                        let line_number = line_idx + 1;
                        let mut excerpt_for_line: Option<String> = None;

                        for &column in &match_columns_buffer {
                            if use_bounded_retention
                                && matches.len() == query.limit
                                && matches.last().is_some_and(|worst| {
                                    !text_match_candidate_order(
                                        &repository_id,
                                        &rel_path,
                                        line_number,
                                        column,
                                        line,
                                        worst,
                                    )
                                    .is_lt()
                                })
                            {
                                continue;
                            }

                            let candidate = TextMatch {
                                repository_id: repository_id.clone(),
                                path: rel_path.clone(),
                                line: line_number,
                                column,
                                excerpt: excerpt_for_line
                                    .get_or_insert_with(|| line.to_owned())
                                    .clone(),
                            };

                            if use_bounded_retention {
                                retain_bounded_match(&mut matches, query.limit, candidate);
                            } else {
                                matches.push(candidate);
                            }
                        }
                    }
                    continue;
                }

                let file = match fs::File::open(&path) {
                    Ok(file) => file,
                    Err(err) => {
                        diagnostics.entries.push(SearchDiagnostic {
                            repository_id: repository_id.clone(),
                            path: Some(rel_path),
                            kind: SearchDiagnosticKind::Read,
                            message: err.to_string(),
                        });
                        continue;
                    }
                };
                let mut reader = BufReader::new(file);
                let mut line = String::new();
                let mut line_number = 0usize;

                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) => break,
                        Ok(_) => {
                            line_number = line_number.saturating_add(1);
                        }
                        Err(err) => {
                            diagnostics.entries.push(SearchDiagnostic {
                                repository_id: repository_id.clone(),
                                path: Some(rel_path.clone()),
                                kind: SearchDiagnosticKind::Read,
                                message: err.to_string(),
                            });
                            break;
                        }
                    }

                    trim_trailing_newline(&mut line);
                    match_columns(&line, &mut match_columns_buffer);
                    if match_columns_buffer.is_empty() {
                        continue;
                    }

                    let mut excerpt_for_line: Option<String> = None;
                    for &column in &match_columns_buffer {
                        if use_bounded_retention
                            && matches.len() == query.limit
                            && matches.last().is_some_and(|worst| {
                                !text_match_candidate_order(
                                    &repository_id,
                                    &rel_path,
                                    line_number,
                                    column,
                                    &line,
                                    worst,
                                )
                                .is_lt()
                            })
                        {
                            continue;
                        }

                        let candidate = TextMatch {
                            repository_id: repository_id.clone(),
                            path: rel_path.clone(),
                            line: line_number,
                            column,
                            excerpt: excerpt_for_line.get_or_insert_with(|| line.clone()).clone(),
                        };

                        if use_bounded_retention {
                            retain_bounded_match(&mut matches, query.limit, candidate);
                        } else {
                            matches.push(candidate);
                        }
                    }
                }
            }
        }

        sort_search_diagnostics_deterministically(&mut diagnostics.entries);

        if use_bounded_retention {
            return Ok(SearchExecutionOutput {
                matches,
                diagnostics,
            });
        }

        sort_matches_deterministically(&mut matches);
        matches.truncate(query.limit);

        Ok(SearchExecutionOutput {
            matches,
            diagnostics,
        })
    }

    fn search_with_matcher<F, P>(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
        mut file_may_match: P,
        mut match_columns: F,
    ) -> FriggResult<SearchExecutionOutput>
    where
        P: FnMut(&str) -> bool,
        F: FnMut(&str, &mut Vec<usize>),
    {
        let use_bounded_retention = query.limit <= BOUNDED_SEARCH_RESULT_LIMIT_THRESHOLD;
        let mut matches = if use_bounded_retention {
            Vec::with_capacity(query.limit)
        } else {
            Vec::new()
        };
        let mut diagnostics = SearchExecutionDiagnostics::default();
        let mut match_columns_buffer = Vec::new();
        let mut repositories = self.config.repositories();
        repositories.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.root_path.cmp(&right.root_path))
        });

        for repo in repositories {
            if filters
                .repository_id
                .as_ref()
                .is_some_and(|repository_id| repository_id != &repo.repository_id.0)
            {
                continue;
            }
            let repository_id = repo.repository_id.0.clone();
            let root = Path::new(&repo.root_path);
            let file_candidates = self.candidate_files_for_repository(
                &repository_id,
                root,
                query,
                filters,
                &mut diagnostics,
            );

            for (rel_path, path) in file_candidates {
                let content = match fs::read_to_string(&path) {
                    Ok(content) => content,
                    Err(err) => {
                        diagnostics.entries.push(SearchDiagnostic {
                            repository_id: repository_id.clone(),
                            path: Some(rel_path),
                            kind: SearchDiagnosticKind::Read,
                            message: err.to_string(),
                        });
                        continue;
                    }
                };
                let content = scrub_search_content(&rel_path, &content);
                if !file_may_match(content.as_ref()) {
                    continue;
                }

                for (line_idx, line) in content.lines().enumerate() {
                    match_columns(line, &mut match_columns_buffer);
                    if match_columns_buffer.is_empty() {
                        continue;
                    }

                    let line_number = line_idx + 1;
                    let mut excerpt_for_line: Option<String> = None;

                    for &column in &match_columns_buffer {
                        if use_bounded_retention
                            && matches.len() == query.limit
                            && matches.last().is_some_and(|worst| {
                                !text_match_candidate_order(
                                    &repository_id,
                                    &rel_path,
                                    line_number,
                                    column,
                                    line,
                                    worst,
                                )
                                .is_lt()
                            })
                        {
                            continue;
                        }

                        let candidate = TextMatch {
                            repository_id: repository_id.clone(),
                            path: rel_path.clone(),
                            line: line_number,
                            column,
                            excerpt: excerpt_for_line
                                .get_or_insert_with(|| line.to_owned())
                                .clone(),
                        };

                        if use_bounded_retention {
                            retain_bounded_match(&mut matches, query.limit, candidate);
                        } else {
                            matches.push(candidate);
                        }
                    }
                }
            }
        }

        sort_search_diagnostics_deterministically(&mut diagnostics.entries);

        if use_bounded_retention {
            return Ok(SearchExecutionOutput {
                matches,
                diagnostics,
            });
        }

        sort_matches_deterministically(&mut matches);
        matches.truncate(query.limit);

        Ok(SearchExecutionOutput {
            matches,
            diagnostics,
        })
    }
}

fn trim_trailing_newline(line: &mut String) {
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
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
}

fn walk_candidate_files_for_repository(
    repository_id: &str,
    root: &Path,
    query: &SearchTextQuery,
    filters: &NormalizedSearchFilters,
    diagnostics: &mut SearchExecutionDiagnostics,
) -> Vec<(String, std::path::PathBuf)> {
    let walker = search_walk_builder(root).build();
    let mut file_candidates = Vec::new();

    for dent in walker {
        let dent = match dent {
            Ok(entry) => entry,
            Err(err) => {
                diagnostics.entries.push(SearchDiagnostic {
                    repository_id: repository_id.to_owned(),
                    path: None,
                    kind: SearchDiagnosticKind::Walk,
                    message: err.to_string(),
                });
                continue;
            }
        };
        if !dent.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = dent.path();
        if hard_excluded_runtime_path(root, path) {
            continue;
        }
        let rel_path = normalize_repository_relative_path(root, path);

        if let Some(language) = filters.language {
            if !language.matches_path(path) {
                continue;
            }
        }

        if let Some(path_regex) = &query.path_regex {
            if !path_regex.is_match(&rel_path) {
                continue;
            }
        }

        file_candidates.push((rel_path, path.to_path_buf()));
    }
    file_candidates.sort_by(|left, right| left.0.cmp(&right.0));
    file_candidates.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    file_candidates
}

fn search_walk_builder(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.standard_filters(true).require_git(false);
    builder
}

fn hard_excluded_runtime_path(root: &Path, path: &Path) -> bool {
    let relative = if path.is_absolute() {
        let Ok(relative) = path.strip_prefix(root) else {
            return true;
        };
        relative
    } else {
        path
    };
    let Some(component) = relative.components().next() else {
        return false;
    };
    matches!(
        component.as_os_str().to_string_lossy().as_ref(),
        ".frigg" | ".git" | "target"
    )
}

pub fn compile_safe_regex(pattern: &str) -> Result<Regex, RegexSearchError> {
    validate_regex_budget(pattern)?;

    RegexBuilder::new(pattern)
        .size_limit(MAX_REGEX_SIZE_LIMIT_BYTES)
        .dfa_size_limit(MAX_REGEX_DFA_SIZE_LIMIT_BYTES)
        .build()
        .map_err(RegexSearchError::InvalidRegex)
}

fn validate_regex_budget(pattern: &str) -> Result<(), RegexSearchError> {
    if pattern.is_empty() {
        return Err(RegexSearchError::EmptyPattern);
    }

    if pattern.len() > MAX_REGEX_PATTERN_BYTES {
        return Err(RegexSearchError::PatternTooLong {
            actual: pattern.len(),
            max: MAX_REGEX_PATTERN_BYTES,
        });
    }

    let alternations = count_unescaped_regex_chars(pattern, &['|']);
    if alternations > MAX_REGEX_ALTERNATIONS {
        return Err(RegexSearchError::TooManyAlternations {
            actual: alternations,
            max: MAX_REGEX_ALTERNATIONS,
        });
    }

    let groups = count_unescaped_regex_chars(pattern, &['(']);
    if groups > MAX_REGEX_GROUPS {
        return Err(RegexSearchError::TooManyGroups {
            actual: groups,
            max: MAX_REGEX_GROUPS,
        });
    }

    let quantifiers = count_unescaped_regex_chars(pattern, &['*', '+', '?', '{']);
    if quantifiers > MAX_REGEX_QUANTIFIERS {
        return Err(RegexSearchError::TooManyQuantifiers {
            actual: quantifiers,
            max: MAX_REGEX_QUANTIFIERS,
        });
    }

    Ok(())
}

fn count_unescaped_regex_chars(pattern: &str, targets: &[char]) -> usize {
    let mut count = 0usize;
    let mut escaped = false;
    let mut in_class = false;

    for ch in pattern.chars() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' {
            escaped = true;
            continue;
        }

        if ch == '[' {
            in_class = true;
            continue;
        }

        if ch == ']' && in_class {
            in_class = false;
            continue;
        }

        if in_class {
            continue;
        }

        if targets.contains(&ch) {
            count += 1;
        }
    }

    count
}

fn build_regex_prefilter_plan(pattern: &str) -> Option<RegexPrefilterPlan> {
    RegexPrefilterPlan::build(pattern)
}

fn extract_required_regex_literals(pattern: &str) -> Option<Vec<String>> {
    if pattern.is_empty() || !pattern.is_ascii() {
        return None;
    }

    let bytes = pattern.as_bytes();
    let mut index = 0usize;
    let mut literals = Vec::new();
    let mut current_literal = Vec::new();

    while index < bytes.len() {
        let atom = parse_regex_atom(bytes, &mut index)?;
        let quantifier = parse_regex_quantifier(bytes, &mut index)?;

        match atom {
            ParsedRegexAtom::Literal(byte) => {
                if quantifier.min_repetitions == 0 {
                    flush_required_literal(&mut literals, &mut current_literal);
                } else if quantifier.exact_one {
                    current_literal.push(byte);
                } else {
                    flush_required_literal(&mut literals, &mut current_literal);
                    literals.push(char::from(byte).to_string());
                }
            }
            ParsedRegexAtom::NonLiteral => {
                flush_required_literal(&mut literals, &mut current_literal);
            }
        }
    }

    flush_required_literal(&mut literals, &mut current_literal);

    if literals.is_empty() {
        return None;
    }

    let mut deduped = Vec::with_capacity(literals.len());
    for literal in literals {
        if !deduped.iter().any(|existing| existing == &literal) {
            deduped.push(literal);
        }
    }

    if deduped.is_empty() {
        None
    } else {
        Some(deduped)
    }
}

fn parse_regex_atom(bytes: &[u8], index: &mut usize) -> Option<ParsedRegexAtom> {
    let byte = *bytes.get(*index)?;
    *index += 1;

    match byte {
        b'|' | b'(' | b')' => None,
        b'[' => {
            parse_char_class(bytes, index)?;
            Some(ParsedRegexAtom::NonLiteral)
        }
        b'\\' => parse_escape_atom(bytes, index),
        b'.' | b'^' | b'$' => Some(ParsedRegexAtom::NonLiteral),
        b'*' | b'+' | b'?' | b'{' | b'}' => None,
        _ => Some(ParsedRegexAtom::Literal(byte)),
    }
}

fn parse_char_class(bytes: &[u8], index: &mut usize) -> Option<()> {
    let mut escaped = false;
    while let Some(&byte) = bytes.get(*index) {
        *index += 1;
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == b']' {
            return Some(());
        }
    }
    None
}

fn parse_escape_atom(bytes: &[u8], index: &mut usize) -> Option<ParsedRegexAtom> {
    let escaped = *bytes.get(*index)?;
    *index += 1;

    if is_regex_literal_escape(escaped) {
        return Some(ParsedRegexAtom::Literal(escaped));
    }

    if is_supported_non_literal_escape(escaped) {
        return Some(ParsedRegexAtom::NonLiteral);
    }

    None
}

fn is_regex_literal_escape(escaped: u8) -> bool {
    matches!(
        escaped,
        b'\\'
            | b'.'
            | b'+'
            | b'*'
            | b'?'
            | b'|'
            | b'('
            | b')'
            | b'['
            | b']'
            | b'{'
            | b'}'
            | b'^'
            | b'$'
            | b'-'
    )
}

fn is_supported_non_literal_escape(escaped: u8) -> bool {
    matches!(
        escaped,
        b'd' | b'D'
            | b's'
            | b'S'
            | b'w'
            | b'W'
            | b'b'
            | b'B'
            | b'A'
            | b'z'
            | b'n'
            | b'r'
            | b't'
            | b'f'
            | b'v'
    )
}

fn parse_regex_quantifier(bytes: &[u8], index: &mut usize) -> Option<ParsedRegexQuantifier> {
    let mut quantifier = ParsedRegexQuantifier {
        min_repetitions: 1,
        exact_one: true,
    };
    let Some(&byte) = bytes.get(*index) else {
        return Some(quantifier);
    };

    match byte {
        b'?' => {
            *index += 1;
            quantifier.min_repetitions = 0;
            quantifier.exact_one = false;
            consume_lazy_quantifier_suffix(bytes, index);
        }
        b'*' => {
            *index += 1;
            quantifier.min_repetitions = 0;
            quantifier.exact_one = false;
            consume_lazy_quantifier_suffix(bytes, index);
        }
        b'+' => {
            *index += 1;
            quantifier.min_repetitions = 1;
            quantifier.exact_one = false;
            consume_lazy_quantifier_suffix(bytes, index);
        }
        b'{' => {
            *index += 1;
            let (min, max) = parse_braced_quantifier(bytes, index)?;
            quantifier.min_repetitions = min;
            quantifier.exact_one = min == 1 && max == Some(1);
            consume_lazy_quantifier_suffix(bytes, index);
        }
        _ => {}
    }

    Some(quantifier)
}

fn parse_braced_quantifier(bytes: &[u8], index: &mut usize) -> Option<(usize, Option<usize>)> {
    let min = parse_quantifier_number(bytes, index)?;
    let mut max = Some(min);

    match bytes.get(*index).copied() {
        Some(b'}') => {
            *index += 1;
        }
        Some(b',') => {
            *index += 1;
            match bytes.get(*index).copied() {
                Some(b'}') => {
                    *index += 1;
                    max = None;
                }
                Some(_) => {
                    let upper = parse_quantifier_number(bytes, index)?;
                    if upper < min {
                        return None;
                    }
                    max = Some(upper);
                    if bytes.get(*index).copied() != Some(b'}') {
                        return None;
                    }
                    *index += 1;
                }
                None => return None,
            }
        }
        _ => return None,
    }

    Some((min, max))
}

fn parse_quantifier_number(bytes: &[u8], index: &mut usize) -> Option<usize> {
    let start = *index;
    while let Some(&byte) = bytes.get(*index) {
        if !byte.is_ascii_digit() {
            break;
        }
        *index += 1;
    }
    if *index == start {
        return None;
    }

    std::str::from_utf8(&bytes[start..*index])
        .ok()?
        .parse()
        .ok()
}

fn consume_lazy_quantifier_suffix(bytes: &[u8], index: &mut usize) {
    if bytes.get(*index).copied() == Some(b'?') {
        *index += 1;
    }
}

fn flush_required_literal(literals: &mut Vec<String>, current_literal: &mut Vec<u8>) {
    if current_literal.is_empty() {
        return;
    }
    literals.push(String::from_utf8_lossy(current_literal).to_string());
    current_literal.clear();
}

fn literal_trigram_hashes(bytes: &[u8]) -> Vec<usize> {
    let mut hashes = bytes
        .windows(3)
        .map(|window| trigram_hash(window[0], window[1], window[2]))
        .collect::<Vec<_>>();
    hashes.sort_unstable();
    hashes.dedup();
    hashes
}

fn trigram_hash(left: u8, middle: u8, right: u8) -> usize {
    let packed = (u32::from(left) << 16) | (u32::from(middle) << 8) | u32::from(right);
    (packed.wrapping_mul(REGEX_TRIGRAM_HASH_MULTIPLIER) as usize) & (REGEX_TRIGRAM_BITMAP_BITS - 1)
}

fn regex_error_to_frigg_error(err: RegexSearchError) -> FriggError {
    FriggError::InvalidInput(format!("regex search error [{}]: {err}", err.code()))
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
        Some(raw) => Some(NormalizedLanguage::parse(raw).ok_or_else(|| {
            FriggError::InvalidInput(format!(
                "unsupported language filter '{raw}'; supported values: rust, rs, php"
            ))
        })?),
        None => None,
    };

    Ok(NormalizedSearchFilters {
        repository_id,
        language,
    })
}

fn sort_matches_deterministically(matches: &mut [TextMatch]) {
    matches.sort_by(text_match_order);
}

fn sort_search_diagnostics_deterministically(diagnostics: &mut [SearchDiagnostic]) {
    diagnostics.sort_by(search_diagnostic_order);
}

fn search_diagnostic_order(
    left: &SearchDiagnostic,
    right: &SearchDiagnostic,
) -> std::cmp::Ordering {
    left.repository_id
        .cmp(&right.repository_id)
        .then(left.path.cmp(&right.path))
        .then(left.kind.cmp(&right.kind))
        .then(left.message.cmp(&right.message))
}

fn normalize_repository_relative_path(root: &Path, path: &Path) -> String {
    let normalized = path
        .strip_prefix(root)
        .ok()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
        .replace('\\', "/");
    normalized.trim_start_matches("./").to_owned()
}

fn text_match_order(left: &TextMatch, right: &TextMatch) -> std::cmp::Ordering {
    left.repository_id
        .cmp(&right.repository_id)
        .then(left.path.cmp(&right.path))
        .then(left.line.cmp(&right.line))
        .then(left.column.cmp(&right.column))
        .then(left.excerpt.cmp(&right.excerpt))
}

fn text_match_candidate_order(
    repository_id: &str,
    path: &str,
    line: usize,
    column: usize,
    excerpt: &str,
    existing: &TextMatch,
) -> std::cmp::Ordering {
    repository_id
        .cmp(&existing.repository_id)
        .then(path.cmp(&existing.path))
        .then(line.cmp(&existing.line))
        .then(column.cmp(&existing.column))
        .then(excerpt.cmp(&existing.excerpt))
}

fn retain_bounded_match(matches: &mut Vec<TextMatch>, limit: usize, candidate: TextMatch) {
    if matches.len() < limit {
        insert_sorted_match(matches, candidate);
        return;
    }

    if matches
        .last()
        .is_some_and(|worst| text_match_order(&candidate, worst).is_lt())
    {
        insert_sorted_match(matches, candidate);
        matches.truncate(limit);
    }
}

fn insert_sorted_match(matches: &mut Vec<TextMatch>, candidate: TextMatch) {
    let insert_at = matches.partition_point(|existing| {
        matches!(
            text_match_order(existing, &candidate),
            std::cmp::Ordering::Less
        )
    });
    matches.insert(insert_at, candidate);
}

#[cfg(test)]
fn build_hybrid_lexical_hits(matches: &[TextMatch]) -> Vec<HybridChannelHit> {
    build_hybrid_lexical_hits_with_intent(matches, &HybridRankingIntent::default())
}

#[cfg(test)]
fn build_hybrid_lexical_hits_for_query(
    matches: &[TextMatch],
    query_text: &str,
) -> Vec<HybridChannelHit> {
    let intent = HybridRankingIntent::from_query(query_text);
    build_hybrid_lexical_hits_with_intent(matches, &intent)
}

fn build_hybrid_lexical_hits_with_intent(
    matches: &[TextMatch],
    intent: &HybridRankingIntent,
) -> Vec<HybridChannelHit> {
    let mut frequency_by_document: BTreeMap<(String, String), f32> = BTreeMap::new();
    for found in matches {
        let key = (found.repository_id.clone(), found.path.clone());
        *frequency_by_document.entry(key).or_insert(0.0) += 1.0;
    }

    matches
        .iter()
        .map(|found| {
            let key = (found.repository_id.clone(), found.path.clone());
            let frequency = *frequency_by_document.get(&key).unwrap_or(&1.0);
            let raw_score =
                frequency.sqrt() * hybrid_path_quality_multiplier_with_intent(&found.path, intent);
            HybridChannelHit {
                document: HybridDocumentRef {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: 1,
                    column: 1,
                },
                raw_score,
                excerpt: found.excerpt.clone(),
                provenance_id: format!("text:{}:{}:{}", found.path, found.line, found.column),
            }
        })
        .collect()
}

fn hybrid_path_quality_multiplier_with_intent(path: &str, intent: &HybridRankingIntent) -> f32 {
    let class = hybrid_source_class(path);
    let mut multiplier = match class {
        HybridSourceClass::ErrorContracts => 1.0,
        HybridSourceClass::ToolContracts => 1.0,
        HybridSourceClass::BenchmarkDocs => 0.98,
        HybridSourceClass::Playbooks => {
            if intent.penalize_playbook_self_reference {
                0.25
            } else {
                0.45
            }
        }
        HybridSourceClass::Documentation => 0.88,
        HybridSourceClass::Readme => 0.78,
        HybridSourceClass::Specs => 0.82,
        HybridSourceClass::Fixtures => 0.92,
        HybridSourceClass::Tests => 0.97,
        HybridSourceClass::Runtime => 1.0,
        HybridSourceClass::Other => {
            match Path::new(path).extension().and_then(|ext| ext.to_str()) {
                Some(
                    "rs" | "php" | "go" | "py" | "ts" | "tsx" | "js" | "jsx" | "java" | "kt"
                    | "kts",
                ) => 1.0,
                _ => 0.9,
            }
        }
    };

    if intent.wants_docs
        && matches!(
            class,
            HybridSourceClass::Documentation
                | HybridSourceClass::ErrorContracts
                | HybridSourceClass::ToolContracts
                | HybridSourceClass::BenchmarkDocs
        )
    {
        multiplier *= 1.36;
    }
    if intent.wants_readme && class == HybridSourceClass::Readme {
        multiplier *= 1.15;
    }
    if intent.wants_readme && path == "README.md" {
        multiplier *= 1.45;
    }
    if intent.wants_contracts
        && matches!(
            class,
            HybridSourceClass::ErrorContracts | HybridSourceClass::ToolContracts
        )
    {
        multiplier *= 1.55;
    }
    if intent.wants_error_taxonomy && class == HybridSourceClass::ErrorContracts {
        multiplier *= 1.95;
    }
    if path == "contracts/errors.md" && (intent.wants_error_taxonomy || intent.wants_contracts) {
        multiplier *= 1.70;
    }
    if intent.wants_error_taxonomy && path == "crates/cli/src/mcp/server.rs" {
        multiplier *= 1.35;
    }
    if intent.wants_error_taxonomy && path == "crates/cli/src/mcp/deep_search.rs" {
        multiplier *= 1.18;
    }
    if intent.wants_tool_contracts && class == HybridSourceClass::ToolContracts {
        multiplier *= 2.10;
    }
    if path == "contracts/tools/v1/README.md" && intent.wants_tool_contracts {
        multiplier *= 1.75;
    }
    if intent.wants_tool_contracts && path == "crates/cli/src/mcp/tool_surface.rs" {
        multiplier *= 1.12;
    }
    if intent.wants_tool_contracts && path == "crates/cli/tests/tool_surface_parity.rs" {
        multiplier *= 1.10;
    }
    if intent.wants_benchmarks && class == HybridSourceClass::BenchmarkDocs {
        multiplier *= 2.00;
    }
    if intent.wants_benchmarks && path == "benchmarks/deep-search.md" {
        multiplier *= 1.65;
    }
    if intent.wants_contracts && class == HybridSourceClass::Readme {
        multiplier *= 0.65;
    }
    if intent.wants_tool_contracts && class == HybridSourceClass::Readme {
        multiplier *= 0.68;
    }
    if intent.wants_benchmarks && class == HybridSourceClass::Readme {
        multiplier *= 0.68;
    }
    if intent.wants_tests && class == HybridSourceClass::Tests {
        multiplier *= 1.12;
    }
    if intent.wants_fixtures && class == HybridSourceClass::Fixtures {
        multiplier *= 1.14;
    }
    if intent.wants_runtime && class == HybridSourceClass::Runtime {
        multiplier *= 1.05;
    }

    multiplier
}

fn hybrid_source_class(path: &str) -> HybridSourceClass {
    if path.starts_with("playbooks/") {
        return HybridSourceClass::Playbooks;
    }
    if is_error_contract_path(path) {
        return HybridSourceClass::ErrorContracts;
    }
    if is_tool_contract_path(path) {
        return HybridSourceClass::ToolContracts;
    }
    if path.starts_with("benchmarks/") {
        return HybridSourceClass::BenchmarkDocs;
    }
    if is_readme_path(path) {
        return HybridSourceClass::Readme;
    }
    if path.starts_with("docs/") {
        return HybridSourceClass::Documentation;
    }
    if path.starts_with("fixtures/") {
        return HybridSourceClass::Fixtures;
    }
    if path.starts_with("specs/") {
        return HybridSourceClass::Specs;
    }
    if path.starts_with("tests/")
        || path.contains("/tests/")
        || path.ends_with("_test.rs")
        || path.ends_with("_tests.rs")
    {
        return HybridSourceClass::Tests;
    }
    if path.starts_with("src/") || path.starts_with("crates/") {
        return HybridSourceClass::Runtime;
    }

    HybridSourceClass::Other
}

fn is_error_contract_path(path: &str) -> bool {
    path == "contracts/errors.md"
}

fn is_tool_contract_path(path: &str) -> bool {
    path.starts_with("contracts/tools/")
}

fn is_readme_path(path: &str) -> bool {
    path == "README.md" || path.ends_with("/README.md")
}

fn hybrid_selection_score(
    evidence: &HybridRankedEvidence,
    intent: &HybridRankingIntent,
    seen_classes: &BTreeMap<HybridSourceClass, usize>,
) -> f32 {
    let class = hybrid_source_class(&evidence.document.path);
    let seen_count = seen_classes.get(&class).copied().unwrap_or(0);
    let mut score = evidence.blended_score
        * hybrid_path_quality_multiplier_with_intent(&evidence.document.path, intent);

    if intent.wants_class(class) && seen_count == 0 {
        score += hybrid_class_novelty_bonus(class);
    }
    if seen_count > 0 {
        score -= hybrid_class_repeat_penalty(class) * seen_count as f32;
    }

    score
}

fn hybrid_class_novelty_bonus(class: HybridSourceClass) -> f32 {
    match class {
        HybridSourceClass::ErrorContracts
        | HybridSourceClass::ToolContracts
        | HybridSourceClass::BenchmarkDocs => 0.08,
        HybridSourceClass::Documentation
        | HybridSourceClass::Runtime
        | HybridSourceClass::Tests => 0.04,
        HybridSourceClass::Fixtures => 0.035,
        HybridSourceClass::Readme => 0.02,
        HybridSourceClass::Playbooks | HybridSourceClass::Specs | HybridSourceClass::Other => 0.0,
    }
}

fn hybrid_class_repeat_penalty(class: HybridSourceClass) -> f32 {
    match class {
        HybridSourceClass::ToolContracts => 0.09,
        HybridSourceClass::BenchmarkDocs => 0.07,
        HybridSourceClass::ErrorContracts | HybridSourceClass::Documentation => 0.05,
        HybridSourceClass::Readme => 0.03,
        HybridSourceClass::Runtime | HybridSourceClass::Tests | HybridSourceClass::Fixtures => {
            0.015
        }
        HybridSourceClass::Playbooks | HybridSourceClass::Specs | HybridSourceClass::Other => 0.01,
    }
}

fn hybrid_ranked_evidence_order(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
) -> std::cmp::Ordering {
    right
        .blended_score
        .total_cmp(&left.blended_score)
        .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
        .then_with(|| right.graph_score.total_cmp(&left.graph_score))
        .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
        .then(left.document.cmp(&right.document))
        .then(left.excerpt.cmp(&right.excerpt))
}

fn build_hybrid_lexical_recall_regex(query_text: &str) -> Option<String> {
    let tokens = hybrid_lexical_recall_tokens(query_text);
    if tokens.len() < 2 {
        return None;
    }

    let token_pattern = tokens
        .into_iter()
        .take(HYBRID_LEXICAL_RECALL_MAX_TOKENS)
        .map(|token| regex::escape(&token))
        .collect::<Vec<_>>()
        .join("|");
    if token_pattern.is_empty() {
        return None;
    }

    Some(format!(r"(?i)\b(?:{token_pattern})\b"))
}

fn hybrid_lexical_recall_tokens(query_text: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in query_text.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            current.push(ch.to_ascii_lowercase());
            continue;
        }

        if let Some(token) = normalize_hybrid_recall_token(&current) {
            if seen.insert(token.clone()) {
                tokens.push(token);
            }
            current.clear();
        } else {
            current.clear();
        }
    }
    if let Some(token) = normalize_hybrid_recall_token(&current) {
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }

    tokens
}

fn normalize_hybrid_recall_token(token: &str) -> Option<String> {
    if token.len() < HYBRID_LEXICAL_RECALL_MIN_TOKEN_LEN {
        return None;
    }

    let token = token.trim().to_ascii_lowercase();
    if token.is_empty() || is_low_signal_hybrid_recall_token(&token) {
        return None;
    }

    Some(token)
}

fn is_low_signal_hybrid_recall_token(token: &str) -> bool {
    matches!(
        token,
        "about"
            | "does"
            | "from"
            | "frigg"
            | "into"
            | "that"
            | "these"
            | "this"
            | "those"
            | "turn"
            | "what"
            | "when"
            | "where"
            | "which"
    )
}

fn merge_hybrid_lexical_search_output(
    base: &mut SearchExecutionOutput,
    supplement: SearchExecutionOutput,
    limit: usize,
) {
    let mut merged_by_key: BTreeMap<(String, String, usize, usize, String), TextMatch> =
        BTreeMap::new();
    for found in &base.matches {
        merged_by_key.insert(
            (
                found.repository_id.clone(),
                found.path.clone(),
                found.line,
                found.column,
                found.excerpt.clone(),
            ),
            found.clone(),
        );
    }
    for found in supplement.matches {
        merged_by_key
            .entry((
                found.repository_id.clone(),
                found.path.clone(),
                found.line,
                found.column,
                found.excerpt.clone(),
            ))
            .or_insert(found);
    }

    base.matches = merged_by_key.into_values().collect::<Vec<_>>();
    sort_matches_deterministically(&mut base.matches);
    base.matches.truncate(limit);

    base.diagnostics
        .entries
        .extend(supplement.diagnostics.entries);
    sort_search_diagnostics_deterministically(&mut base.diagnostics.entries);
    base.diagnostics.entries.dedup();
}

fn semantic_excerpt(content_text: &str, fallback_path: &str) -> String {
    content_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback_path.to_owned())
}

fn cosine_similarity(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.is_empty() || left.len() != right.len() {
        return None;
    }

    let mut dot = 0.0_f32;
    let mut left_norm = 0.0_f32;
    let mut right_norm = 0.0_f32;
    for (left_value, right_value) in left.iter().zip(right.iter()) {
        dot += left_value * right_value;
        left_norm += left_value * left_value;
        right_norm += right_value * right_value;
    }

    if left_norm <= 0.0 || right_norm <= 0.0 {
        return Some(0.0);
    }

    Some(dot / (left_norm.sqrt() * right_norm.sqrt()))
}

fn semantic_projection_score(
    query_embedding: &[f32],
    projection: &SemanticChunkEmbeddingProjection,
    repository_id: &str,
) -> FriggResult<f32> {
    cosine_similarity(query_embedding, &projection.embedding).ok_or_else(|| {
        FriggError::Internal(format!(
            "semantic similarity dimension mismatch for repository '{repository_id}' path '{}' chunk_id='{}' (query={}, chunk={})",
            projection.path,
            projection.chunk_id,
            query_embedding.len(),
            projection.embedding.len()
        ))
    })
}

fn apply_hybrid_channel_hits(
    hits: &[HybridChannelHit],
    channel: HybridChannel,
    by_document: &mut BTreeMap<HybridDocumentRef, HybridScoreAccumulator>,
) {
    if hits.is_empty() {
        return;
    }

    let max_raw_score = hits
        .iter()
        .map(|hit| hit.raw_score.max(0.0))
        .fold(0.0_f32, f32::max);
    let mut ordered_hits = hits.to_vec();
    ordered_hits.sort_by(|left, right| {
        left.document
            .cmp(&right.document)
            .then_with(|| right.raw_score.total_cmp(&left.raw_score))
            .then(left.provenance_id.cmp(&right.provenance_id))
            .then(left.excerpt.cmp(&right.excerpt))
    });

    for hit in ordered_hits {
        let normalized_score = normalize_channel_score(hit.raw_score, max_raw_score);
        let state =
            by_document
                .entry(hit.document.clone())
                .or_insert_with(|| HybridScoreAccumulator {
                    excerpt: hit.excerpt.clone(),
                    lexical_score: 0.0,
                    graph_score: 0.0,
                    semantic_score: 0.0,
                    lexical_sources: Vec::new(),
                    graph_sources: Vec::new(),
                    semantic_sources: Vec::new(),
                });

        if state.excerpt.is_empty() {
            state.excerpt = hit.excerpt.clone();
        }

        match channel {
            HybridChannel::Lexical => {
                if normalized_score > state.lexical_score {
                    state.lexical_score = normalized_score;
                }
                insert_sorted_unique(&mut state.lexical_sources, hit.provenance_id);
            }
            HybridChannel::Graph => {
                if normalized_score > state.graph_score {
                    state.graph_score = normalized_score;
                }
                insert_sorted_unique(&mut state.graph_sources, hit.provenance_id);
            }
            HybridChannel::Semantic => {
                if normalized_score > state.semantic_score {
                    state.semantic_score = normalized_score;
                }
                insert_sorted_unique(&mut state.semantic_sources, hit.provenance_id);
            }
        }
    }
}

fn normalize_channel_score(raw_score: f32, max_raw_score: f32) -> f32 {
    if max_raw_score <= 0.0 {
        return 0.0;
    }

    (raw_score.max(0.0) / max_raw_score).clamp(0.0, 1.0)
}

fn insert_sorted_unique(values: &mut Vec<String>, value: String) {
    match values.binary_search(&value) {
        Ok(_) => {}
        Err(index) => values.insert(index, value),
    }
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
        HybridChannelHit, HybridChannelWeights, HybridDocumentRef, HybridSemanticStatus,
        HybridSourceClass, MAX_REGEX_ALTERNATIONS, MAX_REGEX_GROUPS, MAX_REGEX_PATTERN_BYTES,
        MAX_REGEX_QUANTIFIERS, RegexSearchError, SearchDiagnosticKind, SearchFilters,
        SearchHybridQuery, SearchTextQuery, SemanticRuntimeQueryEmbeddingExecutor, TextSearcher,
        build_hybrid_lexical_hits, build_hybrid_lexical_hits_for_query,
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
