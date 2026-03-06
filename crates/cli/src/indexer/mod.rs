use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::fs::File;
use std::future::Future;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::str::FromStr;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::domain::{FriggError, FriggResult};
use crate::embeddings::{
    EmbeddingProvider, EmbeddingPurpose, EmbeddingRequest, GoogleEmbeddingProvider,
    OpenAiEmbeddingProvider,
};
use crate::graph::{HeuristicConfidence, SymbolGraph, SymbolNode};
use crate::playbooks::scrub_playbook_metadata_header;
use crate::settings::{SemanticRuntimeConfig, SemanticRuntimeCredentials, SemanticRuntimeProvider};
use crate::storage::{ManifestEntry, SemanticChunkEmbeddingRecord, Storage};
use blake3::Hasher;
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser, Query, QueryCursor, StreamingIterator, Tree};

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
pub enum SymbolLanguage {
    Rust,
    Php,
}

impl SymbolLanguage {
    pub fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("rs") => Some(Self::Rust),
            Some("php") => Some(Self::Php),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Php => "php",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Module,
    Struct,
    Enum,
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
            Self::Struct => "struct",
            Self::Enum => "enum",
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

    output.symbols.sort_by(symbol_definition_order);
    output
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

fn tree_sitter_language(language: SymbolLanguage) -> tree_sitter::Language {
    match language {
        SymbolLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        SymbolLanguage::Php => tree_sitter_php::LANGUAGE_PHP.into(),
    }
}

fn parser_for_language(language: SymbolLanguage) -> FriggResult<Parser> {
    let mut parser = Parser::new();
    let ts_language = tree_sitter_language(language);

    parser.set_language(&ts_language).map_err(|err| {
        FriggError::Internal(format!(
            "failed to configure tree-sitter parser for {}: {err}",
            language.as_str()
        ))
    })?;
    Ok(parser)
}

fn collect_symbols_from_tree(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    tree: &Tree,
    symbols: &mut Vec<SymbolDefinition>,
) {
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

fn symbol_from_node(
    language: SymbolLanguage,
    source: &str,
    node: Node<'_>,
) -> Option<(SymbolKind, String)> {
    match language {
        SymbolLanguage::Rust => rust_symbol_from_node(source, node),
        SymbolLanguage::Php => php_symbol_from_node(source, node),
    }
}

fn rust_symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "mod_item" => node_name_text(node, source).map(|name| (SymbolKind::Module, name)),
        "struct_item" => node_name_text(node, source).map(|name| (SymbolKind::Struct, name)),
        "enum_item" => node_name_text(node, source).map(|name| (SymbolKind::Enum, name)),
        "trait_item" => node_name_text(node, source).map(|name| (SymbolKind::Trait, name)),
        "type_item" => node_name_text(node, source).map(|name| (SymbolKind::TypeAlias, name)),
        "const_item" => node_name_text(node, source).map(|name| (SymbolKind::Const, name)),
        "static_item" => node_name_text(node, source).map(|name| (SymbolKind::Static, name)),
        "function_item" | "function_signature_item" => {
            let parent_kind = node.parent().map(|parent| parent.kind());
            let kind = if matches!(parent_kind, Some("impl_item" | "trait_item")) {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            node_name_text(node, source).map(|name| (kind, name))
        }
        "impl_item" => {
            let implemented_type = node_field_text(node, source, "type");
            let implemented_trait = node_field_text(node, source, "trait");
            let name = match (implemented_trait, implemented_type) {
                (Some(trait_name), Some(type_name)) => format!("impl {trait_name} for {type_name}"),
                (None, Some(type_name)) => format!("impl {type_name}"),
                _ => "impl".to_string(),
            };
            Some((SymbolKind::Impl, name))
        }
        _ => None,
    }
}

fn php_symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "function_definition" => {
            node_name_text(node, source).map(|name| (SymbolKind::Function, name))
        }
        "class_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Class, name)),
        "interface_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::Interface, name))
        }
        "trait_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::PhpTrait, name))
        }
        "enum_declaration" => node_name_text(node, source).map(|name| (SymbolKind::PhpEnum, name)),
        "method_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Method, name)),
        "property_element" => node_name_text(node, source).map(|name| (SymbolKind::Property, name)),
        "const_element" => node_name_text(node, source).map(|name| (SymbolKind::Constant, name)),
        _ => None,
    }
}

