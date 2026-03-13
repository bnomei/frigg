use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::model::SymbolMatch;
use crate::graph::SymbolGraph;
use crate::indexer::SymbolDefinition;
use crate::languages::{BladeSourceEvidence, PhpSourceEvidence};
use crate::mcp::types::{
    ExploreLineWindow, ExploreResponse, FindReferencesResponse, ReadFileResponse, RuntimeTaskKind,
    RuntimeTaskStatus, RuntimeTaskSummary, SearchHybridChannelWeightsParams, SearchHybridResponse,
    SearchPatternType, SearchSymbolResponse, SearchTextResponse,
};
use rmcp::ErrorData;
use rmcp::handler::server::wrapper::Json;
use serde_json::Value;

#[derive(Clone)]
pub(crate) struct RepositorySymbolCorpus {
    pub repository_id: String,
    pub root: PathBuf,
    pub root_signature: String,
    pub source_paths: Vec<PathBuf>,
    pub symbols: Vec<SymbolDefinition>,
    pub symbols_by_relative_path: BTreeMap<String, Vec<usize>>,
    pub symbol_index_by_stable_id: BTreeMap<String, usize>,
    pub symbol_indices_by_name: BTreeMap<String, Vec<usize>>,
    pub symbol_indices_by_lower_name: BTreeMap<String, Vec<usize>>,
    pub canonical_symbol_name_by_stable_id: BTreeMap<String, String>,
    pub symbol_indices_by_canonical_name: BTreeMap<String, Vec<usize>>,
    pub symbol_indices_by_lower_canonical_name: BTreeMap<String, Vec<usize>>,
    pub php_evidence_by_relative_path: BTreeMap<String, PhpSourceEvidence>,
    pub blade_evidence_by_relative_path: BTreeMap<String, BladeSourceEvidence>,
    pub diagnostics: RepositoryDiagnosticsSummary,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct RepositoryDiagnosticsSummary {
    pub manifest_walk_count: usize,
    pub manifest_read_count: usize,
    pub symbol_extraction_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SymbolCorpusCacheKey {
    pub repository_id: String,
    pub manifest_token: String,
}

#[derive(Clone)]
pub(crate) struct SymbolCandidate {
    pub rank: u8,
    pub path_class_rank: u8,
    pub path_class: &'static str,
    pub repository_id: String,
    pub root: PathBuf,
    pub symbol: SymbolDefinition,
}

#[derive(Clone)]
pub(crate) struct ResolvedSymbolTarget {
    pub candidate: SymbolCandidate,
    pub corpus: Arc<RepositorySymbolCorpus>,
    pub candidate_count: usize,
    pub selected_rank_candidate_count: usize,
}

#[derive(Clone)]
pub(crate) struct ResolvedNavigationTarget {
    pub symbol_query: String,
    pub target: ResolvedSymbolTarget,
    pub resolution_source: &'static str,
}

pub(crate) struct ReadFileExecution {
    pub result: Result<Json<ReadFileResponse>, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
}

#[allow(dead_code)]
pub(crate) struct SearchTextExecution {
    pub result: Result<Json<SearchTextResponse>, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub total_matches: usize,
    pub effective_limit: Option<usize>,
    pub effective_pattern_type: Option<SearchPatternType>,
    pub diagnostics_count: usize,
    pub walk_diagnostics_count: usize,
    pub read_diagnostics_count: usize,
}

pub(crate) struct ExploreExecution {
    pub result: Result<Json<ExploreResponse>, ErrorData>,
    pub resolved_repository_id: Option<String>,
    pub resolved_path: Option<String>,
    pub resolved_absolute_path: Option<String>,
    pub effective_context_lines: Option<usize>,
    pub effective_max_matches: Option<usize>,
    pub scan_scope: Option<ExploreLineWindow>,
    pub total_matches: usize,
    pub truncated: bool,
}

#[allow(dead_code)]
pub(crate) struct SearchHybridExecution {
    pub result: Result<Json<SearchHybridResponse>, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub effective_limit: Option<usize>,
    pub effective_weights: Option<SearchHybridChannelWeightsParams>,
    pub diagnostics_count: usize,
    pub walk_diagnostics_count: usize,
    pub read_diagnostics_count: usize,
    pub semantic_requested: Option<bool>,
    pub semantic_enabled: Option<bool>,
    pub semantic_status: Option<String>,
    pub semantic_reason: Option<String>,
    pub semantic_candidate_count: Option<usize>,
    pub semantic_hit_count: Option<usize>,
    pub semantic_match_count: Option<usize>,
    pub warning: Option<String>,
    pub channel_metadata: Option<Value>,
    pub match_anchors: Option<Value>,
}

const RUNTIME_TASK_RECENT_LIMIT: usize = 16;

#[derive(Debug, Default)]
pub struct RuntimeTaskRegistry {
    next_sequence: u64,
    active: BTreeMap<String, RuntimeTaskSummary>,
    recent: VecDeque<RuntimeTaskSummary>,
}

impl RuntimeTaskRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_task(
        &mut self,
        kind: RuntimeTaskKind,
        repository_id: impl Into<String>,
        phase: impl Into<String>,
        detail: Option<String>,
    ) -> String {
        self.next_sequence = self.next_sequence.saturating_add(1);
        let repository_id = repository_id.into();
        let task_id = format!(
            "{}:{}:{:04}",
            runtime_task_kind_name(kind),
            repository_id,
            self.next_sequence
        );
        let summary = RuntimeTaskSummary {
            task_id: task_id.clone(),
            kind,
            status: RuntimeTaskStatus::Running,
            repository_id,
            phase: phase.into(),
            created_at_ms: now_unix_ms(),
            finished_at_ms: None,
            detail,
        };
        self.active.insert(task_id.clone(), summary);
        task_id
    }

    pub fn finish_task(
        &mut self,
        task_id: &str,
        status: RuntimeTaskStatus,
        detail: Option<String>,
    ) {
        let Some(mut summary) = self.active.remove(task_id) else {
            return;
        };
        summary.status = status;
        summary.finished_at_ms = Some(now_unix_ms());
        if detail.is_some() {
            summary.detail = detail;
        }
        self.push_recent(summary);
    }

    pub fn active_tasks(&self) -> Vec<RuntimeTaskSummary> {
        let mut tasks = self.active.values().cloned().collect::<Vec<_>>();
        tasks.sort_by(|left, right| {
            left.created_at_ms
                .cmp(&right.created_at_ms)
                .then(left.task_id.cmp(&right.task_id))
        });
        tasks
    }

    pub fn has_active_task(&self, kind: RuntimeTaskKind, repository_id: &str, phase: &str) -> bool {
        self.active.values().any(|task| {
            task.kind == kind && task.repository_id == repository_id && task.phase == phase
        })
    }

    pub fn has_active_task_for_repository(
        &self,
        kind: RuntimeTaskKind,
        repository_id: &str,
    ) -> bool {
        self.active
            .values()
            .any(|task| task.kind == kind && task.repository_id == repository_id)
    }

    pub fn recent_tasks(&self) -> Vec<RuntimeTaskSummary> {
        self.recent.iter().rev().cloned().collect::<Vec<_>>()
    }

    fn push_recent(&mut self, summary: RuntimeTaskSummary) {
        self.recent.push_back(summary);
        while self.recent.len() > RUNTIME_TASK_RECENT_LIMIT {
            self.recent.pop_front();
        }
    }
}

fn runtime_task_kind_name(kind: RuntimeTaskKind) -> &'static str {
    match kind {
        RuntimeTaskKind::ChangedReindex => "changed_reindex",
        RuntimeTaskKind::SemanticRefresh => "semantic_refresh",
        RuntimeTaskKind::PrecisePrewarm => "precise_prewarm",
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_task_registry_tracks_active_and_recent_tasks() {
        let mut registry = RuntimeTaskRegistry::new();
        let first = registry.start_task(
            RuntimeTaskKind::ChangedReindex,
            "repo-001",
            "changed_only_reindex",
            Some("watch root /tmp/repo-001".to_owned()),
        );
        let second = registry.start_task(
            RuntimeTaskKind::SemanticRefresh,
            "repo-001",
            "semantic_attach_refresh",
            None,
        );

        let active = registry.active_tasks();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].task_id, first);
        assert_eq!(active[1].task_id, second);

