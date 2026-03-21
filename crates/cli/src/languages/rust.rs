use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use tree_sitter::Node;

use crate::indexer::{SymbolDefinition, SymbolKind, byte_offset_for_line_column};

use super::registry::{SymbolLanguage, node_field_text, node_name_text, parser_for_path};

#[derive(Debug, Clone)]
pub(crate) struct RustImplementationMatchCandidate<'a> {
    pub(crate) source_symbol: &'a SymbolDefinition,
    pub(crate) symbol: String,
    pub(crate) relation: &'static str,
    pub(crate) fallback_reason: &'static str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RustNavigationQueryHint {
    pub(crate) symbol_query: String,
    pub(crate) prefer_same_file: bool,
    pub(crate) prefer_method: bool,
    pub(crate) module_path_segments: Vec<String>,
    pub(crate) enclosing_trait: Option<String>,
    pub(crate) enclosing_impl_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RustEnclosingSymbolContext {
    pub(crate) trait_name: Option<String>,
    pub(crate) impl_type: Option<String>,
    pub(crate) impl_trait: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RustSymbolContext {
    pub(crate) inside_test_module: bool,
    pub(crate) inside_cfg_test: bool,
    pub(crate) inside_test_fn: bool,
}

impl RustSymbolContext {
    pub(crate) fn is_test_context(&self) -> bool {
        self.inside_test_module || self.inside_cfg_test || self.inside_test_fn
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RustImplementationKind {
    ConcreteTrait,
    BlanketTrait,
    Inherent,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RustImplementationFact {
    pub(crate) source_symbol_id: String,
    pub(crate) trait_name: Option<String>,
    pub(crate) self_type: String,
    pub(crate) kind: RustImplementationKind,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RustSourceAnalysis {
    pub(crate) symbol_contexts_by_stable_id: BTreeMap<String, RustSymbolContext>,
    pub(crate) implementation_facts: Vec<RustImplementationFact>,
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
            let kind = if rust_has_ancestor_kind(node, &["impl_item", "trait_item"]) {
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
    suffix = suffix.trim_start_matches([' ', '\t']);
    suffix = suffix.trim_start_matches(|ch: char| ch.is_ascii_alphanumeric() || ch == '_');
    suffix = suffix.trim_start_matches([' ', '\t']);
    if suffix.starts_with('(') {
        return true;
    }
    if !suffix.starts_with("::") {
        return false;
    }

    suffix = suffix[2..].trim_start_matches([' ', '\t']);
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
        .trim_start_matches([' ', '\t'])
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
            fallback_reason: "precise_absent",
        });
    }

    matches
}

pub(crate) fn analyze_source(source: &str, symbols: &[SymbolDefinition]) -> RustSourceAnalysis {
    let mut symbol_contexts_by_stable_id = BTreeMap::new();
    let mut implementation_facts = Vec::new();

    for symbol in symbols {
        symbol_contexts_by_stable_id.insert(
            symbol.stable_id.clone(),
            rust_symbol_context_for_symbol(source, symbol, symbols),
        );

        if symbol.kind != SymbolKind::Impl {
            continue;
        }
        if let Some(fact) = rust_implementation_fact_for_symbol(source, symbol) {
            implementation_facts.push(fact);
        }
    }

    RustSourceAnalysis {
        symbol_contexts_by_stable_id,
        implementation_facts,
    }
}

pub(crate) fn implementation_candidates_from_facts<'a>(
    target_symbol: &'a SymbolDefinition,
    symbols: &'a [SymbolDefinition],
    facts: &'a [RustImplementationFact],
) -> Vec<RustImplementationMatchCandidate<'a>> {
    let target_name = target_symbol.name.trim();
    if target_name.is_empty() {
        return Vec::new();
    }

    let symbol_lookup = symbols
        .iter()
        .map(|symbol| (symbol.stable_id.as_str(), symbol))
        .collect::<BTreeMap<_, _>>();
    let mut matches = Vec::new();
    let target_is_trait = target_symbol.kind == SymbolKind::Trait;

    for fact in facts {
        let Some(source_symbol) = symbol_lookup.get(fact.source_symbol_id.as_str()).copied() else {
            continue;
        };

        if target_is_trait {
            let Some(implemented_trait) = fact.trait_name.as_deref() else {
                continue;
            };
            if !implemented_trait.eq_ignore_ascii_case(target_name) {
                continue;
            }

            matches.push(RustImplementationMatchCandidate {
                source_symbol,
                symbol: fact.self_type.clone(),
                relation: match fact.kind {
                    RustImplementationKind::ConcreteTrait => "implements",
                    RustImplementationKind::BlanketTrait => "implements_blanket",
                    RustImplementationKind::Inherent => "implements",
                },
                fallback_reason: "precise_absent_rust_impl_index",
            });
            continue;
        }

        if !rust_type_matches_target(&fact.self_type, target_name) {
            continue;
        }

        matches.push(RustImplementationMatchCandidate {
            source_symbol,
            symbol: source_symbol.name.clone(),
            relation: match fact.kind {
                RustImplementationKind::ConcreteTrait => "implementation",
                RustImplementationKind::BlanketTrait => "implementation_blanket",
                RustImplementationKind::Inherent => "inherent_impl",
            },
            fallback_reason: "precise_absent_rust_impl_index",
        });
    }

    matches
}

pub(crate) fn navigation_query_hint_from_source(
    path: &Path,
    source: &str,
    line: usize,
    column: usize,
) -> Option<RustNavigationQueryHint> {
    let mut parser = parser_for_path(SymbolLanguage::Rust, path).ok()?;
    let tree = parser.parse(source, None)?;
    let offset = byte_offset_for_line_column(source, line, column)?;
    let focus = rust_focus_node_for_offset(tree.root_node(), offset);

    let mut ancestors = Vec::new();
    let mut cursor = Some(focus);
    while let Some(node) = cursor {
        ancestors.push(node);
        cursor = node.parent();
    }

    let mut hint = rust_use_query_hint_from_focus(focus, source);
    if hint.is_none() {
        for node in &ancestors {
            if let Some(candidate) = rust_location_query_hint_for_node(*node, focus, source) {
                hint = Some(candidate);
                break;
            }
        }
    }

    let mut hint = hint?;
    for node in &ancestors {
        match node.kind() {
            "trait_item" if hint.enclosing_trait.is_none() => {
                hint.enclosing_trait = node_name_text(*node, source);
            }
            "impl_item" if hint.enclosing_impl_type.is_none() => {
                hint.enclosing_impl_type = node_field_text(*node, source, "type");
                if hint.enclosing_trait.is_none() {
                    hint.enclosing_trait = node_field_text(*node, source, "trait");
                }
            }
            _ => {}
        }
    }

    Some(hint)
}

pub(crate) fn relative_path_module_segments(relative_path: &str) -> Vec<String> {
    let path = Path::new(relative_path);
    let mut segments = path
        .iter()
        .map(|segment| segment.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return segments;
    }

    if matches!(
        segments.first().map(String::as_str),
        Some("src" | "tests" | "examples" | "benches")
    ) {
        segments.remove(0);
    }

    let Some(file_name) = segments.pop() else {
        return segments;
    };
    let stem = Path::new(&file_name)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !stem.is_empty() && !matches!(stem, "lib" | "main" | "mod") {
        segments.push(stem.to_owned());
    }
    segments
}

pub(crate) fn enclosing_symbol_context(
    symbol: &SymbolDefinition,
    symbols: &[SymbolDefinition],
) -> RustEnclosingSymbolContext {
    let mut trait_name = None;
    let mut impl_type = None;
    let mut impl_trait = None;
    let mut best_trait_span = usize::MAX;
    let mut best_impl_span = usize::MAX;

    for candidate in symbols {
        if candidate.path != symbol.path {
            continue;
        }
        if candidate.span.start_byte > symbol.span.start_byte
            || candidate.span.end_byte < symbol.span.end_byte
        {
            continue;
        }
        let span_len = candidate
            .span
            .end_byte
            .saturating_sub(candidate.span.start_byte);
        match candidate.kind {
            SymbolKind::Trait if span_len < best_trait_span => {
                best_trait_span = span_len;
                trait_name = Some(candidate.name.clone());
            }
            SymbolKind::Impl if span_len < best_impl_span => {
                best_impl_span = span_len;
                if let Some((implemented_trait, implementing_type)) =
                    parse_impl_signature(candidate.name.as_str())
                {
                    impl_type = Some(implementing_type.to_owned());
                    impl_trait = implemented_trait.map(str::to_owned);
                }
            }
            _ => {}
        }
    }

    RustEnclosingSymbolContext {
        trait_name,
        impl_type,
        impl_trait,
    }
}

fn rust_location_query_hint_for_node(
    node: Node<'_>,
    focus: Node<'_>,
    source: &str,
) -> Option<RustNavigationQueryHint> {
    match node.kind() {
        "field_expression" if focus.kind() == "field_identifier" => {
            let field = node.child_by_field_name("field")?;
            let symbol_query = rust_node_text(field, source)?;
            Some(RustNavigationQueryHint {
                symbol_query,
                prefer_same_file: true,
                prefer_method: true,
                module_path_segments: Vec::new(),
                enclosing_trait: None,
                enclosing_impl_type: None,
            })
        }
        "scoped_identifier" => {
            let segments = rust_path_segments(node, source)?;
            let symbol_query = segments.last()?.clone();
            let first = segments.first().map(String::as_str);
            let mut hint = RustNavigationQueryHint {
                symbol_query,
                prefer_same_file: matches!(first, Some("self" | "super" | "Self")),
                prefer_method: false,
                module_path_segments: Vec::new(),
                enclosing_trait: None,
                enclosing_impl_type: None,
            };
            if segments.len() > 1 {
                let prefix = &segments[..segments.len() - 1];
                if matches!(first, Some("crate" | "self" | "super")) {
                    hint.module_path_segments = prefix
                        .iter()
                        .filter(|segment| !matches!(segment.as_str(), "crate" | "self" | "super"))
                        .cloned()
                        .collect();
                } else if prefix
                    .first()
                    .and_then(|segment| segment.chars().next())
                    .is_some_and(|ch| ch.is_ascii_uppercase())
                {
                    hint.prefer_method = true;
                    hint.enclosing_impl_type = prefix.first().cloned();
                } else {
                    hint.module_path_segments = prefix.to_vec();
                }
            }
            Some(hint)
        }
        "mod_item" => node_name_text(node, source).map(|symbol_query| RustNavigationQueryHint {
            module_path_segments: vec![symbol_query.clone()],
            symbol_query,
            prefer_same_file: false,
            prefer_method: false,
            enclosing_trait: None,
            enclosing_impl_type: None,
        }),
        "identifier" | "type_identifier" | "field_identifier" => {
            rust_node_text(node, source).map(|symbol_query| RustNavigationQueryHint {
                symbol_query,
                prefer_same_file: true,
                prefer_method: node.kind() == "field_identifier",
                module_path_segments: Vec::new(),
                enclosing_trait: None,
                enclosing_impl_type: None,
            })
        }
        _ => None,
    }
}

fn rust_use_query_hint_from_focus(
    focus: Node<'_>,
    source: &str,
) -> Option<RustNavigationQueryHint> {
    let mut current = Some(focus);
    let mut segments: Option<Vec<String>> = None;
    let mut saw_use_declaration = false;

    while let Some(node) = current {
        match node.kind() {
            "use_declaration" => {
                saw_use_declaration = true;
                break;
            }
            "use_as_clause" => {
                segments = node
                    .child_by_field_name("path")
                    .and_then(|path| rust_path_segments(path, source));
            }
            "scoped_identifier" if segments.as_ref().is_none_or(|existing| existing.len() <= 1) => {
                segments = rust_path_segments(node, source);
            }
            "identifier" | "type_identifier" | "crate" | "self" | "super" if segments.is_none() => {
                segments = rust_node_text(node, source).map(|value| vec![value]);
            }
            "scoped_use_list" => {
                if let Some(prefix) = node
                    .child_by_field_name("path")
                    .and_then(|path| rust_path_segments(path, source))
                    && let Some(existing) = segments.as_mut()
                    && !existing.starts_with(prefix.as_slice())
                {
                    let mut merged = prefix;
                    merged.extend(existing.iter().cloned());
                    *existing = merged;
                }
            }
            _ => {}
        }
        current = node.parent();
    }

    if !saw_use_declaration {
        return None;
    }
    let segments = segments?;
    let symbol_query = segments
        .last()
        .filter(|segment| !matches!(segment.as_str(), "crate" | "self" | "super"))?
        .clone();
    let module_path_segments = segments[..segments.len().saturating_sub(1)]
        .iter()
        .filter(|segment| !matches!(segment.as_str(), "crate" | "self" | "super"))
        .cloned()
        .collect::<Vec<_>>();
    Some(RustNavigationQueryHint {
        symbol_query,
        prefer_same_file: false,
        prefer_method: false,
        module_path_segments,
        enclosing_trait: None,
        enclosing_impl_type: None,
    })
}

fn rust_path_segments(node: Node<'_>, source: &str) -> Option<Vec<String>> {
    match node.kind() {
        "scoped_identifier" => {
            let mut segments = node
                .child_by_field_name("path")
                .and_then(|path| rust_path_segments(path, source))
                .unwrap_or_default();
            let name = node.child_by_field_name("name")?;
            segments.push(rust_node_text(name, source)?);
            Some(segments)
        }
        "use_as_clause" => node
            .child_by_field_name("path")
            .and_then(|path| rust_path_segments(path, source)),
        "identifier" | "type_identifier" | "field_identifier" | "crate" | "self" | "super" => {
            rust_node_text(node, source).map(|value| vec![value])
        }
        _ => None,
    }
}

fn rust_node_text(node: Node<'_>, source: &str) -> Option<String> {
    node.utf8_text(source.as_bytes())
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

fn rust_focus_node_for_offset(root: Node<'_>, offset: usize) -> Node<'_> {
    let mut current = root;
    loop {
        let mut next_named = None;
        let mut next_any = None;
        let mut cursor = current.walk();
        for child in current.children(&mut cursor) {
            if child.start_byte() > offset || child.end_byte() < offset {
                continue;
            }
            if next_any.is_none() {
                next_any = Some(child);
            }
            if child.is_named() {
                next_named = Some(child);
                break;
            }
        }
        let Some(next) = next_named.or(next_any) else {
            break;
        };
        if next == current {
            break;
        }
        current = next;
    }
    current
}

fn rust_symbol_context_for_symbol(
    source: &str,
    symbol: &SymbolDefinition,
    symbols: &[SymbolDefinition],
) -> RustSymbolContext {
    let mut context = RustSymbolContext::default();

    for candidate in symbols {
        if candidate.path != symbol.path {
            continue;
        }
        if candidate.span.start_byte > symbol.span.start_byte
            || candidate.span.end_byte < symbol.span.end_byte
        {
            continue;
        }

        let snippet = source_for_symbol_span(source, candidate);
        let leading = source_before_symbol(source, candidate);
        if snippet.contains("#[cfg(test") || leading.contains("#[cfg(test") {
            context.inside_cfg_test = true;
        }
        if candidate.kind == SymbolKind::Module
            && rust_module_looks_like_test(candidate.name.as_str(), snippet, leading)
        {
            context.inside_test_module = true;
        }
        if matches!(candidate.kind, SymbolKind::Function | SymbolKind::Method)
            && rust_function_looks_like_test(candidate.name.as_str(), snippet, leading)
        {
            context.inside_test_fn = true;
        }
    }

    context
}

fn rust_implementation_fact_for_symbol(
    source: &str,
    symbol: &SymbolDefinition,
) -> Option<RustImplementationFact> {
    let (trait_name, self_type) = parse_impl_signature(symbol.name.as_str())?;
    let header = rust_impl_header(source_for_symbol_span(source, symbol));
    let kind = if trait_name.is_none() {
        RustImplementationKind::Inherent
    } else {
        let generic_params = rust_impl_generic_parameters(header);
        if self_type_mentions_generic_params(self_type, &generic_params) {
            RustImplementationKind::BlanketTrait
        } else {
            RustImplementationKind::ConcreteTrait
        }
    };

    Some(RustImplementationFact {
        source_symbol_id: symbol.stable_id.clone(),
        trait_name: trait_name.map(str::to_owned),
        self_type: self_type.to_owned(),
        kind,
    })
}

fn source_for_symbol_span<'a>(source: &'a str, symbol: &SymbolDefinition) -> &'a str {
    let start = symbol.span.start_byte.min(source.len());
    let end = symbol.span.end_byte.min(source.len());
    if start >= end {
        return "";
    }
    source.get(start..end).unwrap_or("")
}

fn source_before_symbol<'a>(source: &'a str, symbol: &SymbolDefinition) -> &'a str {
    let start = symbol.span.start_byte.min(source.len());
    let leading_start = start.saturating_sub(128);
    source.get(leading_start..start).unwrap_or("")
}

