use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::SymbolKind;

use super::{blade, php, rust};

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
        if blade::is_blade_path(path) {
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

pub(crate) fn semantic_chunk_language_for_path(path: &Path) -> Option<&'static str> {
    if let Some(language) = SymbolLanguage::from_path(path) {
        return Some(language.as_str());
    }

    let extension = path.extension().and_then(|extension| extension.to_str())?;
    if extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown") {
        return Some("markdown");
    }
    if extension.eq_ignore_ascii_case("json") {
        return Some("json");
    }
    if extension.eq_ignore_ascii_case("toml") {
        return Some("toml");
    }
    if extension.eq_ignore_ascii_case("txt") {
        return Some("text");
    }
    if extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml") {
        return Some("yaml");
    }
    None
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
        SymbolLanguage::Rust => rust::symbol_from_node(source, node),
        SymbolLanguage::Php => php::symbol_from_node(source, node),
        SymbolLanguage::Blade => None,
    }
}

pub(super) fn node_name_text(node: Node<'_>, source: &str) -> Option<String> {
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

pub(super) fn node_field_text(node: Node<'_>, source: &str, field_name: &str) -> Option<String> {
    node.child_by_field_name(field_name).and_then(|field_node| {
        field_node
            .utf8_text(source.as_bytes())
            .ok()
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(ToOwned::to_owned)
    })
}
