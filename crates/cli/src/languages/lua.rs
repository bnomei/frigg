use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::{node_field_text, node_name_text};

pub(crate) fn is_lua_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("lua"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    if node.kind() != "function_declaration" {
        return None;
    }

    let name_node = node.child_by_field_name("name")?;
    match name_node.kind() {
        "method_index_expression" => {
            node_field_text(name_node, source, "method").map(|name| (SymbolKind::Method, name))
        }
        "dot_index_expression" => {
            node_field_text(name_node, source, "field").map(|name| (SymbolKind::Function, name))
        }
        _ => node_name_text(name_node, source)
            .or_else(|| node_name_text(node, source))
            .map(|name| (SymbolKind::Function, name)),
    }
}
