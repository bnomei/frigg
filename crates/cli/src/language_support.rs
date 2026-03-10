use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::domain::{FriggError, FriggResult};
use crate::graph::RelationKind;
use crate::indexer::{PhpDeclarationRelation, SymbolDefinition, SymbolKind};

const DOCUMENT_SYMBOLS_LANGUAGES: &[SymbolLanguage] = &[
    SymbolLanguage::Rust,
    SymbolLanguage::Php,
    SymbolLanguage::Blade,
];
const DOCUMENT_SYMBOLS_EXTENSIONS: &[&str] = &[".rs", ".php", ".blade.php"];
const STRUCTURAL_SEARCH_LANGUAGES: &[SymbolLanguage] = &[
    SymbolLanguage::Rust,
    SymbolLanguage::Php,
    SymbolLanguage::Blade,
];
const STRUCTURAL_SEARCH_EXTENSIONS: &[&str] = &[".rs", ".php", ".blade.php"];
const SYMBOL_CORPUS_LANGUAGES: &[SymbolLanguage] = &[
    SymbolLanguage::Rust,
    SymbolLanguage::Php,
    SymbolLanguage::Blade,
];
const SYMBOL_CORPUS_EXTENSIONS: &[&str] = &[".rs", ".php", ".blade.php"];
const SOURCE_FILTER_VALUES: &[&str] = &["rust", "rs", "php", "blade"];
const CANONICAL_LANGUAGE_NAMES: &[&str] = &["rust", "php", "blade"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanguageCapability {
    DocumentSymbols,
    StructuralSearch,
    SymbolCorpus,
    SourceFilter,
}

impl LanguageCapability {
    pub(crate) fn supported_language_names(self) -> &'static [&'static str] {
        match self {
            LanguageCapability::DocumentSymbols
            | LanguageCapability::StructuralSearch
            | LanguageCapability::SymbolCorpus
            | LanguageCapability::SourceFilter => CANONICAL_LANGUAGE_NAMES,
        }
    }

    pub(crate) fn supported_extensions(self) -> &'static [&'static str] {
        match self {
            LanguageCapability::DocumentSymbols => DOCUMENT_SYMBOLS_EXTENSIONS,
            LanguageCapability::StructuralSearch => STRUCTURAL_SEARCH_EXTENSIONS,
            LanguageCapability::SymbolCorpus => SYMBOL_CORPUS_EXTENSIONS,
            LanguageCapability::SourceFilter => DOCUMENT_SYMBOLS_EXTENSIONS,
        }
    }

    pub(crate) fn unsupported_file_message(self, tool_name: &str) -> String {
        format!(
            "{tool_name} only supports {} files",
            supported_language_label(self)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolLanguage {
    Rust,
    Php,
    Blade,
}

impl SymbolLanguage {
    pub fn from_path(path: &Path) -> Option<Self> {
        if is_blade_path(path) {
            return Some(Self::Blade);
        }
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("rs") => Some(Self::Rust),
            Some("php") => Some(Self::Php),
            _ => None,
        }
    }

    pub fn parse_alias(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "php" => Some(Self::Php),
            "blade" => Some(Self::Blade),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Php => "php",
            Self::Blade => "blade",
        }
    }

    pub fn matches_path(self, path: &Path) -> bool {
        Self::from_path(path) == Some(self)
    }

    pub(crate) fn supported_search_filter_values() -> &'static [&'static str] {
        SOURCE_FILTER_VALUES
    }

    pub(crate) fn supports(self, capability: LanguageCapability) -> bool {
        match capability {
            LanguageCapability::DocumentSymbols => DOCUMENT_SYMBOLS_LANGUAGES.contains(&self),
            LanguageCapability::StructuralSearch => STRUCTURAL_SEARCH_LANGUAGES.contains(&self),
            LanguageCapability::SymbolCorpus => SYMBOL_CORPUS_LANGUAGES.contains(&self),
            LanguageCapability::SourceFilter => DOCUMENT_SYMBOLS_LANGUAGES.contains(&self),
        }
    }
}

fn supported_language_label(capability: LanguageCapability) -> &'static str {
    match capability {
        LanguageCapability::DocumentSymbols
        | LanguageCapability::StructuralSearch
        | LanguageCapability::SymbolCorpus
        | LanguageCapability::SourceFilter => "Rust, PHP, and Blade",
    }
}

