use std::path::Path;

use tree_sitter::Node;

use crate::indexer::SymbolKind;

use super::registry::node_name_text;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VariableDeclarationKind {
    Const,
    Let,
    Var,
}

pub(crate) fn is_typescript_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| matches!(extension.to_ascii_lowercase().as_str(), "ts" | "tsx"))
}

pub(crate) fn is_tsx_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("tsx"))
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "internal_module" | "module" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::Module, name))
        }
        "abstract_class_declaration" | "class_declaration" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::Class, name))
        }
        "interface_declaration" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::Interface, name))
        }
        "enum_declaration" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::Enum, name))
        }
        "type_alias_declaration" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::TypeAlias, name))
        }
        "function_declaration" | "generator_function_declaration" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::Function, name))
        }
        "method_definition" | "method_signature" | "abstract_method_signature" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::Method, name))
        }
        "public_field_definition" | "property_signature" => {
            normalized_name_text(node, source).map(|name| (SymbolKind::Property, name))
        }
        "variable_declarator" => variable_declarator_symbol(source, node),
        _ => None,
    }
}

fn variable_declarator_symbol(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    if !is_supported_module_scope_variable(node) {
        return None;
    }

    let name_node = node.child_by_field_name("name")?;
    if name_node.kind() != "identifier" {
        return None;
    }
    let name = name_node.utf8_text(source.as_bytes()).ok()?.trim();
    if name.is_empty() {
        return None;
    }

    let kind = match node.child_by_field_name("value").map(|value| value.kind()) {
        Some("arrow_function" | "function_expression") => SymbolKind::Function,
        Some("class") => SymbolKind::Class,
        _ => match variable_declaration_kind(source, node) {
            Some(VariableDeclarationKind::Const) => SymbolKind::Const,
            Some(VariableDeclarationKind::Let | VariableDeclarationKind::Var) | None => {
                return None;
            }
        },
    };

    Some((kind, name.to_owned()))
}

fn variable_declaration_kind(source: &str, node: Node<'_>) -> Option<VariableDeclarationKind> {
    let declaration = node.parent()?;
    let text = declaration.utf8_text(source.as_bytes()).ok()?.trim_start();
    if text.starts_with("const ") {
        return Some(VariableDeclarationKind::Const);
    }
    if text.starts_with("let ") {
        return Some(VariableDeclarationKind::Let);
    }
    if text.starts_with("var ") {
        return Some(VariableDeclarationKind::Var);
    }
    None
}

fn is_supported_module_scope_variable(node: Node<'_>) -> bool {
    let declaration = match node.parent() {
        Some(parent)
            if matches!(
                parent.kind(),
                "lexical_declaration" | "variable_declaration"
            ) =>
        {
            parent
        }
        _ => return false,
    };

    let Some(mut container) = declaration.parent() else {
        return false;
    };
    while matches!(container.kind(), "export_statement" | "ambient_declaration") {
        let Some(parent) = container.parent() else {
            return false;
        };
        container = parent;
    }

    match container.kind() {
        "program" | "module" | "internal_module" => true,
        "statement_block" => container
            .parent()
            .is_some_and(|parent| matches!(parent.kind(), "module" | "internal_module")),
        _ => false,
    }
}

fn normalized_name_text(node: Node<'_>, source: &str) -> Option<String> {
    let raw = node_name_text(node, source)?;
    normalize_name(&raw)
}

fn normalize_name(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed.starts_with('[') {
        return None;
    }

    for quote in ['"', '\'', '`'] {
        if trimmed.starts_with(quote) && trimmed.ends_with(quote) && trimmed.len() >= 2 {
            let inner = trimmed.trim_matches(quote).trim();
            return (!inner.is_empty()).then(|| inner.to_owned());
        }
    }

    Some(trimmed.to_owned())
}