        registry.finish_task(
            &first,
            RuntimeTaskStatus::Succeeded,
            Some("reindex complete".to_owned()),
        );
        registry.finish_task(
            &second,
            RuntimeTaskStatus::Failed,
            Some("startup validation failed".to_owned()),
        );

        assert!(registry.active_tasks().is_empty());
        let recent = registry.recent_tasks();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].task_id, second);
        assert_eq!(recent[0].status, RuntimeTaskStatus::Failed);
        assert_eq!(recent[1].task_id, first);
        assert_eq!(recent[1].status, RuntimeTaskStatus::Succeeded);
    }
}

pub(crate) struct SearchSymbolExecution {
    pub result: Result<Json<SearchSymbolResponse>, ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub diagnostics_count: usize,
    pub manifest_walk_diagnostics_count: usize,
    pub manifest_read_diagnostics_count: usize,
    pub symbol_extraction_diagnostics_count: usize,
    pub effective_limit: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct RankedSymbolMatch {
    pub rank: u8,
    pub path_class_rank: u8,
    pub matched: SymbolMatch,
}

#[allow(dead_code)]
pub(crate) struct FindReferencesExecution {
    pub result: Result<Json<FindReferencesResponse>, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub total_matches: usize,
    pub selected_symbol_id: Option<String>,
    pub selected_precise_symbol: Option<String>,
    pub resolution_precision: Option<String>,
    pub resolution_source: Option<String>,
    pub diagnostics_count: usize,
    pub manifest_walk_diagnostics_count: usize,
    pub manifest_read_diagnostics_count: usize,
    pub symbol_extraction_diagnostics_count: usize,
    pub source_read_diagnostics_count: usize,
    pub precise_artifacts_discovered: usize,
    pub precise_artifacts_discovered_bytes: u64,
    pub precise_artifacts_ingested: usize,
    pub precise_artifacts_ingested_bytes: u64,
    pub precise_artifacts_failed: usize,
    pub precise_artifacts_failed_bytes: u64,
    pub precise_reference_count: usize,
    pub source_files_discovered: usize,
    pub source_files_loaded: usize,
    pub source_bytes_loaded: u64,
    pub effective_limit: Option<usize>,
}

pub(crate) struct NavigationToolExecution<T> {
    pub result: Result<Json<T>, ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub selected_symbol_id: Option<String>,
    pub selected_precise_symbol: Option<String>,
    pub resolution_precision: Option<String>,
    pub resolution_source: Option<String>,
    pub effective_limit: Option<usize>,
    pub precise_artifacts_ingested: usize,
    pub precise_artifacts_failed: usize,
    pub match_count: usize,
}

#[derive(Debug, Clone)]
pub(crate) struct PreciseArtifactFailureSample {
    pub artifact_label: String,
    pub stage: String,
    pub detail: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct PreciseIngestStats {
    pub candidate_directories: Vec<String>,
    pub discovered_artifacts: Vec<String>,
    pub artifacts_discovered: usize,
    pub artifacts_discovered_bytes: u64,
    pub artifacts_ingested: usize,
    pub artifacts_ingested_bytes: u64,
    pub artifacts_failed: usize,
    pub artifacts_failed_bytes: u64,
    pub failed_artifacts: Vec<PreciseArtifactFailureSample>,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct FindReferencesResourceBudgets {
    pub scip_max_artifacts: usize,
    pub scip_max_artifact_bytes: usize,
    pub scip_max_total_bytes: usize,
    pub scip_max_documents_per_artifact: usize,
    pub scip_max_elapsed_ms: u64,
    pub source_max_files: usize,
    pub source_max_file_bytes: usize,
    pub source_max_total_bytes: usize,
    pub source_max_elapsed_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PreciseGraphCacheKey {
    pub repository_id: String,
    pub scip_signature: String,
    pub corpus_signature: String,
}

#[derive(Debug, Clone)]
pub(crate) struct CachedPreciseGraph {
    pub graph: Arc<SymbolGraph>,
    pub ingest_stats: PreciseIngestStats,
    pub corpus_signature: String,
    pub discovery: ScipArtifactDiscovery,
    pub coverage_mode: PreciseCoverageMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreciseCoverageMode {
    Full,
    Partial,
    None,
}

impl PreciseCoverageMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScipArtifactDigest {
    pub path: PathBuf,
    pub format: ScipArtifactFormat,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ScipArtifactDiscovery {
    pub candidate_directories: Vec<String>,
    pub candidate_directory_digests: Vec<ScipCandidateDirectoryDigest>,
    pub artifact_digests: Vec<ScipArtifactDigest>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScipCandidateDirectoryDigest {
    pub path: PathBuf,
    pub exists: bool,
    pub mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ScipArtifactFormat {
    Json,
    Protobuf,
}

impl ScipArtifactFormat {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Some(Self::Json),
            Some("scip") => Some(Self::Protobuf),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Protobuf => "protobuf",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DeterministicSignatureHasher {
    state: u64,
}

impl DeterministicSignatureHasher {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    pub(crate) fn new() -> Self {
        Self {
            state: Self::OFFSET_BASIS,
        }
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(Self::FNV_PRIME);
        }
    }

    pub(crate) fn write_separator(&mut self) {
        self.write_bytes(&[0xff]);
    }

    pub(crate) fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
        self.write_separator();
    }

    pub(crate) fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
        self.write_separator();
    }

    pub(crate) fn write_optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.write_bytes(&[1]);
                self.write_u64(value);
            }
            None => {
                self.write_bytes(&[0]);
                self.write_separator();
            }
        }
    }

    pub(crate) fn finish_hex(self) -> String {
        format!("{:016x}", self.state)
    }
}