fn node_name_text(node: Node<'_>, source: &str) -> Option<String> {
    node_field_text(node, source, "name").or_else(|| {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .filter(|child| child.is_named())
            .find(|child| matches!(child.kind(), "name" | "identifier" | "variable_name"))
            .and_then(|child| child.utf8_text(source.as_bytes()).ok())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn node_field_text(node: Node<'_>, source: &str, field_name: &str) -> Option<String> {
    node.child_by_field_name(field_name).and_then(|field_node| {
        field_node
            .utf8_text(source.as_bytes())
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemanticChunkCandidate {
    chunk_id: String,
    repository_id: String,
    snapshot_id: String,
    path: String,
    language: String,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    content_hash_blake3: String,
    content_text: String,
}

trait SemanticRuntimeEmbeddingExecutor: Sync {
    fn embed_documents<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        input: Vec<String>,
        trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>>;
}

fn build_semantic_embedding_runtime() -> FriggResult<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to build tokio runtime for semantic embedding requests: {err}"
            ))
        })
}

fn execute_semantic_embedding_batch(
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
    provider: SemanticRuntimeProvider,
    model: &str,
    input: Vec<String>,
    trace_id: Option<String>,
) -> FriggResult<Vec<Vec<f32>>> {
    if tokio::runtime::Handle::try_current().is_ok() {
        let model = model.to_owned();
        return std::thread::scope(|scope| {
            let handle = scope.spawn(|| {
                let runtime = build_semantic_embedding_runtime()?;
                runtime.block_on(executor.embed_documents(provider, &model, input, trace_id))
            });
            match handle.join() {
                Ok(result) => result,
                Err(_) => Err(FriggError::Internal(
                    "semantic embedding provider thread panicked under an active tokio runtime"
                        .to_owned(),
                )),
            }
        });
    }

    let runtime = build_semantic_embedding_runtime()?;
    runtime.block_on(executor.embed_documents(provider, model, input, trace_id))
}

#[derive(Debug, Default)]
struct RuntimeSemanticEmbeddingExecutor {
    credentials: SemanticRuntimeCredentials,
}

impl RuntimeSemanticEmbeddingExecutor {
    fn new(credentials: SemanticRuntimeCredentials) -> Self {
        Self { credentials }
    }
}

impl SemanticRuntimeEmbeddingExecutor for RuntimeSemanticEmbeddingExecutor {
    fn embed_documents<'a>(
        &'a self,
        provider: SemanticRuntimeProvider,
        model: &'a str,
        input: Vec<String>,
        trace_id: Option<String>,
    ) -> Pin<Box<dyn Future<Output = FriggResult<Vec<Vec<f32>>>> + Send + 'a>> {
        let model = model.trim().to_owned();
        let api_key = self
            .credentials
            .api_key_for(provider)
            .map(str::to_owned)
            .unwrap_or_default();
        Box::pin(async move {
            let request = EmbeddingRequest {
                model,
                input,
                purpose: EmbeddingPurpose::Document,
                dimensions: None,
                trace_id,
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
                FriggError::Internal(format!("semantic embedding provider call failed: {err}"))
            })?;

            Ok(response
                .vectors
                .into_iter()
                .map(|vector| vector.values)
                .collect::<Vec<_>>())
        })
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

    if semantic_runtime.enabled {
        let storage = Storage::new(db_path);
        match mode {
            ReindexMode::Full => {
                let semantic_records = build_semantic_embedding_records(
                    repository_id,
                    workspace_root,
                    &snapshot_id,
                    &current_manifest,
                    semantic_runtime,
                    credentials,
                    executor,
                )?;
                storage.replace_semantic_embeddings_for_repository(
                    repository_id,
                    &snapshot_id,
                    &semantic_records,
                )?;
            }
            ReindexMode::ChangedOnly => {
                if files_changed > 0 || files_deleted > 0 || previous_manifest.is_none() {
                    let semantic_manifest = manifest_diff
                        .added
                        .iter()
                        .chain(manifest_diff.modified.iter())
                        .cloned()
                        .collect::<Vec<_>>();
                    let semantic_records = build_semantic_embedding_records(
                        repository_id,
                        workspace_root,
                        &snapshot_id,
                        &semantic_manifest,
                        semantic_runtime,
                        credentials,
                        executor,
                    )?;
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
                    )?;
                }
            }
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

fn resolve_semantic_runtime_config_from_env() -> FriggResult<SemanticRuntimeConfig> {
    let enabled = parse_optional_bool_env(FRIGG_SEMANTIC_RUNTIME_ENABLED_ENV)?.unwrap_or(false);
    if !enabled {
        return Ok(SemanticRuntimeConfig::default());
    }
    let strict_mode =
        parse_optional_bool_env(FRIGG_SEMANTIC_RUNTIME_STRICT_MODE_ENV)?.unwrap_or(false);
    let provider = std::env::var(FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV)
        .ok()
        .map(|raw| {
            SemanticRuntimeProvider::from_str(raw.trim()).map_err(|message| {
                FriggError::InvalidInput(format!(
                    "invalid {} value: {message}",
                    FRIGG_SEMANTIC_RUNTIME_PROVIDER_ENV
                ))
            })
        })
        .transpose()?;
    let model = std::env::var(FRIGG_SEMANTIC_RUNTIME_MODEL_ENV)
        .ok()
        .map(|raw| raw.trim().to_owned());

    Ok(SemanticRuntimeConfig {
        enabled,
        provider,
        model,
        strict_mode,
    })
}

fn parse_optional_bool_env(name: &str) -> FriggResult<Option<bool>> {
    let Some(raw) = std::env::var(name).ok() else {
        return Ok(None);
    };
    let normalized = raw.trim().to_ascii_lowercase();
    let value = match normalized.as_str() {
        "1" | "true" => true,
        "0" | "false" => false,
        _ => {
            return Err(FriggError::InvalidInput(format!(
                "{name} must be one of: true,false,1,0 (received: {normalized})"
            )));
        }
    };
    Ok(Some(value))
}

fn build_semantic_embedding_records(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
    executor: &dyn SemanticRuntimeEmbeddingExecutor,
) -> FriggResult<Vec<SemanticChunkEmbeddingRecord>> {
    semantic_runtime
        .validate_startup(&credentials)
        .map_err(|err| {
            FriggError::InvalidInput(format!(
                "semantic runtime validation failed code={}: {err}",
                err.code()
            ))
        })?;

    let provider = semantic_runtime.provider.ok_or_else(|| {
        FriggError::Internal("semantic runtime provider missing after validation".to_owned())
    })?;
    let model = semantic_runtime.normalized_model().ok_or_else(|| {
        FriggError::Internal("semantic runtime model missing after validation".to_owned())
    })?;
    let chunks = build_semantic_chunk_candidates(
        repository_id,
        workspace_root,
        snapshot_id,
        current_manifest,
    )?;

    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let trace_id = deterministic_semantic_trace_id(repository_id, snapshot_id, provider, model);
    let mut output = Vec::with_capacity(chunks.len());
    for batch in chunks.chunks(SEMANTIC_EMBEDDING_BATCH_SIZE) {
        let batch_input = batch
            .iter()
            .map(|chunk| chunk.content_text.clone())
            .collect::<Vec<_>>();
        let vectors = execute_semantic_embedding_batch(
            executor,
            provider,
            model,
            batch_input,
            Some(trace_id.clone()),
        )?;
        if vectors.len() != batch.len() {
            return Err(FriggError::Internal(format!(
                "semantic embedding provider response length mismatch: expected {} vectors, received {}",
                batch.len(),
                vectors.len()
            )));
        }

        for (chunk, embedding) in batch.iter().zip(vectors.into_iter()) {
            if embedding.is_empty() {
                return Err(FriggError::Internal(format!(
                    "semantic embedding provider returned an empty vector for chunk_id={}",
                    chunk.chunk_id
                )));
            }
            if embedding.iter().any(|value| !value.is_finite()) {
                return Err(FriggError::Internal(format!(
                    "semantic embedding provider returned non-finite vector values for chunk_id={}",
                    chunk.chunk_id
                )));
            }

            output.push(SemanticChunkEmbeddingRecord {
                chunk_id: chunk.chunk_id.clone(),
                repository_id: chunk.repository_id.clone(),
                snapshot_id: chunk.snapshot_id.clone(),
                path: chunk.path.clone(),
                language: chunk.language.clone(),
                chunk_index: chunk.chunk_index,
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                provider: provider.as_str().to_owned(),
                model: model.to_owned(),
                trace_id: Some(trace_id.clone()),
                content_hash_blake3: chunk.content_hash_blake3.clone(),
                content_text: chunk.content_text.clone(),
                embedding,
            });
        }
    }

    output.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.chunk_index.cmp(&right.chunk_index))
            .then(left.chunk_id.cmp(&right.chunk_id))
    });
    Ok(output)
}

