use std::collections::{BTreeMap, BTreeSet};

use tree_sitter::Node;

use crate::graph::RelationKind;
use crate::indexer::{SymbolDefinition, SymbolKind};

use super::super::registry::{SymbolLanguage, node_field_text};
use super::declarations::PhpDeclarationRelation;

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

pub(super) fn php_reference_name_candidates(raw_target_name: &str) -> Vec<String> {
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