pub(crate) fn parse_supported_language(
    raw: &str,
    capability: LanguageCapability,
) -> Option<SymbolLanguage> {
    let language = SymbolLanguage::parse_alias(raw)?;
    language.supports(capability).then_some(language)
}

pub(crate) fn supported_language_for_path(
    path: &Path,
    capability: LanguageCapability,
) -> Option<SymbolLanguage> {
    let language = SymbolLanguage::from_path(path)?;
    language.supports(capability).then_some(language)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HeuristicImplementationStrategy {
    RustImplBlocks,
    PhpDeclarationRelations,
}

pub(crate) fn heuristic_implementation_strategy(
    language: SymbolLanguage,
) -> Option<HeuristicImplementationStrategy> {
    match language {
        SymbolLanguage::Rust => Some(HeuristicImplementationStrategy::RustImplBlocks),
        SymbolLanguage::Php => Some(HeuristicImplementationStrategy::PhpDeclarationRelations),
        SymbolLanguage::Blade => None,
    }
}

pub(crate) fn tree_sitter_language(language: SymbolLanguage) -> tree_sitter::Language {
    match language {
        SymbolLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        SymbolLanguage::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        SymbolLanguage::Blade => tree_sitter_blade::LANGUAGE.into(),
    }
}

pub(crate) fn parser_for_language(language: SymbolLanguage) -> FriggResult<Parser> {
    let mut parser = Parser::new();
    let ts_language = tree_sitter_language(language);

    parser.set_language(&ts_language).map_err(|err| {
        FriggError::Internal(format!(
            "failed to configure tree-sitter parser for {}: {err}",
            language.as_str()
        ))
    })?;
    Ok(parser)
}

pub(crate) fn symbol_from_node(
    language: SymbolLanguage,
    source: &str,
    node: Node<'_>,
) -> Option<(SymbolKind, String)> {
    match language {
        SymbolLanguage::Rust => rust_symbol_from_node(source, node),
        SymbolLanguage::Php => php_symbol_from_node(source, node),
        SymbolLanguage::Blade => None,
    }
}

fn rust_symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
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

fn php_symbol_from_node(source: &str, node: Node<'_>) -> Option<(SymbolKind, String)> {
    match node.kind() {
        "namespace_definition" => {
            node_name_text(node, source).map(|name| (SymbolKind::Module, name))
        }
        "function_definition" => {
            node_name_text(node, source).map(|name| (SymbolKind::Function, name))
        }
        "class_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Class, name)),
        "interface_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::Interface, name))
        }
        "trait_declaration" => {
            node_name_text(node, source).map(|name| (SymbolKind::PhpTrait, name))
        }
        "enum_declaration" => node_name_text(node, source).map(|name| (SymbolKind::PhpEnum, name)),
        "enum_case" => node_name_text(node, source).map(|name| (SymbolKind::EnumCase, name)),
        "method_declaration" => node_name_text(node, source).map(|name| (SymbolKind::Method, name)),
        "property_element" => node_name_text(node, source).map(|name| (SymbolKind::Property, name)),
        "const_element" => node_name_text(node, source).map(|name| (SymbolKind::Constant, name)),
        _ => None,
    }
}

pub(crate) fn is_blade_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|file_name| file_name.to_str())
        .is_some_and(|file_name| file_name.ends_with(".blade.php"))
}

