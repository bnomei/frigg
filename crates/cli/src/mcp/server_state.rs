use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::domain::model::SymbolMatch;
use crate::graph::SymbolGraph;
use crate::indexer::{BladeSourceEvidence, PhpSourceEvidence, SymbolDefinition};
use crate::mcp::types::{
    ExploreLineWindow, ExploreResponse, FindReferencesResponse, ReadFileResponse,
    SearchHybridChannelWeightsParams, SearchHybridResponse, SearchPatternType,
    SearchSymbolResponse, SearchTextResponse,
};
use rmcp::ErrorData;
use rmcp::handler::server::wrapper::Json;

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
    pub resolved_repository_id: Option<String>,
    pub resolved_path: Option<String>,
    pub resolved_absolute_path: Option<String>,
    pub effective_max_bytes: Option<usize>,
    pub effective_line_start: Option<usize>,
    pub effective_line_end: Option<usize>,
}

pub(crate) struct SearchTextExecution {
    pub result: Result<Json<SearchTextResponse>, ErrorData>,
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

pub(crate) struct SearchHybridExecution {
    pub result: Result<Json<SearchHybridResponse>, ErrorData>,
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

pub(crate) struct FindReferencesExecution {
    pub result: Result<Json<FindReferencesResponse>, ErrorData>,
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
