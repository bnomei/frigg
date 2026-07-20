//! Shared symbol, structural-query, and heuristic-reference types for indexer outputs.
//!
//! Defines serde-stable symbol definitions, spans, and heuristic reference records exchanged
//! between extraction, graph registration, and MCP structural delivery surfaces.

use super::*;
use crate::domain::model::GeneratedStructuralFollowUp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Normalized symbol categories shared across language adapters.
pub enum SymbolKind {
    Module,
    Component,
    Section,
    Slot,
    Struct,
    Enum,
    EnumCase,
    Trait,
    Impl,
    Function,
    Method,
    TypeAlias,
    Const,
    Static,
    Class,
    Interface,
    PhpTrait,
    PhpEnum,
    Property,
    Constant,
}

impl SymbolKind {
    /// Stable snake_case label for the symbol kind used in ids, logs, and wire formats.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Module => "module",
            Self::Component => "component",
            Self::Section => "section",
            Self::Slot => "slot",
            Self::Struct => "struct",
            Self::Enum => "enum",
            Self::EnumCase => "enum_case",
            Self::Trait => "trait",
            Self::Impl => "impl",
            Self::Function => "function",
            Self::Method => "method",
            Self::TypeAlias => "type_alias",
            Self::Const => "const",
            Self::Static => "static",
            Self::Class => "class",
            Self::Interface => "interface",
            Self::PhpTrait => "php_trait",
            Self::PhpEnum => "php_enum",
            Self::Property => "property",
            Self::Constant => "constant",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Byte and line/column bounds for one source excerpt.
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One extracted symbol with stable id, language, kind, and source location.
pub struct SymbolDefinition {
    pub stable_id: String,
    pub language: SymbolLanguage,
    pub kind: SymbolKind,
    pub name: String,
    pub path: PathBuf,
    pub line: usize,
    pub span: SourceSpan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Per-file symbol extraction failure carried alongside successful symbols in batch output.
pub struct SymbolExtractionDiagnostic {
    pub path: PathBuf,
    pub language: Option<SymbolLanguage>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
/// Batch symbol extraction result with per-file diagnostics.
pub struct SymbolExtractionOutput {
    pub symbols: Vec<SymbolDefinition>,
    pub diagnostics: Vec<SymbolExtractionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Selects whether structural search returns flat captures or grouped match rows.
pub enum StructuralQueryResultMode {
    Matches,
    Captures,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Policy for choosing the anchor capture that defines a structural query match excerpt.
pub enum StructuralQueryAnchorSelection {
    PrimaryCapture,
    MatchCapture,
    FirstUsefulNamedCapture,
    FirstCapture,
    CaptureRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One named tree-sitter capture within a structural query match.
pub struct StructuralQueryCapture {
    pub name: String,
    pub span: SourceSpan,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One structural query hit with anchor capture metadata and optional follow-up suggestions.
pub struct StructuralQueryMatch {
    pub path: PathBuf,
    pub span: SourceSpan,
    pub excerpt: String,
    pub anchor_capture_name: Option<String>,
    pub anchor_selection: StructuralQueryAnchorSelection,
    pub captures: Vec<StructuralQueryCapture>,
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// One syntax-tree node with kind metadata and a bounded source excerpt.
pub struct SyntaxTreeInspectionNode {
    pub kind: String,
    pub named: bool,
    pub span: SourceSpan,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Focused syntax-tree neighborhood around one source location.
pub struct SyntaxTreeInspection {
    pub language: SymbolLanguage,
    pub focus: SyntaxTreeInspectionNode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw_focus: Option<SyntaxTreeInspectionNode>,
    pub ancestors: Vec<SyntaxTreeInspectionNode>,
    pub children: Vec<SyntaxTreeInspectionNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
/// Confidence tier for heuristic symbol references surfaced outside precise graph resolution.
pub enum HeuristicReferenceConfidence {
    Low,
    Medium,
    High,
}

impl From<HeuristicConfidence> for HeuristicReferenceConfidence {
    fn from(value: HeuristicConfidence) -> Self {
        match value {
            HeuristicConfidence::Low => Self::Low,
            HeuristicConfidence::Medium => Self::Medium,
            HeuristicConfidence::High => Self::High,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
/// Evidence backing one heuristic reference, either graph-derived or lexical.
pub enum HeuristicReferenceEvidence {
    GraphRelation {
        source_symbol_id: String,
        relation: String,
    },
    LexicalToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
/// Probabilistic reference to a symbol discovered outside precise graph resolution.
pub struct HeuristicReference {
    pub repository_id: String,
    pub symbol_id: String,
    pub symbol_name: String,
    pub path: PathBuf,
    pub line: usize,
    pub column: usize,
    pub confidence: HeuristicReferenceConfidence,
    pub heuristic: bool,
    pub evidence: HeuristicReferenceEvidence,
}
