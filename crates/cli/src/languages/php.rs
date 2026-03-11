use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use tree_sitter::Node;

use crate::domain::{FriggError, FriggResult};
use crate::graph::RelationKind;
use crate::indexer::{
    SourceSpan, SymbolDefinition, SymbolKind, push_symbol_definition, source_span,
};

use super::registry::{SymbolLanguage, node_field_text, node_name_text, parser_for_language};

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "namespace_definition" => {
            node_name_text(node, source).map(|name| (SymbolKind::Module, name))
        }
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
        "enum_case" => node_name_text(node, source).map(|name| (SymbolKind::EnumCase, name)),
        "method_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Method, name)),
        "property_element" => node_name_text(node, source).map(|name| (SymbolKind::Property, name)),
        "const_element" => node_name_text(node, source).map(|name| (SymbolKind::Constant, name)),
        _ => None,
    }
}

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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PhpNameResolutionContext {
    pub(crate) namespace: Option<String>,
    pub(crate) class_like_aliases: BTreeMap<String, String>,
}

impl PhpNameResolutionContext {
    pub(crate) fn resolve_class_like_name(
        &self,
        raw_name: &str,
        current_class_canonical_name: Option<&str>,
    ) -> Option<String> {
        let trimmed = raw_name.trim();
        if trimmed.is_empty() {
            return None;
        }

        let optional_trimmed = trimmed.strip_prefix('?').unwrap_or(trimmed);
        let normalized = optional_trimmed.trim_start_matches('\\').trim();
        if normalized.is_empty() {
            return None;
        }

        match normalized.to_ascii_lowercase().as_str() {
            "self" | "static" => {
                return current_class_canonical_name
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned);
            }
            "parent" => return None,
            _ if php_is_builtin_type(normalized) => return None,
            _ => {}
        }

        if optional_trimmed.starts_with('\\') {
            return Some(normalized.to_owned());
        }

        if let Some(relative) = normalized
            .strip_prefix("namespace\\")
            .or_else(|| normalized.strip_prefix("namespace/"))
        {
            return self.namespace.as_ref().map(|namespace| {
                if relative.is_empty() {
                    namespace.clone()
                } else {
                    format!("{namespace}\\{relative}")
                }
            });
        }

        let mut segments = normalized.splitn(2, '\\');
        let first_segment = segments.next().unwrap_or_default().trim();
        let remainder = segments.next().map(str::trim).unwrap_or_default();
        if first_segment.is_empty() {
            return None;
        }

        if let Some(alias_target) = self
            .class_like_aliases
            .get(&first_segment.to_ascii_lowercase())
            .filter(|target| !target.trim().is_empty())
        {
            return Some(if remainder.is_empty() {
                alias_target.clone()
            } else {
                format!("{alias_target}\\{remainder}")
            });
        }

        if let Some(namespace) = &self.namespace {
            return Some(format!("{namespace}\\{normalized}"));
        }

        Some(normalized.to_owned())
    }
}

pub(crate) fn php_name_resolution_context_from_root(
    source: &str,
    root: Node<'_>,
) -> PhpNameResolutionContext {
    let mut context = PhpNameResolutionContext::default();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor).filter(|child| child.is_named()) {
        match child.kind() {
            "namespace_definition" => {
                context.namespace = node_field_text(child, source, "name");
                if let Some(body) = child.child_by_field_name("body") {
                    let mut body_cursor = body.walk();
                    for body_child in body
                        .children(&mut body_cursor)
                        .filter(|node| node.is_named())
                    {
                        if body_child.kind() == "namespace_use_declaration" {
                            collect_php_namespace_use_declaration(source, body_child, &mut context);
                        }
                    }
                }
            }
            "namespace_use_declaration" => {
                collect_php_namespace_use_declaration(source, child, &mut context);
            }
            _ => {}
        }
    }
    context
}

