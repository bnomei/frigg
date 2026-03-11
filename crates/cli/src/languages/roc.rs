use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::{node_field_text, node_name_text};

pub(crate) fn is_roc_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("roc"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "alias_type_def" | "nominal_type_def" | "opaque_type_def" => {
            roc_type_name(node, source).map(|name| (SymbolKind::TypeAlias, name))
        }
        "value_declaration" => {
            let name = value_declaration_name(node, source)?;
            let kind = roc_value_kind(node, source);
            Some((kind, name))
        }
        "var_declaration" => {
            node_field_text(node, source, "name").map(|name| (roc_value_kind(node, source), name))
        }
        _ => None,
    }
}

fn roc_type_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named() && child.kind() == "apply_type")
        .find_map(|child| {
            let mut child_cursor = child.walk();
            child
                .children(&mut child_cursor)
                .filter(|grandchild| grandchild.is_named())
                .find(|grandchild| grandchild.kind() == "concrete_type")
                .and_then(|grandchild| grandchild.utf8_text(source.as_bytes()).ok())
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(ToOwned::to_owned)
        })
}

fn roc_value_kind(node: Node<'_>, source: &str) -> SymbolKind {
    match node.child_by_field_name("body") {
        Some(body) => match body.utf8_text(source.as_bytes()).ok().map(str::trim_start) {
            Some(text) if text.starts_with('\\') || text.contains("->") => SymbolKind::Function,
            _ => SymbolKind::Const,
        },
        None => SymbolKind::Const,
    }
}

fn value_declaration_name(node: Node<'_>, source: &str) -> Option<String> {
    let mut cursor = node.walk();
    node.children(&mut cursor)
        .filter(|child| child.is_named())
        .find_map(|child| match child.kind() {
            "decl_left" => node_name_text(child, source).or_else(|| {
                let mut decl_cursor = child.walk();
                child
                    .children(&mut decl_cursor)
                    .filter(|grandchild| grandchild.is_named())
                    .find_map(|grandchild| {
                        node_name_text(grandchild, source).or_else(|| {
                            let mut pattern_cursor = grandchild.walk();
                            grandchild
                                .children(&mut pattern_cursor)
                                .filter(|pattern_child| pattern_child.is_named())
                                .find_map(|pattern_child| node_name_text(pattern_child, source))
                        })
                    })
            }),
            _ => None,
        })
}
