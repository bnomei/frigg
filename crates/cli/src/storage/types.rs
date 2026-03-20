use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryManifestSnapshot {
    pub repository_id: String,
    pub snapshot_id: String,
    pub entries: Vec<ManifestEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestMetadataEntry {
    pub path: String,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepositoryManifestMetadataSnapshot {
    pub repository_id: String,
    pub snapshot_id: String,
    pub entries: Vec<ManifestMetadataEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceEventRow {
    pub trace_id: String,
    pub tool_name: String,
    pub payload_json: String,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticChunkEmbeddingRecord {
    pub chunk_id: String,
    pub repository_id: String,
    pub snapshot_id: String,
    pub path: String,
    pub language: String,
    pub chunk_index: usize,
    pub start_line: usize,
    pub end_line: usize,
    pub provider: String,
    pub model: String,
    pub trace_id: Option<String>,
    pub content_hash_blake3: String,
    pub content_text: String,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticChunkEmbeddingProjection {
    pub chunk_id: String,
    pub repository_id: String,
    pub snapshot_id: String,
    pub path: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub embedding: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SemanticChunkVectorMatch {
    pub chunk_id: String,
    pub repository_id: String,
    pub snapshot_id: String,
    pub distance: f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticChunkPayload {
    pub chunk_id: String,
    pub path: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub content_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SemanticChunkPreview {
    pub chunk_id: String,
    pub path: String,
    pub language: String,
    pub start_line: usize,
    pub end_line: usize,
    pub preview_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathWitnessProjection {
    pub path: String,
    pub path_class: PathClass,
    pub source_class: SourceClass,
    pub file_stem: String,
    pub path_terms: Vec<String>,
    pub subtree_root: Option<String>,
    pub family_bits: u64,
    pub flags_json: String,
    pub heuristic_version: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestSubjectProjection {
    pub test_path: String,
    pub subject_path: String,
    pub shared_terms: Vec<String>,
    pub score_hint: usize,
    pub flags_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntrypointSurfaceProjection {
    pub path: String,
    pub path_class: PathClass,
    pub source_class: SourceClass,
    pub path_terms: Vec<String>,
    pub surface_terms: Vec<String>,
    pub flags_json: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievalProjectionHeadRecord {
    pub family: String,
    pub heuristic_version: i64,
    pub input_modes: Vec<String>,
    pub row_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRelationProjection {
    pub src_path: String,
    pub dst_path: String,
    pub relation_kind: String,
    pub evidence_source: String,
    pub src_symbol_id: Option<String>,
    pub dst_symbol_id: Option<String>,
    pub src_family_bits: u64,
    pub dst_family_bits: u64,
    pub shared_terms: Vec<String>,
    pub score_hint: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubtreeCoverageProjection {
    pub subtree_root: String,
    pub family: String,
    pub path_count: usize,
    pub exemplar_path: String,
    pub exemplar_score_hint: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathSurfaceTermProjection {
    pub path: String,
    pub term_weights: BTreeMap<String, u16>,
    pub exact_terms: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathAnchorSketchProjection {
    pub path: String,
    pub anchor_rank: usize,
    pub line: usize,
    pub anchor_kind: String,
    pub excerpt: String,
    pub terms: Vec<String>,
    pub score_hint: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RetrievalProjectionBundle {
    pub heads: Vec<RetrievalProjectionHeadRecord>,
    pub path_witness: Vec<PathWitnessProjection>,
    pub test_subject: Vec<TestSubjectProjection>,
    pub entrypoint_surface: Vec<EntrypointSurfaceProjection>,
    pub path_relations: Vec<PathRelationProjection>,
    pub subtree_coverage: Vec<SubtreeCoverageProjection>,
    pub path_surface_terms: Vec<PathSurfaceTermProjection>,
    pub path_anchor_sketches: Vec<PathAnchorSketchProjection>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticHeadRecord {
    pub repository_id: String,
    pub provider: String,
    pub model: String,
    pub covered_snapshot_id: String,
    pub live_chunk_count: usize,
    pub last_refresh_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SemanticStorageHealth {
    pub repository_id: String,
    pub provider: String,
    pub model: String,
    pub covered_snapshot_id: Option<String>,
    pub live_chunk_rows: usize,
    pub live_embedding_rows: usize,
    pub live_vector_rows: usize,
    pub retained_manifest_snapshots: usize,
    pub vector_consistent: bool,
}

#[derive(Debug, Default, Clone)]
pub struct StorageInvariantRepairSummary {
    pub repaired_categories: Vec<String>,
}
