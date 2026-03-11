use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::{node_field_text, node_name_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PythonScope {
    Module,
    Class,
    Function,
}

pub(crate) fn is_python_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("py"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "class_definition" => (enclosing_python_scope(node) != PythonScope::Function)
            .then(|| node_name_text(node, source))
            .flatten()
            .map(|name| (SymbolKind::Class, name)),
        "function_definition" | "async_function_definition" => match enclosing_python_scope(node) {
            PythonScope::Module => {
                node_name_text(node, source).map(|name| (SymbolKind::Function, name))
            }
            PythonScope::Class => {
                node_name_text(node, source).map(|name| (SymbolKind::Method, name))
            }
            PythonScope::Function => None,
        },
        "type_alias_statement" => match enclosing_python_scope(node) {
            PythonScope::Function => None,
            PythonScope::Module | PythonScope::Class => node_field_text(node, source, "left")
                .and_then(|name| normalize_identifier_like_name(&name))
                .map(|name| (SymbolKind::TypeAlias, name)),
        },
        _ => None,
    }
}

fn enclosing_python_scope(node: Node<'_>) -> PythonScope {
    let mut saw_class = false;
    let mut current = node.parent();
    while let Some(parent) = current {
        match parent.kind() {
            "class_definition" => saw_class = true,
            "function_definition" | "async_function_definition" | "lambda" => {
                return PythonScope::Function;
            }
            _ => {}
        }
        current = parent.parent();
    }
    if saw_class {
        PythonScope::Class
    } else {
        PythonScope::Module
    }
}

fn normalize_identifier_like_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()
        && trimmed
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_'))
    .then(|| trimmed.to_owned())
}
