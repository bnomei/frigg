use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::domain::{FriggError, FriggResult};
use crate::embeddings::{
    EmbeddingProvider, EmbeddingPurpose, EmbeddingRequest, GoogleEmbeddingProvider,
    OpenAiEmbeddingProvider,
};
use crate::graph::{HeuristicConfidence, SymbolGraph, SymbolNode};
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
use crate::languages::{
    collect_blade_symbols_from_source, parser_for_path, semantic_chunk_language_for_path,
    symbol_from_node, tree_sitter_language_for_path,
};
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider};
use crate::storage::{ManifestEntry, SemanticChunkEmbeddingRecord, Storage};
use blake3::Hasher;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

mod manifest;
mod semantic;
#[cfg(test)]
pub(crate) use manifest::file_digest_order;
use manifest::{
    deterministic_snapshot_id, diff, file_digest_to_manifest_entry, manifest_entry_to_file_digest,
    normalize_repository_relative_path,
};
use semantic::{
    RuntimeSemanticEmbeddingExecutor, SemanticRuntimeEmbeddingExecutor, build_file_semantic_chunks,
    build_semantic_chunk_candidates, build_semantic_embedding_records,
    resolve_semantic_runtime_config_from_env,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Module,
    Component,
    Section,
    Slot,
    Struct,
    Enum,
    EnumCase,
    Trait,
    Impl,
    Function,
    Method,
    TypeAlias,
    Const,
    Static,
    Class,
    Interface,
    PhpTrait,
    PhpEnum,
    Property,
    Constant,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Component => "component",
            Self::Section => "section",
            Self::Slot => "slot",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::EnumCase => "enum_case",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Function => "function",
            Self::Method => "method",
            Self::TypeAlias => "type_alias",
            Self::Const => "const",
            Self::Static => "static",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::PhpTrait => "php_trait",
            Self::PhpEnum => "php_enum",
            Self::Property => "property",
            Self::Constant => "constant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolDefinition {
    pub stable_id: String,
    pub language: SymbolLanguage,
    pub kind: SymbolKind,
    pub name: String,
    pub path: PathBuf,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolExtractionDiagnostic {
    pub path: PathBuf,
    pub language: Option<SymbolLanguage>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolExtractionOutput {
    pub symbols: Vec<SymbolDefinition>,
    pub diagnostics: Vec<SymbolExtractionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralQueryMatch {
    pub path: PathBuf,
    pub span: SourceSpan,
    pub excerpt: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeuristicReferenceConfidence {
    Low,
    Medium,
    High,
}

impl From<HeuristicConfidence> for HeuristicReferenceConfidence {
    fn from(value: HeuristicConfidence) -> Self {
        match value {
            HeuristicConfidence::Low => Self::Low,
            HeuristicConfidence::Medium => Self::Medium,
            HeuristicConfidence::High => Self::High,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HeuristicReferenceEvidence {
    GraphRelation {
        source_symbol_id: String,
        relation: String,
    },
    LexicalToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeuristicReference {
    pub repository_id: String,
    pub symbol_id: String,
    pub symbol_name: String,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub confidence: HeuristicReferenceConfidence,
    pub heuristic: bool,
    pub evidence: HeuristicReferenceEvidence,
}

pub fn register_symbol_definitions(
    graph: &mut SymbolGraph,
    repository_id: &str,
    symbols: &[SymbolDefinition],
) {
    graph.register_symbols(symbols.iter().map(|symbol| {
        SymbolNode::new(
            symbol.stable_id.clone(),
            repository_id.to_owned(),
            symbol.name.clone(),
            symbol.kind.as_str().to_owned(),
            symbol.path.to_string_lossy().into_owned(),
            symbol.line,
        )
    }));
}

pub fn navigation_symbol_target_rank(symbol: &SymbolDefinition, symbol_query: &str) -> Option<u8> {
    if symbol.stable_id == symbol_query {
        return Some(0);
    }
    if symbol.name == symbol_query {
        return Some(1);
    }
    if symbol.name.eq_ignore_ascii_case(symbol_query) {
        return Some(2);
    }

    None
}

pub struct HeuristicReferenceResolver<'a> {
    repository_id: &'a str,
    target_symbol: &'a SymbolDefinition,
    relation_hint_by_source: HashMap<String, (HeuristicReferenceConfidence, String)>,
    symbols_by_path: HashMap<PathBuf, Vec<&'a SymbolDefinition>>,
    by_location: BTreeMap<(PathBuf, usize, usize), HeuristicReference>,
}

impl<'a> HeuristicReferenceResolver<'a> {
    pub fn new(
        repository_id: &'a str,
        symbol_id: &str,
        symbols: &'a [SymbolDefinition],
        graph: &SymbolGraph,
    ) -> Option<Self> {
        let target_symbol = symbols
            .iter()
            .find(|symbol| symbol.stable_id == symbol_id)?;
        let relation_hints = graph.heuristic_relation_hints_for_target(symbol_id);
        let relation_hint_by_source = relation_hints
            .iter()
            .map(|hint| {
                (
                    hint.source_symbol.symbol_id.clone(),
                    (
                        HeuristicReferenceConfidence::from(hint.confidence),
                        hint.relation.as_str().to_owned(),
                    ),
                )
            })
            .collect::<HashMap<_, _>>();
        let mut by_location = BTreeMap::new();
        for hint in relation_hints {
            let path = PathBuf::from(&hint.source_symbol.path);
            if path == target_symbol.path && hint.source_symbol.line == target_symbol.line {
                continue;
            }
            upsert_heuristic_reference(
                &mut by_location,
                HeuristicReference {
                    repository_id: repository_id.to_owned(),
                    symbol_id: target_symbol.stable_id.clone(),
                    symbol_name: target_symbol.name.clone(),
                    path,
                    line: hint.source_symbol.line,
                    column: 1,
                    confidence: HeuristicReferenceConfidence::from(hint.confidence),
                    heuristic: true,
                    evidence: HeuristicReferenceEvidence::GraphRelation {
                        source_symbol_id: hint.source_symbol.symbol_id,
                        relation: hint.relation.as_str().to_owned(),
                    },
                },
            );
        }

        let mut symbols_by_path: HashMap<PathBuf, Vec<&'a SymbolDefinition>> = HashMap::new();
        for symbol in symbols {
            symbols_by_path
                .entry(symbol.path.clone())
                .or_default()
                .push(symbol);
        }

        Some(Self {
            repository_id,
            target_symbol,
            relation_hint_by_source,
            symbols_by_path,
            by_location,
        })
    }

    pub fn ingest_source(&mut self, path: &Path, source: &str) {
        if !is_identifier_token(&self.target_symbol.name) {
            return;
        }

        let symbols_for_path = self
            .symbols_by_path
            .get(path)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let mut containing_symbol_by_line: HashMap<usize, Option<&SymbolDefinition>> =
            HashMap::new();

        for (line_index, line) in source.lines().enumerate() {
            let line_number = line_index + 1;
            let columns = token_columns(line, &self.target_symbol.name);
            if columns.is_empty() {
                continue;
            }

            for column in columns {
                if path == self.target_symbol.path.as_path()
                    && line_number == self.target_symbol.line
                {
                    continue;
                }

                let containing_symbol = *containing_symbol_by_line
                    .entry(line_number)
                    .or_insert_with(|| {
                        find_innermost_symbol_for_line_in_file(symbols_for_path, line_number)
                    });
                let (confidence, evidence) = containing_symbol
                    .and_then(|symbol| {
                        self.relation_hint_by_source
                            .get(symbol.stable_id.as_str())
                            .map(|(confidence, relation)| {
                                (
                                    *confidence,
                                    HeuristicReferenceEvidence::GraphRelation {
                                        source_symbol_id: symbol.stable_id.clone(),
                                        relation: relation.clone(),
                                    },
                                )
                            })
                    })
                    .unwrap_or((
                        HeuristicReferenceConfidence::Low,
                        HeuristicReferenceEvidence::LexicalToken,
                    ));

                upsert_heuristic_reference(
                    &mut self.by_location,
                    HeuristicReference {
                        repository_id: self.repository_id.to_owned(),
                        symbol_id: self.target_symbol.stable_id.clone(),
                        symbol_name: self.target_symbol.name.clone(),
                        path: path.to_path_buf(),
                        line: line_number,
                        column,
                        confidence,
                        heuristic: true,
                        evidence,
                    },
                );
            }
        }
    }

    pub fn finish(self) -> Vec<HeuristicReference> {
        let mut references = self.by_location.into_values().collect::<Vec<_>>();
        references.sort_by(heuristic_reference_order);
        references
    }
}

pub fn resolve_heuristic_references(
    repository_id: &str,
    symbol_id: &str,
    symbols: &[SymbolDefinition],
    graph: &SymbolGraph,
    sources_by_path: &BTreeMap<PathBuf, String>,
) -> Vec<HeuristicReference> {
    let Some(mut resolver) =
        HeuristicReferenceResolver::new(repository_id, symbol_id, symbols, graph)
    else {
        return Vec::new();
    };

    for (path, source) in sources_by_path {
        resolver.ingest_source(path, source);
    }

    resolver.finish()
}

pub fn extract_symbols_from_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
) -> FriggResult<Vec<SymbolDefinition>> {
    let mut parser = parser_for_path(language, path)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for symbol extraction: {}",
            path.display()
        ))
    })?;
    let mut symbols = Vec::new();
    collect_symbols_from_tree(language, path, source, &tree, &mut symbols);
    symbols.sort_by(symbol_definition_order);
    Ok(symbols)
}

pub fn search_structural_in_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    query: &str,
) -> FriggResult<Vec<StructuralQueryMatch>> {
    let query = query.trim();
    if query.is_empty() {
        return Err(FriggError::InvalidInput(
            "structural query must not be empty".to_owned(),
        ));
    }

    let ts_language = tree_sitter_language_for_path(language, path);
    let compiled_query = Query::new(&ts_language, query).map_err(|error| {
        FriggError::InvalidInput(format!(
            "invalid structural query for {}: {error}",
            language.as_str()
        ))
    })?;

    let mut parser = parser_for_path(language, path)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for structural search: {}",
            path.display()
        ))
    })?;
    let mut cursor = QueryCursor::new();
    let mut matches = Vec::new();
    let mut captures = cursor.captures(&compiled_query, tree.root_node(), source.as_bytes());
    while let Some((query_match, capture_index)) = captures.next() {
        let capture = query_match.captures[*capture_index];
        let span = source_span(capture.node);
        let start_byte = capture.node.start_byte();
        let end_byte = capture.node.end_byte();
        let excerpt = if start_byte <= end_byte && end_byte <= source.len() {
            String::from_utf8_lossy(&source.as_bytes()[start_byte..end_byte])
                .trim()
                .to_owned()
        } else {
            String::new()
        };
        if excerpt.is_empty() {
            continue;
        }
        matches.push(StructuralQueryMatch {
            path: path.to_path_buf(),
            span,
            excerpt,
        });
    }
    matches.sort_by(structural_query_match_order);
    matches.dedup();
    Ok(matches)
}

pub fn extract_symbols_from_file(path: &Path) -> FriggResult<Vec<SymbolDefinition>> {
    let language = SymbolLanguage::from_path(path).ok_or_else(|| {
        FriggError::InvalidInput(format!(
            "unsupported source file extension for symbol extraction: {}",
            path.display()
        ))
    })?;
    let source = fs::read_to_string(path).map_err(FriggError::Io)?;
    extract_symbols_from_source(language, path, &source)
}

pub fn extract_symbols_for_paths(paths: &[PathBuf]) -> SymbolExtractionOutput {
    let mut ordered_paths = paths.to_vec();
    ordered_paths.sort();

    let mut output = SymbolExtractionOutput::default();
    for path in ordered_paths {
        let Some(language) = SymbolLanguage::from_path(&path) else {
            continue;
        };

        match fs::read_to_string(&path) {
            Ok(source) => match extract_symbols_from_source(language, &path, &source) {
                Ok(mut symbols) => output.symbols.append(&mut symbols),
                Err(err) => output.diagnostics.push(SymbolExtractionDiagnostic {
                    path: path.clone(),
                    language: Some(language),
                    message: err.to_string(),
                }),
            },
            Err(err) => output.diagnostics.push(SymbolExtractionDiagnostic {
                path: path.clone(),
                language: Some(language),
                message: err.to_string(),
            }),
        }
    }

    output.symbols.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.span.start_byte.cmp(&right.span.start_byte))
            .then(left.span.end_byte.cmp(&right.span.end_byte))
            .then(left.kind.cmp(&right.kind))
            .then(left.name.cmp(&right.name))
            .then(left.stable_id.cmp(&right.stable_id))
    });
    output
}

fn collect_symbols_from_tree(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    tree: &Tree,
    symbols: &mut Vec<SymbolDefinition>,
) {
    if language == SymbolLanguage::Blade {
        collect_blade_symbols_from_source(path, source, symbols);
        return;
    }
    collect_symbols_from_node(language, path, source, tree.root_node(), symbols);
}

fn collect_symbols_from_node(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    node: Node<'_>,
    symbols: &mut Vec<SymbolDefinition>,
) {
    if let Some((kind, name)) = symbol_from_node(language, source, node) {
        let span = source_span(node);
        symbols.push(SymbolDefinition {
            stable_id: stable_symbol_id(language, kind, path, &name, &span),
            language,
            kind,
            name,
            path: path.to_path_buf(),
            line: span.start_line,
            span,
        });
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_symbols_from_node(language, path, source, child, symbols);
    }
}

