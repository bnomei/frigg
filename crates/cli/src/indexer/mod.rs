use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::domain::{FriggError, FriggResult};
use crate::embeddings::{
    EmbeddingProvider, EmbeddingPurpose, EmbeddingRequest, GoogleEmbeddingProvider,
    OpenAiEmbeddingProvider,
};
use crate::graph::{HeuristicConfidence, RelationKind, SymbolGraph, SymbolNode};
pub use crate::language_support::SymbolLanguage;
use crate::language_support::{
    PhpSymbolLookup, blade_component_name_for_path, blade_view_name_for_path, is_blade_path,
    parser_for_language, php_name_resolution_context_from_root, php_relation_targets_symbol_name,
    resolve_php_declaration_relation_indices, symbol_from_node, tree_sitter_language,
};
use crate::playbooks::scrub_playbook_metadata_header;
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider};
use crate::storage::{ManifestEntry, SemanticChunkEmbeddingRecord, Storage};
use blake3::Hasher;
use ignore::WalkBuilder;
use regex::Regex;
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
pub(crate) use semantic::semantic_chunk_language_for_path;
use semantic::{
    RuntimeSemanticEmbeddingExecutor, SemanticRuntimeEmbeddingExecutor,
    build_semantic_embedding_records, resolve_semantic_runtime_config_from_env,
};
#[cfg(test)]
pub(crate) use semantic::{build_file_semantic_chunks, build_semantic_chunk_candidates};

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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhpDeclarationRelation {
    pub source_kind: SymbolKind,
    pub source_name: String,
    pub source_line: usize,
    pub target_name: String,
    pub relation: RelationKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PhpTypeEvidenceKind {
    Parameter,
    Return,
    Property,
    PromotedProperty,
    Catch,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhpTypeEvidence {
    pub owner_symbol_id: Option<String>,
    pub kind: PhpTypeEvidenceKind,
    pub target_canonical_name: String,
    pub line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PhpTargetEvidenceKind {
    Attribute,
    ClassString,
    Instantiation,
    CallableLiteral,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhpTargetEvidence {
    pub owner_symbol_id: Option<String>,
    pub kind: PhpTargetEvidenceKind,
    pub target_canonical_name: String,
    pub target_member_name: Option<String>,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct PhpLiteralEvidence {
    pub owner_symbol_id: Option<String>,
    pub array_keys: Vec<String>,
    pub named_arguments: Vec<String>,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhpSourceEvidence {
    pub canonical_names_by_stable_id: BTreeMap<String, String>,
    pub type_evidence: Vec<PhpTypeEvidence>,
    pub target_evidence: Vec<PhpTargetEvidence>,
    pub literal_evidence: Vec<PhpLiteralEvidence>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum BladeRelationKind {
    Extends,
    Include,
    Component,
    Yield,
    DynamicComponent,
}

impl BladeRelationKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Extends => "extends",
            Self::Include => "include",
            Self::Component => "component",
            Self::Yield => "yield",
            Self::DynamicComponent => "dynamic_component",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct BladeRelationEvidence {
    pub owner_symbol_id: Option<String>,
    pub kind: BladeRelationKind,
    pub target_name: String,
    pub target_symbol_kind: SymbolKind,
    pub line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FluxComponentHint {
    pub props: Vec<String>,
    pub slots: Vec<String>,
    pub variant_values: Vec<String>,
    pub size_values: Vec<String>,
    pub local_overlay: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BladeSourceEvidence {
    pub relations: Vec<BladeRelationEvidence>,
    pub livewire_components: Vec<String>,
    pub wire_directives: Vec<String>,
    pub flux_components: Vec<String>,
    pub flux_hints: BTreeMap<String, FluxComponentHint>,
}

pub const FLUX_REGISTRY_VERSION: &str = "2026-03-08-mvp";

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
    let mut parser = parser_for_language(language)?;
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

pub fn extract_php_declaration_relations_from_source(
    path: &Path,
    source: &str,
) -> FriggResult<Vec<PhpDeclarationRelation>> {
    let mut parser = parser_for_language(SymbolLanguage::Php)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for php declaration relations: {}",
            path.display()
        ))
    })?;
    let mut relations = Vec::new();
    collect_php_declaration_relations(source, tree.root_node(), &mut relations);
    relations.sort();
    relations.dedup();
    Ok(relations)
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

    let ts_language = tree_sitter_language(language);
    let compiled_query = Query::new(&ts_language, query).map_err(|error| {
        FriggError::InvalidInput(format!(
            "invalid structural query for {}: {error}",
            language.as_str()
        ))
    })?;

    let mut parser = parser_for_language(language)?;
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

pub fn php_declaration_relation_edges_for_file(
    relative_path: &str,
    absolute_path: &Path,
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_name: Option<&BTreeMap<String, Vec<usize>>>,
    symbol_indices_by_lower_name: Option<&BTreeMap<String, Vec<usize>>>,
) -> FriggResult<Vec<(usize, usize, RelationKind)>> {
    if SymbolLanguage::from_path(absolute_path) != Some(SymbolLanguage::Php) {
        return Ok(Vec::new());
    }

    let source = fs::read_to_string(absolute_path).map_err(FriggError::Io)?;
    let relations = extract_php_declaration_relations_from_source(absolute_path, &source)?;
    let owned_name_index;
    let name_index = match symbol_indices_by_name {
        Some(index) => index,
        None => {
            owned_name_index = php_symbol_indices_by_name(symbols);
            &owned_name_index
        }
    };
    let owned_lower_name_index;
    let lower_name_index = match symbol_indices_by_lower_name {
        Some(index) => index,
        None => {
            owned_lower_name_index = php_symbol_indices_by_lower_name(symbols);
            &owned_lower_name_index
        }
    };
    let lookup = PhpSymbolLookup {
        symbols,
        symbols_by_relative_path,
        symbol_indices_by_name: name_index,
        symbol_indices_by_lower_name: lower_name_index,
    };

    let mut edges = Vec::new();
    for relation in relations {
        if let Some((source_symbol_index, target_symbol_index)) =
            resolve_php_declaration_relation_indices(&lookup, relative_path, &relation)
        {
            edges.push((source_symbol_index, target_symbol_index, relation.relation));
        }
    }
    edges.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    edges.dedup();
    Ok(edges)
}

pub fn php_heuristic_implementation_candidates_for_target(
    target_symbol: &SymbolDefinition,
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_name: Option<&BTreeMap<String, Vec<usize>>>,
    symbol_indices_by_lower_name: Option<&BTreeMap<String, Vec<usize>>>,
) -> Vec<(usize, RelationKind)> {
    let target_name = target_symbol.name.trim();
    if target_name.is_empty() {
        return Vec::new();
    }
    if !matches!(
        target_symbol.kind,
        SymbolKind::Interface | SymbolKind::Class
    ) {
        return Vec::new();
    }
    let Some(target_symbol_index) = symbols
        .iter()
        .position(|symbol| symbol.stable_id == target_symbol.stable_id)
    else {
        return Vec::new();
    };

    let owned_name_index;
    let name_index = match symbol_indices_by_name {
        Some(index) => index,
        None => {
            owned_name_index = php_symbol_indices_by_name(symbols);
            &owned_name_index
        }
    };
    let owned_lower_name_index;
    let lower_name_index = match symbol_indices_by_lower_name {
        Some(index) => index,
        None => {
            owned_lower_name_index = php_symbol_indices_by_lower_name(symbols);
            &owned_lower_name_index
        }
    };
    let lookup = PhpSymbolLookup {
        symbols,
        symbols_by_relative_path,
        symbol_indices_by_name: name_index,
        symbol_indices_by_lower_name: lower_name_index,
    };

    let mut matches = Vec::new();
    for (relative_path, absolute_path) in candidate_files {
        if SymbolLanguage::from_path(absolute_path) != Some(SymbolLanguage::Php) {
            continue;
        }
        let Ok(source) = fs::read_to_string(absolute_path) else {
            continue;
        };
        let Ok(relations) = extract_php_declaration_relations_from_source(absolute_path, &source)
        else {
            continue;
        };

        for relation in relations {
            if !php_relation_targets_symbol_name(&relation, target_symbol) {
                continue;
            }
            let Some((source_symbol_index, resolved_target_index)) =
                resolve_php_declaration_relation_indices(&lookup, relative_path, &relation)
            else {
                continue;
            };
            if resolved_target_index != target_symbol_index {
                continue;
            }
            matches.push((source_symbol_index, relation.relation));
        }
    }
    matches.sort_by(|left, right| {
        let left_symbol = &symbols[left.0];
        let right_symbol = &symbols[right.0];
        left_symbol
            .path
            .cmp(&right_symbol.path)
            .then(left_symbol.line.cmp(&right_symbol.line))
            .then(left_symbol.stable_id.cmp(&right_symbol.stable_id))
            .then(left.1.cmp(&right.1))
    });
    matches.dedup();
    matches
}

pub(crate) fn extract_php_source_evidence_from_source(
    path: &Path,
    source: &str,
    file_symbols: &[SymbolDefinition],
) -> FriggResult<PhpSourceEvidence> {
    let mut parser = parser_for_language(SymbolLanguage::Php)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for php evidence extraction: {}",
            path.display()
        ))
    })?;
    let context = php_name_resolution_context_from_root(source, tree.root_node());
    let mut evidence = PhpSourceEvidence::default();
    collect_php_source_evidence(
        source,
        tree.root_node(),
        file_symbols,
        &context,
        context.namespace.as_deref(),
        None,
        None,
        &mut evidence,
    );
    normalize_php_source_evidence(&mut evidence);
    Ok(evidence)
}

pub(crate) fn extract_blade_source_evidence_from_source(
    _path: &Path,
    source: &str,
    file_symbols: &[SymbolDefinition],
) -> BladeSourceEvidence {
    let owner_symbol_id = file_symbols
        .iter()
        .find(|symbol| {
            symbol.language == SymbolLanguage::Blade && symbol.kind == SymbolKind::Module
        })
        .map(|symbol| symbol.stable_id.clone());
    let mut evidence = BladeSourceEvidence::default();

    for capture in blade_view_relation_regex().captures_iter(source) {
        let Some(directive) = capture.name("directive").map(|value| value.as_str()) else {
            continue;
        };
        let Some(target) = capture.get(2).or_else(|| capture.get(3)) else {
            continue;
        };
        let target_name = target.as_str().trim();
        if target_name.is_empty() {
            continue;
        }
        let (kind, target_symbol_kind) = match directive {
            "extends" => (BladeRelationKind::Extends, SymbolKind::Module),
            "component" => (BladeRelationKind::Component, SymbolKind::Module),
            "yield" => (BladeRelationKind::Yield, SymbolKind::Section),
            _ => (BladeRelationKind::Include, SymbolKind::Module),
        };
        evidence.relations.push(BladeRelationEvidence {
            owner_symbol_id: owner_symbol_id.clone(),
            kind,
            target_name: target_name.to_owned(),
            target_symbol_kind,
            line: line_column_for_offset(source, target.start()).0,
        });
    }

    for capture in blade_dynamic_component_regex().captures_iter(source) {
        let Some(target) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        let target_name = normalize_blade_dynamic_component_target(target.as_str());
        if target_name.is_empty() {
            continue;
        }
        evidence.relations.push(BladeRelationEvidence {
            owner_symbol_id: owner_symbol_id.clone(),
            kind: BladeRelationKind::DynamicComponent,
            target_name,
            target_symbol_kind: SymbolKind::Component,
            line: line_column_for_offset(source, target.start()).0,
        });
    }

    for capture in blade_tag_regex().captures_iter(source) {
        let Some(tag_name) = capture.get(1) else {
            continue;
        };
        let normalized = tag_name.as_str().trim();
        if let Some(component_name) = normalized.strip_prefix("livewire:") {
            insert_sorted_unique_owned(
                &mut evidence.livewire_components,
                component_name.to_owned(),
            );
            continue;
        }
        if normalized.starts_with("flux:") {
            insert_sorted_unique_owned(&mut evidence.flux_components, normalized.to_owned());
            if let Some(hint) = flux_registry_hint(normalized) {
                evidence
                    .flux_hints
                    .entry(normalized.to_owned())
                    .or_insert(hint);
            }
            continue;
        }
        if let Some((SymbolKind::Component, component_name)) = classify_blade_tag_name(normalized) {
            evidence.relations.push(BladeRelationEvidence {
                owner_symbol_id: owner_symbol_id.clone(),
                kind: BladeRelationKind::Component,
                target_name: component_name,
                target_symbol_kind: SymbolKind::Component,
                line: line_column_for_offset(source, tag_name.start()).0,
            });
        }
    }

    for capture in blade_livewire_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        insert_sorted_unique_owned(
            &mut evidence.livewire_components,
            name.as_str().trim().to_owned(),
        );
    }

    for capture in blade_wire_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1) else {
            continue;
        };
        insert_sorted_unique_owned(
            &mut evidence.wire_directives,
            name.as_str().trim().to_owned(),
        );
    }

    normalize_blade_source_evidence(&mut evidence);
    evidence
}

