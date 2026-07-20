//! Symbol and relation graph facilities used to power navigation-style retrieval. The graph
//! combines heuristic repository analysis with precise SCIP ingest so MCP tools and search flows
//! can ask structure-aware questions through one reusable substrate.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use petgraph::graph::{DiGraph, NodeIndex};
use protobuf::Enum;
use scip::types::symbol_information::Kind as ScipSymbolKindProto;
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    Relationship as ScipRelationshipProto, SymbolInformation as ScipSymbolInformationProto,
};
use serde::Deserialize;
use thiserror::Error;

mod heuristic_graph;
mod precise_graph;
mod precise_store;
mod scip_support;
use precise_store::*;
use scip_support::{
    apply_scip_documents, map_scip_documents, parse_scip_json, parse_scip_protobuf,
};

/// In-memory symbol node keyed by stable symbol id for graph navigation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolNode {
    pub symbol_id: String,
    pub repository_id: String,
    pub display_name: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
}

impl SymbolNode {
    /// Builds a heuristic graph node from registration inputs (path and line are 1-based source coords).
    pub fn new(
        symbol_id: impl Into<String>,
        repository_id: impl Into<String>,
        display_name: impl Into<String>,
        kind: impl Into<String>,
        path: impl Into<String>,
        line: usize,
    ) -> Self {
        Self {
            symbol_id: symbol_id.into(),
            repository_id: repository_id.into(),
            display_name: display_name.into(),
            kind: kind.into(),
            path: path.into(),
            line,
        }
    }
}

/// Heuristic relation edge label used by the symbol graph substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RelationKind {
    DefinedIn,
    RefersTo,
    Calls,
    Implements,
    Extends,
    Contains,
}

impl RelationKind {
    /// Wire/log label for this heuristic relation (`defined_in`, `calls`, …).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DefinedIn => "defined_in",
            Self::RefersTo => "refers_to",
            Self::Calls => "calls",
            Self::Implements => "implements",
            Self::Extends => "extends",
            Self::Contains => "contains",
        }
    }
}

impl std::fmt::Display for RelationKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Directed heuristic relation edge between two registered symbol nodes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRelation {
    pub from_symbol: String,
    pub to_symbol: String,
    pub relation: RelationKind,
}

/// Neighbor symbol returned from graph traversal with its connecting relation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjacentSymbol {
    pub relation: RelationKind,
    pub symbol: SymbolNode,
}

/// Confidence tier assigned to heuristic symbol relations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeuristicConfidence {
    Low,
    Medium,
    High,
}

impl HeuristicConfidence {
    /// Wire/log label for this confidence tier (`low` / `medium` / `high`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    /// Default confidence for a heuristic relation kind (calls/implements high; defined-in low).
    pub fn from_relation(relation: RelationKind) -> Self {
        match relation {
            RelationKind::Calls
            | RelationKind::RefersTo
            | RelationKind::Implements
            | RelationKind::Extends => Self::High,
            RelationKind::Contains => Self::Medium,
            RelationKind::DefinedIn => Self::Low,
        }
    }

    fn rank(self) -> u8 {
        match self {
            Self::High => 3,
            Self::Medium => 2,
            Self::Low => 1,
        }
    }
}

impl std::fmt::Display for HeuristicConfidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Heuristic relation suggestion with source, target, and confidence metadata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeuristicRelationHint {
    pub source_symbol: SymbolNode,
    pub target_symbol: SymbolNode,
    pub relation: RelationKind,
    pub confidence: HeuristicConfidence,
}

/// SCIP symbol-role bitmask value marking a definition occurrence.
pub const SCIP_SYMBOL_ROLE_DEFINITION: u32 = 0x1;

/// Precise SCIP relationship kind stored in the symbol graph substrate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PreciseRelationshipKind {
    Definition,
    Reference,
    Implementation,
    TypeDefinition,
}

impl PreciseRelationshipKind {
    /// Wire/log label for this precise relationship kind.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Definition => "definition",
            Self::Reference => "reference",
            Self::Implementation => "implementation",
            Self::TypeDefinition => "type_definition",
        }
    }
}

