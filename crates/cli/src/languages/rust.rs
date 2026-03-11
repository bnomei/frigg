use tree_sitter::Node;

use crate::indexer::{SymbolDefinition, SymbolKind};

use super::registry::{node_field_text, node_name_text};

#[derive(Debug, Clone)]
pub(crate) struct RustImplementationMatchCandidate<'a> {
    pub(crate) source_symbol: &'a SymbolDefinition,
    pub(crate) symbol: String,
    pub(crate) relation: &'static str,
}

pub(super) fn symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "mod_item" => node_name_text(node, source).map(|name| (SymbolKind::Module, name)),
        "struct_item" => node_name_text(node, source).map(|name| (SymbolKind::Struct, name)),
        "enum_item" => node_name_text(node, source).map(|name| (SymbolKind::Enum, name)),
        "trait_item" => node_name_text(node, source).map(|name| (SymbolKind::Trait, name)),
        "type_item" => node_name_text(node, source).map(|name| (SymbolKind::TypeAlias, name)),
        "const_item" => node_name_text(node, source).map(|name| (SymbolKind::Const, name)),
        "static_item" => node_name_text(node, source).map(|name| (SymbolKind::Static, name)),
        "function_item" | "function_signature_item" => {
            let parent_kind = node.parent().map(|parent| parent.kind());
            let kind = if matches!(parent_kind, Some("impl_item" | "trait_item")) {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            };
            node_name_text(node, source).map(|name| (kind, name))
        }
        "impl_item" => {
            let implemented_type = node_field_text(node, source, "type");
            let implemented_trait = node_field_text(node, source, "trait");
            let name = match (implemented_trait, implemented_type) {
                (Some(trait_name), Some(type_name)) => format!("impl {trait_name} for {type_name}"),
                (None, Some(type_name)) => format!("impl {type_name}"),
                _ => "impl".to_string(),
            };
            Some((SymbolKind::Impl, name))
        }
        _ => None,
    }
}

pub(crate) fn parse_impl_signature(symbol_name: &str) -> Option<(Option<&str>, &str)> {
    let body = symbol_name.strip_prefix("impl ")?;
    if let Some((implemented_trait, implementing_type)) = body.split_once(" for ") {
        let implemented_trait = implemented_trait.trim();
        let implementing_type = implementing_type.trim();
        if implemented_trait.is_empty() || implementing_type.is_empty() {
            return None;
        }
        return Some((Some(implemented_trait), implementing_type));
    }
    let implementing_type = body.trim();
    if implementing_type.is_empty() {
        return None;
    }
    Some((None, implementing_type))
}

pub(crate) fn source_suffix_looks_like_call(mut suffix: &str) -> bool {
    suffix = suffix.trim_start_matches(|ch: char| ch == ' ' || ch == '\t');
    suffix = suffix.trim_start_matches(|ch: char| ch.is_ascii_alphanumeric() || ch == '_');
    suffix = suffix.trim_start_matches(|ch: char| ch == ' ' || ch == '\t');
    if suffix.starts_with('(') {
        return true;
    }
    if !suffix.starts_with("::") {
        return false;
    }

    suffix = suffix[2..].trim_start_matches(|ch: char| ch == ' ' || ch == '\t');
    if !suffix.starts_with('<') {
        return false;
    }

    let mut depth = 0usize;
    let mut end_index = None;
    for (index, ch) in suffix.char_indices() {
        match ch {
            '<' => depth = depth.saturating_add(1),
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    end_index = Some(index + ch.len_utf8());
                    break;
                }
            }
            _ => {}
        }
    }
    let Some(end_index) = end_index else {
        return false;
    };
    suffix[end_index..]
        .trim_start_matches(|ch: char| ch == ' ' || ch == '\t')
        .starts_with('(')
}