pub(crate) fn mark_local_flux_overlays(
    evidence: &mut BladeSourceEvidence,
    symbols: &[SymbolDefinition],
    symbol_indices_by_name: &BTreeMap<String, Vec<usize>>,
) {
    for component_name in &evidence.flux_components {
        let local_component_name = component_name.replacen("flux:", "flux.", 1);
        let local_overlay = symbol_indices_by_name
            .get(&local_component_name)
            .into_iter()
            .flatten()
            .any(|index| {
                let symbol = &symbols[*index];
                symbol.language == SymbolLanguage::Blade && symbol.kind == SymbolKind::Component
            });
        if !local_overlay {
            continue;
        }
        evidence
            .flux_hints
            .entry(component_name.clone())
            .or_default()
            .local_overlay = true;
    }
}

pub(crate) fn resolve_php_target_evidence_edges(
    symbols: &[SymbolDefinition],
    symbol_index_by_stable_id: &BTreeMap<String, usize>,
    symbol_indices_by_canonical_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_canonical_name: &BTreeMap<String, Vec<usize>>,
    evidence: &PhpSourceEvidence,
) -> Vec<(usize, usize, RelationKind)> {
    let mut edges = Vec::new();
    for target in &evidence.target_evidence {
        let Some(source_symbol_id) = target.owner_symbol_id.as_ref() else {
            continue;
        };
        let Some(source_symbol_index) = symbol_index_by_stable_id.get(source_symbol_id).copied()
        else {
            continue;
        };
        let Some(target_symbol_index) = resolve_php_target_symbol_index(
            symbols,
            symbol_indices_by_canonical_name,
            symbol_indices_by_lower_canonical_name,
            target,
        ) else {
            continue;
        };
        if source_symbol_index == target_symbol_index {
            continue;
        }
        edges.push((
            source_symbol_index,
            target_symbol_index,
            RelationKind::RefersTo,
        ));
    }
    edges.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    edges.dedup();
    edges
}