fn build_semantic_chunk_candidates(
    repository_id: &str,
    workspace_root: &Path,
    snapshot_id: &str,
    current_manifest: &[FileDigest],
) -> FriggResult<Vec<SemanticChunkCandidate>> {
    let mut output = Vec::new();

    for entry in current_manifest {
        let Some(language) = semantic_chunk_language_for_path(&entry.path) else {
            continue;
        };
        let source = match fs::read_to_string(&entry.path) {
            Ok(source) => source,
            Err(_) => continue,
        };
        let repository_relative_path =
            normalize_repository_relative_path(workspace_root, &entry.path)?;
        if repository_relative_path.starts_with("playbooks/") {
            continue;
        }
        let source = scrub_playbook_metadata_header(&source);
        output.extend(build_file_semantic_chunks(
            repository_id,
            snapshot_id,
            &repository_relative_path,
            language,
            source.as_ref(),
        ));
    }

    output.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.chunk_index.cmp(&right.chunk_index))
            .then(left.chunk_id.cmp(&right.chunk_id))
    });
    Ok(output)
}

fn build_file_semantic_chunks(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    language: &str,
    source: &str,
) -> Vec<SemanticChunkCandidate> {
    let mut chunks = Vec::new();
    let mut current_lines: Vec<&str> = Vec::new();
    let mut current_chars = 0usize;
    let mut start_line = 1usize;
    let mut chunk_index = 0usize;
    let markdown_chunking = language == "markdown";

    for (line_idx, line) in source.lines().enumerate() {
        let line_number = line_idx + 1;
        let markdown_heading_boundary =
            markdown_chunking && !current_lines.is_empty() && is_markdown_heading(line);
        let projected_chars = current_chars + line.len() + usize::from(!current_lines.is_empty());
        let should_flush = markdown_heading_boundary
            || (!current_lines.is_empty()
                && (current_lines.len() >= SEMANTIC_CHUNK_MAX_LINES
                    || projected_chars > SEMANTIC_CHUNK_MAX_CHARS));

        if should_flush {
            if let Some(chunk) = create_semantic_chunk_candidate(
                repository_id,
                snapshot_id,
                path,
                language,
                chunk_index,
                start_line,
                line_number.saturating_sub(1),
                &current_lines,
            ) {
                chunks.push(chunk);
                chunk_index += 1;
            }
            current_lines.clear();
            current_chars = 0;
            start_line = line_number;
        }

        current_chars += line.len() + usize::from(!current_lines.is_empty());
        current_lines.push(line);
    }

    if let Some(chunk) = create_semantic_chunk_candidate(
        repository_id,
        snapshot_id,
        path,
        language,
        chunk_index,
        start_line,
        source.lines().count().max(start_line),
        &current_lines,
    ) {
        chunks.push(chunk);
    }

    chunks
}

