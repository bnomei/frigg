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
    pub query: String,
    pub path_regex: Option<regex::Regex>,
    pub limit: usize,
}

#[derive(Debug, Clone, Default)]
/// Shared repository-level filters used to scope both lexical and hybrid retrieval paths.
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
/// Output of a lexical-only search pass, including diagnostics that explain degraded or partial
/// coverage.
pub struct SearchExecutionOutput {
    pub total_matches: usize,
    pub matches: Vec<TextMatch>,
    pub diagnostics: SearchExecutionDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SearchCandidateFile {
    pub(crate) relative_path: String,
    pub(crate) absolute_path: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RepositoryCandidateUniverse {
    pub(crate) repository_id: String,
    pub(crate) root: PathBuf,
    pub(crate) snapshot_id: Option<String>,
    pub(crate) candidates: Vec<SearchCandidateFile>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SearchCandidateUniverse {
    pub(crate) repositories: Vec<RepositoryCandidateUniverse>,
    pub(crate) diagnostics: SearchExecutionDiagnostics,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SearchCandidateUniverseBuild {
    pub(crate) universe: SearchCandidateUniverse,
    pub(crate) repository_count: usize,
    pub(crate) candidate_count: usize,
    pub(crate) manifest_backed_repository_count: usize,
    pub(crate) candidate_intake_elapsed_us: u64,
    pub(crate) freshness_validation_elapsed_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestCandidateFilesBuild {
    pub(crate) snapshot_id: String,
    pub(crate) candidates: Vec<(String, PathBuf)>,
    pub(crate) candidate_intake_elapsed_us: u64,
    pub(crate) freshness_validation_elapsed_us: u64,
}

pub type HybridDocumentRef = EvidenceDocumentRef;
pub type HybridChannelHit = EvidenceHit;

#[derive(Debug, Clone, Copy, PartialEq)]
/// Relative influence assigned to each hybrid retrieval channel before result diversification.
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
/// Input for Frigg's multi-signal retrieval path that can combine lexical, graph, and semantic
/// evidence behind one call.
pub struct SearchHybridQuery {
    pub query: String,
    pub limit: usize,
    pub weights: HybridChannelWeights,
    pub semantic: Option<bool>,
}

pub type HybridSemanticStatus = ChannelHealthStatus;

#[derive(Debug, Clone, PartialEq, Eq)]
/// Execution-side explanation of how the hybrid search actually ran, including whether semantic
/// recall participated or the query fell back to a narrower mode.
pub struct HybridExecutionNote {
    pub semantic_requested: bool,
    pub semantic_enabled: bool,
    pub semantic_status: HybridSemanticStatus,
    pub semantic_reason: Option<String>,
    pub semantic_candidate_count: usize,
    pub semantic_hit_count: usize,
    pub semantic_match_count: usize,
    pub lexical_only_mode: bool,
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
        }
    }
}

#[derive(Debug, Clone, Default)]
/// Top-level result of a hybrid retrieval run, pairing final matches with diagnostics, channel
/// health, and execution attribution.
pub struct SearchHybridExecutionOutput {
    pub matches: Vec<HybridRankedEvidence>,
    pub ranked_anchors: Vec<HybridRankedEvidence>,
    pub(crate) coverage_grouped_pool: Vec<HybridRankedEvidence>,
    pub diagnostics: SearchExecutionDiagnostics,
    pub channel_results: Vec<ChannelResult>,
    pub note: HybridExecutionNote,
    pub stage_attribution: Option<SearchStageAttribution>,
    pub(crate) post_selection_trace: Option<PostSelectionTrace>,
}

#[derive(Debug, Clone, PartialEq)]
/// A ranked anchor after Frigg has merged evidence from multiple retrieval channels around one
/// repository location.
pub struct HybridRankedEvidence {
    pub document: HybridDocumentRef,
    pub anchor: EvidenceAnchor,
    pub excerpt: String,
    pub blended_score: f32,
    pub lexical_score: f32,
    pub witness_score: f32,
    pub graph_score: f32,
    pub semantic_score: f32,
    pub lexical_sources: Vec<String>,
    pub witness_sources: Vec<String>,
    pub graph_sources: Vec<String>,
    pub semantic_sources: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct NormalizedSearchFilters {
    pub(crate) repository_id: Option<String>,
    pub(crate) language: Option<SymbolLanguage>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HybridPathWitnessProjectionCacheKey {
    pub(crate) repository_id: String,
    pub(crate) root: PathBuf,
    pub(crate) snapshot_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct HybridGraphFileAnalysisCacheKey {
    pub(crate) path: PathBuf,
    pub(crate) modified_unix_nanos: u128,
    pub(crate) size_bytes: u64,
}

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
    let lexical_only_mode =
        semantic_status != HybridSemanticStatus::Ok || semantic_match_count == 0;

    HybridExecutionNote {
        semantic_requested,
        semantic_enabled: semantic_match_count > 0,
        semantic_status,
        semantic_reason,
        semantic_candidate_count,
        semantic_hit_count,
        semantic_match_count,
        lexical_only_mode,
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
