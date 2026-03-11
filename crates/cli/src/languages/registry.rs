use std::path::Path;

use serde::{Deserialize, Serialize};
use tree_sitter::{Node, Parser};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::SymbolKind;

use super::{blade, go, kotlin, lua, nim, php, python, roc, rust, typescript};

#[allow(dead_code)]
const _: &str = tree_sitter_nim::NODE_TYPES;
#[allow(dead_code)]
const _: &str = tree_sitter_roc::NODE_TYPES;

const DOCUMENT_SYMBOLS_LANGUAGES: &[SymbolLanguage] = &[
    SymbolLanguage::Rust,
    SymbolLanguage::Php,
    SymbolLanguage::Blade,
    SymbolLanguage::TypeScript,
    SymbolLanguage::Python,
    SymbolLanguage::Go,
    SymbolLanguage::Kotlin,
    SymbolLanguage::Lua,
    SymbolLanguage::Roc,
    SymbolLanguage::Nim,
];
const DOCUMENT_SYMBOLS_EXTENSIONS: &[&str] = &[
    ".rs",
    ".php",
    ".blade.php",
    ".ts",
    ".tsx",
    ".py",
    ".go",
    ".kt",
    ".kts",
    ".lua",
    ".roc",
    ".nim",
    ".nims",
];
const STRUCTURAL_SEARCH_LANGUAGES: &[SymbolLanguage] = &[
    SymbolLanguage::Rust,
    SymbolLanguage::Php,
    SymbolLanguage::Blade,
    SymbolLanguage::TypeScript,
    SymbolLanguage::Python,
    SymbolLanguage::Go,
    SymbolLanguage::Kotlin,
    SymbolLanguage::Lua,
    SymbolLanguage::Roc,
    SymbolLanguage::Nim,
];
const STRUCTURAL_SEARCH_EXTENSIONS: &[&str] = &[
    ".rs",
    ".php",
    ".blade.php",
    ".ts",
    ".tsx",
    ".py",
    ".go",
    ".kt",
    ".kts",
    ".lua",
    ".roc",
    ".nim",
    ".nims",
];
const SYMBOL_CORPUS_LANGUAGES: &[SymbolLanguage] = &[
    SymbolLanguage::Rust,
    SymbolLanguage::Php,
    SymbolLanguage::Blade,
    SymbolLanguage::TypeScript,
    SymbolLanguage::Python,
    SymbolLanguage::Go,
    SymbolLanguage::Kotlin,
    SymbolLanguage::Lua,
    SymbolLanguage::Roc,
    SymbolLanguage::Nim,
];
const SYMBOL_CORPUS_EXTENSIONS: &[&str] = &[
    ".rs",
    ".php",
    ".blade.php",
    ".ts",
    ".tsx",
    ".py",
    ".go",
    ".kt",
    ".kts",
    ".lua",
    ".roc",
    ".nim",
    ".nims",
];
const SOURCE_FILTER_VALUES: &[&str] = &[
    "rust",
    "rs",
    "php",
    "blade",
    "typescript",
    "ts",
    "tsx",
    "python",
    "py",
    "go",
    "golang",
    "kotlin",
    "kt",
    "kts",
    "lua",
    "roc",
    "nim",
    "nims",
];
const CANONICAL_LANGUAGE_NAMES: &[&str] = &[
    "rust",
    "php",
    "blade",
    "typescript",
    "python",
    "go",
    "kotlin",
    "lua",
    "roc",
    "nim",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LanguageCapability {
    DocumentSymbols,
    StructuralSearch,
    SymbolCorpus,
    SourceFilter,
}

impl LanguageCapability {
    pub(crate) fn supported_languages(self) -> &'static [SymbolLanguage] {
        match self {
            LanguageCapability::DocumentSymbols => DOCUMENT_SYMBOLS_LANGUAGES,
            LanguageCapability::StructuralSearch => STRUCTURAL_SEARCH_LANGUAGES,
            LanguageCapability::SymbolCorpus => SYMBOL_CORPUS_LANGUAGES,
            LanguageCapability::SourceFilter => DOCUMENT_SYMBOLS_LANGUAGES,
        }
    }

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
    #[serde(rename = "typescript")]
    TypeScript,
    Python,
    Go,
    Kotlin,
    Lua,
    Roc,
    Nim,
}

