use super::*;
use crate::domain::model::GeneratedStructuralFollowUp;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
pub struct SourceSpan {
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct SymbolExtractionDiagnostic {
    pub path: PathBuf,
    pub language: Option<SymbolLanguage>,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SymbolExtractionOutput {
    pub symbols: Vec<SymbolDefinition>,
    pub diagnostics: Vec<SymbolExtractionDiagnostic>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralQueryResultMode {
    Matches,
    Captures,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StructuralQueryAnchorSelection {
    PrimaryCapture,
    MatchCapture,
    FirstUsefulNamedCapture,
    FirstCapture,
    CaptureRow,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StructuralQueryCapture {
    pub name: String,
    pub span: SourceSpan,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
pub struct SyntaxTreeInspectionNode {
    pub kind: String,
    pub named: bool,
    pub span: SourceSpan,
    pub excerpt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxTreeInspection {
    pub language: SymbolLanguage,
    pub focus: SyntaxTreeInspectionNode,
    pub ancestors: Vec<SyntaxTreeInspectionNode>,
    pub children: Vec<SyntaxTreeInspectionNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
pub enum HeuristicReferenceEvidence {
    GraphRelation {
        source_symbol_id: String,
        relation: String,
    },
    LexicalToken,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