pub(crate) fn resolve_blade_relation_evidence_edges(
    symbols: &[SymbolDefinition],
    symbol_index_by_stable_id: &BTreeMap<String, usize>,
    symbol_indices_by_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_name: &BTreeMap<String, Vec<usize>>,
    evidence: &BladeSourceEvidence,
) -> Vec<(usize, usize, RelationKind)> {
    let mut edges = Vec::new();
    for relation in &evidence.relations {
        let Some(source_symbol_id) = relation.owner_symbol_id.as_ref() else {
            continue;
        };
        let Some(source_symbol_index) = symbol_index_by_stable_id.get(source_symbol_id).copied()
        else {
            continue;
        };
        let Some(target_symbol_index) = resolve_blade_relation_target_symbol_index(
            symbols,
            symbol_indices_by_name,
            symbol_indices_by_lower_name,
            relation,
        ) else {
            continue;
        };
        if source_symbol_index == target_symbol_index {
            continue;
        }
        edges.push((
            source_symbol_index,
            target_symbol_index,
            RelationKind::RefersTo,
        ));
    }
    edges.sort_by(|left, right| {
        left.0
            .cmp(&right.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });
    edges.dedup();
    edges
}

fn collect_php_source_evidence(
    source: &str,
    node: Node<'_>,
    file_symbols: &[SymbolDefinition],
    context: &crate::language_support::PhpNameResolutionContext,
    current_namespace: Option<&str>,
    current_class_canonical_name: Option<&str>,
    current_owner_symbol_id: Option<&str>,
    evidence: &mut PhpSourceEvidence,
) {
    let mut next_namespace = current_namespace.map(ToOwned::to_owned);
    let mut next_class_canonical_name = current_class_canonical_name.map(ToOwned::to_owned);
    let mut next_owner_symbol_id = current_owner_symbol_id.map(ToOwned::to_owned);

    match node.kind() {
        "namespace_definition" => {
            if let Some(namespace_name) = node
                .child_by_field_name("name")
                .and_then(|field| field.utf8_text(source.as_bytes()).ok())
                .map(str::trim)
                .filter(|name| !name.is_empty())
            {
                next_namespace = Some(namespace_name.to_owned());
                if let Some(symbol) =
                    find_symbol_for_node(file_symbols, SymbolKind::Module, namespace_name, node)
                {
                    evidence
                        .canonical_names_by_stable_id
                        .entry(symbol.stable_id.clone())
                        .or_insert_with(|| namespace_name.to_owned());
                    next_owner_symbol_id = Some(symbol.stable_id.clone());
                }
            }
        }
        "class_declaration"
        | "interface_declaration"
        | "trait_declaration"
        | "enum_declaration" => {
            if let Some((kind, name)) = symbol_from_node(SymbolLanguage::Php, source, node) {
                let canonical_name = php_namespace_qualified_name(next_namespace.as_deref(), &name);
                if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                    evidence
                        .canonical_names_by_stable_id
                        .entry(symbol.stable_id.clone())
                        .or_insert_with(|| canonical_name.clone());
                    next_owner_symbol_id = Some(symbol.stable_id.clone());
                }
                next_class_canonical_name = Some(canonical_name);
            }
        }
        "function_definition" => {
            if let Some((kind, name)) = symbol_from_node(SymbolLanguage::Php, source, node) {
                let canonical_name = php_namespace_qualified_name(next_namespace.as_deref(), &name);
                if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                    evidence
                        .canonical_names_by_stable_id
                        .entry(symbol.stable_id.clone())
                        .or_insert_with(|| canonical_name.clone());
                    next_owner_symbol_id = Some(symbol.stable_id.clone());
                    if let Some(parameters) = node.child_by_field_name("parameters") {
                        collect_php_parameter_type_evidence(
                            source,
                            parameters,
                            file_symbols,
                            context,
                            next_class_canonical_name.as_deref(),
                            Some(symbol.stable_id.as_str()),
                            evidence,
                        );
                    }
                    if let Some(return_type) = node.child_by_field_name("return_type") {
                        collect_php_type_evidence(
                            source,
                            return_type,
                            context,
                            next_class_canonical_name.as_deref(),
                            Some(symbol.stable_id.as_str()),
                            PhpTypeEvidenceKind::Return,
                            source_span(node).start_line,
                            &mut evidence.type_evidence,
                        );
                    }
                }
            }
        }
        "method_declaration" => {
            if let Some((kind, name)) = symbol_from_node(SymbolLanguage::Php, source, node) {
                if let Some(class_name) = next_class_canonical_name.as_deref() {
                    let canonical_name = format!("{class_name}::{name}");
                    if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                        evidence
                            .canonical_names_by_stable_id
                            .entry(symbol.stable_id.clone())
                            .or_insert_with(|| canonical_name.clone());
                        next_owner_symbol_id = Some(symbol.stable_id.clone());
                        if let Some(parameters) = node.child_by_field_name("parameters") {
                            collect_php_parameter_type_evidence(
                                source,
                                parameters,
                                file_symbols,
                                context,
                                next_class_canonical_name.as_deref(),
                                Some(symbol.stable_id.as_str()),
                                evidence,
                            );
                        }
                        if let Some(return_type) = node.child_by_field_name("return_type") {
                            collect_php_type_evidence(
                                source,
                                return_type,
                                context,
                                next_class_canonical_name.as_deref(),
                                Some(symbol.stable_id.as_str()),
                                PhpTypeEvidenceKind::Return,
                                source_span(node).start_line,
                                &mut evidence.type_evidence,
                            );
                        }
                    }
                }
            }
        }
        "property_declaration" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                let mut cursor = node.walk();
                for child in node.children(&mut cursor).filter(|child| child.is_named()) {
                    if child.kind() != "property_element" {
                        continue;
                    }
                    let Some((kind, name)) = symbol_from_node(SymbolLanguage::Php, source, child)
                    else {
                        continue;
                    };
                    let owner_symbol_id = find_symbol_for_node(file_symbols, kind, &name, child)
                        .map(|symbol| {
                            if let Some(class_name) = next_class_canonical_name.as_deref() {
                                evidence
                                    .canonical_names_by_stable_id
                                    .entry(symbol.stable_id.clone())
                                    .or_insert_with(|| format!("{class_name}::{name}"));
                            }
                            symbol.stable_id.as_str()
                        });
                    collect_php_type_evidence(
                        source,
                        type_node,
                        context,
                        next_class_canonical_name.as_deref(),
                        owner_symbol_id,
                        PhpTypeEvidenceKind::Property,
                        source_span(child).start_line,
                        &mut evidence.type_evidence,
                    );
                }
            }
        }
        "property_element" => {
            if let Some((kind, name)) = symbol_from_node(SymbolLanguage::Php, source, node) {
                if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                    if let Some(class_name) = next_class_canonical_name.as_deref() {
                        evidence
                            .canonical_names_by_stable_id
                            .entry(symbol.stable_id.clone())
                            .or_insert_with(|| format!("{class_name}::{name}"));
                    }
                }
            }
        }
        "const_element" => {
            if let Some((kind, name)) = symbol_from_node(SymbolLanguage::Php, source, node) {
                if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                    let canonical_name =
                        if let Some(class_name) = next_class_canonical_name.as_deref() {
                            format!("{class_name}::{name}")
                        } else {
                            php_namespace_qualified_name(next_namespace.as_deref(), &name)
                        };
                    evidence
                        .canonical_names_by_stable_id
                        .entry(symbol.stable_id.clone())
                        .or_insert(canonical_name);
                }
            }
        }
        "enum_case" => {
            if let Some((kind, name)) = symbol_from_node(SymbolLanguage::Php, source, node) {
                if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                    if let Some(class_name) = next_class_canonical_name.as_deref() {
                        evidence
                            .canonical_names_by_stable_id
                            .entry(symbol.stable_id.clone())
                            .or_insert_with(|| format!("{class_name}::{name}"));
                    }
                }
            }
        }
        "catch_clause" => {
            if let Some(type_node) = node.child_by_field_name("type") {
                collect_php_type_evidence(
                    source,
                    type_node,
                    context,
                    next_class_canonical_name.as_deref(),
                    next_owner_symbol_id.as_deref().or_else(|| {
                        find_innermost_symbol_for_span_in_file(
                            file_symbols,
                            SymbolLanguage::Php,
                            &source_span(node),
                        )
                        .map(|symbol| symbol.stable_id.as_str())
                    }),
                    PhpTypeEvidenceKind::Catch,
                    source_span(node).start_line,
                    &mut evidence.type_evidence,
                );
            }
        }
        "attribute" => {
            if let Some(target_name) =
                php_attribute_target_name(source, node).and_then(|raw_name| {
                    context.resolve_class_like_name(
                        raw_name.as_str(),
                        next_class_canonical_name.as_deref(),
                    )
                })
            {
                evidence.target_evidence.push(PhpTargetEvidence {
                    owner_symbol_id: next_owner_symbol_id.clone().or_else(|| {
                        find_innermost_symbol_for_span_in_file(
                            file_symbols,
                            SymbolLanguage::Php,
                            &source_span(node),
                        )
                        .map(|symbol| symbol.stable_id.clone())
                    }),
                    kind: PhpTargetEvidenceKind::Attribute,
                    target_canonical_name: target_name,
                    target_member_name: None,
                    line: source_span(node).start_line,
                });
            }
        }
        "class_constant_access_expression" => {
            if let Some(target_name) = php_class_string_target_name(
                source,
                node,
                context,
                next_class_canonical_name.as_deref(),
            ) {
                evidence.target_evidence.push(PhpTargetEvidence {
                    owner_symbol_id: next_owner_symbol_id.clone().or_else(|| {
                        find_innermost_symbol_for_span_in_file(
                            file_symbols,
                            SymbolLanguage::Php,
                            &source_span(node),
                        )
                        .map(|symbol| symbol.stable_id.clone())
                    }),
                    kind: PhpTargetEvidenceKind::ClassString,
                    target_canonical_name: target_name,
                    target_member_name: None,
                    line: source_span(node).start_line,
                });
            }
        }
        "object_creation_expression" => {
            if let Some(target_name) = php_instantiation_target_name(
                source,
                node,
                context,
                next_class_canonical_name.as_deref(),
            ) {
                evidence.target_evidence.push(PhpTargetEvidence {
                    owner_symbol_id: next_owner_symbol_id.clone().or_else(|| {
                        find_innermost_symbol_for_span_in_file(
                            file_symbols,
                            SymbolLanguage::Php,
                            &source_span(node),
                        )
                        .map(|symbol| symbol.stable_id.clone())
                    }),
                    kind: PhpTargetEvidenceKind::Instantiation,
                    target_canonical_name: target_name,
                    target_member_name: None,
                    line: source_span(node).start_line,
                });
            }
        }
        "array_creation_expression" => {
            if let Some((target_canonical_name, target_member_name)) = php_callable_literal_target(
                source,
                node,
                context,
                next_class_canonical_name.as_deref(),
            ) {
                evidence.target_evidence.push(PhpTargetEvidence {
                    owner_symbol_id: next_owner_symbol_id.clone().or_else(|| {
                        find_innermost_symbol_for_span_in_file(
                            file_symbols,
                            SymbolLanguage::Php,
                            &source_span(node),
                        )
                        .map(|symbol| symbol.stable_id.clone())
                    }),
                    kind: PhpTargetEvidenceKind::CallableLiteral,
                    target_canonical_name,
                    target_member_name: Some(target_member_name),
                    line: source_span(node).start_line,
                });
            }
            if let Some(array_keys) = php_literal_array_keys(source, node) {
                evidence.literal_evidence.push(PhpLiteralEvidence {
                    owner_symbol_id: next_owner_symbol_id.clone().or_else(|| {
                        find_innermost_symbol_for_span_in_file(
                            file_symbols,
                            SymbolLanguage::Php,
                            &source_span(node),
                        )
                        .map(|symbol| symbol.stable_id.clone())
                    }),
                    array_keys,
                    named_arguments: Vec::new(),
                    line: source_span(node).start_line,
                });
            }
        }
        "arguments" => {
            if let Some(named_arguments) = php_named_argument_keys(source, node) {
                evidence.literal_evidence.push(PhpLiteralEvidence {
                    owner_symbol_id: next_owner_symbol_id.clone().or_else(|| {
                        find_innermost_symbol_for_span_in_file(
                            file_symbols,
                            SymbolLanguage::Php,
                            &source_span(node),
                        )
                        .map(|symbol| symbol.stable_id.clone())
                    }),
                    array_keys: Vec::new(),
                    named_arguments,
                    line: source_span(node).start_line,
                });
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_php_source_evidence(
            source,
            child,
            file_symbols,
            context,
            next_namespace.as_deref(),
            next_class_canonical_name.as_deref(),
            next_owner_symbol_id.as_deref(),
            evidence,
        );
    }
}