fn rust_module_looks_like_test(name: &str, snippet: &str, leading: &str) -> bool {
    name == "tests"
        || name.ends_with("_tests")
        || snippet.contains("#[cfg(test")
        || leading.contains("#[cfg(test")
}

fn rust_function_looks_like_test(name: &str, snippet: &str, leading: &str) -> bool {
    name.starts_with("test_")
        || name.ends_with("_test")
        || snippet.contains("#[test")
        || snippet.contains("::test]")
        || leading.contains("#[test")
        || leading.contains("::test]")
}

fn rust_impl_header(snippet: &str) -> &str {
    snippet.split('{').next().unwrap_or(snippet).trim()
}

fn rust_impl_generic_parameters(header: &str) -> BTreeSet<String> {
    let mut generics = BTreeSet::new();
    let Some(after_impl) = header.strip_prefix("impl") else {
        return generics;
    };
    let after_impl = after_impl.trim_start();
    if !after_impl.starts_with('<') {
        return generics;
    }

    let mut depth = 0usize;
    let mut body = String::new();
    for ch in after_impl.chars() {
        match ch {
            '<' => {
                depth = depth.saturating_add(1);
                if depth > 1 {
                    body.push(ch);
                }
            }
            '>' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    break;
                }
                body.push(ch);
            }
            _ if depth > 0 => body.push(ch),
            _ => break,
        }
    }

    let mut token = String::new();
    for ch in body.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }
        rust_push_generic_param_token(&mut generics, &mut token);
    }
    rust_push_generic_param_token(&mut generics, &mut token);
    generics
}