pub(crate) fn heuristic_implementation_candidates<'a>(
    target_symbol: &'a SymbolDefinition,
    symbols: &'a [SymbolDefinition],
) -> Vec<RustImplementationMatchCandidate<'a>> {
    let target_name = target_symbol.name.trim();
    if target_name.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for symbol in symbols {
        if symbol.kind != SymbolKind::Impl {
            continue;
        }

        let impl_symbol_name = symbol.name.trim();
        if impl_symbol_name.is_empty() {
            continue;
        }

        let mut relation = "implementation";
        let matched_display_name = if let Some((implemented_trait, implementing_type)) =
            parse_impl_signature(impl_symbol_name)
        {
            if let Some(implemented_trait) = implemented_trait {
                if implemented_trait.eq_ignore_ascii_case(target_name) {
                    relation = "implements";
                    implementing_type.to_owned()
                } else if implementing_type.eq_ignore_ascii_case(target_name) {
                    impl_symbol_name.to_owned()
                } else {
                    continue;
                }
            } else if implementing_type.eq_ignore_ascii_case(target_name) {
                impl_symbol_name.to_owned()
            } else {
                continue;
            }
        } else {
            continue;
        };

        matches.push(RustImplementationMatchCandidate {
            source_symbol: symbol,
            symbol: matched_display_name,
            relation,
        });
    }

    matches
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::languages::SymbolLanguage;

    use super::{
        RustImplementationMatchCandidate, heuristic_implementation_candidates,
        parse_impl_signature, source_suffix_looks_like_call,
    };
    use crate::indexer::{SourceSpan, SymbolDefinition, SymbolKind};

    #[test]
    fn parse_impl_signature_handles_trait_and_inherent_impls() {
        assert_eq!(
            parse_impl_signature("impl Display for App"),
            Some((Some("Display"), "App"))
        );
        assert_eq!(parse_impl_signature("impl App"), Some((None, "App")));
        assert_eq!(parse_impl_signature("impl "), None);
    }

    #[test]
    fn rust_call_suffix_detection_accepts_direct_and_turbofish_calls() {
        assert!(source_suffix_looks_like_call("(arg)"));
        assert!(source_suffix_looks_like_call("::<T>(arg)"));
        assert!(source_suffix_looks_like_call("::<Vec<String>>(arg)"));
        assert!(!source_suffix_looks_like_call("::<Vec<String>"));
        assert!(!source_suffix_looks_like_call("::VALUE"));
    }

    #[test]
    fn heuristic_implementation_candidates_match_trait_and_inherent_impls() {
        let target_trait = symbol("Display", SymbolKind::Trait, 10);
        let target_type = symbol("App", SymbolKind::Struct, 11);
        let trait_impl = symbol("impl Display for App", SymbolKind::Impl, 20);
        let inherent_impl = symbol("impl App", SymbolKind::Impl, 21);
        let unrelated_impl = symbol("impl Debug for Other", SymbolKind::Impl, 22);

        let trait_symbols = [
            trait_impl.clone(),
            inherent_impl.clone(),
            unrelated_impl.clone(),
        ];
        let trait_matches = heuristic_implementation_candidates(&target_trait, &trait_symbols);
        assert_eq!(trait_matches.len(), 1);
        assert_match(&trait_matches[0], &trait_impl, "App", "implements");

        let type_symbols = [trait_impl.clone(), inherent_impl.clone(), unrelated_impl];
        let type_matches = heuristic_implementation_candidates(&target_type, &type_symbols);
        assert_eq!(type_matches.len(), 2);
        assert_match(
            &type_matches[0],
            &trait_impl,
            "impl Display for App",
            "implementation",
        );
        assert_match(
            &type_matches[1],
            &inherent_impl,
            "impl App",
            "implementation",
        );
    }

    fn assert_match(
        candidate: &RustImplementationMatchCandidate<'_>,
        source_symbol: &SymbolDefinition,
        symbol: &str,
        relation: &str,
    ) {
        assert_eq!(candidate.source_symbol, source_symbol);
        assert_eq!(candidate.symbol, symbol);
        assert_eq!(candidate.relation, relation);
    }

    fn symbol(name: &str, kind: SymbolKind, line: usize) -> SymbolDefinition {
        SymbolDefinition {
            stable_id: format!("{name}:{line}"),
            language: SymbolLanguage::Rust,
            kind,
            name: name.to_owned(),
            path: PathBuf::from("src/lib.rs"),
            line,
            span: SourceSpan {
                start_byte: 0,
                end_byte: 0,
                start_line: line,
                start_column: 1,
                end_line: line,
                end_column: 1,
            },
        }
    }
}