fn collect_php_parameter_type_evidence(
    source: &str,
    parameters: Node<'_>,
    file_symbols: &[SymbolDefinition],
    context: &crate::language_support::PhpNameResolutionContext,
    current_class_canonical_name: Option<&str>,
    owner_symbol_id: Option<&str>,
    evidence: &mut PhpSourceEvidence,
) {
    let mut cursor = parameters.walk();
    for parameter in parameters
        .children(&mut cursor)
        .filter(|child| child.is_named())
    {
        let (kind, line) = match parameter.kind() {
            "simple_parameter" => (
                PhpTypeEvidenceKind::Parameter,
                source_span(parameter).start_line,
            ),
            "property_promotion_parameter" => (
                PhpTypeEvidenceKind::PromotedProperty,
                source_span(parameter).start_line,
            ),
            _ => continue,
        };
        if let Some(type_node) = parameter.child_by_field_name("type") {
            collect_php_type_evidence(
                source,
                type_node,
                context,
                current_class_canonical_name,
                owner_symbol_id,
                kind,
                line,
                &mut evidence.type_evidence,
            );
        }
        if let Some(attributes) = parameter.child_by_field_name("attributes") {
            let mut attr_cursor = attributes.walk();
            for attribute_group in attributes
                .children(&mut attr_cursor)
                .filter(|child| child.is_named())
            {
                let mut group_cursor = attribute_group.walk();
                for attribute in attribute_group
                    .children(&mut group_cursor)
                    .filter(|child| child.is_named() && child.kind() == "attribute")
                {
                    if let Some(target_name) = php_attribute_target_name(source, attribute)
                        .and_then(|raw_name| {
                            context.resolve_class_like_name(
                                raw_name.as_str(),
                                current_class_canonical_name,
                            )
                        })
                    {
                        evidence.target_evidence.push(PhpTargetEvidence {
                            owner_symbol_id: owner_symbol_id.map(ToOwned::to_owned).or_else(|| {
                                find_innermost_symbol_for_span_in_file(
                                    file_symbols,
                                    SymbolLanguage::Php,
                                    &source_span(attribute),
                                )
                                .map(|symbol| symbol.stable_id.clone())
                            }),
                            kind: PhpTargetEvidenceKind::Attribute,
                            target_canonical_name: target_name,
                            target_member_name: None,
                            line: source_span(attribute).start_line,
                        });
                    }
                }
            }
        }
    }
}