impl std::fmt::Display for PreciseRelationshipKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Inclusive source range for a precise SCIP occurrence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreciseRange {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

/// Precise symbol metadata keyed by repository and SCIP symbol identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciseSymbolRecord {
    pub repository_id: String,
    pub symbol: String,
    pub display_name: String,
    pub kind: String,
}

/// Precise symbol occurrence with repository path coordinates and SCIP roles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciseOccurrenceRecord {
    pub repository_id: String,
    pub path: String,
    pub symbol: String,
    pub range: PreciseRange,
    pub symbol_roles: u32,
}

impl PreciseOccurrenceRecord {
    /// Whether SCIP roles mark this occurrence as a definition site.
    pub fn is_definition(&self) -> bool {
        (self.symbol_roles & SCIP_SYMBOL_ROLE_DEFINITION) != 0
    }

    /// Whether `line`/`column` fall inside this occurrence range (column optional = any column on line).
    pub fn contains_location(&self, line: usize, column: Option<usize>) -> bool {
        if line < self.range.start_line || line > self.range.end_line {
            return false;
        }
        let Some(column) = column else {
            return true;
        };
        if line == self.range.start_line && column < self.range.start_column {
            return false;
        }
        if line == self.range.end_line && column > self.range.end_column {
            return false;
        }
        true
    }
}

/// Precise directed relationship between two SCIP symbol identifiers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciseRelationshipRecord {
    pub repository_id: String,
    pub from_symbol: String,
    pub to_symbol: String,
    pub kind: PreciseRelationshipKind,
}

/// Extract the terminal identifier token from a SCIP/display symbol string for navigation matching.
pub(crate) fn precise_navigation_identifier(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    let terminal = trimmed
        .trim_end_matches(['.', '#', '/', ':', '$'])
        .rsplit(['#', '/', '.', ':', '$'])
        .find(|segment| !segment.is_empty())
        .unwrap_or(trimmed);
    let identifier = terminal
        .trim_matches(|character: char| matches!(character, '`' | '\'' | '"'))
        .split(['(', '<', ':'])
        .next()
        .unwrap_or(terminal)
        .trim_matches(|character: char| matches!(character, '`' | '\'' | '"'))
        .trim_end_matches(['.', '#', ':', ')', '>'])
        .trim();

    if identifier.is_empty() {
        None
    } else {
        Some(identifier.to_owned())
    }
}

/// Cardinality summary for precise symbol graph records currently loaded.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreciseGraphCounts {
    pub symbols: usize,
    pub occurrences: usize,
    pub relationships: usize,
}

/// Counts produced by a SCIP ingest or overlay pass into the symbol graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScipIngestSummary {
    pub artifact_label: String,
    pub documents_ingested: usize,
    pub symbols_upserted: usize,
    pub occurrences_upserted: usize,
    pub relationships_upserted: usize,
}

/// Resource ceilings enforced during SCIP decode and document application.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScipResourceBudgets {
    pub max_payload_bytes: usize,
    pub max_documents: usize,
    pub max_elapsed_ms: u64,
}

impl ScipResourceBudgets {
    /// Disables all SCIP ingest ceilings (payload, documents, elapsed time).
    pub const fn unbounded() -> Self {
        Self {
            max_payload_bytes: usize::MAX,
            max_documents: usize::MAX,
            max_elapsed_ms: u64::MAX,
        }
    }
}

impl Default for ScipResourceBudgets {
    fn default() -> Self {
        Self::unbounded()
    }
}

/// Stable invalid-input code for SCIP decode and normalization failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScipInvalidInputCode {
    JsonDecode,
    ProtobufDecode,
    MissingDocumentPath,
    InvalidDocumentPath,
    MissingSymbol,
    InvalidRange,
    InvalidRelationship,
}

impl ScipInvalidInputCode {
    /// Stable diagnostic code string for logs and structured error surfaces.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::JsonDecode => "json_decode",
            Self::ProtobufDecode => "protobuf_decode",
            Self::MissingDocumentPath => "missing_document_path",
            Self::InvalidDocumentPath => "invalid_document_path",
            Self::MissingSymbol => "missing_symbol",
            Self::InvalidRange => "invalid_range",
            Self::InvalidRelationship => "invalid_relationship",
        }
    }
}

