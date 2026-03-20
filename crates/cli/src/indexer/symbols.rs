use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{FriggError, FriggResult};
use crate::graph::{HeuristicConfidence, SymbolGraph, SymbolNode};
use crate::languages::{
    SymbolLanguage, collect_blade_symbols_from_source, parser_for_path, symbol_from_node,
    tree_sitter_language_for_path,
};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Query, QueryCursor, StreamingIterator, Tree};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxTreeInspectionNode {
    pub kind: String,
    pub named: bool,
    pub span: SourceSpan,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxTreeInspection {
    pub language: SymbolLanguage,
    pub focus: SyntaxTreeInspectionNode,
    pub ancestors: Vec<SyntaxTreeInspectionNode>,
    pub children: Vec<SyntaxTreeInspectionNode>,
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

pub fn inspect_syntax_tree_in_source(
    language: SymbolLanguage,
    path: &Path,
    source: &str,
    line: Option<usize>,
    column: Option<usize>,
    max_ancestors: usize,
    max_children: usize,
) -> FriggResult<SyntaxTreeInspection> {
    if line == Some(0) {
        return Err(FriggError::InvalidInput(
            "line must be greater than zero when provided".to_owned(),
        ));
    }
    if column == Some(0) {
        return Err(FriggError::InvalidInput(
            "column must be greater than zero when provided".to_owned(),
        ));
    }
    if line.is_none() != column.is_none() {
        return Err(FriggError::InvalidInput(
            "line and column must be provided together".to_owned(),
        ));
    }

    let mut parser = parser_for_path(language, path)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for syntax inspection: {}",
            path.display()
        ))
    })?;
    let root = tree.root_node();
    let focus_node = match (line, column) {
        (Some(line), Some(column)) => {
            let offset = byte_offset_for_line_column(source, line, column).ok_or_else(|| {
                FriggError::InvalidInput(format!(
                    "location {line}:{column} is outside file {}",
                    path.display()
                ))
            })?;
            focus_node_for_offset(root, offset)
        }
        _ => root,
    };

    let mut ancestors = Vec::new();
    let mut cursor = focus_node;
    while let Some(parent) = cursor.parent() {
        ancestors.push(syntax_tree_inspection_node(source, parent));
        cursor = parent;
        if ancestors.len() >= max_ancestors {
            break;
        }
    }

    let mut children = Vec::new();
    let mut child_cursor = focus_node.walk();
    for child in focus_node.children(&mut child_cursor) {
        children.push(syntax_tree_inspection_node(source, child));
        if children.len() >= max_children {
            break;
        }
    }

    Ok(SyntaxTreeInspection {
        language,
        focus: syntax_tree_inspection_node(source, focus_node),
        ancestors,
        children,
    })
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

pub(crate) fn byte_offset_for_line_column(
    source: &str,
    line: usize,
    column: usize,
) -> Option<usize> {
    if line == 0 || column == 0 {
        return None;
    }
    let bytes = source.as_bytes();
    let mut current_line = 1usize;
    let mut line_start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if current_line == line {
            let line_end = bytes[line_start..]
                .iter()
                .position(|candidate| *candidate == b'\n')
                .map(|offset| line_start + offset)
                .unwrap_or(bytes.len());
            let line_len = line_end.saturating_sub(line_start);
            let column_offset = column.saturating_sub(1).min(line_len);
            return Some(line_start + column_offset);
        }
        if *byte == b'\n' {
            current_line = current_line.saturating_add(1);
            line_start = index.saturating_add(1);
        }
    }
    if current_line == line {
        let line_len = bytes.len().saturating_sub(line_start);
        let column_offset = column.saturating_sub(1).min(line_len);
        return Some(line_start + column_offset);
    }
    None
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

fn focus_node_for_offset(root: Node<'_>, offset: usize) -> Node<'_> {
    let mut current = root;
    loop {
        let mut next_named = None;
        let mut next_any = None;
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.start_byte() > offset || child.end_byte() < offset {
                continue;
            }
            if next_any.is_none() {
                next_any = Some(child);
            }
            if child.is_named() {
                next_named = Some(child);
                break;
            }
        }
        let Some(next) = next_named.or(next_any) else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn syntax_tree_inspection_node(source: &str, node: Node<'_>) -> SyntaxTreeInspectionNode {
    SyntaxTreeInspectionNode {
        kind: node.kind().to_owned(),
        named: node.is_named(),
        span: source_span(node),
        excerpt: trim_syntax_excerpt(source, node.start_byte(), node.end_byte()),
    }
}

fn trim_syntax_excerpt(source: &str, start_byte: usize, end_byte: usize) -> String {
    const MAX_EXCERPT_CHARS: usize = 120;
    if start_byte >= end_byte || start_byte >= source.len() {
        return String::new();
    }
    let clamped_end = end_byte.min(source.len());
    let raw = String::from_utf8_lossy(&source.as_bytes()[start_byte..clamped_end]);
    let trimmed = raw.trim();
    if trimmed.chars().count() <= MAX_EXCERPT_CHARS {
        return trimmed.to_owned();
    }
    let mut excerpt = trimmed.chars().take(MAX_EXCERPT_CHARS).collect::<String>();
    excerpt.push_str("...");
    excerpt
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