pub(crate) fn push_symbol_definition(
    symbols: &mut Vec<SymbolDefinition>,
    language: SymbolLanguage,
    kind: SymbolKind,
    path: &Path,
    name: &str,
    span: SourceSpan,
) {
    let trimmed_name = name.trim();
    if trimmed_name.is_empty() {
        return;
    }
    let stable_id = stable_symbol_id(language, kind, path, trimmed_name, &span);
    if symbols.iter().any(|symbol| symbol.stable_id == stable_id) {
        return;
    }
    symbols.push(SymbolDefinition {
        stable_id,
        language,
        kind,
        name: trimmed_name.to_owned(),
        path: path.to_path_buf(),
        line: span.start_line,
        span,
    });
}

pub(crate) fn source_span_from_offsets(
    source: &str,
    start_byte: usize,
    end_byte: usize,
) -> SourceSpan {
    let start_byte = start_byte.min(source.len());
    let end_byte = end_byte.max(start_byte).min(source.len());
    let (start_line, start_column) = line_column_for_offset(source, start_byte);
    let (end_line, end_column) = line_column_for_offset(source, end_byte);
    SourceSpan {
        start_byte,
        end_byte,
        start_line,
        start_column,
        end_line,
        end_column,
    }
}

pub(crate) fn line_column_for_offset(source: &str, offset: usize) -> (usize, usize) {
    let clamped = offset.min(source.len());
    let bytes = source.as_bytes();
    let prefix = &bytes[..clamped];
    let line = prefix.iter().filter(|byte| **byte == b'\n').count() + 1;
    let line_start = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map(|index| index + 1)
        .unwrap_or(0);
    let column = clamped.saturating_sub(line_start) + 1;
    (line, column)
}

pub(crate) fn source_span(node: Node<'_>) -> SourceSpan {
    let start = node.start_position();
    let end = node.end_position();
    SourceSpan {
        start_byte: node.start_byte(),
        end_byte: node.end_byte(),
        start_line: start.row + 1,
        start_column: start.column + 1,
        end_line: end.row + 1,
        end_column: end.column + 1,
    }
}

fn stable_symbol_id(
    language: SymbolLanguage,
    kind: SymbolKind,
    path: &Path,
    name: &str,
    span: &SourceSpan,
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(language.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(kind.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(path.to_string_lossy().as_bytes());
    hasher.update(&[0]);
    hasher.update(name.as_bytes());
    hasher.update(&[0]);
    hasher.update(span.start_byte.to_string().as_bytes());
    hasher.update(&[0]);
    hasher.update(span.end_byte.to_string().as_bytes());
    format!("sym-{}", hasher.finalize().to_hex())
}

fn symbol_definition_order(
    left: &SymbolDefinition,
    right: &SymbolDefinition,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.span.start_byte.cmp(&right.span.start_byte))
        .then(left.span.end_byte.cmp(&right.span.end_byte))
        .then(left.kind.cmp(&right.kind))
        .then(left.name.cmp(&right.name))
        .then(left.stable_id.cmp(&right.stable_id))
}

fn structural_query_match_order(
    left: &StructuralQueryMatch,
    right: &StructuralQueryMatch,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.span.start_byte.cmp(&right.span.start_byte))
        .then(left.span.end_byte.cmp(&right.span.end_byte))
        .then(left.span.start_line.cmp(&right.span.start_line))
        .then(left.span.start_column.cmp(&right.span.start_column))
        .then(left.excerpt.cmp(&right.excerpt))
}

fn heuristic_reference_order(
    left: &HeuristicReference,
    right: &HeuristicReference,
) -> std::cmp::Ordering {
    right
        .confidence
        .cmp(&left.confidence)
        .then(left.path.cmp(&right.path))
        .then(left.line.cmp(&right.line))
        .then(left.column.cmp(&right.column))
        .then(
            heuristic_evidence_rank(&right.evidence).cmp(&heuristic_evidence_rank(&left.evidence)),
        )
}

fn heuristic_evidence_rank(evidence: &HeuristicReferenceEvidence) -> u8 {
    match evidence {
        HeuristicReferenceEvidence::GraphRelation { .. } => 2,
        HeuristicReferenceEvidence::LexicalToken => 1,
    }
}

fn upsert_heuristic_reference(
    by_location: &mut BTreeMap<(PathBuf, usize, usize), HeuristicReference>,
    candidate: HeuristicReference,
) {
    let key = (candidate.path.clone(), candidate.line, candidate.column);
    let should_replace = match by_location.get(&key) {
        None => true,
        Some(existing) => {
            candidate.confidence > existing.confidence
                || (candidate.confidence == existing.confidence
                    && heuristic_evidence_rank(&candidate.evidence)
                        > heuristic_evidence_rank(&existing.evidence))
        }
    };

    if should_replace {
        by_location.insert(key, candidate);
    }
}

fn is_identifier_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(is_identifier_byte)
}

fn token_columns(line: &str, token: &str) -> Vec<usize> {
    if token.is_empty() || token.len() > line.len() {
        return Vec::new();
    }

    let mut columns = Vec::new();
    let mut offset = 0;
    while let Some(relative) = line[offset..].find(token) {
        let start = offset + relative;
        let end = start + token.len();
        if token_has_boundaries(line.as_bytes(), start, end) {
            columns.push(start + 1);
        }
        offset = end;
        if offset >= line.len() {
            break;
        }
    }
    columns
}

fn token_has_boundaries(line: &[u8], start: usize, end: usize) -> bool {
    let left_is_boundary = if start == 0 {
        true
    } else {
        !is_identifier_byte(line[start - 1])
    };
    let right_is_boundary = if end >= line.len() {
        true
    } else {
        !is_identifier_byte(line[end])
    };

    left_is_boundary && right_is_boundary
}

fn is_identifier_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'$'
}

fn find_innermost_symbol_for_line_in_file<'a>(
    symbols_for_path: &[&'a SymbolDefinition],
    line: usize,
) -> Option<&'a SymbolDefinition> {
    symbols_for_path
        .iter()
        .copied()
        .filter(|symbol| line >= symbol.span.start_line && line <= symbol.span.end_line)
        .min_by(|left, right| {
            let left_span = left.span.end_line.saturating_sub(left.span.start_line);
            let right_span = right.span.end_line.saturating_sub(right.span.start_line);
            left_span
                .cmp(&right_span)
                .then(left.span.start_line.cmp(&right.span.start_line))
                .then(left.stable_id.cmp(&right.stable_id))
        })
}

#[derive(Debug, Clone)]
pub struct ManifestStore {
    storage: Storage,
}

impl ManifestStore {
    pub fn new(db_path: impl Into<PathBuf>) -> Self {
        Self {
            storage: Storage::new(db_path),
        }
    }

    pub fn initialize(&self) -> FriggResult<()> {
        self.storage.initialize()
    }

    pub(crate) fn initialize_for_reindex(&self, semantic_enabled: bool) -> FriggResult<()> {
        if semantic_enabled {
            self.storage.initialize()
        } else {
            self.storage.initialize_without_vector_store()
        }
    }

    pub fn persist_snapshot_manifest(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        entries: &[FileDigest],
    ) -> FriggResult<()> {
        let manifest_entries = entries
            .iter()
            .map(file_digest_to_manifest_entry)
            .collect::<Vec<_>>();
        self.storage
            .upsert_manifest(repository_id, snapshot_id, &manifest_entries)
    }

    pub fn load_snapshot_manifest(&self, snapshot_id: &str) -> FriggResult<Vec<FileDigest>> {
        self.storage
            .load_manifest_for_snapshot(snapshot_id)
            .map(|entries| {
                entries
                    .into_iter()
                    .map(manifest_entry_to_file_digest)
                    .collect()
            })
    }

    pub fn load_latest_manifest_for_repository(
        &self,
        repository_id: &str,
    ) -> FriggResult<Option<RepositoryManifest>> {
        self.storage
            .load_latest_manifest_for_repository(repository_id)
            .map(|snapshot| {
                snapshot.map(|snapshot| RepositoryManifest {
                    repository_id: snapshot.repository_id,
                    snapshot_id: snapshot.snapshot_id,
                    entries: snapshot
                        .entries
                        .into_iter()
                        .map(manifest_entry_to_file_digest)
                        .collect(),
                })
            })
    }