fn collect_php_type_evidence(
    source: &str,
    type_node: Node<'_>,
    context: &crate::language_support::PhpNameResolutionContext,
    current_class_canonical_name: Option<&str>,
    owner_symbol_id: Option<&str>,
    kind: PhpTypeEvidenceKind,
    line: usize,
    output: &mut Vec<PhpTypeEvidence>,
) {
    let mut targets = BTreeSet::new();
    collect_php_type_targets(
        source,
        type_node,
        context,
        current_class_canonical_name,
        &mut targets,
    );
    for target_canonical_name in targets {
        output.push(PhpTypeEvidence {
            owner_symbol_id: owner_symbol_id.map(ToOwned::to_owned),
            kind,
            target_canonical_name,
            line,
        });
    }
}

fn collect_php_type_targets(
    source: &str,
    node: Node<'_>,
    context: &crate::language_support::PhpNameResolutionContext,
    current_class_canonical_name: Option<&str>,
    targets: &mut BTreeSet<String>,
) {
    match node.kind() {
        "named_type" | "name" | "qualified_name" | "relative_name" => {
            if let Ok(raw_name) = node.utf8_text(source.as_bytes()) {
                if let Some(target) =
                    context.resolve_class_like_name(raw_name, current_class_canonical_name)
                {
                    targets.insert(target);
                }
            }
        }
        _ => {
            let mut cursor = node.walk();
            for child in node.children(&mut cursor).filter(|child| child.is_named()) {
                collect_php_type_targets(
                    source,
                    child,
                    context,
                    current_class_canonical_name,
                    targets,
                );
            }
        }
    }
}

fn find_symbol_for_node<'a>(
    file_symbols: &'a [SymbolDefinition],
    kind: SymbolKind,
    name: &str,
    node: Node<'_>,
) -> Option<&'a SymbolDefinition> {
    let span = source_span(node);
    file_symbols.iter().find(|symbol| {
        symbol.kind == kind
            && symbol.name == name
            && symbol.span.start_byte == span.start_byte
            && symbol.span.end_byte == span.end_byte
    })
}

fn find_innermost_symbol_for_span_in_file<'a>(
    file_symbols: &'a [SymbolDefinition],
    language: SymbolLanguage,
    span: &SourceSpan,
) -> Option<&'a SymbolDefinition> {
    file_symbols
        .iter()
        .filter(|symbol| {
            symbol.language == language
                && span.start_byte >= symbol.span.start_byte
                && span.end_byte <= symbol.span.end_byte
        })
        .min_by(|left, right| {
            let left_width = left.span.end_byte.saturating_sub(left.span.start_byte);
            let right_width = right.span.end_byte.saturating_sub(right.span.start_byte);
            left_width
                .cmp(&right_width)
                .then(left.span.start_byte.cmp(&right.span.start_byte))
                .then(left.stable_id.cmp(&right.stable_id))
        })
}

fn php_namespace_qualified_name(namespace: Option<&str>, short_name: &str) -> String {
    let short_name = short_name.trim();
    if short_name.is_empty() {
        return String::new();
    }
    match namespace
        .map(str::trim)
        .filter(|namespace| !namespace.is_empty())
    {
        Some(namespace) => format!("{namespace}\\{short_name}"),
        None => short_name.to_owned(),
    }
}

fn php_attribute_target_name(source: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .find(|child| matches!(child.kind(), "name" | "qualified_name" | "relative_name"))
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn php_class_string_target_name(
    source: &str,
    node: Node<'_>,
    context: &crate::language_support::PhpNameResolutionContext,
    current_class_canonical_name: Option<&str>,
) -> Option<String> {
    let named_children = named_children(node);
    if named_children.len() != 2 {
        return None;
    }
    let name = named_children[1]
        .utf8_text(source.as_bytes())
        .ok()
        .map(str::trim)?;
    if !name.eq_ignore_ascii_case("class") {
        return None;
    }
    let raw_scope = named_children[0].utf8_text(source.as_bytes()).ok()?.trim();
    context.resolve_class_like_name(raw_scope, current_class_canonical_name)
}

fn php_instantiation_target_name(
    source: &str,
    node: Node<'_>,
    context: &crate::language_support::PhpNameResolutionContext,
    current_class_canonical_name: Option<&str>,
) -> Option<String> {
    let named_children = named_children(node);
    let first = named_children.first()?;
    if first.kind() == "anonymous_class" {
        return None;
    }
    if !matches!(first.kind(), "name" | "qualified_name" | "relative_name") {
        return None;
    }
    let raw_name = first.utf8_text(source.as_bytes()).ok()?.trim();
    context.resolve_class_like_name(raw_name, current_class_canonical_name)
}

fn php_callable_literal_target(
    source: &str,
    node: Node<'_>,
    context: &crate::language_support::PhpNameResolutionContext,
    current_class_canonical_name: Option<&str>,
) -> Option<(String, String)> {
    let initializers = named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "array_element_initializer")
        .collect::<Vec<_>>();
    if initializers.len() != 2 {
        return None;
    }
    let first = named_children(initializers[0]).into_iter().next()?;
    let second = named_children(initializers[1]).into_iter().next()?;
    let target_name =
        php_class_string_target_name(source, first, context, current_class_canonical_name)?;
    let target_member_name = php_string_literal_value(source, second)?;
    Some((target_name, target_member_name))
}

fn php_string_literal_value(source: &str, node: Node<'_>) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    let unquoted = text
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            text.strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })
        .unwrap_or(text)
        .trim();
    (!unquoted.is_empty()).then(|| unquoted.to_owned())
}

