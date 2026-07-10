//! Public query and result types for Frigg's retrieval layer. These records keep the searcher
//! boundary explicit so MCP handlers, playbooks, and tests can all talk about the same execution
//! semantics.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::domain::{
    ChannelDiagnostic, ChannelHealth, ChannelHealthStatus, ChannelResult, ChannelStats,
    EvidenceAnchor, EvidenceChannel, EvidenceDocumentRef, EvidenceHit, FriggError, FriggResult,
    model::TextMatch,
};
use crate::indexer::PhpDeclarationRelation;
use crate::languages::{BladeSourceEvidence, PhpSourceEvidence, SymbolLanguage};

use super::attribution::SearchStageAttribution;
use super::policy::PostSelectionTrace;

#[derive(Debug, Clone)]
/// Input for direct lexical search when callers want raw text recall without the hybrid ranking
/// stack.
pub struct SearchTextQuery {
    /// Literal or regex pattern text, depending on the search entry point invoked.
    pub query: String,
    /// Optional repository-relative path filter applied before scanning candidates.
    pub path_regex: Option<regex::Regex>,
    /// Maximum number of matches to retain after deterministic ordering.
    pub limit: usize,
}

#[derive(Debug, Clone)]
/// Shared repository-level filters used to scope both lexical and hybrid retrieval paths.
pub struct SearchFilters {
    /// Restrict retrieval to one configured repository id when set.
    pub repository_id: Option<String>,
    /// Restrict retrieval to files classified as one supported source language when set.
    pub language: Option<String>,
    /// Include hidden repository paths during candidate intake.
    pub include_hidden: bool,
}

impl Default for SearchFilters {
    fn default() -> Self {
        Self {
            repository_id: None,
            language: None,
            include_hidden: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// Category of non-fatal issue encountered while walking or reading repository candidates.
pub enum SearchDiagnosticKind {
    /// Candidate discovery failed for a subtree or repository root.
    Walk,
    /// A candidate file could not be read during scanning.
    Read,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One diagnostic emitted while building or scanning the candidate universe.
pub struct SearchDiagnostic {
    /// Repository that produced the diagnostic.
    pub repository_id: String,
    /// Candidate path when the issue is file-specific.
    pub path: Option<String>,
    /// Whether the issue occurred during discovery or file read.
    pub kind: SearchDiagnosticKind,
    /// Human-readable explanation suitable for surfacing to callers.
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
/// Aggregated diagnostics from candidate intake and lexical scanning.
pub struct SearchExecutionDiagnostics {
    /// Ordered diagnostic entries collected across repositories.
    pub entries: Vec<SearchDiagnostic>,
}

impl SearchExecutionDiagnostics {
    /// Total number of diagnostic entries recorded for the run.
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    /// Count of diagnostics matching one [`SearchDiagnosticKind`].
    pub fn count_by_kind(&self, kind: SearchDiagnosticKind) -> usize {
        self.entries
            .iter()
            .filter(|diagnostic| diagnostic.kind == kind)
            .count()
    }
}

#[derive(Debug, Clone, Default)]
/// Output of a lexical-only search pass, including diagnostics that explain degraded or partial
/// coverage.
pub struct SearchExecutionOutput {
    /// Number of matches before caller-side truncation.
    pub total_matches: usize,
    /// Bounded, deterministically ordered lexical matches.
    pub matches: Vec<TextMatch>,
    /// Walk and read issues encountered while scanning candidates.
    pub diagnostics: SearchExecutionDiagnostics,
    /// Backend that produced lexical hits when an accelerator was selected.
    pub lexical_backend: Option<SearchLexicalBackend>,
    /// Optional explanation when the backend fell back or mixed native and ripgrep paths.
    pub lexical_backend_note: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Lexical scan backend used for a search execution.
pub enum SearchLexicalBackend {
    /// Frigg's streaming native scanner over the candidate universe.
    Native,
    /// External `rg` accelerator over non-scrubbed candidates.
    Ripgrep,
    /// Ripgrep for most candidates with native fallback for scrubbed markdown content.
    Mixed,
}

impl SearchLexicalBackend {
    /// Stable snake_case label for diagnostics and MCP payloads.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Ripgrep => "ripgrep",
            Self::Mixed => "mixed",
        }
    }
}

/// One filesystem candidate admitted into lexical or hybrid scan scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchCandidateFile {
    pub(crate) relative_path: String,
    pub(crate) absolute_path: PathBuf,
}

/// Per-repository candidate set, optionally pinned to a validated manifest snapshot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RepositoryCandidateUniverse {
    pub(crate) repository_id: String,
    pub(crate) root: PathBuf,
    pub(crate) snapshot_id: Option<String>,
    pub(crate) candidates: Vec<SearchCandidateFile>,
}

/// Multi-repo candidate universe plus walk/read diagnostics from universe construction.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SearchCandidateUniverse {
    pub(crate) repositories: Vec<RepositoryCandidateUniverse>,
    pub(crate) diagnostics: SearchExecutionDiagnostics,
}

/// Candidate-universe build result with intake timing and manifest-backed repository counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SearchCandidateUniverseBuild {
    pub(crate) universe: SearchCandidateUniverse,
    pub(crate) repository_count: usize,
    pub(crate) candidate_count: usize,
    pub(crate) manifest_backed_repository_count: usize,
    pub(crate) candidate_intake_elapsed_us: u64,
    pub(crate) freshness_validation_elapsed_us: u64,
}