fn create_semantic_chunk_candidate(
    repository_id: &str,
    snapshot_id: &str,
    path: &str,
    language: &str,
    chunk_index: usize,
    start_line: usize,
    end_line: usize,
    lines: &[&str],
) -> Option<SemanticChunkCandidate> {
    if lines.is_empty() {
        return None;
    }
    let content_text = lines.join("\n");
    if content_text.trim().is_empty() {
        return None;
    }

    let mut content_hasher = Hasher::new();
    content_hasher.update(content_text.as_bytes());
    let content_hash_blake3 = content_hasher.finalize().to_hex().to_string();

    let mut chunk_id_hasher = Hasher::new();
    chunk_id_hasher.update(repository_id.as_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(path.as_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(chunk_index.to_string().as_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(start_line.to_string().as_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(end_line.to_string().as_bytes());
    chunk_id_hasher.update(&[0]);
    chunk_id_hasher.update(content_hash_blake3.as_bytes());

    Some(SemanticChunkCandidate {
        chunk_id: format!("chunk-{}", chunk_id_hasher.finalize().to_hex()),
        repository_id: repository_id.to_owned(),
        snapshot_id: snapshot_id.to_owned(),
        path: path.to_owned(),
        language: language.to_owned(),
        chunk_index,
        start_line,
        end_line,
        content_hash_blake3,
        content_text,
    })
}

pub(crate) fn semantic_chunk_language_for_path(path: &Path) -> Option<&'static str> {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("rs") => Some("rust"),
        Some("php") => Some("php"),
        Some("md" | "markdown") => Some("markdown"),
        Some("json") => Some("json"),
        Some("toml") => Some("toml"),
        Some("txt") => Some("text"),
        Some("yaml" | "yml") => Some("yaml"),
        _ => None,
    }
}

fn is_markdown_heading(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut heading_hashes = 0usize;
    for ch in trimmed.chars() {
        if ch == '#' {
            heading_hashes += 1;
            continue;
        }
        return heading_hashes > 0 && heading_hashes <= 6 && ch.is_ascii_whitespace();
    }

    false
}

fn normalize_repository_relative_path(workspace_root: &Path, path: &Path) -> FriggResult<String> {
    if let Ok(relative) = path.strip_prefix(workspace_root) {
        return Ok(relative.to_string_lossy().replace('\\', "/"));
    }

    let root_canonical = workspace_root.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize semantic workspace root '{}': {err}",
            workspace_root.display()
        ))
    })?;
    let path_canonical = path.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize semantic source path '{}': {err}",
            path.display()
        ))
    })?;
    let relative = path_canonical
        .strip_prefix(&root_canonical)
        .map_err(|err| {
            FriggError::Internal(format!(
                "semantic chunk path '{}' escapes workspace root '{}': {err}",
                path.display(),
                workspace_root.display()
            ))
        })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
}