impl std::fmt::Display for ScipInvalidInputCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Structured invalid-input diagnostic emitted during SCIP ingest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScipInvalidInputDiagnostic {
    pub artifact_label: String,
    pub code: ScipInvalidInputCode,
    pub message: String,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

impl std::fmt::Display for ScipInvalidInputDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(
                f,
                "scip invalid input ({}): {} at line {line}, column {column}",
                self.code, self.message
            ),
            _ => write!(f, "scip invalid input ({}): {}", self.code, self.message),
        }
    }
}

/// Resource budget dimension exceeded during SCIP ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScipResourceBudgetCode {
    PayloadBytes,
    Documents,
    ElapsedMs,
}

impl ScipResourceBudgetCode {
    /// Stable diagnostic code string for which budget dimension was exceeded.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PayloadBytes => "payload_bytes",
            Self::Documents => "documents",
            Self::ElapsedMs => "elapsed_ms",
        }
    }
}

impl std::fmt::Display for ScipResourceBudgetCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Structured resource-budget diagnostic emitted during SCIP ingest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScipResourceBudgetDiagnostic {
    pub artifact_label: String,
    pub code: ScipResourceBudgetCode,
    pub message: String,
    pub limit: u64,
    pub actual: u64,
}

impl std::fmt::Display for ScipResourceBudgetDiagnostic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "scip resource budget exceeded ({}): {} (actual={}, limit={})",
            self.code, self.message, self.actual, self.limit
        )
    }
}

/// Failure modes for SCIP ingest bounded by input validation and resource budgets.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ScipIngestError {
    #[error("{diagnostic}")]
    InvalidInput {
        diagnostic: ScipInvalidInputDiagnostic,
    },
    #[error("{diagnostic}")]
    ResourceBudgetExceeded {
        diagnostic: ScipResourceBudgetDiagnostic,
    },
}

/// Result alias for SCIP ingest operations bounded by validation and budgets.
pub type ScipIngestResult<T> = Result<T, ScipIngestError>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PreciseOccurrenceKey {
    repository_id: String,
    path: String,
    symbol: String,
    range: PreciseRange,
}

