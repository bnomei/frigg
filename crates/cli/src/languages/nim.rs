use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::node_field_text;

pub(crate) fn is_nim_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "nim" | "nims"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "proc_declaration"
        | "func_declaration"
        | "iterator_declaration"
        | "template_declaration"
        | "macro_declaration"
        | "converter_declaration" => {
            node_field_text(node, source, "name").map(|name| (SymbolKind::Function, name))
        }
        "method_declaration" => {
            node_field_text(node, source, "name").map(|name| (SymbolKind::Method, name))
        }
        "type_declaration" => nim_type_symbol(source, node),
        _ => None,
    }
}

fn nim_type_symbol(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    let mut cursor = node.walk();
    let name = node
        .children(&mut cursor)
        .find(|child| child.is_named() && child.kind() == "type_symbol_declaration")
        .and_then(|child| node_field_text(child, source, "name"))?;

    let mut cursor = node.walk();
    let kind = node
        .children(&mut cursor)
        .filter(|child| child.is_named())
        .find_map(|child| match child.kind() {
            "enum_declaration" => Some(SymbolKind::Enum),
            "object_declaration" => Some(SymbolKind::Struct),
            _ => None,
        })
        .unwrap_or(SymbolKind::TypeAlias);

    Some((kind, name))
}
