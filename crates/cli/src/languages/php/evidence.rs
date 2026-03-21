use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use tree_sitter::Node;

use crate::domain::{FriggError, FriggResult};
use crate::graph::RelationKind;
use crate::indexer::{SourceSpan, SymbolDefinition, SymbolKind, source_span};

use super::super::registry::{SymbolLanguage, parser_for_language};
use super::resolution::PhpNameResolutionContext;
use super::symbol_from_node;

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
    let context = super::php_name_resolution_context_from_root(source, tree.root_node());
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

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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