fn php_literal_array_keys(source: &str, node: Node<'_>) -> Option<Vec<String>> {
    let mut keys = BTreeSet::new();
    for initializer in named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "array_element_initializer")
    {
        let children = named_children(initializer);
        if children.len() < 2 {
            continue;
        }
        if let Some(key) = php_literal_key_text(source, children[0]) {
            keys.insert(key);
        }
    }
    (!keys.is_empty()).then(|| keys.into_iter().collect())
}

fn php_literal_key_text(source: &str, node: Node<'_>) -> Option<String> {
    let text = node.utf8_text(source.as_bytes()).ok()?.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(unquoted) = text
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
        .or_else(|| {
            text.strip_prefix('"')
                .and_then(|value| value.strip_suffix('"'))
        })
    {
        let normalized = unquoted.trim();
        return (!normalized.is_empty()).then(|| normalized.to_owned());
    }
    text.chars()
        .all(|value| value.is_ascii_digit() || value == '-' || value == '+')
        .then(|| text.to_owned())
}

fn php_named_argument_keys(source: &str, node: Node<'_>) -> Option<Vec<String>> {
    let mut keys = BTreeSet::new();
    for argument in named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "argument")
    {
        let Some(name) = argument
            .child_by_field_name("name")
            .and_then(|field| field.utf8_text(source.as_bytes()).ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        keys.insert(name.to_owned());
    }
    (!keys.is_empty()).then(|| keys.into_iter().collect())
}

fn resolve_php_target_symbol_index(
    symbols: &[SymbolDefinition],
    symbol_indices_by_canonical_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_canonical_name: &BTreeMap<String, Vec<usize>>,
    target: &PhpTargetEvidence,
) -> Option<usize> {
    if let Some(member_name) = target.target_member_name.as_deref() {
        let candidate = format!("{}::{member_name}", target.target_canonical_name);
        if let Some(index) = resolve_unique_canonical_symbol_index(
            symbols,
            symbol_indices_by_canonical_name,
            symbol_indices_by_lower_canonical_name,
            &candidate,
            Some(&[
                SymbolKind::Method,
                SymbolKind::Property,
                SymbolKind::Constant,
                SymbolKind::EnumCase,
            ]),
        ) {
            return Some(index);
        }
    }
    resolve_unique_canonical_symbol_index(
        symbols,
        symbol_indices_by_canonical_name,
        symbol_indices_by_lower_canonical_name,
        &target.target_canonical_name,
        Some(&[
            SymbolKind::Class,
            SymbolKind::Interface,
            SymbolKind::PhpTrait,
            SymbolKind::PhpEnum,
        ]),
    )
}

fn resolve_blade_relation_target_symbol_index(
    symbols: &[SymbolDefinition],
    symbol_indices_by_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_name: &BTreeMap<String, Vec<usize>>,
    relation: &BladeRelationEvidence,
) -> Option<usize> {
    resolve_unique_symbol_index_by_name(
        symbols,
        symbol_indices_by_name,
        symbol_indices_by_lower_name,
        relation.target_name.as_str(),
        relation.target_symbol_kind,
    )
}

fn resolve_unique_canonical_symbol_index(
    symbols: &[SymbolDefinition],
    symbol_indices_by_canonical_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_canonical_name: &BTreeMap<String, Vec<usize>>,
    target_name: &str,
    allowed_kinds: Option<&[SymbolKind]>,
) -> Option<usize> {
    if let Some(indices) = symbol_indices_by_canonical_name.get(target_name) {
        let matches = indices
            .iter()
            .copied()
            .filter(|index| {
                allowed_kinds.is_none_or(|allowed| allowed.contains(&symbols[*index].kind))
            })
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            return matches.first().copied();
        }
        if !matches.is_empty() {
            return None;
        }
    }
    let lower = target_name.to_ascii_lowercase();
    let matches = symbol_indices_by_lower_canonical_name
        .get(&lower)
        .into_iter()
        .flatten()
        .copied()
        .filter(|index| allowed_kinds.is_none_or(|allowed| allowed.contains(&symbols[*index].kind)))
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.first().copied()
    } else {
        None
    }
}

fn resolve_unique_symbol_index_by_name(
    symbols: &[SymbolDefinition],
    symbol_indices_by_name: &BTreeMap<String, Vec<usize>>,
    symbol_indices_by_lower_name: &BTreeMap<String, Vec<usize>>,
    target_name: &str,
    required_kind: SymbolKind,
) -> Option<usize> {
    if let Some(indices) = symbol_indices_by_name.get(target_name) {
        let matches = indices
            .iter()
            .copied()
            .filter(|index| symbols[*index].kind == required_kind)
            .collect::<Vec<_>>();
        if matches.len() == 1 {
            return matches.first().copied();
        }
        if !matches.is_empty() {
            return None;
        }
    }
    let matches = symbol_indices_by_lower_name
        .get(&target_name.to_ascii_lowercase())
        .into_iter()
        .flatten()
        .copied()
        .filter(|index| symbols[*index].kind == required_kind)
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.first().copied()
    } else {
        None
    }
}

fn normalize_php_source_evidence(evidence: &mut PhpSourceEvidence) {
    evidence.type_evidence.sort();
    evidence.type_evidence.dedup();
    evidence.target_evidence.sort();
    evidence.target_evidence.dedup();
    evidence.literal_evidence.sort();
    evidence.literal_evidence.dedup();
}

fn normalize_blade_source_evidence(evidence: &mut BladeSourceEvidence) {
    evidence.relations.sort();
    evidence.relations.dedup();
    evidence.livewire_components.sort();
    evidence.livewire_components.dedup();
    evidence.wire_directives.sort();
    evidence.wire_directives.dedup();
    evidence.flux_components.sort();
    evidence.flux_components.dedup();
    for hint in evidence.flux_hints.values_mut() {
        hint.props.sort();
        hint.props.dedup();
        hint.slots.sort();
        hint.slots.dedup();
        hint.variant_values.sort();
        hint.variant_values.dedup();
        hint.size_values.sort();
        hint.size_values.dedup();
    }
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .collect()
}

fn insert_sorted_unique_owned(values: &mut Vec<String>, value: String) {
    match values.binary_search(&value) {
        Ok(_) => {}
        Err(index) => values.insert(index, value),
    }
}