fn php_is_builtin_type(raw_name: &str) -> bool {
    matches!(
        raw_name.trim().to_ascii_lowercase().as_str(),
        "array"
            | "bool"
            | "callable"
            | "false"
            | "float"
            | "int"
            | "iterable"
            | "mixed"
            | "never"
            | "null"
            | "object"
            | "resource"
            | "string"
            | "true"
            | "void"
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn php_class_like_name_candidates(
    context: Option<&PhpNameResolutionContext>,
    raw_target_name: &str,
    current_class_canonical_name: Option<&str>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(canonical) = context.and_then(|context| {
        context.resolve_class_like_name(raw_target_name, current_class_canonical_name)
    }) {
        candidates.push(canonical);
    }
    for candidate in php_reference_name_candidates(raw_target_name) {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn collect_php_namespace_use_declaration(
    source: &str,
    declaration: Node<'_>,
    context: &mut PhpNameResolutionContext,
) {
    let declaration_type = declaration
        .child_by_field_name("type")
        .map(|node| node.kind());
    let body_id = declaration
        .child_by_field_name("body")
        .map(|node| node.id());
    let grouped_prefix = if body_id.is_some() {
        let mut cursor = declaration.walk();
        declaration
            .children(&mut cursor)
            .filter(|child| child.is_named())
            .find(|child| child.kind() == "namespace_name")
            .and_then(|child| child.utf8_text(source.as_bytes()).ok())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    } else {
        None
    };

    let mut cursor = declaration.walk();
    for child in declaration
        .children(&mut cursor)
        .filter(|child| child.is_named())
    {
        if Some(child.id()) == body_id {
            let mut group_cursor = child.walk();
            for clause in child
                .children(&mut group_cursor)
                .filter(|node| node.is_named())
            {
                if clause.kind() == "namespace_use_clause" {
                    collect_php_namespace_use_clause(
                        source,
                        clause,
                        grouped_prefix.as_deref(),
                        declaration_type,
                        context,
                    );
                }
            }
            continue;
        }
        if child.kind() == "namespace_use_clause" {
            collect_php_namespace_use_clause(source, child, None, declaration_type, context);
        }
    }
}

fn collect_php_namespace_use_clause(
    source: &str,
    clause: Node<'_>,
    grouped_prefix: Option<&str>,
    declaration_type: Option<&str>,
    context: &mut PhpNameResolutionContext,
) {
    let clause_type = clause
        .child_by_field_name("type")
        .map(|node| node.kind())
        .or(declaration_type);
    if matches!(clause_type, Some("function" | "const")) {
        return;
    }

    let alias_node = clause.child_by_field_name("alias");
    let alias_node_id = alias_node.as_ref().map(Node::id);
    let alias = alias_node
        .and_then(|node| node.utf8_text(source.as_bytes()).ok())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);

    let mut referenced_name = None;
    let mut cursor = clause.walk();
    for child in clause
        .children(&mut cursor)
        .filter(|child| child.is_named())
    {
        if Some(child.id()) == alias_node_id {
            continue;
        }
        referenced_name = child
            .utf8_text(source.as_bytes())
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned);
        if referenced_name.is_some() {
            break;
        }
    }

    let Some(referenced_name) = referenced_name else {
        return;
    };
    let canonical = grouped_prefix
        .filter(|prefix| !prefix.is_empty())
        .map(|prefix| format!("{prefix}\\{referenced_name}"))
        .unwrap_or_else(|| referenced_name.clone());
    let alias = alias.unwrap_or_else(|| {
        referenced_name
            .rsplit('\\')
            .next()
            .unwrap_or(&referenced_name)
            .to_owned()
    });
    if alias.trim().is_empty() || canonical.trim().is_empty() {
        return;
    }

    context
        .class_like_aliases
        .insert(alias.to_ascii_lowercase(), canonical);
}

pub(crate) struct PhpSymbolLookup<'a> {
    pub(crate) symbols: &'a [SymbolDefinition],
    pub(crate) symbols_by_relative_path: &'a BTreeMap<String, Vec<usize>>,
    pub(crate) symbol_indices_by_name: &'a BTreeMap<String, Vec<usize>>,
    pub(crate) symbol_indices_by_lower_name: &'a BTreeMap<String, Vec<usize>>,
}

pub(crate) fn resolve_php_declaration_relation_indices(
    lookup: &PhpSymbolLookup<'_>,
    relative_path: &str,
    relation: &PhpDeclarationRelation,
) -> Option<(usize, usize)> {
    let source_symbol_index = resolve_php_relation_source_symbol(lookup, relative_path, relation)?;
    let target_symbol_index = resolve_php_relation_target_symbol(lookup, relation)?;
    let source_symbol = &lookup.symbols[source_symbol_index];
    let target_symbol = &lookup.symbols[target_symbol_index];
    if source_symbol.stable_id == target_symbol.stable_id {
        return None;
    }
    Some((source_symbol_index, target_symbol_index))
}

pub(crate) fn php_relation_targets_symbol_name(
    relation: &PhpDeclarationRelation,
    target_symbol: &SymbolDefinition,
) -> bool {
    let target_name = target_symbol.name.trim();
    if target_name.is_empty() {
        return false;
    }
    (match target_symbol.kind {
        SymbolKind::Interface => matches!(
            relation.relation,
            RelationKind::Implements | RelationKind::Extends
        ),
        SymbolKind::Class => relation.relation == RelationKind::Extends,
        _ => false,
    }) && php_reference_name_candidates(&relation.target_name)
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(target_name))
}