fn deterministic_semantic_trace_id(
    repository_id: &str,
    snapshot_id: &str,
    provider: SemanticRuntimeProvider,
    model: &str,
) -> String {
    let mut hasher = Hasher::new();
    hasher.update(repository_id.as_bytes());
    hasher.update(&[0]);
    hasher.update(snapshot_id.as_bytes());
    hasher.update(&[0]);
    hasher.update(provider.as_str().as_bytes());
    hasher.update(&[0]);
    hasher.update(model.as_bytes());
    format!("trace-semantic-{}", hasher.finalize().to_hex())
}

impl ManifestBuilder {
    pub fn build(&self, root: &Path) -> FriggResult<Vec<FileDigest>> {
        if !root.exists() {
            return Err(FriggError::InvalidInput(format!(
                "index root does not exist: {}",
                root.display()
            )));
        }

        let mut out = Vec::new();
        let internal_storage_dir = root.join(".frigg");
        let walker = frigg_walk_builder(root, self.follow_symlinks).build();

        for dent in walker {
            let dent = match dent {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !dent.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = dent.path().to_path_buf();
            if path.starts_with(&internal_storage_dir) || hard_excluded_runtime_path(root, &path) {
                continue;
            }
            let mtime_ns = dent
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_to_unix_nanos);
            let (size_bytes, digest) = stream_file_blake3_digest(&path).map_err(FriggError::Io)?;

            out.push(FileDigest {
                path,
                size_bytes,
                mtime_ns,
                hash_blake3_hex: digest,
            });
        }
        out.sort_by(file_digest_order);
        out.dedup_by(|left, right| left.path == right.path);

        Ok(out)
    }