impl From<&PreciseOccurrenceRecord> for PreciseOccurrenceKey {
    fn from(value: &PreciseOccurrenceRecord) -> Self {
        Self {
            repository_id: value.repository_id.clone(),
            path: value.path.clone(),
            symbol: value.symbol.clone(),
            range: value.range,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
struct PreciseRelationshipKey {
    repository_id: String,
    from_symbol: String,
    to_symbol: String,
    kind: PreciseRelationshipKind,
}

impl From<&PreciseRelationshipRecord> for PreciseRelationshipKey {
    fn from(value: &PreciseRelationshipRecord) -> Self {
        Self {
            repository_id: value.repository_id.clone(),
            from_symbol: value.from_symbol.clone(),
            to_symbol: value.to_symbol.clone(),
            kind: value.kind,
        }
    }
}

#[derive(Debug, Deserialize)]
struct ScipIndexJson {
    #[serde(default)]
    documents: Vec<ScipDocumentJson>,
}

#[derive(Debug, Deserialize)]
struct ScipDocumentJson {
    relative_path: String,
    #[serde(default)]
    occurrences: Vec<ScipOccurrenceJson>,
    #[serde(default)]
    symbols: Vec<ScipSymbolInformationJson>,
}

#[derive(Debug, Deserialize)]
struct ScipOccurrenceJson {
    symbol: String,
    range: Vec<u32>,
    #[serde(default)]
    symbol_roles: u32,
}

#[derive(Debug, Deserialize)]
struct ScipSymbolInformationJson {
    symbol: String,
    #[serde(default)]
    display_name: String,
    #[serde(default)]
    kind: Option<ScipSymbolKindJson>,
    #[serde(default)]
    relationships: Vec<ScipRelationshipJson>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ScipSymbolKindJson {
    Numeric(i64),
    Text(String),
}

#[derive(Debug, Deserialize)]
struct ScipRelationshipJson {
    symbol: String,
    #[serde(default)]
    is_reference: bool,
    #[serde(default)]
    is_implementation: bool,
    #[serde(default)]
    is_type_definition: bool,
    #[serde(default)]
    is_definition: bool,
}

#[derive(Debug)]
struct ParsedScipDocument {
    repository_id: String,
    path: String,
    symbols: Vec<PreciseSymbolRecord>,
    occurrences: Vec<PreciseOccurrenceRecord>,
    relationships: Vec<PreciseRelationshipRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScipPayloadEncoding {
    Json,
    Protobuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScipFileIngestMode {
    Replace,
    Overlay,
}

/// Failure modes for heuristic symbol graph mutation and traversal.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SymbolGraphError {
    #[error("symbol graph relation insertion failed: unknown from symbol '{0}'")]
    UnknownFromSymbol(String),

    #[error("symbol graph relation insertion failed: unknown to symbol '{0}'")]
    UnknownToSymbol(String),
}

/// Result alias for symbol graph operations.
pub type SymbolGraphResult<T> = Result<T, SymbolGraphError>;

/// Repository-scoped symbol relation graph that can absorb precise SCIP data while still serving
/// as the shared navigation substrate for heuristic and fallback workflows.
#[derive(Debug, Clone, Default)]
pub struct SymbolGraph {
    graph: DiGraph<SymbolNode, RelationKind>,
    node_by_symbol: BTreeMap<String, NodeIndex>,
    precise_symbols: BTreeMap<(String, String), PreciseSymbolRecord>,
    precise_symbol_keys_by_repository: BTreeMap<String, BTreeSet<String>>,
    precise_symbols_by_file: BTreeMap<(String, String), BTreeSet<String>>,
    precise_symbol_ref_counts: BTreeMap<(String, String), usize>,
    precise_occurrences: BTreeMap<PreciseOccurrenceKey, PreciseOccurrenceRecord>,
    precise_occurrence_keys_by_file: BTreeMap<(String, String), BTreeSet<PreciseOccurrenceKey>>,
    precise_occurrence_keys_by_symbol: BTreeMap<(String, String), BTreeSet<PreciseOccurrenceKey>>,
    precise_relationships: BTreeMap<PreciseRelationshipKey, PreciseRelationshipRecord>,
    precise_relationship_keys_by_from_symbol:
        BTreeMap<(String, String), BTreeSet<PreciseRelationshipKey>>,
    precise_relationship_keys_by_to_symbol:
        BTreeMap<(String, String), BTreeSet<PreciseRelationshipKey>>,
    precise_relationships_by_file: BTreeMap<(String, String), BTreeSet<PreciseRelationshipKey>>,
    precise_relationship_ref_counts: BTreeMap<PreciseRelationshipKey, usize>,
}

impl SymbolGraph {
    /// Replaces per-file precise records from a JSON SCIP index (unbounded resource budgets).
    ///
    /// On success returns upsert counts; fails on decode/normalization errors or invalid document paths.
    pub fn ingest_scip_json(
        &mut self,
        repository_id: &str,
        artifact_label: &str,
        payload: &[u8],
    ) -> ScipIngestResult<ScipIngestSummary> {
        self.ingest_scip_with_budgets_and_mode(
            repository_id,
            artifact_label,
            payload,
            ScipResourceBudgets::default(),
            ScipPayloadEncoding::Json,
            ScipFileIngestMode::Replace,
        )
    }

    /// Like [`Self::ingest_scip_json`] but enforces payload, document, and elapsed-time budgets.
    pub fn ingest_scip_json_with_budgets(
        &mut self,
        repository_id: &str,
        artifact_label: &str,
        payload: &[u8],
        budgets: ScipResourceBudgets,
    ) -> ScipIngestResult<ScipIngestSummary> {
        self.ingest_scip_with_budgets_and_mode(
            repository_id,
            artifact_label,
            payload,
            budgets,
            ScipPayloadEncoding::Json,
            ScipFileIngestMode::Replace,
        )
    }

    /// Merges JSON SCIP records without clearing prior per-file symbols/occurrences/relationships.
    pub(crate) fn overlay_scip_json_with_budgets(
        &mut self,
        repository_id: &str,
        artifact_label: &str,
        payload: &[u8],
        budgets: ScipResourceBudgets,
    ) -> ScipIngestResult<ScipIngestSummary> {
        self.ingest_scip_with_budgets_and_mode(
            repository_id,
            artifact_label,
            payload,
            budgets,
            ScipPayloadEncoding::Json,
            ScipFileIngestMode::Overlay,
        )
    }

    /// Replaces per-file precise records from a protobuf SCIP index (unbounded resource budgets).
    pub fn ingest_scip_protobuf(
        &mut self,
        repository_id: &str,
        artifact_label: &str,
        payload: &[u8],
    ) -> ScipIngestResult<ScipIngestSummary> {
        self.ingest_scip_with_budgets_and_mode(
            repository_id,
            artifact_label,
            payload,
            ScipResourceBudgets::default(),
            ScipPayloadEncoding::Protobuf,
            ScipFileIngestMode::Replace,
        )
    }

    /// Like [`Self::ingest_scip_protobuf`] but enforces payload, document, and elapsed-time budgets.
    pub fn ingest_scip_protobuf_with_budgets(
        &mut self,
        repository_id: &str,
        artifact_label: &str,
        payload: &[u8],
        budgets: ScipResourceBudgets,
    ) -> ScipIngestResult<ScipIngestSummary> {
        self.ingest_scip_with_budgets_and_mode(
            repository_id,
            artifact_label,
            payload,
            budgets,
            ScipPayloadEncoding::Protobuf,
            ScipFileIngestMode::Replace,
        )
    }

    /// Merges protobuf SCIP records without clearing prior per-file precise graph state.
    pub(crate) fn overlay_scip_protobuf_with_budgets(
        &mut self,
        repository_id: &str,
        artifact_label: &str,
        payload: &[u8],
        budgets: ScipResourceBudgets,
    ) -> ScipIngestResult<ScipIngestSummary> {
        self.ingest_scip_with_budgets_and_mode(
            repository_id,
            artifact_label,
            payload,
            budgets,
            ScipPayloadEncoding::Protobuf,
            ScipFileIngestMode::Overlay,
        )
    }

    fn ingest_scip_with_budgets_and_mode(
        &mut self,
        repository_id: &str,
        artifact_label: &str,
        payload: &[u8],
        budgets: ScipResourceBudgets,
        encoding: ScipPayloadEncoding,
        mode: ScipFileIngestMode,
    ) -> ScipIngestResult<ScipIngestSummary> {
        if budgets.max_elapsed_ms == 0 {
            return Err(resource_budget_exceeded(
                artifact_label,
                ScipResourceBudgetCode::ElapsedMs,
                "scip ingest elapsed time budget is zero",
                0,
                0,
            ));
        }

        let payload_bytes = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        let max_payload_bytes = u64::try_from(budgets.max_payload_bytes).unwrap_or(u64::MAX);
        if payload_bytes > max_payload_bytes {
            return Err(resource_budget_exceeded(
                artifact_label,
                ScipResourceBudgetCode::PayloadBytes,
                "scip payload bytes exceed configured budget",
                max_payload_bytes,
                payload_bytes,
            ));
        }

        let started_at = Instant::now();
        enforce_elapsed_budget(artifact_label, started_at, budgets, "decoding payload")?;

        let index_json = match encoding {
            ScipPayloadEncoding::Json => parse_scip_json(artifact_label, payload)?,
            ScipPayloadEncoding::Protobuf => parse_scip_protobuf(artifact_label, payload)?,
        };
        let documents_len = u64::try_from(index_json.documents.len()).unwrap_or(u64::MAX);
        let max_documents = u64::try_from(budgets.max_documents).unwrap_or(u64::MAX);
        if documents_len > max_documents {
            return Err(resource_budget_exceeded(
                artifact_label,
                ScipResourceBudgetCode::Documents,
                "scip document count exceeds configured budget",
                max_documents,
                documents_len,
            ));
        }

        enforce_elapsed_budget(artifact_label, started_at, budgets, "mapping documents")?;
        let mapped_documents = map_scip_documents(repository_id, artifact_label, index_json)?;
        enforce_elapsed_budget(artifact_label, started_at, budgets, "applying documents")?;
        let summary = apply_scip_documents(self, artifact_label, &mapped_documents, mode);
        enforce_elapsed_budget(artifact_label, started_at, budgets, "finalizing ingest")?;
        Ok(summary)
    }
}

#[cfg(test)]
mod tests;