impl SymbolLanguage {
    pub fn from_path(path: &Path) -> Option<Self> {
        if blade::is_blade_path(path) {
            return Some(Self::Blade);
        }
        if typescript::is_typescript_path(path) {
            return Some(Self::TypeScript);
        }
        if python::is_python_path(path) {
            return Some(Self::Python);
        }
        if go::is_go_path(path) {
            return Some(Self::Go);
        }
        if kotlin::is_kotlin_path(path) {
            return Some(Self::Kotlin);
        }
        if lua::is_lua_path(path) {
            return Some(Self::Lua);
        }
        if roc::is_roc_path(path) {
            return Some(Self::Roc);
        }
        if nim::is_nim_path(path) {
            return Some(Self::Nim);
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
            "typescript" | "ts" | "tsx" => Some(Self::TypeScript),
            "python" | "py" => Some(Self::Python),
            "go" | "golang" => Some(Self::Go),
            "kotlin" | "kt" | "kts" => Some(Self::Kotlin),
            "lua" => Some(Self::Lua),
            "roc" => Some(Self::Roc),
            "nim" | "nims" => Some(Self::Nim),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
            Self::Php => "php",
            Self::Blade => "blade",
            Self::TypeScript => "typescript",
            Self::Python => "python",
            Self::Go => "go",
            Self::Kotlin => "kotlin",
            Self::Lua => "lua",
            Self::Roc => "roc",
            Self::Nim => "nim",
        }
    }

    pub(crate) fn display_name(self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::Php => "PHP",
            Self::Blade => "Blade",
            Self::TypeScript => "TypeScript / TSX",
            Self::Python => "Python",
            Self::Go => "Go",
            Self::Kotlin => "Kotlin / KTS",
            Self::Lua => "Lua",
            Self::Roc => "Roc",
            Self::Nim => "Nim",
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

fn supported_language_label(capability: LanguageCapability) -> String {
    let labels = capability
        .supported_languages()
        .iter()
        .copied()
        .map(SymbolLanguage::display_name)
        .collect::<Vec<_>>();
    match labels.as_slice() {
        [] => "no".to_owned(),
        [only] => only.to_string(),
        [rest @ .., last] => format!("{}, and {}", rest.join(", "), last),
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
    if blade::is_blade_path(path) {
        return Some(SymbolLanguage::Blade.as_str());
    }

    let extension = path.extension().and_then(|extension| extension.to_str())?;
    if extension.eq_ignore_ascii_case("rs") {
        return Some(SymbolLanguage::Rust.as_str());
    }
    if extension.eq_ignore_ascii_case("php") {
        return Some(SymbolLanguage::Php.as_str());
    }
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
        SymbolLanguage::Blade
        | SymbolLanguage::TypeScript
        | SymbolLanguage::Python
        | SymbolLanguage::Go
        | SymbolLanguage::Kotlin
        | SymbolLanguage::Lua
        | SymbolLanguage::Roc
        | SymbolLanguage::Nim => None,
    }
}

pub(crate) fn tree_sitter_language(language: SymbolLanguage) -> tree_sitter::Language {
    match language {
        SymbolLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        SymbolLanguage::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        SymbolLanguage::Blade => tree_sitter_blade::LANGUAGE.into(),
        SymbolLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        SymbolLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        SymbolLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        SymbolLanguage::Kotlin => tree_sitter_kotlin::LANGUAGE.into(),
        SymbolLanguage::Lua => tree_sitter_lua::LANGUAGE.into(),
        SymbolLanguage::Roc => tree_sitter_roc::LANGUAGE.into(),
        SymbolLanguage::Nim => tree_sitter_nim::LANGUAGE.into(),
    }
}

pub(crate) fn tree_sitter_language_for_path(
    language: SymbolLanguage,
    path: &Path,
) -> tree_sitter::Language {
    match language {
        SymbolLanguage::TypeScript if typescript::is_tsx_path(path) => {
            tree_sitter_typescript::LANGUAGE_TSX.into()
        }
        _ => tree_sitter_language(language),
    }
}

pub(crate) fn parser_for_language(language: SymbolLanguage) -> FriggResult<Parser> {
    parser_for_tree_sitter_language(tree_sitter_language(language), language)
}

pub(crate) fn parser_for_path(language: SymbolLanguage, path: &Path) -> FriggResult<Parser> {
    parser_for_tree_sitter_language(tree_sitter_language_for_path(language, path), language)
}

fn parser_for_tree_sitter_language(
    ts_language: tree_sitter::Language,
    language: SymbolLanguage,
) -> FriggResult<Parser> {
    let mut parser = Parser::new();

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
        SymbolLanguage::TypeScript => typescript::symbol_from_node(source, node),
        SymbolLanguage::Python => python::symbol_from_node(source, node),
        SymbolLanguage::Go => go::symbol_from_node(source, node),
        SymbolLanguage::Kotlin => kotlin::symbol_from_node(source, node),
        SymbolLanguage::Lua => lua::symbol_from_node(source, node),
        SymbolLanguage::Roc => roc::symbol_from_node(source, node),
        SymbolLanguage::Nim => nim::symbol_from_node(source, node),
        SymbolLanguage::Blade => None,
    }
}

pub(super) fn node_name_text(node: Node<'_>, source: &str) -> Option<String> {
    node_field_text(node, source, "name").or_else(|| {
        let mut cursor = node.walk();
        node.children(&mut cursor)
            .filter(|child| child.is_named())
            .find(|child| {
                matches!(
                    child.kind(),
                    "name"
                        | "identifier"
                        | "variable_name"
                        | "type_identifier"
                        | "field_identifier"
                        | "simple_identifier"
                        | "package_identifier"
                        | "exported_symbol"
                )
            })
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
