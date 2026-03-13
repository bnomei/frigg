use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::{FriggError, FriggResult};
use crate::embeddings::{
    EmbeddingProvider, EmbeddingPurpose, EmbeddingRequest, GoogleEmbeddingProvider,
    OpenAiEmbeddingProvider,
};
use crate::languages::semantic_chunk_language_for_path;
#[allow(unused_imports)]
pub(crate) use crate::languages::{
    BladeRelationKind, PhpDeclarationRelation, PhpGraphSourceAnalysis, PhpLiteralEvidence,
    PhpSourceEvidence, PhpTargetEvidence, PhpTargetEvidenceKind, PhpTypeEvidence,
    PhpTypeEvidenceKind, SymbolLanguage, extract_blade_source_evidence_from_source,
    extract_php_declaration_relations_from_source, extract_php_graph_analysis_from_source,
    extract_php_source_evidence_from_source, mark_local_flux_overlays,
    php_declaration_relation_edges_for_file, php_declaration_relation_edges_for_relations,
    php_declaration_relation_edges_for_source, php_heuristic_implementation_candidates_for_target,
    resolve_blade_relation_evidence_edges, resolve_php_target_evidence_edges,
};
use blake3::Hasher;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};

mod manifest;
mod reindex;
mod semantic;
mod symbols;
#[cfg(test)]
use manifest::diff;
#[cfg(test)]
use manifest::file_digest_order;
#[cfg(test)]
use reindex::reindex_repository_with_semantic_executor;
pub use reindex::{
    ManifestStore, ReindexDiagnostics, ReindexMode, ReindexSummary, reindex_repository,
    reindex_repository_with_runtime_config, reindex_repository_with_runtime_config_and_dirty_paths,
};
#[cfg(test)]
use semantic::{RuntimeSemanticEmbeddingExecutor, SemanticRuntimeEmbeddingExecutor};
use semantic::{build_file_semantic_chunks, build_semantic_chunk_candidates};
pub use symbols::{
    HeuristicReference, HeuristicReferenceConfidence, HeuristicReferenceEvidence,
    HeuristicReferenceResolver, SourceSpan, StructuralQueryMatch, SymbolDefinition,
    SymbolExtractionDiagnostic, SymbolExtractionOutput, SymbolKind, extract_symbols_for_paths,
    extract_symbols_from_file, extract_symbols_from_source, navigation_symbol_target_rank,
    register_symbol_definitions, resolve_heuristic_references, search_structural_in_source,
};
pub(crate) use symbols::{
    line_column_for_offset, push_symbol_definition, source_span, source_span_from_offsets,
};

const FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_ENABLED";
const FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_PROVIDER";
const FRIGG_SEMANTIC_RUNTIME_MODEL_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_MODEL";
const FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV: &str = "FRIGG_SEMANTIC_RUNTIME_STRICT_MODE";
const SEMANTIC_EMBEDDING_BATCH_SIZE: usize = 24;
const SEMANTIC_CHUNK_MAX_LINES: usize = 64;
const SEMANTIC_CHUNK_MAX_CHARS: usize = 2_400;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileDigest {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
    pub hash_blake3_hex: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMetadataDigest {
    pub path: PathBuf,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticChunkBenchmarkSummary {
    pub chunk_count: usize,
    pub total_content_bytes: usize,
    pub max_chunk_bytes: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestDiff {
    pub added: Vec<FileDigest>,
    pub modified: Vec<FileDigest>,
    pub deleted: Vec<FileDigest>,
}

#[derive(Debug, Clone, Default)]
pub struct ManifestBuilder {
    pub follow_symlinks: bool,
}

#[doc(hidden)]
pub fn benchmark_build_file_semantic_chunks(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    language: &str,
    source: &str,
) -> SemanticChunkBenchmarkSummary {
    summarize_semantic_chunk_candidates(build_file_semantic_chunks(
        repository_id,
        snapshot_id,
        path,
        language,
        source,
    ))
}

#[doc(hidden)]
pub fn benchmark_build_semantic_chunk_candidates(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
) -> FriggResult<SemanticChunkBenchmarkSummary> {
    build_semantic_chunk_candidates(repository_id, workspace_root, snapshot_id, current_manifest)
        .map(summarize_semantic_chunk_candidates)
}

fn summarize_semantic_chunk_candidates(
    chunks: Vec<semantic::SemanticChunkCandidate>,
) -> SemanticChunkBenchmarkSummary {
    let chunk_count = chunks.len();
    let mut total_content_bytes = 0usize;
    let mut max_chunk_bytes = 0usize;

    for chunk in chunks {
        let chunk_len = chunk.content_text.len();
        total_content_bytes += chunk_len;
        max_chunk_bytes = max_chunk_bytes.max(chunk_len);
    }

    SemanticChunkBenchmarkSummary {
        chunk_count,
        total_content_bytes,
        max_chunk_bytes,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestDiagnosticKind {
    Walk,
    Read,
}

impl ManifestDiagnosticKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Walk => "walk",
            Self::Read => "read",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestBuildDiagnostic {
    pub path: Option<PathBuf>,
    pub kind: ManifestDiagnosticKind,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestBuildOutput {
    pub entries: Vec<FileDigest>,
    pub diagnostics: Vec<ManifestBuildDiagnostic>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestMetadataBuildOutput {
    pub entries: Vec<FileMetadataDigest>,
    pub diagnostics: Vec<ManifestBuildDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryManifest {
    pub repository_id: String,
    pub snapshot_id: String,
    pub entries: Vec<FileDigest>,
}

#[cfg(test)]
mod tests;