/// Manifest-derived candidate paths for one repository after freshness validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestCandidateFilesBuild {
    pub(crate) snapshot_id: String,
    pub(crate) candidates: Vec<(String, PathBuf)>,
    pub(crate) candidate_intake_elapsed_us: u64,
    pub(crate) freshness_validation_elapsed_us: u64,
}

/// Repository document identity shared by hybrid channel hits and ranked evidence.
pub type HybridDocumentRef = EvidenceDocumentRef;
/// Single retrieval hit from one hybrid channel before ranker blending.
pub type HybridChannelHit = EvidenceHit;

#[derive(Debug, Clone, Copy, PartialEq)]
/// Relative influence assigned to each hybrid retrieval channel before result diversification.
pub struct HybridChannelWeights {
    /// Weight applied to lexical and path-witness family scores.
    pub lexical: f32,
    /// Weight applied to graph-precise expansion hits.
    pub graph: f32,
    /// Weight applied to semantic vector retrieval hits.
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
    /// Rejects negative weights and the all-zero configuration that would leave fusion with no channel signal.
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
/// Input for Frigg's multi-signal retrieval path that can combine lexical, graph, and semantic
/// evidence behind one call.
pub struct SearchHybridQuery {
    /// Natural-language or keyword query text driving all retrieval channels.
    pub query: String,
    /// Maximum diversified matches to return after post-selection guardrails.
    pub limit: usize,
    /// Relative channel weights validated before ranker fusion.
    pub weights: HybridChannelWeights,
    /// Explicit semantic on/off override; defaults to runtime configuration when unset.
    pub semantic: Option<bool>,
}

/// Semantic channel health status surfaced on hybrid execution notes.
pub type HybridSemanticStatus = ChannelHealthStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Execution-side explanation of how the hybrid search actually ran, including whether semantic
/// recall participated or the query fell back to a narrower mode.
pub struct HybridExecutionNote {
    /// Whether the caller or runtime asked for semantic retrieval.
    pub semantic_requested: bool,
    /// Whether semantic retrieval produced at least one fused match.
    pub semantic_enabled: bool,
    /// Semantic channel health after embedding and vector lookup.
    pub semantic_status: HybridSemanticStatus,
    /// Disabled or degraded reason when semantic recall did not run cleanly.
    pub semantic_reason: Option<String>,
    /// Semantic candidates considered before relative score retention.
    pub semantic_candidate_count: usize,
    /// Semantic hits retained for ranker fusion.
    pub semantic_hit_count: usize,
    /// Semantic hits that survived into the final diversified match list.
    pub semantic_match_count: usize,
    /// True when semantic recall did not contribute usable pre-fusion hits.
    pub lexical_only_mode: bool,
    /// Lexical backend used while seeding hybrid channels.
    pub lexical_backend: Option<SearchLexicalBackend>,
    /// Optional note when lexical seeding mixed or fell back across backends.
    pub lexical_backend_note: Option<String>,
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
            lexical_only_mode: true,
            lexical_backend: None,
            lexical_backend_note: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
/// Top-level result of a hybrid retrieval run, pairing final matches with diagnostics, channel
/// health, and execution attribution.
pub struct SearchHybridExecutionOutput {
    /// Final diversified matches delivered to callers.
    pub matches: Vec<HybridRankedEvidence>,
    /// Pre-diversification ranked anchors retained for inspection and tooling.
    pub ranked_anchors: Vec<HybridRankedEvidence>,
    #[allow(dead_code)]
    pub(crate) coverage_grouped_pool: Vec<HybridRankedEvidence>,
    /// Walk and read issues encountered while scanning candidates.
    pub diagnostics: SearchExecutionDiagnostics,
    /// Per-channel hit counts, health, and diagnostics after fan-out.
    pub channel_results: Vec<ChannelResult>,
    /// Summary of semantic participation and lexical backend behavior.
    pub note: HybridExecutionNote,
    /// Optional stage timing and cardinality samples for hybrid profiling.
    pub stage_attribution: Option<SearchStageAttribution>,
    #[allow(dead_code)]
    pub(crate) post_selection_trace: Option<PostSelectionTrace>,
}

#[derive(Debug, Clone, PartialEq)]
/// A ranked anchor after Frigg has merged evidence from multiple retrieval channels around one
/// repository location.
pub struct HybridRankedEvidence {
    /// Repository and path identity for the matched document.
    pub document: HybridDocumentRef,
    /// Line- or symbol-scoped anchor within the document.
    pub anchor: EvidenceAnchor,
    /// Excerpt chosen from the highest-priority contributing channel.
    pub excerpt: String,
    /// Weighted blend of channel scores after policy multipliers.
    pub blended_score: f32,
    /// Lexical manifest channel contribution.
    pub lexical_score: f32,
    /// Path-surface witness channel contribution.
    pub witness_score: f32,
    /// Graph-precise expansion contribution.
    pub graph_score: f32,
    /// Semantic vector retrieval contribution.
    pub semantic_score: f32,
    /// Source labels explaining lexical score components.
    pub lexical_sources: Vec<String>,
    /// Source labels explaining path-witness score components.
    pub witness_sources: Vec<String>,
    /// Source labels explaining graph score components.
    pub graph_sources: Vec<String>,
    /// Source labels explaining semantic score components.
    pub semantic_sources: Vec<String>,
}

/// Caller filters normalized once before candidate intake and channel fan-out.
#[derive(Debug, Clone, Default)]
pub(crate) struct NormalizedSearchFilters {
    pub(crate) repository_id: Option<String>,
    pub(crate) language: Option<SymbolLanguage>,
    pub(crate) include_hidden: bool,
}

/// Cache key for durable path-witness projections keyed by snapshot and heuristic version.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HybridPathWitnessProjectionCacheKey {
    pub(crate) repository_id: String,
    pub(crate) root: PathBuf,
    pub(crate) snapshot_id: String,
    pub(crate) heuristic_version: i64,
}

/// Cache key for per-file hybrid graph analysis invalidated on mtime or size change.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HybridGraphFileAnalysisCacheKey {
    pub(crate) path: PathBuf,
    pub(crate) modified_unix_nanos: u128,
    pub(crate) size_bytes: u64,
}