    pub fn delete_snapshot(&self, snapshot_id: &str) -> FriggResult<()> {
        self.storage.delete_snapshot(snapshot_id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReindexMode {
    Full,
    ChangedOnly,
}

impl ReindexMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::ChangedOnly => "changed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReindexSummary {
    pub repository_id: String,
    pub snapshot_id: String,
    pub files_scanned: usize,
    pub files_changed: usize,
    pub files_deleted: usize,
    pub diagnostics: ReindexDiagnostics,
    pub duration_ms: u128,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReindexDiagnostics {
    pub entries: Vec<ManifestBuildDiagnostic>,
}

impl ReindexDiagnostics {
    pub fn total_count(&self) -> usize {
        self.entries.len()
    }

    pub fn count_by_kind(&self, kind: ManifestDiagnosticKind) -> usize {
        self.entries
            .iter()
            .filter(|diagnostic| diagnostic.kind == kind)
            .count()
    }
}

pub fn reindex_repository(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
) -> FriggResult<ReindexSummary> {
    let semantic_runtime = resolve_semantic_runtime_config_from_env()?;
    let credentials = SemanticRuntimeCredentials::from_process_env();
    reindex_repository_with_runtime_config(
        repository_id,
        workspace_root,
        db_path,
        mode,
        &semantic_runtime,
        &credentials,
    )
}

pub fn reindex_repository_with_runtime_config(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
) -> FriggResult<ReindexSummary> {
    reindex_repository_with_runtime_config_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &[],
    )
}

pub fn reindex_repository_with_runtime_config_and_dirty_paths(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
) -> FriggResult<ReindexSummary> {
    let executor = RuntimeSemanticEmbeddingExecutor::new(credentials.clone());
    reindex_repository_with_semantic_executor_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        dirty_path_hints,
        &executor,
    )
}

#[cfg(test)]
fn reindex_repository_with_semantic_executor(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<ReindexSummary> {
    reindex_repository_with_semantic_executor_and_dirty_paths(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &[],
        executor,
    )
}

fn reindex_repository_with_semantic_executor_and_dirty_paths(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    dirty_path_hints: &[PathBuf],
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<ReindexSummary> {
    let started_at = Instant::now();
    let db_preexisted = db_path.exists();
    let manifest_store = ManifestStore::new(db_path);
    manifest_store.initialize_for_reindex(semantic_runtime.enabled)?;
    let previous_manifest = if mode == ReindexMode::Full && !db_preexisted {
        None
    } else {
        manifest_store.load_latest_manifest_for_repository(repository_id)?
    };
    let previous_snapshot_id = previous_manifest
        .as_ref()
        .map(|manifest| manifest.snapshot_id.clone());
    let previous_entries = previous_manifest
        .as_ref()
        .map(|manifest| manifest.entries.as_slice())
        .unwrap_or(&[]);

    let manifest_builder = ManifestBuilder::default();
    let manifest_output = match mode {
        ReindexMode::Full => manifest_builder.build_with_diagnostics(workspace_root)?,
        ReindexMode::ChangedOnly if previous_manifest.is_some() => manifest_builder
            .build_changed_only_with_hints_and_diagnostics(
                workspace_root,
                previous_entries,
                dirty_path_hints,
            )?,
        ReindexMode::ChangedOnly => manifest_builder.build_with_diagnostics(workspace_root)?,
    };
    let current_manifest = manifest_output.entries;
    let diagnostics = ReindexDiagnostics {
        entries: manifest_output.diagnostics,
    };
    let manifest_diff = if mode == ReindexMode::Full && previous_entries.is_empty() {
        ManifestDiff::default()
    } else {
        diff(previous_entries, &current_manifest)
    };
    let files_scanned = current_manifest.len();
    let files_changed = match mode {
        ReindexMode::Full => files_scanned,
        ReindexMode::ChangedOnly => manifest_diff.added.len() + manifest_diff.modified.len(),
    };
    let files_deleted = manifest_diff.deleted.len();

    let snapshot_id = if mode == ReindexMode::ChangedOnly
        && files_changed == 0
        && files_deleted == 0
        && previous_manifest.is_some()
    {
        previous_manifest
            .as_ref()
            .map(|manifest| manifest.snapshot_id.clone())
            .ok_or_else(|| {
                FriggError::Internal(
                    "failed to resolve previous snapshot identifier for unchanged manifest"
                        .to_owned(),
                )
            })?
    } else {
        let snapshot_id = deterministic_snapshot_id(repository_id, &current_manifest);
        manifest_store.persist_snapshot_manifest(repository_id, &snapshot_id, &current_manifest)?;
        snapshot_id
    };
    let rollback_snapshot_on_semantic_failure = previous_snapshot_id
        .as_deref()
        .map(|previous| previous != snapshot_id)
        .unwrap_or(true);

    if semantic_runtime.enabled {
        let storage = Storage::new(db_path);
        let semantic_result = match mode {
            ReindexMode::Full => build_semantic_embedding_records(
                repository_id,
                workspace_root,
                &snapshot_id,
                &current_manifest,
                semantic_runtime,
                credentials,
                executor,
            )
            .and_then(|semantic_records| {
                storage.replace_semantic_embeddings_for_repository(
                    repository_id,
                    &snapshot_id,
                    &semantic_records,
                )
            }),
            ReindexMode::ChangedOnly => {
                if files_changed > 0 || files_deleted > 0 || previous_manifest.is_none() {
                    let semantic_manifest = manifest_diff
                        .added
                        .iter()
                        .chain(manifest_diff.modified.iter())
                        .cloned()
                        .collect::<Vec<_>>();
                    build_semantic_embedding_records(
                        repository_id,
                        workspace_root,
                        &snapshot_id,
                        &semantic_manifest,
                        semantic_runtime,
                        credentials,
                        executor,
                    )
                    .and_then(|semantic_records| {
                        let changed_paths = manifest_diff
                            .added
                            .iter()
                            .chain(manifest_diff.modified.iter())
                            .map(|digest| {
                                normalize_repository_relative_path(workspace_root, &digest.path)
                            })
                            .collect::<FriggResult<Vec<_>>>()?;
                        let deleted_paths = manifest_diff
                            .deleted
                            .iter()
                            .map(|digest| {
                                normalize_repository_relative_path(workspace_root, &digest.path)
                            })
                            .collect::<FriggResult<Vec<_>>>()?;
                        let previous_snapshot_id = previous_manifest
                            .as_ref()
                            .map(|manifest| manifest.snapshot_id.as_str());
                        storage.advance_semantic_embeddings_for_repository(
                            repository_id,
                            previous_snapshot_id,
                            &snapshot_id,
                            &changed_paths,
                            &deleted_paths,
                            &semantic_records,
                        )
                    })
                } else {
                    Ok(())
                }
            }
        };
        if let Err(err) = semantic_result {
            if rollback_snapshot_on_semantic_failure {
                if let Err(rollback_err) = manifest_store.delete_snapshot(&snapshot_id) {
                    return Err(FriggError::Internal(format!(
                        "{err}; failed to roll back snapshot '{snapshot_id}' after semantic reindex failure: {rollback_err}"
                    )));
                }
            }
            return Err(err);
        }
    }

    Ok(ReindexSummary {
        repository_id: repository_id.to_owned(),
        snapshot_id,
        files_scanned,
        files_changed,
        files_deleted,
        diagnostics,
        duration_ms: started_at.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::env;
    use std::future::Future;
    use std::path::{Path, PathBuf};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::time::{SystemTime, UNIX_EPOCH};
    use std::{fs, iter};

    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;

    use super::{
        BladeRelationKind, FileDigest, HeuristicReferenceConfidence, HeuristicReferenceEvidence,
        ManifestBuilder, ManifestDiagnosticKind, ManifestStore, PhpDeclarationRelation,
        PhpTargetEvidenceKind, PhpTypeEvidenceKind, ReindexMode, RuntimeSemanticEmbeddingExecutor,
        SEMANTIC_CHUNK_MAX_CHARS, SemanticRuntimeEmbeddingExecutor, SymbolKind, SymbolLanguage,
        build_file_semantic_chunks, build_semantic_chunk_candidates, diff,
        extract_blade_source_evidence_from_source, extract_php_declaration_relations_from_source,
        extract_php_source_evidence_from_source, extract_symbols_for_paths,
        extract_symbols_from_source, file_digest_order, mark_local_flux_overlays,
        navigation_symbol_target_rank, register_symbol_definitions, reindex_repository,
        reindex_repository_with_semantic_executor, resolve_heuristic_references,
        search_structural_in_source, semantic_chunk_language_for_path,
    };
    use crate::domain::{FriggError, FriggResult};
    use crate::graph::{RelationKind, SymbolGraph};
    use crate::settings::{
        SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider,
    };
    use crate::storage::Storage;

    #[derive(Debug, Default)]
    struct FixtureSemanticEmbeddingExecutor;

    impl SemanticRuntimeEmbeddingExecutor for FixtureSemanticEmbeddingExecutor {
        fn embed_documents<'a>(
            &'a self,
            _provider: SemanticRuntimeProvider,
            _model: &'a str,
            input: Vec<String>,
            _trace_id: Option<String>,
        ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
            Box::pin(async move {
                Ok(input
                    .into_iter()
                    .enumerate()
                    .map(|(index, text)| deterministic_fixture_embedding(&text, index))
                    .collect::<Vec<_>>())
            })
        }
    }

    #[derive(Debug, Default, Clone)]
    struct CountingSemanticEmbeddingExecutor {
        inputs: Arc<Mutex<Vec<String>>>,
    }

    impl CountingSemanticEmbeddingExecutor {
        fn observed_inputs(&self) -> Vec<String> {
            self.inputs
                .lock()
                .expect("counting semantic executor mutex poisoned")
                .clone()
        }
    }

    impl SemanticRuntimeEmbeddingExecutor for CountingSemanticEmbeddingExecutor {
        fn embed_documents<'a>(
            &'a self,
            _provider: SemanticRuntimeProvider,
            _model: &'a str,
            input: Vec<String>,
            _trace_id: Option<String>,
        ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
            let inputs = self.inputs.clone();
            Box::pin(async move {
                inputs
                    .lock()
                    .expect("counting semantic executor mutex poisoned")
                    .extend(input.iter().cloned());
                Ok(input
                    .into_iter()
                    .enumerate()
                    .map(|(index, text)| deterministic_fixture_embedding(&text, index))
                    .collect::<Vec<_>>())
            })
        }
    }

    #[derive(Debug, Default)]
    struct FailingSemanticEmbeddingExecutor;

    impl SemanticRuntimeEmbeddingExecutor for FailingSemanticEmbeddingExecutor {
        fn embed_documents<'a>(
            &'a self,
            _provider: SemanticRuntimeProvider,
            _model: &'a str,
            _input: Vec<String>,
            _trace_id: Option<String>,
        ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
            Box::pin(async move {
                Err(FriggError::Internal(
                    "synthetic semantic provider failure request_context{model=text-embedding-3-small, inputs=1, input_chars_total=23, max_input_chars=23, body_bytes=96, body_blake3=test-hash, trace_id=trace-test}".to_owned(),
                ))
            })
        }
    }

    fn deterministic_fixture_embedding(text: &str, index: usize) -> Vec<f32> {
        let mut hasher = super::Hasher::new();
        hasher.update(index.to_string().as_bytes());
        hasher.update(&[0]);
        hasher.update(text.as_bytes());
        let digest = hasher.finalize();
        let mut embedding = digest
            .as_bytes()
            .chunks_exact(4)
            .take(8)
            .map(|chunk| {
                let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                (value as f32) / (u32::MAX as f32)
            })
            .collect::<Vec<_>>();
        embedding.resize(crate::storage::DEFAULT_VECTOR_DIMENSIONS, 0.0);
        embedding
    }

    #[test]
    fn manifest_diff_classifies_added_modified_deleted_in_path_order() {
        let old = vec![
            digest("repo/zeta.rs", 10, Some(10), "hash-z"),
            digest("repo/alpha.rs", 1, Some(1), "hash-a"),
            digest("repo/charlie.rs", 3, Some(3), "hash-c-old"),
        ];
        let new = vec![
            digest("repo/bravo.rs", 2, Some(2), "hash-b"),
            digest("repo/charlie.rs", 4, Some(4), "hash-c-new"),
            digest("repo/zeta.rs", 10, Some(10), "hash-z"),
        ];

        let manifest_diff = diff(&old, &new);

        assert_eq!(
            manifest_diff.added,
            vec![digest("repo/bravo.rs", 2, Some(2), "hash-b")]
        );
        assert_eq!(
            manifest_diff.modified,
            vec![digest("repo/charlie.rs", 4, Some(4), "hash-c-new")]
        );
        assert_eq!(
            manifest_diff.deleted,
            vec![digest("repo/alpha.rs", 1, Some(1), "hash-a")]
        );
    }

    #[test]
    fn navigation_symbol_target_rank_is_stable_and_precedence_ordered() {
        let symbol = super::SymbolDefinition {
            stable_id: "sym-user-001".to_owned(),
            language: SymbolLanguage::Rust,
            kind: SymbolKind::Struct,
            name: "User".to_owned(),
            path: PathBuf::from("src/lib.rs"),
            line: 1,
            span: super::SourceSpan {
                start_byte: 0,
                end_byte: 10,
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 10,
            },
        };

        assert_eq!(
            navigation_symbol_target_rank(&symbol, "sym-user-001"),
            Some(0)
        );
        assert_eq!(navigation_symbol_target_rank(&symbol, "User"), Some(1));
        assert_eq!(navigation_symbol_target_rank(&symbol, "user"), Some(2));
        assert_eq!(navigation_symbol_target_rank(&symbol, "Account"), None);
    }

    #[test]
    fn manifest_diff_detects_mtime_only_change_as_modified() {
        let old = vec![digest("repo/file.rs", 10, Some(100), "same-hash")];
        let new = vec![digest("repo/file.rs", 10, Some(200), "same-hash")];

        let manifest_diff = diff(&old, &new);

        assert!(manifest_diff.added.is_empty());
        assert_eq!(
            manifest_diff.modified,
            vec![digest("repo/file.rs", 10, Some(200), "same-hash")]
        );
        assert!(manifest_diff.deleted.is_empty());
    }

    #[test]
    fn manifest_diff_is_empty_for_identical_records_with_different_input_order() {
        let old = vec![
            digest("repo/b.rs", 2, Some(2), "hash-b"),
            digest("repo/a.rs", 1, Some(1), "hash-a"),
            digest("repo/c.rs", 3, Some(3), "hash-c"),
        ];
        let new = vec![
            digest("repo/c.rs", 3, Some(3), "hash-c"),
            digest("repo/a.rs", 1, Some(1), "hash-a"),
            digest("repo/b.rs", 2, Some(2), "hash-b"),
        ];

        let manifest_diff = diff(&old, &new);

        assert!(manifest_diff.added.is_empty());
        assert!(manifest_diff.modified.is_empty());
        assert!(manifest_diff.deleted.is_empty());
    }

    #[test]
    fn determinism_manifest_builder_repeated_runs_match_exactly() -> FriggResult<()> {
        let fixture_root = fixture_repo_root();
        let builder = ManifestBuilder::default();

        let first = builder.build(&fixture_root)?;
        let second = builder.build(&fixture_root)?;
        let third = builder.build(&fixture_root)?;

        assert_eq!(first, second);
        assert_eq!(second, third);
        Ok(())
    }

    #[test]
    fn determinism_manifest_builder_uses_fixture_only_expected_paths() -> FriggResult<()> {
        let fixture_root = fixture_repo_root();
        let builder = ManifestBuilder::default();

        let manifest = builder.build(&fixture_root)?;
        let relative_paths = manifest_relative_paths(&manifest, &fixture_root)?;

        assert_eq!(
            relative_paths,
            vec![
                PathBuf::from("README.md"),
                PathBuf::from("src/lib.rs"),
                PathBuf::from("src/nested/data.txt"),
            ]
        );
        Ok(())
    }

    #[test]
    fn manifest_builder_respects_gitignored_contract_artifacts() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("manifest-builder-gitignored-contracts");
        prepare_workspace(
            &workspace_root,
            &[
                ("contracts/errors.md", "invalid_params\n"),
                ("src/main.rs", "fn main() {}\n"),
            ],
        )?;
        fs::write(workspace_root.join(".gitignore"), "contracts\n").map_err(FriggError::Io)?;

        let manifest = ManifestBuilder::default().build(&workspace_root)?;
        let relative_paths = manifest_relative_paths(&manifest, &workspace_root)?;

        assert!(
            !relative_paths.contains(&PathBuf::from("contracts/errors.md")),
            "manifest discovery should respect gitignored contract artifacts"
        );

        Ok(())
    }

    #[test]
    fn manifest_builder_excludes_target_artifacts_without_gitignore() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("manifest-builder-target-exclusion");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/main.rs", "fn main() {}\n"),
                ("target/debug/app", "binary\n"),
            ],
        )?;

        let manifest = ManifestBuilder::default().build(&workspace_root)?;
        let relative_paths = manifest_relative_paths(&manifest, &workspace_root)?;

        assert!(
            !relative_paths
                .iter()
                .any(|path| path.starts_with(Path::new("target"))),
            "target artifacts must stay excluded from manifest discovery: {relative_paths:?}"
        );

        Ok(())
    }

    #[test]
    fn manifest_builder_respects_root_ignore_file_for_auxiliary_trees() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("manifest-builder-root-ignore");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/main.rs", "fn main() {}\n"),
                ("auxiliary/embedded-repo/src/lib.rs", "pub fn leaked() {}\n"),
            ],
        )?;
        fs::write(workspace_root.join(".ignore"), "auxiliary/\n").map_err(FriggError::Io)?;

        let manifest = ManifestBuilder::default().build(&workspace_root)?;
        let relative_paths = manifest_relative_paths(&manifest, &workspace_root)?;

        assert!(
            !relative_paths
                .iter()
                .any(|path| path.starts_with(Path::new("auxiliary"))),
            "root ignore files must exclude auxiliary trees from manifest discovery: {relative_paths:?}"
        );

        Ok(())
    }

    #[test]
    fn incremental_roundtrip_persist_load_and_diff() -> FriggResult<()> {
        let db_path = temp_db_path("incremental-roundtrip");
        let fixture_root = fixture_repo_root();
        let manifest_store = ManifestStore::new(&db_path);
        manifest_store.initialize()?;

        let repository_id = "repo-001";
        let snapshot_old = "snapshot-001";
        let snapshot_new = "snapshot-002";
        let builder = ManifestBuilder::default();
        let old_manifest = builder.build(&fixture_root)?;

        manifest_store.persist_snapshot_manifest(repository_id, snapshot_old, &old_manifest)?;
        let loaded_old = manifest_store.load_snapshot_manifest(snapshot_old)?;
        assert_eq!(loaded_old, old_manifest);

        let latest_before = manifest_store
            .load_latest_manifest_for_repository(repository_id)?
            .expect("expected latest repository manifest");
        assert_eq!(latest_before.snapshot_id, snapshot_old);
        assert_eq!(latest_before.entries, old_manifest);

        let new_manifest = mutate_manifest_for_incremental_roundtrip(&old_manifest, &fixture_root)?;
        manifest_store.persist_snapshot_manifest(repository_id, snapshot_new, &new_manifest)?;

        let loaded_new = manifest_store.load_snapshot_manifest(snapshot_new)?;
        assert_eq!(loaded_new, new_manifest);

        let latest_after = manifest_store
            .load_latest_manifest_for_repository(repository_id)?
            .expect("expected latest repository manifest after second snapshot");
        assert_eq!(latest_after.snapshot_id, snapshot_new);
        assert_eq!(latest_after.entries, new_manifest);

        let manifest_diff = diff(&latest_before.entries, &latest_after.entries);
        assert_eq!(manifest_diff.added.len(), 1);
        assert_eq!(manifest_diff.modified.len(), 1);
        assert_eq!(manifest_diff.deleted.len(), 1);
        assert_eq!(
            manifest_diff.added[0].path,
            fixture_root.join("src/incremental-new.rs")
        );
        assert_eq!(
            manifest_diff.modified[0].path,
            fixture_root.join("README.md")
        );
        assert_eq!(
            manifest_diff.deleted[0].path,
            fixture_root.join("src/nested/data.txt")
        );

        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn incremental_roundtrip_changed_only_reports_zero_for_unchanged_workspace() -> FriggResult<()>
    {
        let db_path = temp_db_path("incremental-unchanged-db");
        let workspace_root = temp_workspace_root("incremental-unchanged-workspace");
        prepare_workspace(
            &workspace_root,
            &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
        )?;

        let full_summary =
            reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;
        let changed_summary = reindex_repository(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
        )?;

        assert_eq!(full_summary.files_scanned, 2);
        assert_eq!(full_summary.files_changed, 2);
        assert_eq!(full_summary.files_deleted, 0);
        assert_eq!(full_summary.diagnostics.total_count(), 0);
        assert_eq!(changed_summary.files_scanned, 2);
        assert_eq!(changed_summary.files_changed, 0);
        assert_eq!(changed_summary.files_deleted, 0);
        assert_eq!(changed_summary.diagnostics.total_count(), 0);
        assert_eq!(changed_summary.snapshot_id, full_summary.snapshot_id);

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn incremental_roundtrip_changed_only_detects_modified_added_and_deleted_files()
    -> FriggResult<()> {
        let db_path = temp_db_path("incremental-changed-db");
        let workspace_root = temp_workspace_root("incremental-changed-workspace");
        prepare_workspace(
            &workspace_root,
            &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
        )?;

        let full_summary =
            reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;

        fs::write(workspace_root.join("README.md"), "hello changed\n").map_err(FriggError::Io)?;
        fs::remove_file(workspace_root.join("src/main.rs")).map_err(FriggError::Io)?;
        fs::write(workspace_root.join("src/new.rs"), "pub fn added() {}\n")
            .map_err(FriggError::Io)?;

        let changed_summary = reindex_repository(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
        )?;

        assert_eq!(changed_summary.files_scanned, 2);
        assert_eq!(changed_summary.files_changed, 2);
        assert_eq!(changed_summary.files_deleted, 1);
        assert_eq!(changed_summary.diagnostics.total_count(), 0);
        assert_ne!(changed_summary.snapshot_id, full_summary.snapshot_id);

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_indexing_reindex_persists_deterministic_embeddings_when_enabled() -> FriggResult<()>
    {
        let db_path = temp_db_path("semantic-enabled-roundtrip");
        let workspace_root = temp_workspace_root("semantic-enabled-roundtrip");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/main.rs", "pub fn main_api() { println!(\"main\"); }\n"),
                (
                    "src/lib.rs",
                    "pub struct User;\nimpl User { pub fn id(&self) -> u64 { 7 } }\n",
                ),
                ("README.md", "# Frigg\nsemantic runtime indexed\n"),
            ],
        )?;

        let semantic_runtime = semantic_runtime_enabled_openai();
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let executor = FixtureSemanticEmbeddingExecutor;
        let first = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &semantic_runtime,
            &credentials,
            &executor,
        )?;
        let storage = Storage::new(&db_path);
        let first_semantic = storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &first.snapshot_id)?;
        assert!(
            !first_semantic.is_empty(),
            "expected semantic embeddings for supported source and markdown files"
        );
        assert!(
            first_semantic
                .iter()
                .all(|record| record.path.starts_with("src/") || record.path == "README.md"),
            "semantic indexing should use repository-relative canonical source paths"
        );
        assert!(
            first_semantic
                .iter()
                .any(|record| record.path == "README.md"),
            "README.md should participate in semantic indexing"
        );
        assert!(
            first_semantic
                .windows(2)
                .all(|window| window[0].path <= window[1].path),
            "semantic records should be deterministically ordered by path"
        );

        let second = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &semantic_runtime,
            &credentials,
            &executor,
        )?;
        let second_semantic = storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?;
        assert_eq!(first.snapshot_id, second.snapshot_id);
        assert_eq!(first_semantic, second_semantic);

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_indexing_enabled_succeeds_inside_existing_tokio_runtime() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-enabled-inside-runtime");
        let workspace_root = temp_workspace_root("semantic-enabled-inside-runtime");
        prepare_workspace(
            &workspace_root,
            &[("src/main.rs", "pub fn inside_runtime() {}\n")],
        )?;

        let semantic_runtime = semantic_runtime_enabled_openai();
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let executor = FixtureSemanticEmbeddingExecutor;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime should build");
        let summary = runtime.block_on(async {
            reindex_repository_with_semantic_executor(
                "repo-001",
                &workspace_root,
                &db_path,
                ReindexMode::Full,
                &semantic_runtime,
                &credentials,
                &executor,
            )
        })?;

        let storage = Storage::new(&db_path);
        let semantic_rows = storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
        assert!(
            !semantic_rows.is_empty(),
            "expected semantic embeddings when reindex runs inside a tokio runtime"
        );

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_indexing_disabled_preserves_reindex_behavior() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-disabled-preserves");
        let workspace_root = temp_workspace_root("semantic-disabled-preserves");
        prepare_workspace(
            &workspace_root,
            &[("src/main.rs", "fn main() {}\n"), ("README.md", "hello\n")],
        )?;

        let summary = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
            &RuntimeSemanticEmbeddingExecutor::new(SemanticRuntimeCredentials::default()),
        )?;
        assert_eq!(summary.files_scanned, 2);
        assert_eq!(summary.files_changed, 2);
        assert_eq!(summary.files_deleted, 0);

        let storage = Storage::new(&db_path);
        let semantic_rows = storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &summary.snapshot_id)?;
        assert!(semantic_rows.is_empty());

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_indexing_validation_failure_keeps_existing_semantic_state() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-invalid-does-not-mutate");
        let workspace_root = temp_workspace_root("semantic-invalid-does-not-mutate");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/main.rs", "pub fn stable() {}\n"),
                ("README.md", "hello\n"),
            ],
        )?;

        let executor = FixtureSemanticEmbeddingExecutor;
        let valid_runtime = semantic_runtime_enabled_openai();
        let valid_credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let valid_summary = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &valid_runtime,
            &valid_credentials,
            &executor,
        )?;

        let storage = Storage::new(&db_path);
        let before = storage.load_semantic_embeddings_for_repository_snapshot(
            "repo-001",
            &valid_summary.snapshot_id,
        )?;
        assert!(
            !before.is_empty(),
            "expected seeded semantic records before invalid reindex attempt"
        );

        let invalid_runtime = SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode: false,
        };
        let invalid_credentials = SemanticRuntimeCredentials::default();
        let error = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &invalid_runtime,
            &invalid_credentials,
            &executor,
        )
        .expect_err("missing provider credentials should fail semantic indexing");
        assert!(
            matches!(error, FriggError::InvalidInput(_)),
            "expected invalid input from semantic startup validation, got {error}"
        );
        assert!(
            error
                .to_string()
                .contains("semantic runtime validation failed code=invalid_params"),
            "unexpected semantic validation error: {error}"
        );

        let after = storage.load_semantic_embeddings_for_repository_snapshot(
            "repo-001",
            &valid_summary.snapshot_id,
        )?;
        assert_eq!(before, after);

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_indexing_failure_rolls_back_new_manifest_snapshot() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-failure-rolls-back-manifest");
        let workspace_root = temp_workspace_root("semantic-failure-rolls-back-manifest");
        prepare_workspace(&workspace_root, &[("src/main.rs", "pub fn stable() {}\n")])?;

        let semantic_runtime = semantic_runtime_enabled_openai();
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let executor = FixtureSemanticEmbeddingExecutor;
        let first = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &semantic_runtime,
            &credentials,
            &executor,
        )?;

        fs::write(
            workspace_root.join("src/main.rs"),
            "pub fn changed_after_failure() {}\n",
        )
        .map_err(FriggError::Io)?;

        let error = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &semantic_runtime,
            &credentials,
            &FailingSemanticEmbeddingExecutor,
        )
        .expect_err("failing semantic executor should abort reindex");
        assert!(
            error
                .to_string()
                .contains("semantic embedding batch failed batch_index=0 total_batches=1"),
            "unexpected semantic failure: {error}"
        );

        let storage = Storage::new(&db_path);
        let latest = storage
            .load_latest_manifest_for_repository("repo-001")?
            .expect("expected previous manifest snapshot to remain active");
        assert_eq!(
            latest.snapshot_id, first.snapshot_id,
            "failed semantic reindex must not advance the latest manifest snapshot"
        );
        let semantic_rows = storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &first.snapshot_id)?;
        assert!(
            !semantic_rows.is_empty(),
            "previous semantic rows should remain intact after rollback"
        );

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_indexing_changed_only_updates_only_changed_paths() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-changed-only-updates");
        let workspace_root = temp_workspace_root("semantic-changed-only-updates");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/main.rs", "pub fn main_api() { println!(\"main\"); }\n"),
                ("src/lib.rs", "pub fn stable_lib() -> u64 { 7 }\n"),
            ],
        )?;

        let semantic_runtime = semantic_runtime_enabled_openai();
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let first_executor = CountingSemanticEmbeddingExecutor::default();
        let first = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &semantic_runtime,
            &credentials,
            &first_executor,
        )?;
        let first_inputs = first_executor.observed_inputs();
        assert_eq!(first.snapshot_id.is_empty(), false);
        assert_eq!(first_inputs.len(), 2);

        fs::write(
            workspace_root.join("src/lib.rs"),
            "pub fn changed_lib() -> u64 { 9 }\n",
        )
        .map_err(FriggError::Io)?;

        let second_executor = CountingSemanticEmbeddingExecutor::default();
        let second = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
            &semantic_runtime,
            &credentials,
            &second_executor,
        )?;
        let second_inputs = second_executor.observed_inputs();
        assert_eq!(second.files_changed, 1);
        assert!(second.snapshot_id != first.snapshot_id);
        assert_eq!(second_inputs.len(), 1);
        assert!(
            second_inputs[0].contains("changed_lib"),
            "changed-only semantic indexing should embed only modified path chunks"
        );

        let storage = Storage::new(&db_path);
        let semantic_rows = storage
            .load_semantic_embeddings_for_repository_snapshot("repo-001", &second.snapshot_id)?;
        assert!(
            semantic_rows.len() >= 2,
            "expected unchanged and changed semantic rows in the advanced snapshot"
        );
        assert!(
            semantic_rows.iter().any(|record| {
                record.path == "src/main.rs" && record.content_text.contains("main_api")
            }),
            "unchanged semantic rows should advance into the new snapshot"
        );
        assert!(
            semantic_rows.iter().any(|record| {
                record.path == "src/lib.rs" && record.content_text.contains("changed_lib")
            }),
            "changed semantic rows should be replaced in the new snapshot"
        );
        assert!(
            semantic_rows.iter().all(|record| {
                !(record.path == "src/lib.rs" && record.content_text.contains("stable_lib"))
            }),
            "stale semantic chunks for modified paths should be removed from the advanced snapshot"
        );

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_indexing_reindex_failure_surfaces_batch_context() -> FriggResult<()> {
        let db_path = temp_db_path("semantic-failure-batch-context");
        let workspace_root = temp_workspace_root("semantic-failure-batch-context");
        prepare_workspace(
            &workspace_root,
            &[("src/main.rs", "pub fn failing_semantic_case() {}\n")],
        )?;

        let semantic_runtime = semantic_runtime_enabled_openai();
        let credentials = SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        };
        let error = reindex_repository_with_semantic_executor(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::Full,
            &semantic_runtime,
            &credentials,
            &FailingSemanticEmbeddingExecutor,
        )
        .expect_err("semantic indexing should surface failing batch context");
        let message = error.to_string();
        assert!(
            message.contains("semantic embedding batch failed batch_index=0 total_batches=1"),
            "semantic failure should include batch index context: {message}"
        );
        assert!(
            message.contains("batch_size=1"),
            "semantic failure should include batch size: {message}"
        );
        assert!(
            message.contains("first_chunk=src/main.rs:1-1"),
            "semantic failure should include the first chunk anchor: {message}"
        );
        assert!(
            message.contains("last_chunk=src/main.rs:1-1"),
            "semantic failure should include the last chunk anchor: {message}"
        );
        assert!(
            message.contains("request_context{model=text-embedding-3-small"),
            "semantic failure should preserve provider diagnostics: {message}"
        );

        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn semantic_chunk_candidates_include_docs_and_fixture_text_sources() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("semantic-chunk-doc-sources");
        prepare_workspace(
            &workspace_root,
            &[
                ("README.md", "# Frigg\nsemantic runtime docs\n"),
                (
                    "contracts/errors.md",
                    "# Errors\ninvalid_params maps to -32602\n",
                ),
                (
                    "fixtures/playbooks/deep-search-suite-core.playbook.json",
                    "{\n  \"playbook_id\": \"suite-core\"\n}\n",
                ),
                ("src/lib.rs", "pub fn semantic_runtime() {}\n"),
            ],
        )?;

        let manifest = ManifestBuilder::default().build(&workspace_root)?;
        let chunks = build_semantic_chunk_candidates(
            "repo-001",
            &workspace_root,
            "snapshot-001",
            &manifest,
        )?;

        assert!(
            chunks.iter().any(|chunk| {
                chunk.path.as_ref() == "README.md" && chunk.language.as_ref() == "markdown"
            }),
            "README.md should participate in semantic chunking"
        );
        assert!(
            chunks.iter().any(|chunk| {
                chunk.path.as_ref() == "contracts/errors.md"
                    && chunk.language.as_ref() == "markdown"
            }),
            "contract markdown should participate in semantic chunking"
        );
        assert!(
            chunks.iter().any(|chunk| {
                chunk.path.as_ref() == "fixtures/playbooks/deep-search-suite-core.playbook.json"
                    && chunk.language.as_ref() == "json"
            }),
            "fixture json should participate in semantic chunking"
        );
        assert!(
            chunks.iter().any(|chunk| {
                chunk.path.as_ref() == "src/lib.rs" && chunk.language.as_ref() == "rust"
            }),
            "source files should remain in semantic chunking"
        );

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[test]
    fn semantic_chunk_candidates_include_playbook_markdown_under_generic_policy() -> FriggResult<()>
    {
        let workspace_root = temp_workspace_root("semantic-chunk-skip-playbooks");
        prepare_workspace(
            &workspace_root,
            &[
                (
                    "playbooks/hybrid-search-context-retrieval.md",
                    "# Playbook\nquery echo\n",
                ),
                ("contracts/errors.md", "# Errors\ninvalid_params\n"),
            ],
        )?;

        let manifest = ManifestBuilder::default().build(&workspace_root)?;
        let chunks = build_semantic_chunk_candidates(
            "repo-001",
            &workspace_root,
            "snapshot-001",
            &manifest,
        )?;

        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.path.as_ref() == "playbooks/hybrid-search-context-retrieval.md"),
            "playbook markdown should no longer receive a repo-specific semantic exclusion"
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.path.as_ref() == "contracts/errors.md"),
            "docs markdown should still remain eligible for semantic chunking"
        );

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[test]
    fn semantic_chunking_flushes_markdown_headings_into_separate_chunks() {
        let source = [
            "# Hybrid Search Context Retrieval",
            "",
            "semantic runtime strict failure note metadata",
            "",
            "## Expected Return Cues",
            "",
            "semantic_status",
            "semantic_reason",
        ]
        .join("\n");

        let chunks = build_file_semantic_chunks(
            "repo-001",
            "snapshot-001",
            "contracts/hybrid-search.md",
            "markdown",
            &source,
        );

        assert_eq!(chunks.len(), 2);
        assert!(
            chunks[0]
                .content_text
                .starts_with("# Hybrid Search Context Retrieval")
        );
        assert!(
            chunks[1]
                .content_text
                .starts_with("## Expected Return Cues")
        );
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[1].start_line, 5);
    }

    #[test]
    fn semantic_chunking_splits_oversized_single_line_inputs() {
        let source = "x".repeat(SEMANTIC_CHUNK_MAX_CHARS * 2 + 17);

        let chunks = build_file_semantic_chunks(
            "repo-001",
            "snapshot-001",
            "fixtures/huge.yaml",
            "yaml",
            &source,
        );

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 1);
        assert_eq!(chunks[1].start_line, 1);
        assert_eq!(chunks[1].end_line, 1);
        assert_eq!(chunks[2].start_line, 1);
        assert_eq!(chunks[2].end_line, 1);
        assert!(
            chunks
                .iter()
                .all(|chunk| chunk.content_text.chars().count() <= SEMANTIC_CHUNK_MAX_CHARS)
        );
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| chunk.content_text.len())
                .sum::<usize>(),
            source.len()
        );
    }

    #[test]
    fn semantic_chunk_language_supports_blade_paths() {
        assert_eq!(
            semantic_chunk_language_for_path(Path::new("resources/views/welcome.blade.php")),
            Some("blade")
        );
    }

    #[cfg(unix)]
    #[test]
    fn reindex_continues_with_read_diagnostics_for_unreadable_files() -> FriggResult<()> {
        let db_path = temp_db_path("incremental-unreadable-db");
        let workspace_root = temp_workspace_root("incremental-unreadable-workspace");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/main.rs", "fn main() {}\n"),
                ("src/private.rs", "pub fn hidden() {}\n"),
            ],
        )?;

        let unreadable_path = workspace_root.join("src/private.rs");
        set_file_mode(&unreadable_path, 0o000)?;

        let first = reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;
        let second = reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;

        assert_eq!(first.snapshot_id, second.snapshot_id);
        assert_eq!(first.files_scanned, 1);
        assert_eq!(first.files_changed, 1);
        assert_eq!(first.files_deleted, 0);
        assert_eq!(first.diagnostics.entries, second.diagnostics.entries);
        assert_eq!(first.diagnostics.total_count(), 1);
        assert_eq!(
            first
                .diagnostics
                .count_by_kind(ManifestDiagnosticKind::Read),
            1
        );
        assert_eq!(
            first
                .diagnostics
                .count_by_kind(ManifestDiagnosticKind::Walk),
            0
        );
        assert_eq!(
            first.diagnostics.entries[0].path.as_deref(),
            Some(unreadable_path.as_path())
        );
        assert_eq!(
            first.diagnostics.entries[0].kind,
            ManifestDiagnosticKind::Read
        );
        assert!(
            !first.diagnostics.entries[0].message.is_empty(),
            "read diagnostics should include an error message"
        );

        set_file_mode(&unreadable_path, 0o644)?;
        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn changed_only_reuses_previous_digests_for_unchanged_unreadable_files() -> FriggResult<()> {
        let db_path = temp_db_path("incremental-changed-unreadable-db");
        let workspace_root = temp_workspace_root("incremental-changed-unreadable-workspace");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/main.rs", "fn main() {}\n"),
                ("src/private.rs", "pub fn hidden() {}\n"),
            ],
        )?;

        let first = reindex_repository("repo-001", &workspace_root, &db_path, ReindexMode::Full)?;
        let unreadable_path = workspace_root.join("src/private.rs");
        set_file_mode(&unreadable_path, 0o000)?;

        let second = reindex_repository(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
        )?;

        assert_eq!(second.snapshot_id, first.snapshot_id);
        assert_eq!(second.files_scanned, 2);
        assert_eq!(second.files_changed, 0);
        assert_eq!(second.files_deleted, 0);
        assert_eq!(second.diagnostics.total_count(), 0);

        set_file_mode(&unreadable_path, 0o644)?;
        cleanup_workspace(&workspace_root);
        cleanup_db(&db_path);
        Ok(())
    }

    #[test]
    fn symbols_rust_php_extracts_rust_definition_metadata() -> FriggResult<()> {
        let symbols = extract_symbols_from_source(
            SymbolLanguage::Rust,
            Path::new("fixtures/rust_symbols.rs"),
            rust_symbols_fixture(),
        )?;

        assert!(
            find_symbol(&symbols, SymbolKind::Module, "api", 1).is_some(),
            "expected rust module symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Struct, "User", 2).is_some(),
            "expected rust struct symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Enum, "Role", 3).is_some(),
            "expected rust enum symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Trait, "Repo", 4).is_some(),
            "expected rust trait symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Impl, "impl Repo for User", 5).is_some(),
            "expected rust impl symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Const, "LIMIT", 6).is_some(),
            "expected rust const symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Static, "NAME", 7).is_some(),
            "expected rust static symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::TypeAlias, "UserId", 8).is_some(),
            "expected rust type alias symbol"
        );
        let helper = find_symbol(&symbols, SymbolKind::Function, "helper", 9)
            .expect("expected rust function symbol");
        assert!(
            helper.stable_id.starts_with("sym-"),
            "expected stable symbol id prefix"
        );
        assert_eq!(helper.path, PathBuf::from("fixtures/rust_symbols.rs"));
        assert_eq!(helper.line, helper.span.start_line);

        Ok(())
    }

    #[test]
    fn symbols_rust_php_extracts_php_definition_metadata() -> FriggResult<()> {
        let symbols = extract_symbols_from_source(
            SymbolLanguage::Php,
            Path::new("fixtures/php_symbols.php"),
            php_symbols_fixture(),
        )?;

        assert!(
            find_symbol(&symbols, SymbolKind::Module, "App\\Models", 2).is_some(),
            "expected php namespace module symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Function, "top_level", 3).is_some(),
            "expected php top-level function symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Class, "User", 4).is_some(),
            "expected php class symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Property, "$name", 5).is_some(),
            "expected php property symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "save", 6).is_some(),
            "expected php method symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Constant, "LIMIT", 7).is_some(),
            "expected php constant symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Interface, "Repo", 9).is_some(),
            "expected php interface symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::PhpTrait, "Logs", 10).is_some(),
            "expected php trait symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::PhpEnum, "Status", 11).is_some(),
            "expected php enum symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::EnumCase, "Active", 12).is_some(),
            "expected php enum case symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_blade_extracts_view_component_and_template_metadata() -> FriggResult<()> {
        let path = Path::new("resources/views/components/dashboard/panel.blade.php");
        let symbols =
            extract_symbols_from_source(SymbolLanguage::Blade, path, blade_symbols_fixture())?;

        assert!(
            find_symbol(
                &symbols,
                SymbolKind::Module,
                "components.dashboard.panel",
                1
            )
            .is_some(),
            "expected blade view module symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Component, "dashboard.panel", 1).is_some(),
            "expected blade anonymous component symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Section, "hero", 1).is_some(),
            "expected blade section symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Property, "$title", 2).is_some(),
            "expected blade props symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Property, "$tone", 3).is_some(),
            "expected blade aware symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Slot, "icon", 4).is_some(),
            "expected blade named slot symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Component, "alert.banner", 5).is_some(),
            "expected x-component symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Component, "livewire:orders.table", 6).is_some(),
            "expected livewire tag symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Component, "livewire:stats-card", 7).is_some(),
            "expected @livewire directive symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Component, "flux:button", 8).is_some(),
            "expected flux tag symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_typescript_extracts_definition_metadata() -> FriggResult<()> {
        let path = Path::new("fixtures/typescript_symbols.ts");
        let symbols = extract_symbols_from_source(
            SymbolLanguage::TypeScript,
            path,
            typescript_symbols_fixture(),
        )?;

        assert!(
            find_symbol(&symbols, SymbolKind::Module, "Api", 1).is_some(),
            "expected TypeScript namespace/module symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Class, "User", 2).is_some(),
            "expected TypeScript class symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Property, "id", 3).is_some(),
            "expected TypeScript class field symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "save", 4).is_some(),
            "expected TypeScript method symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Interface, "Repository", 6).is_some(),
            "expected TypeScript interface symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "find", 7).is_some(),
            "expected TypeScript interface method symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Property, "status", 8).is_some(),
            "expected TypeScript interface property symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Enum, "Role", 10).is_some(),
            "expected TypeScript enum symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::TypeAlias, "UserId", 11).is_some(),
            "expected TypeScript type alias symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Function, "renderUser", 12).is_some(),
            "expected TypeScript arrow-function binding symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Const, "LIMIT", 13).is_some(),
            "expected TypeScript const binding symbol"
        );

        let tsx_symbols = extract_symbols_from_source(
            SymbolLanguage::TypeScript,
            Path::new("fixtures/component.tsx"),
            typescript_tsx_fixture(),
        )?;
        assert!(
            find_symbol(&tsx_symbols, SymbolKind::Function, "App", 1).is_some(),
            "expected TSX component binding to be discoverable as a function symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_python_extracts_definition_metadata() -> FriggResult<()> {
        let path = Path::new("fixtures/python_symbols.py");
        let symbols =
            extract_symbols_from_source(SymbolLanguage::Python, path, python_symbols_fixture())?;

        assert!(
            find_symbol(&symbols, SymbolKind::TypeAlias, "Alias", 1).is_some(),
            "expected Python type alias symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Class, "Service", 2).is_some(),
            "expected Python class symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "run", 3).is_some(),
            "expected Python method symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Function, "helper", 6).is_some(),
            "expected Python function symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_go_extracts_definition_metadata() -> FriggResult<()> {
        let path = Path::new("fixtures/go_symbols.go");
        let symbols = extract_symbols_from_source(SymbolLanguage::Go, path, go_symbols_fixture())?;

        assert!(
            find_symbol(&symbols, SymbolKind::Module, "main", 1).is_some(),
            "expected Go package symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Struct, "Service", 2).is_some(),
            "expected Go struct symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Interface, "Runner", 3).is_some(),
            "expected Go interface symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::TypeAlias, "ID", 4).is_some(),
            "expected Go type alias symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Const, "Limit", 5).is_some(),
            "expected Go const symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Function, "helper", 6).is_some(),
            "expected Go function symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "Run", 7).is_some(),
            "expected Go method symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_kotlin_extracts_definition_metadata() -> FriggResult<()> {
        let path = Path::new("fixtures/kotlin_symbols.kt");
        let symbols =
            extract_symbols_from_source(SymbolLanguage::Kotlin, path, kotlin_symbols_fixture())?;

        assert!(
            find_symbol(&symbols, SymbolKind::Enum, "Role", 1).is_some(),
            "expected Kotlin enum symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Class, "Service", 2).is_some(),
            "expected Kotlin class symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Property, "name", 3).is_some(),
            "expected Kotlin property symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "run", 4).is_some(),
            "expected Kotlin method symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::TypeAlias, "Alias", 6).is_some(),
            "expected Kotlin type alias symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Function, "helper", 7).is_some(),
            "expected Kotlin top-level function symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_lua_extracts_definition_metadata() -> FriggResult<()> {
        let path = Path::new("fixtures/lua_symbols.lua");
        let symbols =
            extract_symbols_from_source(SymbolLanguage::Lua, path, lua_symbols_fixture())?;

        assert!(
            find_symbol(&symbols, SymbolKind::Function, "run", 1).is_some(),
            "expected Lua dotted function symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "save", 4).is_some(),
            "expected Lua method symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_nim_extracts_definition_metadata() -> FriggResult<()> {
        let path = Path::new("fixtures/nim_symbols.nim");
        let symbols =
            extract_symbols_from_source(SymbolLanguage::Nim, path, nim_symbols_fixture())?;

        assert!(
            find_symbol(&symbols, SymbolKind::Struct, "Service", 1).is_some(),
            "expected Nim object symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Enum, "Mode", 2).is_some(),
            "expected Nim enum-like type symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Function, "helper", 4).is_some(),
            "expected Nim proc symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "run", 6).is_some(),
            "expected Nim method symbol"
        );

        Ok(())
    }

    #[test]
    fn symbols_roc_extracts_definition_metadata() -> FriggResult<()> {
        let path = Path::new("fixtures/roc_symbols.roc");
        let symbols =
            extract_symbols_from_source(SymbolLanguage::Roc, path, roc_symbols_fixture())?;

        assert!(
            find_symbol(&symbols, SymbolKind::TypeAlias, "UserId", 1).is_some(),
            "expected Roc nominal type symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Const, "id", 2).is_some(),
            "expected Roc value symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Function, "greet", 4).is_some(),
            "expected Roc function value symbol"
        );

        Ok(())
    }

    #[test]
    fn php_source_evidence_extracts_canonical_type_target_and_literal_metadata() -> FriggResult<()>
    {
        let path = Path::new("src/OrderListener.php");
        let source = php_source_evidence_fixture();
        let symbols = extract_symbols_from_source(SymbolLanguage::Php, path, source)?;
        let evidence = extract_php_source_evidence_from_source(path, source, &symbols)?;

        let class_symbol = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Class && symbol.name == "OrderListener")
            .expect("expected class symbol for php evidence fixture");
        let method_symbol = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Method && symbol.name == "boot")
            .expect("expected method symbol for php evidence fixture");

        assert_eq!(
            evidence
                .canonical_names_by_stable_id
                .get(&class_symbol.stable_id),
            Some(&"App\\Listeners\\OrderListener".to_owned())
        );
        assert_eq!(
            evidence
                .canonical_names_by_stable_id
                .get(&method_symbol.stable_id),
            Some(&"App\\Listeners\\OrderListener::boot".to_owned())
        );
        assert!(
            evidence.type_evidence.iter().any(|entry| {
                entry.kind == PhpTypeEvidenceKind::PromotedProperty
                    && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
            }),
            "expected promoted-property type evidence for aliased contract handler"
        );
        assert!(
            evidence.type_evidence.iter().any(|entry| {
                entry.kind == PhpTypeEvidenceKind::Parameter
                    && entry.target_canonical_name == "App\\Contracts\\Dispatcher"
            }),
            "expected parameter type evidence for imported dispatcher type"
        );
        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == PhpTargetEvidenceKind::Attribute
                    && entry.target_canonical_name == "App\\Attributes\\AsListener"
            }),
            "expected attribute target evidence for class and method attributes"
        );
        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == PhpTargetEvidenceKind::Instantiation
                    && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
            }),
            "expected instantiation target evidence"
        );
        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == PhpTargetEvidenceKind::ClassString
                    && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
            }),
            "expected class-string target evidence"
        );
        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == PhpTargetEvidenceKind::CallableLiteral
                    && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
                    && entry.target_member_name.as_deref() == Some("handle")
            }),
            "expected callable-literal target evidence"
        );
        assert!(
            evidence.literal_evidence.iter().any(|entry| {
                entry.array_keys == vec!["queue".to_owned()] && entry.named_arguments.is_empty()
            }),
            "expected literal array-key evidence"
        );
        assert!(
            evidence.literal_evidence.iter().any(|entry| {
                entry.array_keys.is_empty() && entry.named_arguments == vec!["handler".to_owned()]
            }),
            "expected named-argument evidence"
        );

        Ok(())
    }

    #[test]
    fn blade_source_evidence_extracts_relations_livewire_wire_and_flux_hints() -> FriggResult<()> {
        let path = Path::new("resources/views/dashboard/show.blade.php");
        let source = blade_source_evidence_fixture();
        let symbols = extract_symbols_from_source(SymbolLanguage::Blade, path, source)?;
        let mut evidence = extract_blade_source_evidence_from_source(source, &symbols);

        let overlay_path = Path::new("resources/views/components/flux/button.blade.php");
        let overlay_symbols = extract_symbols_from_source(SymbolLanguage::Blade, overlay_path, "")?;
        let mut combined_symbols = symbols.clone();
        combined_symbols.extend(overlay_symbols);
        let mut symbol_indices_by_name = BTreeMap::new();
        for (index, symbol) in combined_symbols.iter().enumerate() {
            symbol_indices_by_name
                .entry(symbol.name.clone())
                .or_insert_with(Vec::new)
                .push(index);
        }
        mark_local_flux_overlays(&mut evidence, &combined_symbols, &symbol_indices_by_name);

        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == BladeRelationKind::Extends
                    && relation.target_name == "layouts.app"
                    && relation.target_symbol_kind == SymbolKind::Module
            }),
            "expected @extends relation evidence"
        );
        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == BladeRelationKind::Include
                    && relation.target_name == "partials.flash"
                    && relation.target_symbol_kind == SymbolKind::Module
            }),
            "expected @include relation evidence"
        );
        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == BladeRelationKind::Component
                    && relation.target_name == "alert.banner"
                    && relation.target_symbol_kind == SymbolKind::Component
            }),
            "expected x-component relation evidence"
        );
        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == BladeRelationKind::DynamicComponent
                    && relation.target_name == "panels.metric"
                    && relation.target_symbol_kind == SymbolKind::Component
            }),
            "expected normalized dynamic-component relation evidence"
        );
        assert_eq!(
            evidence.livewire_components,
            vec!["orders.table".to_owned(), "stats-card".to_owned()]
        );
        assert_eq!(
            evidence.wire_directives,
            vec!["wire:click".to_owned(), "wire:model.live".to_owned()]
        );
        assert_eq!(evidence.flux_components, vec!["flux:button".to_owned()]);
        assert!(
            evidence
                .flux_hints
                .get("flux:button")
                .is_some_and(|hint| hint.local_overlay),
            "expected local overlay discovery to enrich flux component hints"
        );

        Ok(())
    }

    #[test]
    fn php_declaration_relations_extract_extends_and_implements_deterministically()
    -> FriggResult<()> {
        let source = "<?php\n\
             interface ProviderInterface {}\n\
             interface ExtendedProviderInterface extends ProviderInterface, BaseProviderInterface {}\n\
             class ListCompletionProvider implements ProviderInterface {}\n\
             class EnumCompletionProvider extends ListCompletionProvider implements ProviderInterface {}\n\
             enum UserIdCompletionProvider implements ProviderInterface {}\n";
        let relations = extract_php_declaration_relations_from_source(
            Path::new("fixtures/php_relations.php"),
            source,
        )?;

        assert_eq!(
            relations,
            vec![
                PhpDeclarationRelation {
                    source_kind: SymbolKind::Class,
                    source_name: "EnumCompletionProvider".to_owned(),
                    source_line: 5,
                    target_name: "ListCompletionProvider".to_owned(),
                    relation: RelationKind::Extends,
                },
                PhpDeclarationRelation {
                    source_kind: SymbolKind::Class,
                    source_name: "EnumCompletionProvider".to_owned(),
                    source_line: 5,
                    target_name: "ProviderInterface".to_owned(),
                    relation: RelationKind::Implements,
                },
                PhpDeclarationRelation {
                    source_kind: SymbolKind::Class,
                    source_name: "ListCompletionProvider".to_owned(),
                    source_line: 4,
                    target_name: "ProviderInterface".to_owned(),
                    relation: RelationKind::Implements,
                },
                PhpDeclarationRelation {
                    source_kind: SymbolKind::Interface,
                    source_name: "ExtendedProviderInterface".to_owned(),
                    source_line: 3,
                    target_name: "BaseProviderInterface".to_owned(),
                    relation: RelationKind::Extends,
                },
                PhpDeclarationRelation {
                    source_kind: SymbolKind::Interface,
                    source_name: "ExtendedProviderInterface".to_owned(),
                    source_line: 3,
                    target_name: "ProviderInterface".to_owned(),
                    relation: RelationKind::Extends,
                },
                PhpDeclarationRelation {
                    source_kind: SymbolKind::PhpEnum,
                    source_name: "UserIdCompletionProvider".to_owned(),
                    source_line: 6,
                    target_name: "ProviderInterface".to_owned(),
                    relation: RelationKind::Implements,
                },
            ]
        );

        Ok(())
    }

    #[test]
    fn php_source_evidence_extracts_canonical_names_types_targets_and_literals() -> FriggResult<()>
    {
        let path = Path::new("src/OrderListener.php");
        let source = php_source_evidence_fixture();
        let symbols = extract_symbols_from_source(SymbolLanguage::Php, path, source)?;
        let evidence = extract_php_source_evidence_from_source(path, source, &symbols)?;

        let canonical_names = evidence
            .canonical_names_by_stable_id
            .values()
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            canonical_names
                .iter()
                .any(|name| name == "App\\Listeners\\OrderListener"),
            "expected canonical class name in php evidence"
        );
        assert!(
            canonical_names
                .iter()
                .any(|name| name == "App\\Listeners\\OrderListener::boot"),
            "expected canonical method name in php evidence"
        );
        assert!(
            canonical_names
                .iter()
                .any(|name| name == "App\\Listeners\\OrderListener::$dispatcher"),
            "expected canonical property name in php evidence"
        );

        let type_targets = evidence
            .type_evidence
            .iter()
            .map(|entry| entry.target_canonical_name.as_str())
            .collect::<Vec<_>>();
        assert!(
            type_targets.contains(&"App\\Contracts\\Dispatcher"),
            "expected dispatcher type evidence"
        );
        assert!(
            type_targets.contains(&"App\\Handlers\\OrderHandler"),
            "expected handler type evidence"
        );
        assert!(
            type_targets.contains(&"App\\Exceptions\\OrderException"),
            "expected catch type evidence"
        );

        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == super::PhpTargetEvidenceKind::Attribute
                    && entry.target_canonical_name == "App\\Attributes\\AsListener"
            }),
            "expected attribute target evidence"
        );
        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == super::PhpTargetEvidenceKind::ClassString
                    && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
            }),
            "expected class-string target evidence"
        );
        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == super::PhpTargetEvidenceKind::Instantiation
                    && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
            }),
            "expected instantiation target evidence"
        );
        assert!(
            evidence.target_evidence.iter().any(|entry| {
                entry.kind == super::PhpTargetEvidenceKind::CallableLiteral
                    && entry.target_canonical_name == "App\\Handlers\\OrderHandler"
                    && entry.target_member_name.as_deref() == Some("handle")
            }),
            "expected callable-literal target evidence"
        );

        assert!(
            evidence
                .literal_evidence
                .iter()
                .any(|entry| { entry.array_keys == vec!["queue".to_owned()] }),
            "expected literal array-key evidence"
        );
        assert!(
            evidence
                .literal_evidence
                .iter()
                .any(|entry| { entry.named_arguments == vec!["handler".to_owned()] }),
            "expected named-argument evidence"
        );

        Ok(())
    }

    #[test]
    fn blade_source_evidence_extracts_relations_and_ui_metadata() -> FriggResult<()> {
        let path = Path::new("resources/views/dashboard/index.blade.php");
        let source = blade_source_evidence_fixture();
        let symbols = extract_symbols_from_source(SymbolLanguage::Blade, path, source)?;
        let evidence = extract_blade_source_evidence_from_source(source, &symbols);

        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == super::BladeRelationKind::Extends
                    && relation.target_name == "layouts.app"
                    && relation.target_symbol_kind == SymbolKind::Module
            }),
            "expected @extends relation evidence"
        );
        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == super::BladeRelationKind::Include
                    && relation.target_name == "partials.flash"
            }),
            "expected @include relation evidence"
        );
        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == super::BladeRelationKind::Yield
                    && relation.target_name == "hero"
                    && relation.target_symbol_kind == SymbolKind::Section
            }),
            "expected @yield relation evidence"
        );
        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == super::BladeRelationKind::Component
                    && relation.target_name == "alert.banner"
                    && relation.target_symbol_kind == SymbolKind::Component
            }),
            "expected x-component relation evidence"
        );
        assert!(
            evidence.relations.iter().any(|relation| {
                relation.kind == super::BladeRelationKind::DynamicComponent
                    && relation.target_name == "panels.metric"
                    && relation.target_symbol_kind == SymbolKind::Component
            }),
            "expected x-dynamic-component relation evidence"
        );
        assert_eq!(
            evidence.livewire_components,
            vec!["orders.table".to_owned(), "stats-card".to_owned()]
        );
        assert_eq!(
            evidence.wire_directives,
            vec!["wire:click".to_owned(), "wire:model.live".to_owned()]
        );
        assert_eq!(evidence.flux_components, vec!["flux:button".to_owned()]);
        assert!(
            evidence.flux_hints.contains_key("flux:button"),
            "expected offline flux registry hints for flux:button"
        );

        Ok(())
    }

    #[test]
    fn symbols_supported_language_extraction_is_deterministic() -> FriggResult<()> {
        let first = extract_symbols_from_source(
            SymbolLanguage::Rust,
            Path::new("fixtures/rust_symbols.rs"),
            rust_symbols_fixture(),
        )?;
        let second = extract_symbols_from_source(
            SymbolLanguage::Rust,
            Path::new("fixtures/rust_symbols.rs"),
            rust_symbols_fixture(),
        )?;
        let third = extract_symbols_from_source(
            SymbolLanguage::Php,
            Path::new("fixtures/php_symbols.php"),
            php_symbols_fixture(),
        )?;
        let fourth = extract_symbols_from_source(
            SymbolLanguage::Php,
            Path::new("fixtures/php_symbols.php"),
            php_symbols_fixture(),
        )?;
        let fifth = extract_symbols_from_source(
            SymbolLanguage::TypeScript,
            Path::new("fixtures/typescript_symbols.ts"),
            typescript_symbols_fixture(),
        )?;
        let sixth = extract_symbols_from_source(
            SymbolLanguage::TypeScript,
            Path::new("fixtures/typescript_symbols.ts"),
            typescript_symbols_fixture(),
        )?;
        let seventh = extract_symbols_from_source(
            SymbolLanguage::Python,
            Path::new("fixtures/python_symbols.py"),
            python_symbols_fixture(),
        )?;
        let eighth = extract_symbols_from_source(
            SymbolLanguage::Python,
            Path::new("fixtures/python_symbols.py"),
            python_symbols_fixture(),
        )?;
        let ninth = extract_symbols_from_source(
            SymbolLanguage::Go,
            Path::new("fixtures/go_symbols.go"),
            go_symbols_fixture(),
        )?;
        let tenth = extract_symbols_from_source(
            SymbolLanguage::Go,
            Path::new("fixtures/go_symbols.go"),
            go_symbols_fixture(),
        )?;
        let eleventh = extract_symbols_from_source(
            SymbolLanguage::Kotlin,
            Path::new("fixtures/kotlin_symbols.kt"),
            kotlin_symbols_fixture(),
        )?;
        let twelfth = extract_symbols_from_source(
            SymbolLanguage::Kotlin,
            Path::new("fixtures/kotlin_symbols.kt"),
            kotlin_symbols_fixture(),
        )?;
        let thirteenth = extract_symbols_from_source(
            SymbolLanguage::Lua,
            Path::new("fixtures/lua_symbols.lua"),
            lua_symbols_fixture(),
        )?;
        let fourteenth = extract_symbols_from_source(
            SymbolLanguage::Lua,
            Path::new("fixtures/lua_symbols.lua"),
            lua_symbols_fixture(),
        )?;
        let fifteenth = extract_symbols_from_source(
            SymbolLanguage::Nim,
            Path::new("fixtures/nim_symbols.nim"),
            nim_symbols_fixture(),
        )?;
        let sixteenth = extract_symbols_from_source(
            SymbolLanguage::Nim,
            Path::new("fixtures/nim_symbols.nim"),
            nim_symbols_fixture(),
        )?;
        let seventeenth = extract_symbols_from_source(
            SymbolLanguage::Roc,
            Path::new("fixtures/roc_symbols.roc"),
            roc_symbols_fixture(),
        )?;
        let eighteenth = extract_symbols_from_source(
            SymbolLanguage::Roc,
            Path::new("fixtures/roc_symbols.roc"),
            roc_symbols_fixture(),
        )?;

        assert_eq!(first, second);
        assert_eq!(third, fourth);
        assert_eq!(fifth, sixth);
        assert_eq!(seventh, eighth);
        assert_eq!(ninth, tenth);
        assert_eq!(eleventh, twelfth);
        assert_eq!(thirteenth, fourteenth);
        assert_eq!(fifteenth, sixteenth);
        assert_eq!(seventeenth, eighteenth);
        Ok(())
    }

    #[test]
    fn structural_search_rust_returns_deterministic_captures() -> FriggResult<()> {
        let source = "pub fn first() {}\n\
             pub fn second() {}\n";
        let path = Path::new("fixtures/structural.rs");
        let query = "(function_item) @function";

        let first = search_structural_in_source(SymbolLanguage::Rust, path, source, query)?;
        let second = search_structural_in_source(SymbolLanguage::Rust, path, source, query)?;

        assert_eq!(first, second, "structural captures should be deterministic");
        assert_eq!(first.len(), 2);
        assert_eq!(
            first
                .iter()
                .map(|matched| {
                    (
                        matched.path.clone(),
                        matched.span.start_line,
                        matched.span.start_column,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                (PathBuf::from("fixtures/structural.rs"), 1, 1),
                (PathBuf::from("fixtures/structural.rs"), 2, 1),
            ]
        );
        Ok(())
    }

    #[test]
    fn structural_search_typescript_tsx_uses_extension_aware_grammar() -> FriggResult<()> {
        let matches = search_structural_in_source(
            SymbolLanguage::TypeScript,
            Path::new("fixtures/component.tsx"),
            typescript_tsx_fixture(),
            "(jsx_self_closing_element) @jsx",
        )?;

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, PathBuf::from("fixtures/component.tsx"));
        assert_eq!(matches[0].span.start_line, 1);
        assert_eq!(matches[0].excerpt, "<Button />");

        Ok(())
    }

    #[test]
    fn structural_search_python_returns_deterministic_captures() -> FriggResult<()> {
        let first = search_structural_in_source(
            SymbolLanguage::Python,
            Path::new("fixtures/python_symbols.py"),
            python_symbols_fixture(),
            "(function_definition) @fn",
        )?;
        let second = search_structural_in_source(
            SymbolLanguage::Python,
            Path::new("fixtures/python_symbols.py"),
            python_symbols_fixture(),
            "(function_definition) @fn",
        )?;

        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].span.start_line, 3);
        assert_eq!(first[1].span.start_line, 6);

        Ok(())
    }

    #[test]
    fn structural_search_go_returns_deterministic_captures() -> FriggResult<()> {
        let first = search_structural_in_source(
            SymbolLanguage::Go,
            Path::new("fixtures/go_symbols.go"),
            go_symbols_fixture(),
            "(function_declaration) @fn",
        )?;
        let second = search_structural_in_source(
            SymbolLanguage::Go,
            Path::new("fixtures/go_symbols.go"),
            go_symbols_fixture(),
            "(function_declaration) @fn",
        )?;

        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].span.start_line, 6);

        Ok(())
    }

    #[test]
    fn structural_search_kotlin_returns_deterministic_captures() -> FriggResult<()> {
        let first = search_structural_in_source(
            SymbolLanguage::Kotlin,
            Path::new("fixtures/kotlin_symbols.kt"),
            kotlin_symbols_fixture(),
            "(function_declaration) @fn",
        )?;
        let second = search_structural_in_source(
            SymbolLanguage::Kotlin,
            Path::new("fixtures/kotlin_symbols.kt"),
            kotlin_symbols_fixture(),
            "(function_declaration) @fn",
        )?;

        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].span.start_line, 4);
        assert_eq!(first[1].span.start_line, 7);

        Ok(())
    }

    #[test]
    fn structural_search_lua_returns_deterministic_captures() -> FriggResult<()> {
        let first = search_structural_in_source(
            SymbolLanguage::Lua,
            Path::new("fixtures/lua_symbols.lua"),
            lua_symbols_fixture(),
            "(function_declaration) @fn",
        )?;
        let second = search_structural_in_source(
            SymbolLanguage::Lua,
            Path::new("fixtures/lua_symbols.lua"),
            lua_symbols_fixture(),
            "(function_declaration) @fn",
        )?;

        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].span.start_line, 1);
        assert_eq!(first[1].span.start_line, 4);

        Ok(())
    }

    #[test]
    fn structural_search_nim_returns_deterministic_captures() -> FriggResult<()> {
        let first = search_structural_in_source(
            SymbolLanguage::Nim,
            Path::new("fixtures/nim_symbols.nim"),
            nim_symbols_fixture(),
            "(proc_declaration) @proc",
        )?;
        let second = search_structural_in_source(
            SymbolLanguage::Nim,
            Path::new("fixtures/nim_symbols.nim"),
            nim_symbols_fixture(),
            "(proc_declaration) @proc",
        )?;

        assert_eq!(first, second);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].span.start_line, 4);

        Ok(())
    }

    #[test]
    fn structural_search_roc_returns_deterministic_captures() -> FriggResult<()> {
        let first = search_structural_in_source(
            SymbolLanguage::Roc,
            Path::new("fixtures/roc_symbols.roc"),
            roc_symbols_fixture(),
            "(value_declaration) @value",
        )?;
        let second = search_structural_in_source(
            SymbolLanguage::Roc,
            Path::new("fixtures/roc_symbols.roc"),
            roc_symbols_fixture(),
            "(value_declaration) @value",
        )?;

        assert_eq!(first, second);
        assert_eq!(first.len(), 2);
        assert_eq!(first[0].span.start_line, 2);
        assert_eq!(first[1].span.start_line, 4);

        Ok(())
    }

    #[test]
    fn structural_search_rejects_invalid_query_with_typed_error() {
        let error = search_structural_in_source(
            SymbolLanguage::Rust,
            Path::new("fixtures/structural.rs"),
            "pub fn first() {}\n",
            "(function_item @broken",
        )
        .expect_err("invalid query must return error");

        match error {
            FriggError::InvalidInput(message) => {
                assert!(message.contains("invalid structural query"));
            }
            other => panic!("expected invalid input, got {other:?}"),
        }
    }

    #[test]
    fn symbols_rust_php_path_batch_reports_diagnostics_and_continues() -> FriggResult<()> {
        let workspace_root = temp_workspace_root("symbols-rust-php-batch");
        prepare_workspace(
            &workspace_root,
            &[
                ("src/lib.rs", rust_symbols_fixture()),
                ("src/known.php", php_symbols_fixture()),
            ],
        )?;
        let missing_path = workspace_root.join("src/missing.php");
        let paths = vec![
            missing_path.clone(),
            workspace_root.join("src/lib.rs"),
            workspace_root.join("src/known.php"),
        ];

        let output = extract_symbols_for_paths(&paths);

        assert!(
            output.symbols.iter().any(|symbol| {
                symbol.path == workspace_root.join("src/lib.rs")
                    && symbol.kind == SymbolKind::Function
                    && symbol.name == "helper"
            }),
            "expected rust symbols from existing file"
        );
        assert!(
            output.symbols.iter().any(|symbol| {
                symbol.path == workspace_root.join("src/known.php")
                    && symbol.kind == SymbolKind::Function
                    && symbol.name == "top_level"
            }),
            "expected php symbols from existing file"
        );
        assert_eq!(output.diagnostics.len(), 1);
        assert_eq!(output.diagnostics[0].path, missing_path);
        assert_eq!(output.diagnostics[0].language, Some(SymbolLanguage::Php));

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[test]
    fn heuristic_references_combines_graph_hints_and_lexical_fallback_deterministically()
    -> FriggResult<()> {
        let source = "pub struct User;\n\
             pub fn create_user() -> User { User }\n\
             pub fn use_user() { let _ = User; }\n\
             pub fn marker() { let _ = User; }\n\
             pub fn unrelated() { let _ = \"SuperUser\"; }\n";
        let path = PathBuf::from("fixtures/heuristic.rs");
        let symbols = extract_symbols_from_source(SymbolLanguage::Rust, &path, source)?;

        let target =
            find_symbol(&symbols, SymbolKind::Struct, "User", 1).expect("expected target symbol");
        let create_user = find_symbol(&symbols, SymbolKind::Function, "create_user", 2)
            .expect("expected create_user symbol");
        let use_user = find_symbol(&symbols, SymbolKind::Function, "use_user", 3)
            .expect("expected use_user symbol");

        let mut graph = SymbolGraph::default();
        register_symbol_definitions(&mut graph, "repo-001", &symbols);
        assert!(
            graph
                .add_relation(
                    &create_user.stable_id,
                    &target.stable_id,
                    RelationKind::RefersTo
                )
                .expect("refers_to relation should be added")
        );
        assert!(
            graph
                .add_relation(&use_user.stable_id, &target.stable_id, RelationKind::Calls)
                .expect("calls relation should be added")
        );

        let mut sources = BTreeMap::new();
        sources.insert(path.clone(), source.to_owned());

        let first =
            resolve_heuristic_references("repo-001", &target.stable_id, &symbols, &graph, &sources);
        let second =
            resolve_heuristic_references("repo-001", &target.stable_id, &symbols, &graph, &sources);

        assert_eq!(
            first, second,
            "heuristic references should be deterministic"
        );
        assert_eq!(
            first
                .iter()
                .map(|reference| (reference.line, reference.confidence))
                .collect::<Vec<_>>(),
            vec![
                (2, HeuristicReferenceConfidence::High),
                (2, HeuristicReferenceConfidence::High),
                (2, HeuristicReferenceConfidence::High),
                (3, HeuristicReferenceConfidence::High),
                (3, HeuristicReferenceConfidence::High),
                (4, HeuristicReferenceConfidence::Low),
            ]
        );
        let line_two_columns = first
            .iter()
            .filter(|reference| reference.line == 2)
            .map(|reference| reference.column)
            .collect::<Vec<_>>();
        assert_eq!(line_two_columns.len(), 3);
        assert!(
            line_two_columns
                .windows(2)
                .all(|window| window[0] <= window[1]),
            "line-2 references should be ordered by column deterministically"
        );
        assert!(
            matches!(
                first[0].evidence,
                HeuristicReferenceEvidence::GraphRelation { .. }
            ),
            "highest-confidence hint should come from graph relation evidence"
        );
        assert!(
            !first.iter().any(|reference| reference.line == 5),
            "substring-only lexical tokens should not be returned"
        );

        Ok(())
    }

    #[test]
    fn heuristic_references_false_positive_bound_for_substring_tokens() -> FriggResult<()> {
        let source = "<?php\n\
             class User {}\n\
             function true_ref(): void { $x = new User(); }\n\
             function noise(): void {\n\
                 $a = 'SuperUser';\n\
                 $b = 'UserService';\n\
             }\n";
        let path = PathBuf::from("fixtures/heuristic.php");
        let symbols = extract_symbols_from_source(SymbolLanguage::Php, &path, source)?;

        let target =
            find_symbol(&symbols, SymbolKind::Class, "User", 2).expect("expected class symbol");
        let true_ref = find_symbol(&symbols, SymbolKind::Function, "true_ref", 3)
            .expect("expected true_ref symbol");

        let mut graph = SymbolGraph::default();
        register_symbol_definitions(&mut graph, "repo-001", &symbols);
        assert!(
            graph
                .add_relation(
                    &true_ref.stable_id,
                    &target.stable_id,
                    RelationKind::RefersTo
                )
                .expect("refers_to relation should be added")
        );

        let mut sources = BTreeMap::new();
        sources.insert(path, source.to_owned());
        let references =
            resolve_heuristic_references("repo-001", &target.stable_id, &symbols, &graph, &sources);

        assert_eq!(
            references
                .iter()
                .map(|reference| (reference.line, reference.confidence))
                .collect::<Vec<_>>(),
            vec![
                (3, HeuristicReferenceConfidence::High),
                (3, HeuristicReferenceConfidence::High),
            ],
            "same-line heuristic references should preserve both graph and lexical hits"
        );
        let same_line_columns = references
            .iter()
            .map(|reference| reference.column)
            .collect::<Vec<_>>();
        assert_eq!(same_line_columns.len(), 2);
        assert!(
            same_line_columns
                .windows(2)
                .all(|window| window[0] <= window[1]),
            "same-line references should be ordered by column"
        );
        let low_confidence = references
            .iter()
            .filter(|reference| reference.confidence == HeuristicReferenceConfidence::Low)
            .count();
        assert_eq!(
            low_confidence, 0,
            "false-positive lower bound violated: expected no low-confidence noise hits"
        );

        Ok(())
    }

    #[test]
    fn heuristic_references_preserve_multiple_same_line_lexical_hits() -> FriggResult<()> {
        let source = "pub struct User;\n\
             pub fn use_user() { let _a = User; let _b = User; }\n";
        let path = PathBuf::from("fixtures/heuristic-same-line.rs");
        let symbols = extract_symbols_from_source(SymbolLanguage::Rust, &path, source)?;

        let target =
            find_symbol(&symbols, SymbolKind::Struct, "User", 1).expect("expected target symbol");
        let sources = BTreeMap::from([(path, source.to_owned())]);
        let references = resolve_heuristic_references(
            "repo-001",
            &target.stable_id,
            &symbols,
            &SymbolGraph::default(),
            &sources,
        );

        assert_eq!(
            references.len(),
            2,
            "same-line lexical references should retain both token hits"
        );
        assert_eq!(
            references
                .iter()
                .map(|reference| (reference.line, reference.column, reference.confidence))
                .collect::<Vec<_>>(),
            vec![
                (2, 30, HeuristicReferenceConfidence::Low),
                (2, 45, HeuristicReferenceConfidence::Low),
            ]
        );
        assert!(
            references.iter().all(|reference| matches!(
                reference.evidence,
                HeuristicReferenceEvidence::LexicalToken
            )),
            "same-line lexical hits should retain lexical evidence"
        );

        Ok(())
    }

    fn fixture_repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/repos/manifest-determinism")
    }

    fn manifest_relative_paths(entries: &[FileDigest], root: &Path) -> FriggResult<Vec<PathBuf>> {
        entries
            .iter()
            .map(|entry| {
                entry
                    .path
                    .strip_prefix(root)
                    .map(|path| path.to_path_buf())
                    .map_err(|err| {
                        FriggError::Internal(format!(
                            "failed to relativize fixture path {} against {}: {err}",
                            entry.path.display(),
                            root.display()
                        ))
                    })
            })
            .collect()
    }

    fn semantic_runtime_enabled_openai() -> SemanticRuntimeConfig {
        SemanticRuntimeConfig {
            enabled: true,
            provider: Some(SemanticRuntimeProvider::OpenAi),
            model: Some("text-embedding-3-small".to_owned()),
            strict_mode: false,
        }
    }

    fn digest(path: &str, size_bytes: u64, mtime_ns: Option<u64>, hash: &str) -> FileDigest {
        FileDigest {
            path: PathBuf::from(path),
            size_bytes,
            mtime_ns,
            hash_blake3_hex: hash.to_owned(),
        }
    }

    fn mutate_manifest_for_incremental_roundtrip(
        manifest: &[FileDigest],
        fixture_root: &Path,
    ) -> FriggResult<Vec<FileDigest>> {
        let mut next = manifest.to_vec();
        let modified_path = fixture_root.join("README.md");
        let deleted_path = fixture_root.join("src/nested/data.txt");

        let modified_entry = next
            .iter_mut()
            .find(|entry| entry.path == modified_path)
            .ok_or_else(|| {
                FriggError::Internal(format!(
                    "fixture manifest missing expected file for modification: {}",
                    modified_path.display()
                ))
            })?;
        modified_entry.size_bytes += 1;
        modified_entry.mtime_ns = Some(modified_entry.mtime_ns.unwrap_or(0) + 1);
        modified_entry.hash_blake3_hex = "roundtrip-modified-hash".to_string();

        let previous_len = next.len();
        next.retain(|entry| entry.path != deleted_path);
        if next.len() == previous_len {
            return Err(FriggError::Internal(format!(
                "fixture manifest missing expected file for deletion: {}",
                deleted_path.display()
            )));
        }

        next.extend(iter::once(FileDigest {
            path: fixture_root.join("src/incremental-new.rs"),
            size_bytes: 17,
            mtime_ns: Some(17_000),
            hash_blake3_hex: "roundtrip-added-hash".to_string(),
        }));
        next.sort_by(file_digest_order);

        Ok(next)
    }

    fn temp_db_path(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!(
            "frigg-indexer-{test_name}-{nonce}-{}.sqlite3",
            std::process::id()
        ))
    }

    fn cleanup_db(path: &Path) {
        let _ = fs::remove_file(path);
    }

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!(
            "frigg-indexer-{test_name}-{nonce}-{}",
            std::process::id()
        ))
    }

    fn prepare_workspace(root: &Path, files: &[(&str, &str)]) -> FriggResult<()> {
        fs::create_dir_all(root).map_err(FriggError::Io)?;
        for (relative_path, contents) in files {
            let file_path = root.join(relative_path);
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent).map_err(FriggError::Io)?;
            }
            fs::write(file_path, contents).map_err(FriggError::Io)?;
        }

        Ok(())
    }

    #[cfg(unix)]
    fn set_file_mode(path: &Path, mode: u32) -> FriggResult<()> {
        let mut permissions = fs::metadata(path).map_err(FriggError::Io)?.permissions();
        permissions.set_mode(mode);
        fs::set_permissions(path, permissions).map_err(FriggError::Io)
    }

    fn cleanup_workspace(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }

    fn find_symbol<'a>(
        symbols: &'a [super::SymbolDefinition],
        kind: SymbolKind,
        name: &str,
        line: usize,
    ) -> Option<&'a super::SymbolDefinition> {
        symbols
            .iter()
            .find(|symbol| symbol.kind == kind && symbol.name == name && symbol.line == line)
    }

    fn rust_symbols_fixture() -> &'static str {
        "pub mod api {}\n\
         pub struct User;\n\
         pub enum Role { Admin }\n\
         pub trait Repo { fn save(&self); }\n\
         impl Repo for User { fn save(&self) {} }\n\
         pub const LIMIT: usize = 32;\n\
         pub static NAME: &str = \"frigg\";\n\
         pub type UserId = u64;\n\
         pub fn helper() {}\n"
    }

    fn php_symbols_fixture() -> &'static str {
        "<?php\n\
         namespace App\\Models;\n\
         function top_level(): void {}\n\
         class User {\n\
             public string $name;\n\
             public function save(): void {}\n\
             const LIMIT = 10;\n\
         }\n\
         interface Repo { public function find(): ?User; }\n\
         trait Logs { public function logMessage(): void {} }\n\
         enum Status: string {\n\
             case Active = 'active';\n\
         }\n"
    }

    fn blade_symbols_fixture() -> &'static str {
        "@section('hero')\n\
         @props(['title' => 'Dashboard'])\n\
         @aware(['tone'])\n\
         <x-slot:icon />\n\
         <x-alert.banner />\n\
         <livewire:orders.table />\n\
         @livewire('stats-card')\n\
         <flux:button variant=\"primary\">Save</flux:button>\n"
    }

    fn typescript_symbols_fixture() -> &'static str {
        "namespace Api {}\n\
         export class User {\n\
             readonly id: string;\n\
             save(): void {}\n\
         }\n\
         export interface Repository {\n\
             find(id: string): User;\n\
             status: string;\n\
         }\n\
         export enum Role { Admin }\n\
         export type UserId = string;\n\
         export const renderUser = (user: User) => user.id;\n\
         const LIMIT = 10;\n"
    }

    fn typescript_tsx_fixture() -> &'static str {
        "export const App = () => <Button />;\n"
    }

    fn python_symbols_fixture() -> &'static str {
        concat!(
            "type Alias = str\n",
            "class Service:\n",
            "    def run(self) -> None:\n",
            "        pass\n",
            "\n",
            "def helper() -> Alias:\n",
            "    return \"ok\"\n",
        )
    }

    fn go_symbols_fixture() -> &'static str {
        concat!(
            "package main\n",
            "type Service struct{}\n",
            "type Runner interface{ Run() }\n",
            "type ID = string\n",
            "const Limit = 10\n",
            "func helper() string { return \"ok\" }\n",
            "func (s *Service) Run() string { return \"ok\" }\n",
        )
    }

    fn kotlin_symbols_fixture() -> &'static str {
        concat!(
            "enum class Role { Admin }\n",
            "class Service {\n",
            "    val name: String = \"ok\"\n",
            "    fun run(): String = name\n",
            "}\n",
            "typealias Alias = String\n",
            "fun helper(): Alias = \"ok\"\n",
        )
    }

    fn lua_symbols_fixture() -> &'static str {
        concat!(
            "function Service.run()\n",
            "    return \"ok\"\n",
            "end\n",
            "function Service:save()\n",
            "    return true\n",
            "end\n",
        )
    }

    fn nim_symbols_fixture() -> &'static str {
        concat!(
            "type Service = object\n",
            "type Mode = enum\n",
            "  Ready\n",
            "proc helper(): string =\n",
            "  \"ok\"\n",
            "method run(self: Service): string =\n",
            "  \"ok\"\n",
        )
    }

    fn roc_symbols_fixture() -> &'static str {
        concat!(
            "UserId := U64\n",
            "id : U64\n",
            "id = 1\n",
            "greet = \\name -> name\n",
        )
    }

    fn php_source_evidence_fixture() -> &'static str {
        "<?php\n\
         namespace App\\Listeners;\n\
         use App\\Attributes\\AsListener;\n\
         use App\\Contracts\\Dispatcher;\n\
         use App\\Exceptions\\OrderException;\n\
         use App\\Handlers\\OrderHandler as Handler;\n\
         #[AsListener]\n\
         class OrderListener {\n\
             public Dispatcher $dispatcher;\n\
             public function __construct(public Handler $handler) {}\n\
             public function boot(Handler $handler, Dispatcher $dispatcher): Handler {\n\
                 $meta = ['queue' => 'high'];\n\
                 $dispatcher->map(handler: Handler::class);\n\
                 $callable = [Handler::class, 'handle'];\n\
                 $fresh = new Handler();\n\
                 try {} catch (OrderException $e) {}\n\
                 return $handler;\n\
             }\n\
         }\n"
    }

    fn blade_source_evidence_fixture() -> &'static str {
        "@extends('layouts.app')\n\
         @includeIf('partials.flash')\n\
         @yield('hero')\n\
         <x-alert.banner />\n\
         <x-dynamic-component :component=\"'panels.metric'\" />\n\
         <livewire:orders.table />\n\
         @livewire('stats-card')\n\
         <flux:button wire:click=\"save\" wire:model.live=\"state\" />\n"
    }
}