fn resolve_php_relation_source_symbol(
    lookup: &PhpSymbolLookup<'_>,
    relative_path: &str,
    relation: &PhpDeclarationRelation,
) -> Option<usize> {
    let matches = lookup
        .symbols_by_relative_path
        .get(relative_path)?
        .iter()
        .copied()
        .filter(|index| {
            let symbol = &lookup.symbols[*index];
            symbol.language == SymbolLanguage::Php
                && symbol.kind == relation.source_kind
                && symbol.line == relation.source_line
                && symbol.name == relation.source_name
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.first().copied()
    } else {
        None
    }
}

fn resolve_php_relation_target_symbol(
    lookup: &PhpSymbolLookup<'_>,
    relation: &PhpDeclarationRelation,
) -> Option<usize> {
    let allowed_kinds = php_allowed_target_kinds(relation.source_kind, relation.relation);
    if allowed_kinds.is_empty() {
        return None;
    }

    let candidates = php_reference_name_candidates(&relation.target_name);
    if candidates.is_empty() {
        return None;
    }

    let mut exact_matches = BTreeSet::new();
    for candidate in &candidates {
        if let Some(indices) = lookup.symbol_indices_by_name.get(candidate) {
            for index in indices {
                let symbol = &lookup.symbols[*index];
                if symbol.language == SymbolLanguage::Php && allowed_kinds.contains(&symbol.kind) {
                    exact_matches.insert(*index);
                }
            }
        }
    }
    if exact_matches.len() == 1 {
        return exact_matches.iter().next().copied();
    }
    if !exact_matches.is_empty() {
        return None;
    }

    let mut case_insensitive_matches = BTreeSet::new();
    for candidate in &candidates {
        let lower = candidate.to_ascii_lowercase();
        if let Some(indices) = lookup.symbol_indices_by_lower_name.get(&lower) {
            for index in indices {
                let symbol = &lookup.symbols[*index];
                if symbol.language == SymbolLanguage::Php && allowed_kinds.contains(&symbol.kind) {
                    case_insensitive_matches.insert(*index);
                }
            }
        }
    }
    if case_insensitive_matches.len() == 1 {
        case_insensitive_matches.iter().next().copied()
    } else {
        None
    }
}

fn php_allowed_target_kinds(
    source_kind: SymbolKind,
    relation: RelationKind,
) -> &'static [SymbolKind] {
    const CLASS_ONLY: &[SymbolKind] = &[SymbolKind::Class];
    const INTERFACE_ONLY: &[SymbolKind] = &[SymbolKind::Interface];
    const NONE: &[SymbolKind] = &[];

    match relation {
        RelationKind::Implements => INTERFACE_ONLY,
        RelationKind::Extends => match source_kind {
            SymbolKind::Class => CLASS_ONLY,
            SymbolKind::Interface => INTERFACE_ONLY,
            _ => NONE,
        },
        _ => NONE,
    }
}