/// Cached language-specific graph facts reused while expanding hybrid graph neighbors.
#[derive(Debug, Clone, Default)]
pub(crate) struct HybridGraphFileAnalysis {
    pub(crate) symbols: Vec<crate::indexer::SymbolDefinition>,
    pub(crate) php_declaration_relations: Option<Vec<PhpDeclarationRelation>>,
    pub(crate) php_evidence: Option<PhpSourceEvidence>,
    pub(crate) blade_evidence: Option<BladeSourceEvidence>,
}

pub(crate) fn search_diagnostics_to_channel_diagnostics(
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

pub(crate) fn empty_channel_result(
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

#[cfg(test)]
mod projection_cache_key_tests {
    use super::*;

    #[test]
    fn projection_cache_key_includes_heuristic_version() {
        let base = HybridPathWitnessProjectionCacheKey {
            repository_id: "repo-001".to_owned(),
            root: PathBuf::from("/tmp/repo"),
            snapshot_id: "snapshot-001".to_owned(),
            heuristic_version: 1,
        };
        let upgraded = HybridPathWitnessProjectionCacheKey {
            heuristic_version: 2,
            ..base.clone()
        };

        assert_ne!(base, upgraded);
    }
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
        ChannelHealthStatus::Filtered => ChannelHealthStatus::Disabled,
        other => other,
    }
}

pub(crate) fn hybrid_lexical_only_mode(
    semantic_status: ChannelHealthStatus,
    semantic_hit_count: usize,
) -> bool {
    semantic_status != ChannelHealthStatus::Ok || semantic_hit_count == 0
}

pub(crate) fn hybrid_execution_note_from_channel_results(
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
    let lexical_only_mode = hybrid_lexical_only_mode(semantic_status, semantic_hit_count);

    HybridExecutionNote {
        semantic_requested,
        semantic_enabled: semantic_match_count > 0,
        semantic_status,
        semantic_reason,
        semantic_candidate_count,
        semantic_hit_count,
        semantic_match_count,
        lexical_only_mode,
        lexical_backend: None,
        lexical_backend_note: None,
    }
}

pub(crate) fn match_count_for_hits(
    matches: &[HybridRankedEvidence],
    hits: &[HybridChannelHit],
) -> usize {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn semantic_channel_result(
        status: ChannelHealthStatus,
        hit_count: usize,
        match_count: usize,
    ) -> ChannelResult {
        ChannelResult::new(
            EvidenceChannel::Semantic,
            Vec::new(),
            ChannelHealth::new(status, None),
            Vec::new(),
            ChannelStats {
                candidate_count: hit_count,
                hit_count,
                match_count,
            },
        )
    }

    #[test]
    fn lexical_only_mode_keys_on_pre_fusion_hits_not_post_fusion_matches() {
        assert!(!hybrid_lexical_only_mode(ChannelHealthStatus::Ok, 5));
        assert!(hybrid_lexical_only_mode(ChannelHealthStatus::Ok, 0));
        assert!(hybrid_lexical_only_mode(ChannelHealthStatus::Disabled, 5));
    }

    #[test]
    fn execution_note_lexical_only_mode_matches_pipeline_guardrail_on_dropped_hits() {
        let channel_results = vec![semantic_channel_result(ChannelHealthStatus::Ok, 5, 0)];
        let note = hybrid_execution_note_from_channel_results(Some(true), true, &channel_results);
        assert_eq!(note.semantic_hit_count, 5);
        assert_eq!(note.semantic_match_count, 0);
        assert!(
            !note.lexical_only_mode,
            "a healthy semantic channel with pre-fusion hits is not lexical-only, matching guardrails"
        );
    }

    #[test]
    fn execution_note_lexical_only_mode_true_when_semantic_produced_no_hits() {
        let channel_results = vec![semantic_channel_result(ChannelHealthStatus::Ok, 0, 0)];
        let note = hybrid_execution_note_from_channel_results(Some(true), true, &channel_results);
        assert!(note.lexical_only_mode);
    }
}
