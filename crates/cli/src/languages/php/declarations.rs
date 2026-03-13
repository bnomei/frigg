use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use crate::domain::{FriggError, FriggResult};
use crate::graph::RelationKind;
use crate::indexer::{SymbolDefinition, SymbolKind, push_symbol_definition, source_span};

use super::super::registry::{SymbolLanguage, parser_for_language};
use super::evidence::{PhpSourceEvidence, extract_source_evidence_from_source};
use super::resolution::{
    PhpSymbolLookup, php_relation_targets_symbol_name, resolve_php_declaration_relation_indices,
};
use super::symbol_from_node;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PhpDeclarationRelation {
    pub(crate) source_kind: SymbolKind,
    pub(crate) source_name: String,
    pub(crate) source_line: usize,
    pub(crate) target_name: String,
    pub(crate) relation: RelationKind,
}

#[derive(Debug, Clone)]
pub(crate) struct PhpGraphSourceAnalysis {
    pub(crate) symbols: Vec<SymbolDefinition>,
    pub(crate) declaration_relations: Vec<PhpDeclarationRelation>,
    pub(crate) source_evidence: PhpSourceEvidence,
}

pub(crate) fn symbol_indices_by_name(symbols: &[SymbolDefinition]) -> BTreeMap<String, Vec<usize>> {
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

pub(crate) fn symbol_indices_by_lower_name(
    symbols: &[SymbolDefinition],
) -> BTreeMap<String, Vec<usize>> {
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

pub(crate) fn extract_declaration_relations_from_source(
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
    collect_declaration_relations(source, tree.root_node(), &mut relations);
    relations.sort();
    relations.dedup();
    Ok(relations)
}

pub(crate) fn declaration_relation_edges_for_file(
    relative_path: &str,
    absolute_path: &Path,
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    provided_symbol_indices_by_name: Option<&BTreeMap<String, Vec<usize>>>,
    provided_symbol_indices_by_lower_name: Option<&BTreeMap<String, Vec<usize>>>,
) -> FriggResult<Vec<(usize, usize, RelationKind)>> {
    if SymbolLanguage::from_path(absolute_path) != Some(SymbolLanguage::Php) {
        return Ok(Vec::new());
    }

    let source = fs::read_to_string(absolute_path).map_err(FriggError::Io)?;
    declaration_relation_edges_for_source(
        relative_path,
        absolute_path,
        &source,
        symbols,
        symbols_by_relative_path,
        provided_symbol_indices_by_name,
        provided_symbol_indices_by_lower_name,
    )
}

pub(crate) fn declaration_relation_edges_for_source(
    relative_path: &str,
    absolute_path: &Path,
    source: &str,
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    provided_symbol_indices_by_name: Option<&BTreeMap<String, Vec<usize>>>,
    provided_symbol_indices_by_lower_name: Option<&BTreeMap<String, Vec<usize>>>,
) -> FriggResult<Vec<(usize, usize, RelationKind)>> {
    if SymbolLanguage::from_path(absolute_path) != Some(SymbolLanguage::Php) {
        return Ok(Vec::new());
    }

    let relations = extract_declaration_relations_from_source(absolute_path, source)?;
    Ok(declaration_relation_edges_for_relations(
        relative_path,
        symbols,
        symbols_by_relative_path,
        provided_symbol_indices_by_name,
        provided_symbol_indices_by_lower_name,
        &relations,
    ))
}

pub(crate) fn declaration_relation_edges_for_relations(
    relative_path: &str,
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    provided_symbol_indices_by_name: Option<&BTreeMap<String, Vec<usize>>>,
    provided_symbol_indices_by_lower_name: Option<&BTreeMap<String, Vec<usize>>>,
    relations: &[PhpDeclarationRelation],
) -> Vec<(usize, usize, RelationKind)> {
    let owned_name_index;
    let name_index = match provided_symbol_indices_by_name {
        Some(index) => index,
        None => {
            owned_name_index = self::symbol_indices_by_name(symbols);
            &owned_name_index
        }
    };
    let owned_lower_name_index;
    let lower_name_index = match provided_symbol_indices_by_lower_name {
        Some(index) => index,
        None => {
            owned_lower_name_index = self::symbol_indices_by_lower_name(symbols);
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
            resolve_php_declaration_relation_indices(&lookup, relative_path, relation)
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
    edges
}

pub(crate) fn heuristic_implementation_candidates_for_target(
    target_symbol: &SymbolDefinition,
    candidate_files: &[(String, PathBuf)],
    symbols: &[SymbolDefinition],
    symbols_by_relative_path: &BTreeMap<String, Vec<usize>>,
    provided_symbol_indices_by_name: Option<&BTreeMap<String, Vec<usize>>>,
    provided_symbol_indices_by_lower_name: Option<&BTreeMap<String, Vec<usize>>>,
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
    let name_index = match provided_symbol_indices_by_name {
        Some(index) => index,
        None => {
            owned_name_index = self::symbol_indices_by_name(symbols);
            &owned_name_index
        }
    };
    let owned_lower_name_index;
    let lower_name_index = match provided_symbol_indices_by_lower_name {
        Some(index) => index,
        None => {
            owned_lower_name_index = self::symbol_indices_by_lower_name(symbols);
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
        let Ok(relations) = extract_declaration_relations_from_source(absolute_path, &source)
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

pub(crate) fn extract_graph_analysis_from_source(
    path: &Path,
    source: &str,
) -> FriggResult<PhpGraphSourceAnalysis> {
    let mut parser = parser_for_language(SymbolLanguage::Php)?;
    let tree = parser.parse(source, None).ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to parse source for php graph analysis: {}",
            path.display()
        ))
    })?;
    let root = tree.root_node();

    let mut symbols = Vec::new();
    collect_symbols_from_root(path, source, root, &mut symbols);
    symbols.sort_by(symbol_definition_order);

    let mut declaration_relations = Vec::new();
    collect_declaration_relations(source, root, &mut declaration_relations);
    declaration_relations.sort();
    declaration_relations.dedup();

    let source_evidence = extract_source_evidence_from_source(path, source, &symbols)?;
    Ok(PhpGraphSourceAnalysis {
        symbols,
        declaration_relations,
        source_evidence,
    })
}

fn collect_symbols_from_root(
    path: &Path,
    source: &str,
    node: Node<'_>,
    symbols: &mut Vec<SymbolDefinition>,
) {
    if let Some((kind, name)) = symbol_from_node(source, node) {
        push_symbol_definition(
            symbols,
            SymbolLanguage::Php,
            kind,
            path,
            &name,
            source_span(node),
        );
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        collect_symbols_from_root(path, source, child, symbols);
    }
}

fn collect_declaration_relations(
    source: &str,
    node: Node<'_>,
    relations: &mut Vec<PhpDeclarationRelation>,
) {
    if let Some((source_kind, source_name)) = symbol_from_node(source, node) {
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
        collect_declaration_relations(source, child, relations);
    }
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