fn php_reference_name_candidates(raw_target_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let trimmed = raw_target_name.trim().trim_start_matches('\\');
    if trimmed.is_empty() {
        return candidates;
    }

    for candidate in [
        Some(trimmed),
        trimmed.rsplit('\\').next(),
        trimmed.rsplit(':').next(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|candidate| !candidate.is_empty())
    {
        if matches!(
            candidate.to_ascii_lowercase().as_str(),
            "self" | "static" | "parent"
        ) {
            continue;
        }
        if !candidates.iter().any(|existing| existing == candidate) {
            candidates.push(candidate.to_owned());
        }
    }

    candidates
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PhpTypeEvidenceKind {
    Parameter,
    Return,
    Property,
    PromotedProperty,
    Catch,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PhpTypeEvidence {
    pub(crate) owner_symbol_id: Option<String>,
    pub(crate) kind: PhpTypeEvidenceKind,
    pub(crate) target_canonical_name: String,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum PhpTargetEvidenceKind {
    Attribute,
    ClassString,
    Instantiation,
    CallableLiteral,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PhpTargetEvidence {
    pub(crate) owner_symbol_id: Option<String>,
    pub(crate) kind: PhpTargetEvidenceKind,
    pub(crate) target_canonical_name: String,
    pub(crate) target_member_name: Option<String>,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PhpLiteralEvidence {
    pub(crate) owner_symbol_id: Option<String>,
    pub(crate) array_keys: Vec<String>,
    pub(crate) named_arguments: Vec<String>,
    pub(crate) line: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PhpSourceEvidence {
    pub(crate) canonical_names_by_stable_id: BTreeMap<String, String>,
    pub(crate) type_evidence: Vec<PhpTypeEvidence>,
    pub(crate) target_evidence: Vec<PhpTargetEvidence>,
    pub(crate) literal_evidence: Vec<PhpLiteralEvidence>,
}

pub(crate) fn extract_source_evidence_from_source(
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
    collect_source_evidence(
        source,
        tree.root_node(),
        file_symbols,
        &context,
        context.namespace.as_deref(),
        None,
        None,
        &mut evidence,
    );
    normalize_source_evidence(&mut evidence);
    Ok(evidence)
}

pub(crate) fn resolve_target_evidence_edges(
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
        let Some(target_symbol_index) = resolve_target_symbol_index(
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

fn collect_source_evidence(
    source: &str,
    node: Node<'_>,
    file_symbols: &[SymbolDefinition],
    context: &PhpNameResolutionContext,
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
            if let Some((kind, name)) = symbol_from_node(source, node) {
                let canonical_name = namespace_qualified_name(next_namespace.as_deref(), &name);
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
            if let Some((kind, name)) = symbol_from_node(source, node) {
                let canonical_name = namespace_qualified_name(next_namespace.as_deref(), &name);
                if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                    evidence
                        .canonical_names_by_stable_id
                        .entry(symbol.stable_id.clone())
                        .or_insert_with(|| canonical_name.clone());
                    next_owner_symbol_id = Some(symbol.stable_id.clone());
                    if let Some(parameters) = node.child_by_field_name("parameters") {
                        collect_parameter_type_evidence(
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
                        collect_type_evidence(
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
            if let Some((kind, name)) = symbol_from_node(source, node) {
                if let Some(class_name) = next_class_canonical_name.as_deref() {
                    let canonical_name = format!("{class_name}::{name}");
                    if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                        evidence
                            .canonical_names_by_stable_id
                            .entry(symbol.stable_id.clone())
                            .or_insert_with(|| canonical_name.clone());
                        next_owner_symbol_id = Some(symbol.stable_id.clone());
                        if let Some(parameters) = node.child_by_field_name("parameters") {
                            collect_parameter_type_evidence(
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
                            collect_type_evidence(
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
                    let Some((kind, name)) = symbol_from_node(source, child) else {
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
                    collect_type_evidence(
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
            if let Some((kind, name)) = symbol_from_node(source, node) {
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
            if let Some((kind, name)) = symbol_from_node(source, node) {
                if let Some(symbol) = find_symbol_for_node(file_symbols, kind, &name, node) {
                    let canonical_name =
                        if let Some(class_name) = next_class_canonical_name.as_deref() {
                            format!("{class_name}::{name}")
                        } else {
                            namespace_qualified_name(next_namespace.as_deref(), &name)
                        };
                    evidence
                        .canonical_names_by_stable_id
                        .entry(symbol.stable_id.clone())
                        .or_insert(canonical_name);
                }
            }
        }
        "enum_case" => {
            if let Some((kind, name)) = symbol_from_node(source, node) {
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
                collect_type_evidence(
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
            if let Some(target_name) = attribute_target_name(source, node).and_then(|raw_name| {
                context.resolve_class_like_name(
                    raw_name.as_str(),
                    next_class_canonical_name.as_deref(),
                )
            }) {
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
            if let Some(target_name) = class_string_target_name(
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
            if let Some(target_name) = instantiation_target_name(
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
            if let Some((target_canonical_name, target_member_name)) =
                callable_literal_target(source, node, context, next_class_canonical_name.as_deref())
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
                    kind: PhpTargetEvidenceKind::CallableLiteral,
                    target_canonical_name,
                    target_member_name: Some(target_member_name),
                    line: source_span(node).start_line,
                });
            }
            if let Some(array_keys) = literal_array_keys(source, node) {
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
            if let Some(named_arguments) = named_argument_keys(source, node) {
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
        collect_source_evidence(
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

fn collect_parameter_type_evidence(
    source: &str,
    parameters: Node<'_>,
    file_symbols: &[SymbolDefinition],
    context: &PhpNameResolutionContext,
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
            collect_type_evidence(
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
                    if let Some(target_name) =
                        attribute_target_name(source, attribute).and_then(|raw_name| {
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

fn collect_type_evidence(
    source: &str,
    type_node: Node<'_>,
    context: &PhpNameResolutionContext,
    current_class_canonical_name: Option<&str>,
    owner_symbol_id: Option<&str>,
    kind: PhpTypeEvidenceKind,
    line: usize,
    output: &mut Vec<PhpTypeEvidence>,
) {
    let mut targets = BTreeSet::new();
    collect_type_targets(
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

fn collect_type_targets(
    source: &str,
    node: Node<'_>,
    context: &PhpNameResolutionContext,
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
                collect_type_targets(
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

fn namespace_qualified_name(namespace: Option<&str>, short_name: &str) -> String {
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

fn attribute_target_name(source: &str, node: Node<'_>) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .find(|child| matches!(child.kind(), "name" | "qualified_name" | "relative_name"))
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn class_string_target_name(
    source: &str,
    node: Node<'_>,
    context: &PhpNameResolutionContext,
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

fn instantiation_target_name(
    source: &str,
    node: Node<'_>,
    context: &PhpNameResolutionContext,
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

fn callable_literal_target(
    source: &str,
    node: Node<'_>,
    context: &PhpNameResolutionContext,
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
        class_string_target_name(source, first, context, current_class_canonical_name)?;
    let target_member_name = string_literal_value(source, second)?;
    Some((target_name, target_member_name))
}

fn string_literal_value(source: &str, node: Node<'_>) -> Option<String> {
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

fn literal_array_keys(source: &str, node: Node<'_>) -> Option<Vec<String>> {
    let mut keys = BTreeSet::new();
    for initializer in named_children(node)
        .into_iter()
        .filter(|child| child.kind() == "array_element_initializer")
    {
        let children = named_children(initializer);
        if children.len() < 2 {
            continue;
        }
        if let Some(key) = literal_key_text(source, children[0]) {
            keys.insert(key);
        }
    }
    (!keys.is_empty()).then(|| keys.into_iter().collect())
}

fn literal_key_text(source: &str, node: Node<'_>) -> Option<String> {
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

fn named_argument_keys(source: &str, node: Node<'_>) -> Option<Vec<String>> {
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

fn resolve_target_symbol_index(
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

fn normalize_source_evidence(evidence: &mut PhpSourceEvidence) {
    evidence.type_evidence.sort();
    evidence.type_evidence.dedup();
    evidence.target_evidence.sort();
    evidence.target_evidence.dedup();
    evidence.literal_evidence.sort();
    evidence.literal_evidence.dedup();
}

fn named_children(node: Node<'_>) -> Vec<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .collect()
}
