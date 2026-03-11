use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::node_name_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KotlinScope {
    Module,
    ClassLike,
    Function,
}

pub(crate) fn is_kotlin_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "kt" | "kts"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "package_header" => declaration_name_text(node, source).map(|name| (SymbolKind::Module, name)),
        "class_declaration" => {
            declaration_name_text(node, source).map(|name| (class_kind(source, node), name))
        }
        "interface_declaration" => {
            declaration_name_text(node, source).map(|name| (SymbolKind::Interface, name))
        }
        "object_declaration" | "companion_object" => {
            declaration_name_text(node, source).map(|name| (SymbolKind::Class, name))
        }
        "function_declaration" => match enclosing_kotlin_scope(node) {
            KotlinScope::Module => {
                declaration_name_text(node, source).map(|name| (SymbolKind::Function, name))
            }
            KotlinScope::ClassLike => {
                declaration_name_text(node, source).map(|name| (SymbolKind::Method, name))
            }
            KotlinScope::Function => None,
        },
        "property_declaration" => match enclosing_kotlin_scope(node) {
            KotlinScope::Function => None,
            KotlinScope::Module | KotlinScope::ClassLike => {
                property_name_text(node, source).map(|name| (SymbolKind::Property, name))
            }
        },
        "type_alias" => declaration_name_text(node, source).map(|name| (SymbolKind::TypeAlias, name)),
        _ => None,
    }
}

fn enclosing_kotlin_scope(node: Node<'_>) -> KotlinScope {
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "function_declaration" | "anonymous_function" | "lambda_literal" => {
                return KotlinScope::Function;
            }
            "class_declaration" | "interface_declaration" | "object_declaration"
            | "companion_object" => {
                return KotlinScope::ClassLike;
            }
            _ => {}
        }
        current = parent.parent();
    }
    KotlinScope::Module
}

fn class_kind(source: &str, node: Node<'_>) -> SymbolKind {
    let mut body_cursor = node.walk();
    if node
        .children(&mut body_cursor)
        .filter(|child| child.is_named())
        .any(|child| child.kind() == "enum_class_body")
    {
        SymbolKind::Enum
    } else {
        let mut modifier_cursor = node.walk();
        if node
            .children(&mut modifier_cursor)
            .filter(|child| child.is_named() && child.kind() == "modifiers")
            .filter_map(|child| child.utf8_text(source.as_bytes()).ok())
            .any(|text| text.split_whitespace().any(|token| token == "enum"))
        {
            SymbolKind::Enum
        } else {
            SymbolKind::Class
        }
    }
}

fn declaration_name_text(node: Node<'_>, source: &str) -> Option<String> {
    node_name_text(node, source).or_else(|| {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .filter(|child| child.is_named())
            .find(|child| matches!(child.kind(), "type_identifier" | "simple_identifier"))
            .and_then(|child| child.utf8_text(source.as_bytes()).ok())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn property_name_text(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .find_map(|child| match child.kind() {
            "variable_declaration" => declaration_name_text(child, source),
            "simple_identifier" => child
                .utf8_text(source.as_bytes())
                .ok()
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned),
            _ => None,
        })
}
