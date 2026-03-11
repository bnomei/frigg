use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::{node_field_text, node_name_text};

pub(crate) fn is_go_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("go"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "package_clause" => node_name_text(node, source).map(|name| (SymbolKind::Module, name)),
        "function_declaration" => {
            node_field_text(node, source, "name").map(|name| (SymbolKind::Function, name))
        }
        "method_declaration" => {
            node_field_text(node, source, "name").map(|name| (SymbolKind::Method, name))
        }
        "type_spec" => {
            let name = node_field_text(node, source, "name")?;
            let kind = match node.child_by_field_name("type").map(|value| value.kind()) {
                Some("struct_type") => SymbolKind::Struct,
                Some("interface_type") => SymbolKind::Interface,
                _ => SymbolKind::TypeAlias,
            };
            Some((kind, name))
        }
        "type_alias" => {
            node_field_text(node, source, "name").map(|name| (SymbolKind::TypeAlias, name))
        }
        "const_spec" => node_field_text(node, source, "name").map(|name| (SymbolKind::Const, name)),
        _ => None,
    }
}