pub(crate) fn blade_view_name_for_path(path: &Path) -> Option<String> {
    let normalized = normalize_path_components(path);
    let blade_index = normalized
        .iter()
        .position(|component| component == "views")?;
    let tail = normalized.get(blade_index + 1..)?;
    let segments = tail
        .iter()
        .map(|segment| strip_blade_suffix(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("."))
}

pub(crate) fn blade_component_name_for_path(path: &Path) -> Option<String> {
    let normalized = normalize_path_components(path);
    let components_index = normalized
        .iter()
        .position(|component| component == "components")?;
    let tail = normalized.get(components_index + 1..)?;
    let segments = tail
        .iter()
        .map(|segment| strip_blade_suffix(segment))
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.is_empty() {
        return None;
    }
    Some(segments.join("."))
}

fn normalize_path_components(path: &Path) -> Vec<String> {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>()
}

fn strip_blade_suffix(segment: &str) -> String {
    segment
        .strip_suffix(".blade.php")
        .or_else(|| segment.strip_suffix(".php"))
        .unwrap_or(segment)
        .to_owned()
}

fn node_name_text(node: Node<'_>, source: &str) -> Option<String> {
    node_field_text(node, source, "name").or_else(|| {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .filter(|child| child.is_named())
            .find(|child| matches!(child.kind(), "name" | "identifier" | "variable_name"))
            .and_then(|child| child.utf8_text(source.as_bytes()).ok())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn node_field_text(node: Node<'_>, source: &str, field_name: &str) -> Option<String> {
    node.child_by_field_name(field_name).and_then(|field_node| {
        field_node
            .utf8_text(source.as_bytes())
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PhpNameResolutionContext {
    pub(crate) namespace: Option<String>,
    pub(crate) class_like_aliases: BTreeMap<String, String>,
}

impl PhpNameResolutionContext {
    pub(crate) fn resolve_class_like_name(
        &self,
        raw_name: &str,
        current_class_canonical_name: Option<&str>,
    ) -> Option<String> {
        let trimmed = raw_name.trim();
        if trimmed.is_empty() {
            return None;
        }

        let optional_trimmed = trimmed.strip_prefix('?').unwrap_or(trimmed);
        let normalized = optional_trimmed.trim_start_matches('\\').trim();
        if normalized.is_empty() {
            return None;
        }

        match normalized.to_ascii_lowercase().as_str() {
            "self" | "static" => {
                return current_class_canonical_name
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .map(ToOwned::to_owned);
            }
            "parent" => return None,
            _ if php_is_builtin_type(normalized) => return None,
            _ => {}
        }

        if optional_trimmed.starts_with('\\') {
            return Some(normalized.to_owned());
        }

        if let Some(relative) = normalized
            .strip_prefix("namespace\\")
            .or_else(|| normalized.strip_prefix("namespace/"))
        {
            return self.namespace.as_ref().map(|namespace| {
                if relative.is_empty() {
                    namespace.clone()
                } else {
                    format!("{namespace}\\{relative}")
                }
            });
        }

        let mut segments = normalized.splitn(2, '\\');
        let first_segment = segments.next().unwrap_or_default().trim();
        let remainder = segments.next().map(str::trim).unwrap_or_default();
        if first_segment.is_empty() {
            return None;
        }

        if let Some(alias_target) = self
            .class_like_aliases
            .get(&first_segment.to_ascii_lowercase())
            .filter(|target| !target.trim().is_empty())
        {
            return Some(if remainder.is_empty() {
                alias_target.clone()
            } else {
                format!("{alias_target}\\{remainder}")
            });
        }

        if let Some(namespace) = &self.namespace {
            return Some(format!("{namespace}\\{normalized}"));
        }

        Some(normalized.to_owned())
    }
}

pub(crate) fn php_name_resolution_context_from_root(
    source: &str,
    root: Node<'_>,
) -> PhpNameResolutionContext {
    let mut context = PhpNameResolutionContext::default();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor).filter(|child| child.is_named()) {
        match child.kind() {
            "namespace_definition" => {
                context.namespace = node_field_text(child, source, "name");
                if let Some(body) = child.child_by_field_name("body") {
                    let mut body_cursor = body.walk();
                    for body_child in body
                        .children(&mut body_cursor)
                        .filter(|node| node.is_named())
                    {
                        if body_child.kind() == "namespace_use_declaration" {
                            collect_php_namespace_use_declaration(source, body_child, &mut context);
                        }
                    }
                }
            }
            "namespace_use_declaration" => {
                collect_php_namespace_use_declaration(source, child, &mut context);
            }
            _ => {}
        }
    }
    context
}

pub(crate) fn php_is_builtin_type(raw_name: &str) -> bool {
    matches!(
        raw_name.trim().to_ascii_lowercase().as_str(),
        "array"
            | "bool"
            | "callable"
            | "false"
            | "float"
            | "int"
            | "iterable"
            | "mixed"
            | "never"
            | "null"
            | "object"
            | "resource"
            | "string"
            | "true"
            | "void"
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn php_class_like_name_candidates(
    context: Option<&PhpNameResolutionContext>,
    raw_target_name: &str,
    current_class_canonical_name: Option<&str>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(canonical) = context.and_then(|context| {
        context.resolve_class_like_name(raw_target_name, current_class_canonical_name)
    }) {
        candidates.push(canonical);
    }
    for candidate in php_reference_name_candidates(raw_target_name) {
        if !candidates.iter().any(|existing| existing == &candidate) {
            candidates.push(candidate);
        }
    }
    candidates
}

fn collect_php_namespace_use_declaration(
    source: &str,
    declaration: Node<'_>,
    context: &mut PhpNameResolutionContext,
) {
    let declaration_type = declaration
        .child_by_field_name("type")
        .map(|node| node.kind());
    let body_id = declaration
        .child_by_field_name("body")
        .map(|node| node.id());
    let grouped_prefix = if body_id.is_some() {
        let mut cursor = declaration.walk();
        declaration
            .children(&mut cursor)
            .filter(|child| child.is_named())
            .find(|child| child.kind() == "namespace_name")
            .and_then(|child| child.utf8_text(source.as_bytes()).ok())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    } else {
        None
    };

    let mut cursor = declaration.walk();
    for child in declaration
        .children(&mut cursor)
        .filter(|child| child.is_named())
    {
        if Some(child.id()) == body_id {
            let mut group_cursor = child.walk();
            for clause in child
                .children(&mut group_cursor)
                .filter(|node| node.is_named())
            {
                if clause.kind() == "namespace_use_clause" {
                    collect_php_namespace_use_clause(
                        source,
                        clause,
                        grouped_prefix.as_deref(),
                        declaration_type,
                        context,
                    );
                }
            }
            continue;
        }
        if child.kind() == "namespace_use_clause" {
            collect_php_namespace_use_clause(source, child, None, declaration_type, context);
        }
    }
}

fn collect_php_namespace_use_clause(
    source: &str,
    clause: Node<'_>,
    grouped_prefix: Option<&str>,
    declaration_type: Option<&str>,
    context: &mut PhpNameResolutionContext,
) {
    let clause_type = clause
        .child_by_field_name("type")
        .map(|node| node.kind())
        .or(declaration_type);
    if matches!(clause_type, Some("function" | "const")) {
        return;
    }

    let alias_node = clause.child_by_field_name("alias");
    let alias_node_id = alias_node.as_ref().map(Node::id);
    let alias = alias_node
        .and_then(|node| node.utf8_text(source.as_bytes()).ok())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToOwned::to_owned);

    let mut referenced_name = None;
    let mut cursor = clause.walk();
    for child in clause
        .children(&mut cursor)
        .filter(|child| child.is_named())
    {
        if Some(child.id()) == alias_node_id {
            continue;
        }
        referenced_name = child
            .utf8_text(source.as_bytes())
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned);
        if referenced_name.is_some() {
            break;
        }
    }

    let Some(referenced_name) = referenced_name else {
        return;
    };
    let canonical = grouped_prefix
        .filter(|prefix| !prefix.is_empty())
        .map(|prefix| format!("{prefix}\\{referenced_name}"))
        .unwrap_or_else(|| referenced_name.clone());
    let alias = alias.unwrap_or_else(|| {
        referenced_name
            .rsplit('\\')
            .next()
            .unwrap_or(&referenced_name)
            .to_owned()
    });
    if alias.trim().is_empty() || canonical.trim().is_empty() {
        return;
    }

    context
        .class_like_aliases
        .insert(alias.to_ascii_lowercase(), canonical);
}

pub(crate) struct PhpSymbolLookup<'a> {
    pub(crate) symbols: &'a [SymbolDefinition],
    pub(crate) symbols_by_relative_path: &'a BTreeMap<String, Vec<usize>>,
    pub(crate) symbol_indices_by_name: &'a BTreeMap<String, Vec<usize>>,
    pub(crate) symbol_indices_by_lower_name: &'a BTreeMap<String, Vec<usize>>,
}

pub(crate) fn resolve_php_declaration_relation_indices(
    lookup: &PhpSymbolLookup<'_>,
    relative_path: &str,
    relation: &PhpDeclarationRelation,
) -> Option<(usize, usize)> {
    let source_symbol_index = resolve_php_relation_source_symbol(lookup, relative_path, relation)?;
    let target_symbol_index = resolve_php_relation_target_symbol(lookup, relation)?;
    let source_symbol = &lookup.symbols[source_symbol_index];
    let target_symbol = &lookup.symbols[target_symbol_index];
    if source_symbol.stable_id == target_symbol.stable_id {
        return None;
    }
    Some((source_symbol_index, target_symbol_index))
}

pub(crate) fn php_relation_targets_symbol_name(
    relation: &PhpDeclarationRelation,
    target_symbol: &SymbolDefinition,
) -> bool {
    let target_name = target_symbol.name.trim();
    if target_name.is_empty() {
        return false;
    }
    (match target_symbol.kind {
        SymbolKind::Interface => matches!(
            relation.relation,
            RelationKind::Implements | RelationKind::Extends
        ),
        SymbolKind::Class => relation.relation == RelationKind::Extends,
        _ => false,
    }) && php_reference_name_candidates(&relation.target_name)
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(target_name))
}

fn resolve_php_relation_source_symbol(
    lookup: &PhpSymbolLookup<'_>,
    relative_path: &str,
    relation: &PhpDeclarationRelation,
) -> Option<usize> {
    let matches = lookup
        .symbols_by_relative_path
        .get(relative_path)?
        .iter()
        .copied()
        .filter(|index| {
            let symbol = &lookup.symbols[*index];
            symbol.language == SymbolLanguage::Php
                && symbol.kind == relation.source_kind
                && symbol.line == relation.source_line
                && symbol.name == relation.source_name
        })
        .collect::<Vec<_>>();
    if matches.len() == 1 {
        matches.first().copied()
    } else {
        None
    }
}

fn resolve_php_relation_target_symbol(
    lookup: &PhpSymbolLookup<'_>,
    relation: &PhpDeclarationRelation,
) -> Option<usize> {
    let allowed_kinds = php_allowed_target_kinds(relation.source_kind, relation.relation);
    if allowed_kinds.is_empty() {
        return None;
    }

    let candidates = php_reference_name_candidates(&relation.target_name);
    if candidates.is_empty() {
        return None;
    }

    let mut exact_matches = BTreeSet::new();
    for candidate in &candidates {
        if let Some(indices) = lookup.symbol_indices_by_name.get(candidate) {
            for index in indices {
                let symbol = &lookup.symbols[*index];
                if symbol.language == SymbolLanguage::Php && allowed_kinds.contains(&symbol.kind) {
                    exact_matches.insert(*index);
                }
            }
        }
    }
    if exact_matches.len() == 1 {
        return exact_matches.iter().next().copied();
    }
    if !exact_matches.is_empty() {
        return None;
    }

    let mut case_insensitive_matches = BTreeSet::new();
    for candidate in &candidates {
        let lower = candidate.to_ascii_lowercase();
        if let Some(indices) = lookup.symbol_indices_by_lower_name.get(&lower) {
            for index in indices {
                let symbol = &lookup.symbols[*index];
                if symbol.language == SymbolLanguage::Php && allowed_kinds.contains(&symbol.kind) {
                    case_insensitive_matches.insert(*index);
                }
            }
        }
    }
    if case_insensitive_matches.len() == 1 {
        case_insensitive_matches.iter().next().copied()
    } else {
        None
    }
}

fn php_allowed_target_kinds(
    source_kind: SymbolKind,
    relation: RelationKind,
) -> &'static [SymbolKind] {
    const CLASS_ONLY: &[SymbolKind] = &[SymbolKind::Class];
    const INTERFACE_ONLY: &[SymbolKind] = &[SymbolKind::Interface];
    const NONE: &[SymbolKind] = &[];

    match relation {
        RelationKind::Implements => INTERFACE_ONLY,
        RelationKind::Extends => match source_kind {
            SymbolKind::Class => CLASS_ONLY,
            SymbolKind::Interface => INTERFACE_ONLY,
            _ => NONE,
        },
        _ => NONE,
    }
}

fn php_reference_name_candidates(raw_target_name: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let trimmed = raw_target_name.trim().trim_start_matches('\\');
    if trimmed.is_empty() {
        return candidates;
    }

    for candidate in [
        Some(trimmed),
        trimmed.rsplit('\\').next(),
        trimmed.rsplit(':').next(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|candidate| !candidate.is_empty())
    {
        if matches!(
            candidate.to_ascii_lowercase().as_str(),
            "self" | "static" | "parent"
        ) {
            continue;
        }
        if !candidates.iter().any(|existing| existing == candidate) {
            candidates.push(candidate.to_owned());
        }
    }

    candidates
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        HeuristicImplementationStrategy, LanguageCapability, PhpNameResolutionContext,
        SymbolLanguage, blade_component_name_for_path, blade_view_name_for_path,
        heuristic_implementation_strategy, parse_supported_language,
        php_class_like_name_candidates, php_name_resolution_context_from_root,
        supported_language_for_path,
    };
    use tree_sitter::Parser;

    #[test]
    fn capability_parsing_uses_shared_alias_table() {
        assert_eq!(
            parse_supported_language("rs", LanguageCapability::DocumentSymbols),
            Some(SymbolLanguage::Rust)
        );
        assert_eq!(
            parse_supported_language("php", LanguageCapability::StructuralSearch),
            Some(SymbolLanguage::Php)
        );
        assert_eq!(
            parse_supported_language("blade", LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Blade)
        );
        assert_eq!(
            parse_supported_language("ts", LanguageCapability::SymbolCorpus),
            None
        );
    }

    #[test]
    fn path_support_filters_use_capability_tables() {
        assert_eq!(
            supported_language_for_path(Path::new("src/lib.rs"), LanguageCapability::SymbolCorpus),
            Some(SymbolLanguage::Rust)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("src/server.php"),
                LanguageCapability::DocumentSymbols
            ),
            Some(SymbolLanguage::Php)
        );
        assert_eq!(
            supported_language_for_path(
                Path::new("resources/views/welcome.blade.php"),
                LanguageCapability::StructuralSearch
            ),
            Some(SymbolLanguage::Blade)
        );
        assert_eq!(
            supported_language_for_path(Path::new("src/app.ts"), LanguageCapability::SymbolCorpus),
            None
        );
    }

    #[test]
    fn heuristic_implementation_dispatch_stays_centralized() {
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Rust),
            Some(HeuristicImplementationStrategy::RustImplBlocks)
        );
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Php),
            Some(HeuristicImplementationStrategy::PhpDeclarationRelations)
        );
        assert_eq!(
            heuristic_implementation_strategy(SymbolLanguage::Blade),
            None
        );
    }

    #[test]
    fn blade_path_helpers_normalize_view_and_component_names() {
        assert_eq!(
            blade_view_name_for_path(Path::new("resources/views/dashboard/index.blade.php")),
            Some("dashboard.index".to_owned())
        );
        assert_eq!(
            blade_component_name_for_path(Path::new(
                "resources/views/components/forms/input.blade.php"
            )),
            Some("forms.input".to_owned())
        );
    }

    #[test]
    fn php_name_resolution_context_resolves_aliases_grouped_imports_and_namespace_relative_names() {
        let source = "<?php\n\
            namespace App\\Http\\Controllers;\n\
            use App\\Contracts\\Handler as ContractHandler;\n\
            use App\\Support\\{Mailer, Logger as ActivityLogger};\n";
        let mut parser = Parser::new();
        let language = tree_sitter_php::LANGUAGE_PHP.into();
        parser
            .set_language(&language)
            .expect("php parser should configure");
        let tree = parser.parse(source, None).expect("php source should parse");
        let context = php_name_resolution_context_from_root(source, tree.root_node());

        assert_eq!(
            context,
            PhpNameResolutionContext {
                namespace: Some("App\\Http\\Controllers".to_owned()),
                class_like_aliases: [
                    (
                        "contracthandler".to_owned(),
                        "App\\Contracts\\Handler".to_owned(),
                    ),
                    ("mailer".to_owned(), "App\\Support\\Mailer".to_owned()),
                    (
                        "activitylogger".to_owned(),
                        "App\\Support\\Logger".to_owned(),
                    ),
                ]
                .into_iter()
                .collect(),
            }
        );
        assert_eq!(
            context.resolve_class_like_name("ContractHandler", None),
            Some("App\\Contracts\\Handler".to_owned())
        );
        assert_eq!(
            context.resolve_class_like_name("Mailer", None),
            Some("App\\Support\\Mailer".to_owned())
        );
        assert_eq!(
            context.resolve_class_like_name("namespace\\Responder", None),
            Some("App\\Http\\Controllers\\Responder".to_owned())
        );
        assert_eq!(
            php_class_like_name_candidates(Some(&context), "ActivityLogger", None),
            vec![
                "App\\Support\\Logger".to_owned(),
                "ActivityLogger".to_owned()
            ]
        );
    }
}