    pub fn build_with_diagnostics(&self, root: &Path) -> FriggResult<ManifestBuildOutput> {
        if !root.exists() {
            return Err(FriggError::InvalidInput(format!(
                "index root does not exist: {}",
                root.display()
            )));
        }

        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        let internal_storage_dir = root.join(".frigg");
        let walker = frigg_walk_builder(root, self.follow_symlinks).build();

        for dent in walker {
            let dent = match dent {
                Ok(entry) => entry,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: None,
                        kind: ManifestDiagnosticKind::Walk,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            if !dent.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = dent.path().to_path_buf();
            if path.starts_with(&internal_storage_dir) || hard_excluded_runtime_path(root, &path) {
                continue;
            }
            let mtime_ns = dent
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_to_unix_nanos);
            let (size_bytes, digest) = match stream_file_blake3_digest(&path) {
                Ok(result) => result,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: Some(path),
                        kind: ManifestDiagnosticKind::Read,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            entries.push(FileDigest {
                path,
                size_bytes,
                mtime_ns,
                hash_blake3_hex: digest,
            });
        }
        entries.sort_by(file_digest_order);
        entries.dedup_by(|left, right| left.path == right.path);
        diagnostics.sort_by(manifest_build_diagnostic_order);

        Ok(ManifestBuildOutput {
            entries,
            diagnostics,
        })
    }

    pub fn build_metadata_with_diagnostics(
        &self,
        root: &Path,
    ) -> FriggResult<ManifestMetadataBuildOutput> {
        if !root.exists() {
            return Err(FriggError::InvalidInput(format!(
                "index root does not exist: {}",
                root.display()
            )));
        }

        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        let internal_storage_dir = root.join(".frigg");
        let walker = frigg_walk_builder(root, self.follow_symlinks).build();

        for dent in walker {
            let dent = match dent {
                Ok(entry) => entry,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: None,
                        kind: ManifestDiagnosticKind::Walk,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            if !dent.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = dent.path().to_path_buf();
            if path.starts_with(&internal_storage_dir) || hard_excluded_runtime_path(root, &path) {
                continue;
            }
            let metadata = match dent.metadata() {
                Ok(metadata) => metadata,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: Some(path),
                        kind: ManifestDiagnosticKind::Read,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            let mtime_ns = metadata.modified().ok().and_then(system_time_to_unix_nanos);
            entries.push(FileMetadataDigest {
                path,
                size_bytes: metadata.len(),
                mtime_ns,
            });
        }
        entries.sort_by(file_metadata_digest_order);
        entries.dedup_by(|left, right| left.path == right.path);
        diagnostics.sort_by(manifest_build_diagnostic_order);

        Ok(ManifestMetadataBuildOutput {
            entries,
            diagnostics,
        })
    }
}

fn frigg_walk_builder(root: &Path, follow_symlinks: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(true)
        .require_git(false)
        .follow_links(follow_symlinks);
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

fn stream_file_blake3_digest(path: &Path) -> std::io::Result<(u64, String)> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes = 0_u64;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
        total_bytes = total_bytes.saturating_add(bytes_read as u64);
    }

    Ok((total_bytes, hasher.finalize().to_hex().to_string()))
}

pub fn diff(old: &[FileDigest], new: &[FileDigest]) -> ManifestDiff {
    let old_by_path = manifest_by_path(old);
    let new_by_path = manifest_by_path(new);

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    for (path, new_entry) in &new_by_path {
        match old_by_path.get(path) {
            None => added.push(new_entry.clone()),
            Some(old_entry) if !same_manifest_record(old_entry, new_entry) => {
                modified.push(new_entry.clone())
            }
            Some(_) => {}
        }
    }

    for (path, old_entry) in &old_by_path {
        if !new_by_path.contains_key(path) {
            deleted.push(old_entry.clone());
        }
    }

    ManifestDiff {
        added,
        modified,
        deleted,
    }
}

fn file_digest_to_manifest_entry(entry: &FileDigest) -> ManifestEntry {
    ManifestEntry {
        path: entry.path.to_string_lossy().to_string(),
        sha256: entry.hash_blake3_hex.clone(),
        size_bytes: entry.size_bytes,
        mtime_ns: entry.mtime_ns,
    }
}

fn manifest_entry_to_file_digest(entry: ManifestEntry) -> FileDigest {
    FileDigest {
        path: PathBuf::from(entry.path),
        size_bytes: entry.size_bytes,
        mtime_ns: entry.mtime_ns,
        hash_blake3_hex: entry.sha256,
    }
}

fn deterministic_snapshot_id(repository_id: &str, entries: &[FileDigest]) -> String {
    let mut ordered = entries.to_vec();
    ordered.sort_by(file_digest_order);

    let mut hasher = Hasher::new();
    hasher.update(repository_id.as_bytes());
    hasher.update(&[0]);

    for entry in ordered {
        hasher.update(entry.path.to_string_lossy().as_bytes());
        hasher.update(&[0]);
        hasher.update(entry.size_bytes.to_string().as_bytes());
        hasher.update(&[0]);
        match entry.mtime_ns {
            Some(mtime_ns) => {
                hasher.update(b"1");
                hasher.update(mtime_ns.to_string().as_bytes());
            }
            None => {
                hasher.update(b"0");
            }
        }
        hasher.update(&[0]);
        hasher.update(entry.hash_blake3_hex.as_bytes());
        hasher.update(&[0]);
    }

    format!("snapshot-{}", hasher.finalize().to_hex())
}

fn same_manifest_record(left: &FileDigest, right: &FileDigest) -> bool {
    left.size_bytes == right.size_bytes
        && left.mtime_ns == right.mtime_ns
        && left.hash_blake3_hex == right.hash_blake3_hex
}

fn manifest_by_path(entries: &[FileDigest]) -> BTreeMap<PathBuf, FileDigest> {
    let mut ordered = entries.to_vec();
    ordered.sort_by(file_digest_order);

    let mut by_path = BTreeMap::new();
    for entry in ordered {
        by_path.insert(entry.path.clone(), entry);
    }

    by_path
}

fn file_digest_order(left: &FileDigest, right: &FileDigest) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.size_bytes.cmp(&right.size_bytes))
        .then(left.mtime_ns.cmp(&right.mtime_ns))
        .then(left.hash_blake3_hex.cmp(&right.hash_blake3_hex))
}