fn flux_registry_hint(component_name: &str) -> Option<FluxComponentHint> {
    match component_name {
        "flux:button" => Some(FluxComponentHint {
            props: vec!["icon".to_owned(), "size".to_owned(), "variant".to_owned()],
            slots: vec!["default".to_owned()],
            variant_values: vec![
                "danger".to_owned(),
                "ghost".to_owned(),
                "primary".to_owned(),
                "subtle".to_owned(),
            ],
            size_values: vec!["sm".to_owned(), "base".to_owned(), "lg".to_owned()],
            local_overlay: false,
        }),
        "flux:input" => Some(FluxComponentHint {
            props: vec!["size".to_owned(), "type".to_owned()],
            slots: Vec::new(),
            variant_values: Vec::new(),
            size_values: vec!["sm".to_owned(), "base".to_owned(), "lg".to_owned()],
            local_overlay: false,
        }),
        "flux:modal" => Some(FluxComponentHint {
            props: vec!["name".to_owned(), "variant".to_owned()],
            slots: vec![
                "default".to_owned(),
                "footer".to_owned(),
                "heading".to_owned(),
            ],
            variant_values: vec!["danger".to_owned(), "default".to_owned()],
            size_values: Vec::new(),
            local_overlay: false,
        }),
        "flux:dropdown" => Some(FluxComponentHint {
            props: vec!["align".to_owned(), "position".to_owned()],
            slots: vec!["default".to_owned(), "trigger".to_owned()],
            variant_values: Vec::new(),
            size_values: Vec::new(),
            local_overlay: false,
        }),
        "flux:select" => Some(FluxComponentHint {
            props: vec!["multiple".to_owned(), "size".to_owned()],
            slots: vec!["default".to_owned()],
            variant_values: Vec::new(),
            size_values: vec!["sm".to_owned(), "base".to_owned(), "lg".to_owned()],
            local_overlay: false,
        }),
        _ => None,
    }
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

fn collect_blade_symbols_from_source(
    path: &Path,
    source: &str,
    symbols: &mut Vec<SymbolDefinition>,
) {
    let whole_file_span = source_span_from_offsets(source, 0, source.len());
    let file_anchor = usize::from(!source.is_empty());
    let module_name = blade_view_name_for_path(path).unwrap_or_else(|| {
        path.file_name()
            .and_then(|name| name.to_str())
            .map(strip_blade_suffix)
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "blade".to_owned())
    });
    push_symbol_definition(
        symbols,
        SymbolLanguage::Blade,
        SymbolKind::Module,
        path,
        &module_name,
        whole_file_span.clone(),
    );

    if let Some(component_name) = blade_component_name_for_path(path) {
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Component,
            path,
            &component_name,
            source_span_from_offsets(source, file_anchor, file_anchor),
        );
    }

    for capture in blade_section_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Section,
            path,
            name.as_str().trim(),
            source_span_from_offsets(source, name.start(), name.end()),
        );
    }

    for capture in blade_livewire_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        let normalized = format!("livewire:{}", name.as_str().trim());
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Component,
            path,
            &normalized,
            source_span_from_offsets(
                source,
                capture.get(0).unwrap().start(),
                capture.get(0).unwrap().end(),
            ),
        );
    }

    for capture in blade_tag_regex().captures_iter(source) {
        let Some(tag_name) = capture.get(1) else {
            continue;
        };
        let Some((kind, normalized_name)) = classify_blade_tag_name(tag_name.as_str()) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            kind,
            path,
            &normalized_name,
            source_span_from_offsets(source, tag_name.start(), tag_name.end()),
        );
    }

    for capture in blade_named_slot_tag_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Slot,
            path,
            name.as_str().trim(),
            source_span_from_offsets(source, name.start(), name.end()),
        );
    }

    for capture in blade_slot_directive_regex().captures_iter(source) {
        let Some(name) = capture.get(1).or_else(|| capture.get(2)) else {
            continue;
        };
        push_symbol_definition(
            symbols,
            SymbolLanguage::Blade,
            SymbolKind::Slot,
            path,
            name.as_str().trim(),
            source_span_from_offsets(source, name.start(), name.end()),
        );
    }

    collect_blade_property_symbols(source, path, symbols, "@props");
    collect_blade_property_symbols(source, path, symbols, "@aware");
}

fn collect_blade_property_symbols(
    source: &str,
    path: &Path,
    symbols: &mut Vec<SymbolDefinition>,
    directive: &str,
) {
    let bytes = source.as_bytes();
    let mut cursor = 0usize;
    while let Some(relative) = source[cursor..].find(directive) {
        let start = cursor + relative;
        let mut offset = start + directive.len();
        while offset < bytes.len() && bytes[offset].is_ascii_whitespace() {
            offset += 1;
        }
        if offset >= bytes.len() || bytes[offset] != b'(' {
            cursor = start + directive.len();
            continue;
        }
        let Some((parameter_start, parameter_end)) =
            blade_directive_parameter_bounds(source, offset)
        else {
            cursor = start + directive.len();
            continue;
        };
        let parameter_source = &source[parameter_start..parameter_end];
        for capture in blade_property_key_regex().captures_iter(parameter_source) {
            let Some(name_match) = capture.get(1).or_else(|| capture.get(2)) else {
                continue;
            };
            let normalized_name = format!("${}", name_match.as_str().trim());
            push_symbol_definition(
                symbols,
                SymbolLanguage::Blade,
                SymbolKind::Property,
                path,
                &normalized_name,
                source_span_from_offsets(
                    source,
                    parameter_start + name_match.start(),
                    parameter_start + name_match.end(),
                ),
            );
        }
        cursor = parameter_end.saturating_add(1);
    }
}

fn blade_directive_parameter_bounds(
    source: &str,
    open_paren_index: usize,
) -> Option<(usize, usize)> {
    let bytes = source.as_bytes();
    if bytes.get(open_paren_index) != Some(&b'(') {
        return None;
    }
    let mut depth = 0usize;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;
    for (offset, byte) in bytes[open_paren_index..].iter().copied().enumerate() {
        let index = open_paren_index + offset;
        if escaped {
            escaped = false;
            continue;
        }
        match byte {
            b'\\' if in_single_quote || in_double_quote => {
                escaped = true;
            }
            b'\'' if !in_double_quote => {
                in_single_quote = !in_single_quote;
            }
            b'"' if !in_single_quote => {
                in_double_quote = !in_double_quote;
            }
            b'(' if !in_single_quote && !in_double_quote => {
                depth += 1;
            }
            b')' if !in_single_quote && !in_double_quote => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some((open_paren_index + 1, index));
                }
            }
            _ => {}
        }
    }
    None
}

fn classify_blade_tag_name(raw_tag_name: &str) -> Option<(SymbolKind, String)> {
    let normalized = raw_tag_name.trim();
    if normalized.is_empty() {
        return None;
    }
    if let Some(slot_name) = normalized
        .strip_prefix("x-slot:")
        .or_else(|| normalized.strip_prefix("x-slot."))
    {
        let slot_name = slot_name.trim();
        return (!slot_name.is_empty()).then(|| (SymbolKind::Slot, slot_name.to_owned()));
    }
    if let Some(component_name) = normalized.strip_prefix("x-") {
        if component_name == "dynamic-component" {
            return None;
        }
        let component_name = component_name.trim();
        return (!component_name.is_empty())
            .then(|| (SymbolKind::Component, component_name.to_owned()));
    }
    if normalized.starts_with("livewire:") || normalized.starts_with("flux:") {
        return Some((SymbolKind::Component, normalized.to_owned()));
    }
    None
}

fn push_symbol_definition(
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

fn blade_section_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"@(?:section|yield)\s*\(\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade section regex must compile")
    })
}

fn blade_livewire_directive_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"@livewire\s*\(\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade livewire directive regex must compile")
    })
}

fn blade_view_relation_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"@(?P<directive>extends|component|include(?:If|When|Unless|First)?|yield)\s*\(\s*(?:"([^"]+)"|'([^']+)')"#,
        )
        .expect("blade view relation regex must compile")
    })
}

fn blade_dynamic_component_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"<\s*x-dynamic-component\b[^>]*\b(?::component|component)\s*=\s*(?:"([^"]+)"|'([^']+)')"#,
        )
        .expect("blade dynamic component regex must compile")
    })
}