fn rust_push_generic_param_token(generics: &mut BTreeSet<String>, token: &mut String) {
    if token
        .chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_uppercase())
    {
        generics.insert(token.clone());
    }
    token.clear();
}

fn self_type_mentions_generic_params(self_type: &str, generic_params: &BTreeSet<String>) -> bool {
    if generic_params.is_empty() {
        return false;
    }
    let mut token = String::new();
    for ch in self_type.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' {
            token.push(ch);
            continue;
        }
        if generic_params.contains(&token) {
            return true;
        }
        token.clear();
    }
    generic_params.contains(&token)
}

fn rust_type_matches_target(self_type: &str, target_name: &str) -> bool {
    let normalized = self_type
        .split("::")
        .last()
        .unwrap_or(self_type)
        .split('<')
        .next()
        .unwrap_or(self_type)
        .trim();
    normalized.eq_ignore_ascii_case(target_name)
}

fn rust_has_ancestor_kind(node: Node<'_>, expected_kinds: &[&str]) -> bool {
    let mut cursor = node.parent();
    while let Some(parent) = cursor {
        if expected_kinds.iter().any(|kind| *kind == parent.kind()) {
            return true;
        }
        cursor = parent.parent();
    }
    false
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::languages::SymbolLanguage;

    use super::{
        RustEnclosingSymbolContext, RustImplementationFact, RustImplementationKind,
        RustImplementationMatchCandidate, analyze_source, enclosing_symbol_context,
        heuristic_implementation_candidates, implementation_candidates_from_facts,
        navigation_query_hint_from_source, parse_impl_signature, relative_path_module_segments,
        source_suffix_looks_like_call,
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
        assert_match(
            &trait_matches[0],
            &trait_impl,
            "App",
            "implements",
            "precise_absent",
        );

        let type_symbols = [trait_impl.clone(), inherent_impl.clone(), unrelated_impl];
        let type_matches = heuristic_implementation_candidates(&target_type, &type_symbols);
        assert_eq!(type_matches.len(), 2);
        assert_match(
            &type_matches[0],
            &trait_impl,
            "impl Display for App",
            "implementation",
            "precise_absent",
        );
        assert_match(
            &type_matches[1],
            &inherent_impl,
            "impl App",
            "implementation",
            "precise_absent",
        );
    }

    #[test]
    fn rust_source_analysis_marks_test_context_and_blanket_impls() {
        let source = "pub trait Service {}\n\
                      pub struct App;\n\
                      impl<T> Service for Wrapper<T> {}\n\
                      #[cfg(test)] mod tests {\n\
                          fn helper() {}\n\
                      }\n";
        let symbols = crate::indexer::extract_symbols_from_source(
            SymbolLanguage::Rust,
            Path::new("src/lib.rs"),
            source,
        )
        .expect("rust source should extract symbols");
        let analysis = analyze_source(source, &symbols);
        let tests_module = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Module && symbol.name == "tests")
            .expect("tests module symbol");
        let helper = symbols
            .iter()
            .find(|symbol| symbol.kind == SymbolKind::Function && symbol.name == "helper")
            .expect("helper symbol");
        let impl_fact = analysis
            .implementation_facts
            .iter()
            .find(|fact| fact.trait_name.as_deref() == Some("Service"))
            .expect("trait impl fact");

        assert!(
            analysis
                .symbol_contexts_by_stable_id
                .get(helper.stable_id.as_str())
                .expect("helper context")
                .is_test_context()
        );
        assert!(
            analysis
                .symbol_contexts_by_stable_id
                .get(tests_module.stable_id.as_str())
                .expect("tests module context")
                .inside_cfg_test
        );
        assert_eq!(impl_fact.self_type, "Wrapper<T>");
        assert_eq!(impl_fact.kind, RustImplementationKind::BlanketTrait);
    }

    #[test]
    fn rust_impl_index_candidates_distinguish_blanket_and_inherent_impls() {
        let target_trait = symbol("Service", SymbolKind::Trait, 1);
        let target_type = symbol("Wrapper", SymbolKind::Struct, 2);
        let blanket_impl = symbol("impl Service for Wrapper<T>", SymbolKind::Impl, 8);
        let inherent_impl = symbol("impl Wrapper<T>", SymbolKind::Impl, 12);
        let facts = vec![
            RustImplementationFact {
                source_symbol_id: blanket_impl.stable_id.clone(),
                trait_name: Some("Service".to_owned()),
                self_type: "Wrapper<T>".to_owned(),
                kind: RustImplementationKind::BlanketTrait,
            },
            RustImplementationFact {
                source_symbol_id: inherent_impl.stable_id.clone(),
                trait_name: None,
                self_type: "Wrapper<T>".to_owned(),
                kind: RustImplementationKind::Inherent,
            },
        ];
        let symbols = vec![blanket_impl.clone(), inherent_impl.clone()];

        let trait_matches = implementation_candidates_from_facts(&target_trait, &symbols, &facts);
        assert_eq!(trait_matches.len(), 1);
        assert_match(
            &trait_matches[0],
            &blanket_impl,
            "Wrapper<T>",
            "implements_blanket",
            "precise_absent_rust_impl_index",
        );

        let type_matches = implementation_candidates_from_facts(&target_type, &symbols, &facts);
        assert_eq!(type_matches.len(), 2);
        assert_match(
            &type_matches[0],
            &blanket_impl,
            "impl Service for Wrapper<T>",
            "implementation_blanket",
            "precise_absent_rust_impl_index",
        );
        assert_match(
            &type_matches[1],
            &inherent_impl,
            "impl Wrapper<T>",
            "inherent_impl",
            "precise_absent_rust_impl_index",
        );
    }

    #[test]
    fn rust_navigation_query_hint_prefers_underlying_import_path_over_alias() {
        let source = "pub use crate::worker::helper as local_helper;\nfn local_helper() {}\n";
        let hint = navigation_query_hint_from_source(Path::new("src/lib.rs"), source, 1, 35)
            .expect("rust import alias should produce a navigation hint");
        assert_eq!(hint.symbol_query, "helper");
        assert_eq!(hint.module_path_segments, vec!["worker"]);
        assert!(!hint.prefer_same_file);
    }

    #[test]
    fn rust_navigation_query_hint_prefers_methods_for_field_calls() {
        let call_line = "    fn call(&self) { self.render(); }\n";
        let source = format!("impl App {{\n    fn render(&self) {{}}\n{call_line}}}\n");
        let hint = navigation_query_hint_from_source(
            Path::new("src/lib.rs"),
            &source,
            3,
            call_line.rfind("render").expect("method token present") + 1,
        )
        .expect("field method call should produce a navigation hint");
        assert_eq!(hint.symbol_query, "render");
        assert!(hint.prefer_method);
        assert!(hint.prefer_same_file);
        assert_eq!(hint.enclosing_impl_type.as_deref(), Some("App"));
    }

    #[test]
    fn rust_relative_path_module_segments_ignore_runtime_roots_and_mod_files() {
        assert_eq!(
            relative_path_module_segments("src/worker/mod.rs"),
            vec!["worker"]
        );
        assert_eq!(
            relative_path_module_segments("src/worker/helpers.rs"),
            vec!["worker", "helpers"]
        );
        assert_eq!(
            relative_path_module_segments("src/lib.rs"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn rust_enclosing_symbol_context_finds_trait_and_impl_containers() {
        let method = SymbolDefinition {
            stable_id: "method".to_owned(),
            language: SymbolLanguage::Rust,
            kind: SymbolKind::Method,
            name: "render".to_owned(),
            path: PathBuf::from("src/lib.rs"),
            line: 12,
            span: SourceSpan {
                start_byte: 120,
                end_byte: 160,
                start_line: 12,
                start_column: 1,
                end_line: 14,
                end_column: 1,
            },
        };
        let symbols = vec![
            SymbolDefinition {
                stable_id: "trait".to_owned(),
                language: SymbolLanguage::Rust,
                kind: SymbolKind::Trait,
                name: "Renderer".to_owned(),
                path: PathBuf::from("src/lib.rs"),
                line: 1,
                span: SourceSpan {
                    start_byte: 0,
                    end_byte: 240,
                    start_line: 1,
                    start_column: 1,
                    end_line: 20,
                    end_column: 1,
                },
            },
            SymbolDefinition {
                stable_id: "impl".to_owned(),
                language: SymbolLanguage::Rust,
                kind: SymbolKind::Impl,
                name: "impl Renderer for App".to_owned(),
                path: PathBuf::from("src/lib.rs"),
                line: 10,
                span: SourceSpan {
                    start_byte: 100,
                    end_byte: 220,
                    start_line: 10,
                    start_column: 1,
                    end_line: 18,
                    end_column: 1,
                },
            },
            method.clone(),
        ];

        let context = enclosing_symbol_context(&method, &symbols);
        assert_eq!(
            context,
            RustEnclosingSymbolContext {
                trait_name: Some("Renderer".to_owned()),
                impl_type: Some("App".to_owned()),
                impl_trait: Some("Renderer".to_owned()),
            }
        );
    }

    fn assert_match(
        candidate: &RustImplementationMatchCandidate<'_>,
        source_symbol: &SymbolDefinition,
        symbol: &str,
        relation: &str,
        fallback_reason: &str,
    ) {
        assert_eq!(candidate.source_symbol, source_symbol);
        assert_eq!(candidate.symbol, symbol);
        assert_eq!(candidate.relation, relation);
        assert_eq!(candidate.fallback_reason, fallback_reason);
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