fn file_metadata_digest_order(
    left: &FileMetadataDigest,
    right: &FileMetadataDigest,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.size_bytes.cmp(&right.size_bytes))
        .then(left.mtime_ns.cmp(&right.mtime_ns))
}

fn manifest_build_diagnostic_order(
    left: &ManifestBuildDiagnostic,
    right: &ManifestBuildDiagnostic,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.kind.cmp(&right.kind))
        .then(left.message.cmp(&right.message))
}

fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
    system_time
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
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
        FileDigest, HeuristicReferenceConfidence, HeuristicReferenceEvidence, ManifestBuilder,
        ManifestDiagnosticKind, ManifestStore, ReindexMode, RuntimeSemanticEmbeddingExecutor,
        SemanticRuntimeEmbeddingExecutor, SymbolKind, SymbolLanguage, build_file_semantic_chunks,
        build_semantic_chunk_candidates, diff, extract_symbols_for_paths,
        extract_symbols_from_source, file_digest_order, navigation_symbol_target_rank,
        register_symbol_definitions, reindex_repository, reindex_repository_with_semantic_executor,
        resolve_heuristic_references, search_structural_in_source,
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
            find_symbol(&symbols, SymbolKind::Function, "top_level", 2).is_some(),
            "expected php top-level function symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Class, "User", 3).is_some(),
            "expected php class symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Property, "$name", 4).is_some(),
            "expected php property symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Method, "save", 5).is_some(),
            "expected php method symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Constant, "LIMIT", 6).is_some(),
            "expected php constant symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::Interface, "Repo", 8).is_some(),
            "expected php interface symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::PhpTrait, "Logs", 9).is_some(),
            "expected php trait symbol"
        );
        assert!(
            find_symbol(&symbols, SymbolKind::PhpEnum, "Status", 10).is_some(),
            "expected php enum symbol"
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
         function top_level(): void {}\n\
         class User {\n\
             public string $name;\n\
             public function save(): void {}\n\
             const LIMIT = 10;\n\
         }\n\
         interface Repo { public function find(): ?User; }\n\
         trait Logs { public function logMessage(): void {} }\n\
         enum Status: string { case Active = 'active'; }\n"
    }
}