fn blade_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"<\s*(x-[A-Za-z0-9_.:-]+|livewire:[A-Za-z0-9_.:-]+|flux:[A-Za-z0-9_.:-]+)(?:[\s>/])"#,
        )
        .expect("blade tag regex must compile")
    })
}

fn blade_named_slot_tag_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"<\s*x-slot\b[^>]*\bname\s*=\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade named slot regex must compile")
    })
}

fn blade_slot_directive_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"@slot\s*\(\s*(?:"([^"]+)"|'([^']+)')"#)
            .expect("blade slot directive regex must compile")
    })
}

fn blade_wire_directive_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(r#"\b(wire:[A-Za-z0-9_.-]+)\s*="#)
            .expect("blade wire directive regex must compile")
    })
}

fn normalize_blade_dynamic_component_target(raw_target: &str) -> String {
    raw_target
        .trim()
        .trim_matches(['"', '\''])
        .trim()
        .to_owned()
}

fn blade_property_key_regex() -> &'static Regex {
    static REGEX: OnceLock<Regex> = OnceLock::new();
    REGEX.get_or_init(|| {
        Regex::new(
            r#"(?x)
            (?:^|[\[,])\s*"([^"]+)"(?:\s*=>)?
            |
            (?:^|[\[,])\s*'([^']+)'(?:\s*=>)?
        "#,
        )
        .expect("blade property key regex must compile")
    })
}

fn source_span_from_offsets(source: &str, start_byte: usize, end_byte: usize) -> SourceSpan {
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

fn line_column_for_offset(source: &str, offset: usize) -> (usize, usize) {
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

fn strip_blade_suffix(name: &str) -> String {
    name.strip_suffix(".blade.php")
        .or_else(|| name.strip_suffix(".php"))
        .unwrap_or(name)
        .to_owned()
}

fn collect_php_declaration_relations(
    source: &str,
    node: Node<'_>,
    relations: &mut Vec<PhpDeclarationRelation>,
) {
    if let Some((source_kind, source_name)) = symbol_from_node(SymbolLanguage::Php, source, node) {
        let relation_kind = match source_kind {
            SymbolKind::Class | SymbolKind::Interface | SymbolKind::PhpEnum => Some(source_kind),
            _ => None,
        };
        if let Some(source_kind) = relation_kind {
            let source_line = source_span(node).start_line;
            let mut cursor = node.walk();
            for child in node.children(&mut cursor).filter(|child| child.is_named()) {
                let relation = match child.kind() {
                    "base_clause" => Some(RelationKind::Extends),
                    "class_interface_clause" => Some(RelationKind::Implements),
                    _ => None,
                };
                let Some(relation) = relation else {
                    continue;
                };

                let mut clause_cursor = child.walk();
                for target in child
                    .children(&mut clause_cursor)
                    .filter(|child| child.is_named())
                {
                    let Some(target_name) = target
                        .utf8_text(source.as_bytes())
                        .ok()
                        .map(str::trim)
                        .filter(|text| !text.is_empty())
                        .map(ToOwned::to_owned)
                    else {
                        continue;
                    };
                    relations.push(PhpDeclarationRelation {
                        source_kind,
                        source_name: source_name.clone(),
                        source_line,
                        target_name,
                        relation,
                    });
                }
            }
        }
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_php_declaration_relations(source, child, relations);
    }
}

fn source_span(node: Node<'_>) -> SourceSpan {
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

fn php_symbol_indices_by_name(symbols: &[SymbolDefinition]) -> BTreeMap<String, Vec<usize>> {
    let mut indices = BTreeMap::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if symbol.language == SymbolLanguage::Php {
            indices
                .entry(symbol.name.clone())
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    indices
}

fn php_symbol_indices_by_lower_name(symbols: &[SymbolDefinition]) -> BTreeMap<String, Vec<usize>> {
    let mut indices = BTreeMap::new();
    for (index, symbol) in symbols.iter().enumerate() {
        if symbol.language == SymbolLanguage::Php {
            indices
                .entry(symbol.name.to_ascii_lowercase())
                .or_insert_with(Vec::new)
                .push(index);
        }
    }
    indices
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
    let executor = RuntimeSemanticEmbeddingExecutor::new(credentials.clone());
    reindex_repository_with_semantic_executor(
        repository_id,
        workspace_root,
        db_path,
        mode,
        semantic_runtime,
        credentials,
        &executor,
    )
}

fn reindex_repository_with_semantic_executor(
    repository_id: &str,
    workspace_root: &Path,
    db_path: &Path,
    mode: ReindexMode,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<ReindexSummary> {
    let started_at = Instant::now();
    let manifest_store = ManifestStore::new(db_path);
    manifest_store.initialize()?;

    let manifest_builder = ManifestBuilder::default();
    let manifest_output = manifest_builder.build_with_diagnostics(workspace_root)?;
    let current_manifest = manifest_output.entries;
    let diagnostics = ReindexDiagnostics {
        entries: manifest_output.diagnostics,
    };
    let previous_manifest = manifest_store.load_latest_manifest_for_repository(repository_id)?;
    let previous_snapshot_id = previous_manifest
        .as_ref()
        .map(|manifest| manifest.snapshot_id.clone());
    let previous_entries = previous_manifest
        .as_ref()
        .map(|manifest| manifest.entries.clone())
        .unwrap_or_default();
    let manifest_diff = diff(&previous_entries, &current_manifest);
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
        digest
            .as_bytes()
            .chunks_exact(4)
            .take(8)
            .map(|chunk| {
                let value = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                (value as f32) / (u32::MAX as f32)
            })
            .collect()
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
            chunks
                .iter()
                .any(|chunk| chunk.path == "README.md" && chunk.language == "markdown"),
            "README.md should participate in semantic chunking"
        );
        assert!(
            chunks.iter().any(|chunk| {
                chunk.path == "contracts/errors.md" && chunk.language == "markdown"
            }),
            "contract markdown should participate in semantic chunking"
        );
        assert!(
            chunks.iter().any(|chunk| {
                chunk.path == "fixtures/playbooks/deep-search-suite-core.playbook.json"
                    && chunk.language == "json"
            }),
            "fixture json should participate in semantic chunking"
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.path == "src/lib.rs" && chunk.language == "rust"),
            "source files should remain in semantic chunking"
        );

        cleanup_workspace(&workspace_root);
        Ok(())
    }

    #[test]
    fn semantic_chunk_candidates_skip_playbook_markdown_self_references() -> FriggResult<()> {
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
                .all(|chunk| chunk.path != "playbooks/hybrid-search-context-retrieval.md"),
            "playbook markdown should be excluded from semantic chunking to avoid self-reference"
        );
        assert!(
            chunks
                .iter()
                .any(|chunk| chunk.path == "contracts/errors.md"),
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
        let mut evidence = extract_blade_source_evidence_from_source(path, source, &symbols);

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
        let evidence = extract_blade_source_evidence_from_source(path, source, &symbols);

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
    fn symbols_rust_php_extraction_is_deterministic() -> FriggResult<()> {
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

        assert_eq!(first, second);
        assert_eq!(third, fourth);
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
