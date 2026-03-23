use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::{node_field_text, node_name_text};

pub(crate) fn is_java_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("java"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "package_declaration" => {
            package_name_text(node, source).map(|name| (SymbolKind::Module, name))
        }
        "class_declaration" | "record_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::Class, name))
        }
        "interface_declaration" | "annotation_type_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::Interface, name))
        }
        "enum_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Enum, name)),
        "enum_constant" => node_name_text(node, source).map(|name| (SymbolKind::EnumCase, name)),
        "constructor_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::Method, name))
        }
        "compact_constructor_declaration" => {
            enclosing_type_name(node, source).map(|name| (SymbolKind::Method, name))
        }
        "method_declaration" | "annotation_type_element_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::Method, name))
        }
        "field_declaration" => {
            first_declarator_name(node, source).map(|name| (field_kind(node, source), name))
        }
        "constant_declaration" => {
            first_declarator_name(node, source).map(|name| (SymbolKind::Constant, name))
        }
        _ => None,
    }
}

fn package_name_text(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .find(|child| matches!(child.kind(), "identifier" | "scoped_identifier"))
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned)
}

fn first_declarator_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named() && child.kind() == "variable_declarator")
        .find_map(|child| node_field_text(child, source, "name"))
}

fn field_kind(node: Node<'_>, source: &str) -> SymbolKind {
    let mut cursor = node.walk();
    let modifiers = node
        .children(&mut cursor)
        .find(|child| child.is_named() && child.kind() == "modifiers")
        .and_then(|child| child.utf8_text(source.as_bytes()).ok())
        .unwrap_or_default();
    let has_static = modifiers.split_whitespace().any(|token| token == "static");
    let has_final = modifiers.split_whitespace().any(|token| token == "final");
    if has_static && has_final {
        SymbolKind::Constant
    } else {
        SymbolKind::Property
    }
}

fn enclosing_type_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if matches!(
            parent.kind(),
            "class_declaration"
                | "record_declaration"
                | "interface_declaration"
                | "enum_declaration"
        ) {
            return node_name_text(parent, source);
        }
        current = parent.parent();
    }
    None
}
