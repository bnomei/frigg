use std::collections::{BTreeMap, BTreeSet};
use std::time::Instant;

use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use protobuf::Enum;
use scip::types::symbol_information::Kind as ScipSymbolKindProto;
use scip::types::{
    Document as ScipDocumentProto, Index as ScipIndexProto, Occurrence as ScipOccurrenceProto,
    Relationship as ScipRelationshipProto, SymbolInformation as ScipSymbolInformationProto,
};
use serde::Deserialize;
use thiserror::Error;

mod scip_support;
use scip_support::{
    apply_scip_documents, map_scip_documents, parse_scip_json, parse_scip_protobuf,
};

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRelation {
    pub from_symbol: String,
    pub to_symbol: String,
    pub relation: RelationKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjacentSymbol {
    pub relation: RelationKind,
    pub symbol: SymbolNode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HeuristicConfidence {
    Low,
    Medium,
    High,
}

impl HeuristicConfidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeuristicRelationHint {
    pub source_symbol: SymbolNode,
    pub target_symbol: SymbolNode,
    pub relation: RelationKind,
    pub confidence: HeuristicConfidence,
}

pub const SCIP_SYMBOL_ROLE_DEFINITION: u32 = 0x1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PreciseRelationshipKind {
    Definition,
    Reference,
    Implementation,
    TypeDefinition,
}

impl PreciseRelationshipKind {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PreciseRange {
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciseSymbolRecord {
    pub repository_id: String,
    pub symbol: String,
    pub display_name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciseOccurrenceRecord {
    pub repository_id: String,
    pub path: String,
    pub symbol: String,
    pub range: PreciseRange,
    pub symbol_roles: u32,
}

impl PreciseOccurrenceRecord {
    pub fn is_definition(&self) -> bool {
        (self.symbol_roles & SCIP_SYMBOL_ROLE_DEFINITION) != 0
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreciseRelationshipRecord {
    pub repository_id: String,
    pub from_symbol: String,
    pub to_symbol: String,
    pub kind: PreciseRelationshipKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PreciseGraphCounts {
    pub symbols: usize,
    pub occurrences: usize,
    pub relationships: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScipIngestSummary {
    pub artifact_label: String,
    pub documents_ingested: usize,
    pub symbols_upserted: usize,
    pub occurrences_upserted: usize,
    pub relationships_upserted: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScipResourceBudgets {
    pub max_payload_bytes: usize,
    pub max_documents: usize,
    pub max_elapsed_ms: u64,
}

impl ScipResourceBudgets {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScipInvalidInputCode {
    JsonDecode,
    ProtobufDecode,
    MissingDocumentPath,
    MissingSymbol,
    InvalidRange,
    InvalidRelationship,
}

impl ScipInvalidInputCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::JsonDecode => "json_decode",
            Self::ProtobufDecode => "protobuf_decode",
            Self::MissingDocumentPath => "missing_document_path",
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScipResourceBudgetCode {
    PayloadBytes,
    Documents,
    ElapsedMs,
}

impl ScipResourceBudgetCode {
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

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum SymbolGraphError {
    #[error("symbol graph relation insertion failed: unknown from symbol '{0}'")]
    UnknownFromSymbol(String),

    #[error("symbol graph relation insertion failed: unknown to symbol '{0}'")]
    UnknownToSymbol(String),
}

pub type SymbolGraphResult<T> = Result<T, SymbolGraphError>;

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
    pub fn register_symbol(&mut self, symbol: SymbolNode) -> bool {
        if let Some(index) = self.node_by_symbol.get(&symbol.symbol_id).copied() {
            if let Some(existing) = self.graph.node_weight_mut(index) {
                *existing = symbol;
            }
            return false;
        }

        let symbol_id = symbol.symbol_id.clone();
        let index = self.graph.add_node(symbol);
        self.node_by_symbol.insert(symbol_id, index);
        true
    }

    pub fn register_symbols<I>(&mut self, symbols: I)
    where
        I: IntoIterator<Item = SymbolNode>,
    {
        for symbol in symbols {
            let _ = self.register_symbol(symbol);
        }
    }

    pub fn symbol(&self, symbol_id: &str) -> Option<&SymbolNode> {
        let index = self.node_by_symbol.get(symbol_id)?;
        self.graph.node_weight(*index)
    }

    pub fn symbol_count(&self) -> usize {
        self.node_by_symbol.len()
    }

    pub fn relation_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn precise_counts(&self) -> PreciseGraphCounts {
        PreciseGraphCounts {
            symbols: self.precise_symbols.len(),
            occurrences: self.precise_occurrences.len(),
            relationships: self.precise_relationships.len(),
        }
    }

    pub fn clear_precise_data(&mut self) {
        self.precise_symbols.clear();
        self.precise_symbol_keys_by_repository.clear();
        self.precise_symbols_by_file.clear();
        self.precise_symbol_ref_counts.clear();
        self.precise_occurrences.clear();
        self.precise_occurrence_keys_by_file.clear();
        self.precise_occurrence_keys_by_symbol.clear();
        self.precise_relationships.clear();
        self.precise_relationship_keys_by_from_symbol.clear();
        self.precise_relationship_keys_by_to_symbol.clear();
        self.precise_relationships_by_file.clear();
        self.precise_relationship_ref_counts.clear();
    }

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

    pub fn precise_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Option<&PreciseSymbolRecord> {
        self.precise_symbols
            .get(&(repository_id.to_owned(), symbol.to_owned()))
    }

    pub fn precise_symbols_for_repository(&self, repository_id: &str) -> Vec<PreciseSymbolRecord> {
        let mut symbols = self
            .precise_symbol_keys_by_repository
            .get(repository_id)
            .into_iter()
            .flat_map(|symbol_ids| symbol_ids.iter())
            .filter_map(|symbol_id| {
                self.precise_symbols
                    .get(&(repository_id.to_owned(), symbol_id.clone()))
                    .cloned()
            })
            .collect::<Vec<_>>();
        symbols.sort_by(precise_symbol_order);
        symbols
    }

    pub fn precise_occurrences_for_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Vec<PreciseOccurrenceRecord> {
        let mut occurrences = self
            .precise_occurrence_keys_by_symbol
            .get(&precise_symbol_key(repository_id, symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_occurrences.get(key).cloned())
            .collect::<Vec<_>>();
        occurrences.sort_by(precise_occurrence_order);
        occurrences
    }

    pub fn precise_definition_occurrence_for_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Option<PreciseOccurrenceRecord> {
        self.precise_occurrence_keys_by_symbol
            .get(&precise_symbol_key(repository_id, symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_occurrences.get(key))
            .find(|occurrence| occurrence.is_definition())
            .cloned()
    }

    pub fn precise_references_for_symbol(
        &self,
        repository_id: &str,
        symbol: &str,
    ) -> Vec<PreciseOccurrenceRecord> {
        self.precise_occurrences_for_symbol(repository_id, symbol)
            .into_iter()
            .filter(|occurrence| !occurrence.is_definition())
            .collect()
    }

    pub fn precise_occurrences_for_file(
        &self,
        repository_id: &str,
        path: &str,
    ) -> Vec<PreciseOccurrenceRecord> {
        let mut occurrences = self
            .precise_occurrence_keys_by_file
            .get(&precise_file_key(repository_id, path))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_occurrences.get(key).cloned())
            .collect::<Vec<_>>();
        occurrences.sort_by(precise_occurrence_order);
        occurrences
    }

    pub fn select_precise_symbol_for_location(
        &self,
        repository_id: &str,
        path: &str,
        line: usize,
        column: Option<usize>,
    ) -> Option<PreciseSymbolRecord> {
        let mut ranked = self
            .precise_occurrences_for_file(repository_id, path)
            .into_iter()
            .filter(|occurrence| occurrence.range.start_line <= line)
            .filter(|occurrence| {
                column.is_none_or(|value| {
                    occurrence.range.start_line < line || occurrence.range.start_column <= value
                })
            })
            .filter_map(|occurrence| {
                let symbol = self
                    .precise_symbol(repository_id, &occurrence.symbol)?
                    .clone();
                let line_distance = line.saturating_sub(occurrence.range.start_line);
                let column_distance = if line_distance == 0 {
                    column
                        .map(|value| value.saturating_sub(occurrence.range.start_column))
                        .unwrap_or(0)
                } else {
                    0
                };
                let containment_rank = if occurrence.contains_location(line, column) {
                    0u8
                } else {
                    1u8
                };
                Some((
                    containment_rank,
                    line_distance,
                    column_distance,
                    occurrence.range.start_line,
                    occurrence.range.start_column,
                    occurrence.symbol.clone(),
                    symbol,
                ))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.cmp(&right.1))
                .then(left.2.cmp(&right.2))
                .then(right.3.cmp(&left.3))
                .then(right.4.cmp(&left.4))
                .then(left.5.cmp(&right.5))
        });
        ranked
            .into_iter()
            .next()
            .map(|(_, _, _, _, _, _, symbol)| symbol)
    }

    pub fn precise_relationships_from_symbol(
        &self,
        repository_id: &str,
        from_symbol: &str,
    ) -> Vec<PreciseRelationshipRecord> {
        let mut relationships = self
            .precise_relationship_keys_by_from_symbol
            .get(&precise_symbol_key(repository_id, from_symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_relationships.get(key).cloned())
            .collect::<Vec<_>>();
        relationships.sort_by(precise_relationship_order);
        relationships
    }

    pub fn precise_relationships_to_symbol_by_kinds(
        &self,
        repository_id: &str,
        to_symbol: &str,
        kinds: &[PreciseRelationshipKind],
    ) -> Vec<PreciseRelationshipRecord> {
        let mut relationships = self
            .precise_relationship_keys_by_to_symbol
            .get(&precise_symbol_key(repository_id, to_symbol))
            .into_iter()
            .flat_map(|keys| keys.iter())
            .filter_map(|key| self.precise_relationships.get(key))
            .filter(|relationship| kinds.contains(&relationship.kind))
            .cloned()
            .collect::<Vec<_>>();
        relationships.sort_by(precise_relationship_order);
        relationships
    }

    pub fn select_precise_symbol_for_navigation(
        &self,
        repository_id: &str,
        symbol_query: &str,
        fallback_symbol_name: &str,
    ) -> Option<PreciseSymbolRecord> {
        self.matching_precise_symbols_for_navigation(
            repository_id,
            symbol_query,
            fallback_symbol_name,
        )
        .into_iter()
        .next()
    }

    pub fn matching_precise_symbols_for_navigation(
        &self,
        repository_id: &str,
        symbol_query: &str,
        fallback_symbol_name: &str,
    ) -> Vec<PreciseSymbolRecord> {
        let mut ranked = self
            .precise_symbols_for_repository(repository_id)
            .into_iter()
            .filter_map(|precise_symbol| {
                precise_navigation_symbol_rank(&precise_symbol, symbol_query, fallback_symbol_name)
                    .map(|rank| (rank, precise_symbol))
            })
            .collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then(left.1.symbol.cmp(&right.1.symbol))
                .then(left.1.display_name.cmp(&right.1.display_name))
                .then(left.1.kind.cmp(&right.1.kind))
        });
        ranked
            .into_iter()
            .map(|(_, precise_symbol)| precise_symbol)
            .collect()
    }

    pub fn add_relation(
        &mut self,
        from_symbol: &str,
        to_symbol: &str,
        relation: RelationKind,
    ) -> SymbolGraphResult<bool> {
        let from_index = self
            .node_by_symbol
            .get(from_symbol)
            .copied()
            .ok_or_else(|| SymbolGraphError::UnknownFromSymbol(from_symbol.to_owned()))?;
        let to_index = self
            .node_by_symbol
            .get(to_symbol)
            .copied()
            .ok_or_else(|| SymbolGraphError::UnknownToSymbol(to_symbol.to_owned()))?;

        if self
            .graph
            .edges_connecting(from_index, to_index)
            .any(|edge| edge.weight() == &relation)
        {
            return Ok(false);
        }

        self.graph.add_edge(from_index, to_index, relation);
        Ok(true)
    }

    pub fn outgoing_relations(&self, symbol_id: &str) -> Vec<SymbolRelation> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut relations = self
            .graph
            .edges_directed(index, Direction::Outgoing)
            .filter_map(|edge| {
                let from_symbol = self.graph.node_weight(edge.source())?;
                let to_symbol = self.graph.node_weight(edge.target())?;
                Some(SymbolRelation {
                    from_symbol: from_symbol.symbol_id.clone(),
                    to_symbol: to_symbol.symbol_id.clone(),
                    relation: *edge.weight(),
                })
            })
            .collect::<Vec<_>>();

        relations.sort_by(symbol_relation_order);
        relations
    }

    pub fn incoming_relations(&self, symbol_id: &str) -> Vec<SymbolRelation> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut relations = self
            .graph
            .edges_directed(index, Direction::Incoming)
            .filter_map(|edge| {
                let from_symbol = self.graph.node_weight(edge.source())?;
                let to_symbol = self.graph.node_weight(edge.target())?;
                Some(SymbolRelation {
                    from_symbol: from_symbol.symbol_id.clone(),
                    to_symbol: to_symbol.symbol_id.clone(),
                    relation: *edge.weight(),
                })
            })
            .collect::<Vec<_>>();

        relations.sort_by(symbol_relation_order);
        relations
    }

    pub fn outgoing_adjacency(&self, symbol_id: &str) -> Vec<AdjacentSymbol> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut adjacency = self
            .graph
            .edges_directed(index, Direction::Outgoing)
            .filter_map(|edge| {
                let target = self.graph.node_weight(edge.target())?;
                Some(AdjacentSymbol {
                    relation: *edge.weight(),
                    symbol: target.clone(),
                })
            })
            .collect::<Vec<_>>();

        adjacency.sort_by(adjacent_symbol_order);
        adjacency
    }

    pub fn incoming_adjacency(&self, symbol_id: &str) -> Vec<AdjacentSymbol> {
        let Some(index) = self.node_by_symbol.get(symbol_id).copied() else {
            return Vec::new();
        };

        let mut adjacency = self
            .graph
            .edges_directed(index, Direction::Incoming)
            .filter_map(|edge| {
                let source = self.graph.node_weight(edge.source())?;
                Some(AdjacentSymbol {
                    relation: *edge.weight(),
                    symbol: source.clone(),
                })
            })
            .collect::<Vec<_>>();

        adjacency.sort_by(adjacent_symbol_order);
        adjacency
    }

    pub fn heuristic_relation_hints_for_target(
        &self,
        target_symbol_id: &str,
    ) -> Vec<HeuristicRelationHint> {
        let Some(target_index) = self.node_by_symbol.get(target_symbol_id).copied() else {
            return Vec::new();
        };

        let mut hints = self
            .graph
            .edges_directed(target_index, Direction::Incoming)
            .filter_map(|edge| {
                let source_symbol = self.graph.node_weight(edge.source())?;
                let target_symbol = self.graph.node_weight(edge.target())?;
                Some(HeuristicRelationHint {
                    source_symbol: source_symbol.clone(),
                    target_symbol: target_symbol.clone(),
                    relation: *edge.weight(),
                    confidence: HeuristicConfidence::from_relation(*edge.weight()),
                })
            })
            .collect::<Vec<_>>();

        hints.sort_by(heuristic_relation_hint_order);
        hints
    }
}

fn replace_precise_occurrences_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    occurrences: &[PreciseOccurrenceRecord],
) {
    let keys = graph
        .precise_occurrence_keys_by_file
        .remove(&precise_file_key(repository_id, path))
        .unwrap_or_default()
        .into_iter()
        .collect::<Vec<_>>();
    for key in keys {
        remove_precise_occurrence(graph, &key);
    }
    for occurrence in occurrences {
        upsert_precise_occurrence(graph, occurrence);
    }
}

fn overlay_precise_occurrences_for_file(
    graph: &mut SymbolGraph,
    _repository_id: &str,
    _path: &str,
    occurrences: &[PreciseOccurrenceRecord],
) {
    for occurrence in occurrences {
        upsert_precise_occurrence(graph, occurrence);
    }
}

fn replace_precise_symbols_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    symbols: &[PreciseSymbolRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let previous_symbols = graph
        .precise_symbols_by_file
        .remove(&file_key)
        .unwrap_or_default();

    for previous_symbol in previous_symbols {
        decrement_precise_symbol_ref_count(graph, repository_id, &previous_symbol);
    }

    let mut next_symbols = BTreeSet::new();
    for symbol in symbols {
        let symbol_key = (symbol.repository_id.clone(), symbol.symbol.clone());
        upsert_precise_symbol_record(graph, &symbol_key, symbol);
        increment_precise_symbol_ref_count(graph, &symbol_key);
        next_symbols.insert(symbol.symbol.clone());
    }

    graph.precise_symbols_by_file.insert(file_key, next_symbols);
}

fn overlay_precise_symbols_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    symbols: &[PreciseSymbolRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let mut newly_referenced_symbols = Vec::new();
    for symbol in symbols {
        let symbol_key = (symbol.repository_id.clone(), symbol.symbol.clone());
        let is_new_for_file = {
            let file_symbols = graph
                .precise_symbols_by_file
                .entry(file_key.clone())
                .or_default();
            file_symbols.insert(symbol.symbol.clone())
        };
        upsert_precise_symbol_record(graph, &symbol_key, symbol);
        if is_new_for_file {
            newly_referenced_symbols.push(symbol_key);
        }
    }

    for symbol_key in newly_referenced_symbols {
        increment_precise_symbol_ref_count(graph, &symbol_key);
    }
}

fn replace_precise_relationships_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    relationships: &[PreciseRelationshipRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let previous_relationship_keys = graph
        .precise_relationships_by_file
        .remove(&file_key)
        .unwrap_or_default();

    for relationship_key in previous_relationship_keys {
        decrement_precise_relationship_ref_count(graph, &relationship_key);
    }

    let mut next_relationship_keys = BTreeSet::new();
    for relationship in relationships {
        let relationship_key = PreciseRelationshipKey::from(relationship);
        upsert_precise_relationship(graph, relationship);
        increment_precise_relationship_ref_count(graph, &relationship_key);
        next_relationship_keys.insert(relationship_key);
    }

    graph
        .precise_relationships_by_file
        .insert(file_key, next_relationship_keys);
}

fn overlay_precise_relationships_for_file(
    graph: &mut SymbolGraph,
    repository_id: &str,
    path: &str,
    relationships: &[PreciseRelationshipRecord],
) {
    let file_key = precise_file_key(repository_id, path);
    let mut newly_referenced_relationships = Vec::new();
    for relationship in relationships {
        let relationship_key = PreciseRelationshipKey::from(relationship);
        let is_new_for_file = {
            let file_relationships = graph
                .precise_relationships_by_file
                .entry(file_key.clone())
                .or_default();
            file_relationships.insert(relationship_key.clone())
        };
        upsert_precise_relationship(graph, relationship);
        if is_new_for_file {
            newly_referenced_relationships.push(relationship_key);
        }
    }

    for relationship_key in newly_referenced_relationships {
        increment_precise_relationship_ref_count(graph, &relationship_key);
    }
}

fn upsert_precise_symbol_record(
    graph: &mut SymbolGraph,
    symbol_key: &(String, String),
    symbol: &PreciseSymbolRecord,
) {
    graph
        .precise_symbols
        .insert(symbol_key.clone(), symbol.clone());
    graph
        .precise_symbol_keys_by_repository
        .entry(symbol_key.0.clone())
        .or_default()
        .insert(symbol_key.1.clone());
}

fn upsert_precise_occurrence(graph: &mut SymbolGraph, occurrence: &PreciseOccurrenceRecord) {
    let key = PreciseOccurrenceKey::from(occurrence);
    if let Some(previous) = graph.precise_occurrences.get(&key).cloned() {
        remove_precise_occurrence_indexes(graph, &key, &previous);
    }
    graph
        .precise_occurrences
        .insert(key.clone(), occurrence.clone());
    insert_precise_occurrence_indexes(graph, &key, occurrence);
}

fn remove_precise_occurrence(graph: &mut SymbolGraph, key: &PreciseOccurrenceKey) {
    if let Some(previous) = graph.precise_occurrences.remove(key) {
        remove_precise_occurrence_indexes(graph, key, &previous);
    }
}

fn insert_precise_occurrence_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseOccurrenceKey,
    occurrence: &PreciseOccurrenceRecord,
) {
    graph
        .precise_occurrence_keys_by_file
        .entry(precise_file_key(
            &occurrence.repository_id,
            &occurrence.path,
        ))
        .or_default()
        .insert(key.clone());
    graph
        .precise_occurrence_keys_by_symbol
        .entry(precise_symbol_key(
            &occurrence.repository_id,
            &occurrence.symbol,
        ))
        .or_default()
        .insert(key.clone());
}

fn remove_precise_occurrence_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseOccurrenceKey,
    occurrence: &PreciseOccurrenceRecord,
) {
    let file_key = precise_file_key(&occurrence.repository_id, &occurrence.path);
    let remove_file_entry =
        if let Some(keys) = graph.precise_occurrence_keys_by_file.get_mut(&file_key) {
            keys.remove(key);
            keys.is_empty()
        } else {
            false
        };
    if remove_file_entry {
        graph.precise_occurrence_keys_by_file.remove(&file_key);
    }

    let symbol_key = precise_symbol_key(&occurrence.repository_id, &occurrence.symbol);
    let remove_symbol_entry =
        if let Some(keys) = graph.precise_occurrence_keys_by_symbol.get_mut(&symbol_key) {
            keys.remove(key);
            keys.is_empty()
        } else {
            false
        };
    if remove_symbol_entry {
        graph.precise_occurrence_keys_by_symbol.remove(&symbol_key);
    }
}

fn upsert_precise_relationship(graph: &mut SymbolGraph, relationship: &PreciseRelationshipRecord) {
    let key = PreciseRelationshipKey::from(relationship);
    if let Some(previous) = graph.precise_relationships.get(&key).cloned() {
        remove_precise_relationship_indexes(graph, &key, &previous);
    }
    graph
        .precise_relationships
        .insert(key.clone(), relationship.clone());
    insert_precise_relationship_indexes(graph, &key, relationship);
}

fn insert_precise_relationship_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseRelationshipKey,
    relationship: &PreciseRelationshipRecord,
) {
    graph
        .precise_relationship_keys_by_from_symbol
        .entry(precise_symbol_key(
            &relationship.repository_id,
            &relationship.from_symbol,
        ))
        .or_default()
        .insert(key.clone());
    graph
        .precise_relationship_keys_by_to_symbol
        .entry(precise_symbol_key(
            &relationship.repository_id,
            &relationship.to_symbol,
        ))
        .or_default()
        .insert(key.clone());
}

fn remove_precise_relationship_indexes(
    graph: &mut SymbolGraph,
    key: &PreciseRelationshipKey,
    relationship: &PreciseRelationshipRecord,
) {
    let from_symbol_key =
        precise_symbol_key(&relationship.repository_id, &relationship.from_symbol);
    let remove_from_entry = if let Some(keys) = graph
        .precise_relationship_keys_by_from_symbol
        .get_mut(&from_symbol_key)
    {
        keys.remove(key);
        keys.is_empty()
    } else {
        false
    };
    if remove_from_entry {
        graph
            .precise_relationship_keys_by_from_symbol
            .remove(&from_symbol_key);
    }

    let to_symbol_key = precise_symbol_key(&relationship.repository_id, &relationship.to_symbol);
    let remove_to_entry = if let Some(keys) = graph
        .precise_relationship_keys_by_to_symbol
        .get_mut(&to_symbol_key)
    {
        keys.remove(key);
        keys.is_empty()
    } else {
        false
    };
    if remove_to_entry {
        graph
            .precise_relationship_keys_by_to_symbol
            .remove(&to_symbol_key);
    }
}

fn increment_precise_symbol_ref_count(graph: &mut SymbolGraph, symbol_key: &(String, String)) {
    let next = graph
        .precise_symbol_ref_counts
        .get(symbol_key)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    graph
        .precise_symbol_ref_counts
        .insert(symbol_key.clone(), next);
}

fn decrement_precise_symbol_ref_count(graph: &mut SymbolGraph, repository_id: &str, symbol: &str) {
    let symbol_key = precise_symbol_key(repository_id, symbol);
    let current = graph
        .precise_symbol_ref_counts
        .get(&symbol_key)
        .copied()
        .unwrap_or(0);
    match current {
        0 | 1 => {
            graph.precise_symbol_ref_counts.remove(&symbol_key);
            graph.precise_symbols.remove(&symbol_key);
            let remove_repository_entry = if let Some(symbols) = graph
                .precise_symbol_keys_by_repository
                .get_mut(repository_id)
            {
                symbols.remove(symbol);
                symbols.is_empty()
            } else {
                false
            };
            if remove_repository_entry {
                graph
                    .precise_symbol_keys_by_repository
                    .remove(repository_id);
            }
        }
        count => {
            graph
                .precise_symbol_ref_counts
                .insert(symbol_key, count - 1);
        }
    }
}

fn increment_precise_relationship_ref_count(
    graph: &mut SymbolGraph,
    relationship_key: &PreciseRelationshipKey,
) {
    let next = graph
        .precise_relationship_ref_counts
        .get(relationship_key)
        .copied()
        .unwrap_or(0)
        .saturating_add(1);
    graph
        .precise_relationship_ref_counts
        .insert(relationship_key.clone(), next);
}

fn decrement_precise_relationship_ref_count(
    graph: &mut SymbolGraph,
    relationship_key: &PreciseRelationshipKey,
) {
    let current = graph
        .precise_relationship_ref_counts
        .get(relationship_key)
        .copied()
        .unwrap_or(0);
    match current {
        0 | 1 => {
            graph
                .precise_relationship_ref_counts
                .remove(relationship_key);
            if let Some(relationship) = graph.precise_relationships.remove(relationship_key) {
                remove_precise_relationship_indexes(graph, relationship_key, &relationship);
            }
        }
        count => {
            graph
                .precise_relationship_ref_counts
                .insert(relationship_key.clone(), count - 1);
        }
    }
}

fn precise_symbol_key(repository_id: &str, symbol: &str) -> (String, String) {
    (repository_id.to_owned(), symbol.to_owned())
}

fn precise_file_key(repository_id: &str, path: &str) -> (String, String) {
    (repository_id.to_owned(), path.to_owned())
}

fn precise_symbol_order(
    left: &PreciseSymbolRecord,
    right: &PreciseSymbolRecord,
) -> std::cmp::Ordering {
    left.repository_id
        .cmp(&right.repository_id)
        .then(left.symbol.cmp(&right.symbol))
        .then(left.display_name.cmp(&right.display_name))
        .then(left.kind.cmp(&right.kind))
}

fn precise_occurrence_order(
    left: &PreciseOccurrenceRecord,
    right: &PreciseOccurrenceRecord,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.range.start_line.cmp(&right.range.start_line))
        .then(left.range.start_column.cmp(&right.range.start_column))
        .then(left.range.end_line.cmp(&right.range.end_line))
        .then(left.range.end_column.cmp(&right.range.end_column))
        .then(left.symbol.cmp(&right.symbol))
        .then(left.symbol_roles.cmp(&right.symbol_roles))
}

fn precise_relationship_order(
    left: &PreciseRelationshipRecord,
    right: &PreciseRelationshipRecord,
) -> std::cmp::Ordering {
    left.from_symbol
        .cmp(&right.from_symbol)
        .then(left.to_symbol.cmp(&right.to_symbol))
        .then(left.kind.cmp(&right.kind))
}

fn precise_navigation_symbol_rank(
    precise_symbol: &PreciseSymbolRecord,
    symbol_query: &str,
    fallback_symbol_name: &str,
) -> Option<u8> {
    if precise_symbol.symbol == symbol_query {
        return Some(0);
    }
    if precise_symbol.display_name == symbol_query {
        return Some(1);
    }
    if precise_symbol
        .display_name
        .eq_ignore_ascii_case(symbol_query)
    {
        return Some(2);
    }
    if precise_symbol.display_name == fallback_symbol_name {
        return Some(3);
    }
    if precise_symbol
        .display_name
        .eq_ignore_ascii_case(fallback_symbol_name)
    {
        return Some(4);
    }

    None
}

fn invalid_input(
    artifact_label: &str,
    code: ScipInvalidInputCode,
    message: impl Into<String>,
) -> ScipIngestError {
    ScipIngestError::InvalidInput {
        diagnostic: ScipInvalidInputDiagnostic {
            artifact_label: artifact_label.to_owned(),
            code,
            message: message.into(),
            line: None,
            column: None,
        },
    }
}

fn resource_budget_exceeded(
    artifact_label: &str,
    code: ScipResourceBudgetCode,
    message: impl Into<String>,
    limit: u64,
    actual: u64,
) -> ScipIngestError {
    ScipIngestError::ResourceBudgetExceeded {
        diagnostic: ScipResourceBudgetDiagnostic {
            artifact_label: artifact_label.to_owned(),
            code,
            message: message.into(),
            limit,
            actual,
        },
    }
}

fn enforce_elapsed_budget(
    artifact_label: &str,
    started_at: Instant,
    budgets: ScipResourceBudgets,
    phase: &str,
) -> ScipIngestResult<()> {
    if budgets.max_elapsed_ms == u64::MAX {
        return Ok(());
    }

    let elapsed_ms = u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
    if elapsed_ms > budgets.max_elapsed_ms {
        return Err(resource_budget_exceeded(
            artifact_label,
            ScipResourceBudgetCode::ElapsedMs,
            format!("scip ingest elapsed time exceeded while {phase}"),
            budgets.max_elapsed_ms,
            elapsed_ms,
        ));
    }

    Ok(())
}

fn symbol_relation_order(left: &SymbolRelation, right: &SymbolRelation) -> std::cmp::Ordering {
    left.from_symbol
        .cmp(&right.from_symbol)
        .then(left.to_symbol.cmp(&right.to_symbol))
        .then(left.relation.cmp(&right.relation))
}

fn adjacent_symbol_order(left: &AdjacentSymbol, right: &AdjacentSymbol) -> std::cmp::Ordering {
    left.relation
        .cmp(&right.relation)
        .then(left.symbol.symbol_id.cmp(&right.symbol.symbol_id))
        .then(left.symbol.path.cmp(&right.symbol.path))
        .then(left.symbol.line.cmp(&right.symbol.line))
}

fn heuristic_relation_hint_order(
    left: &HeuristicRelationHint,
    right: &HeuristicRelationHint,
) -> std::cmp::Ordering {
    right
        .confidence
        .rank()
        .cmp(&left.confidence.rank())
        .then(left.source_symbol.path.cmp(&right.source_symbol.path))
        .then(left.source_symbol.line.cmp(&right.source_symbol.line))
        .then(
            left.source_symbol
                .symbol_id
                .cmp(&right.source_symbol.symbol_id),
        )
        .then(
            left.target_symbol
                .symbol_id
                .cmp(&right.target_symbol.symbol_id),
        )
        .then(left.relation.cmp(&right.relation))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use protobuf::{EnumOrUnknown, Message};

    use super::*;

    fn symbol(
        symbol_id: &str,
        display_name: &str,
        kind: &str,
        path: &str,
        line: usize,
    ) -> SymbolNode {
        SymbolNode::new(symbol_id, "repo-001", display_name, kind, path, line)
    }

    #[test]
    fn relation_traversal_registers_symbols_and_relations_deterministically() {
        let mut graph = SymbolGraph::default();
        graph.register_symbols([
            symbol("sym-class-user", "User", "class", "src/user.php", 3),
            symbol("sym-method-save", "save", "method", "src/user.php", 5),
            symbol(
                "sym-method-validate",
                "validate",
                "method",
                "src/user.php",
                9,
            ),
            symbol("sym-const-limit", "LIMIT", "constant", "src/user.php", 12),
        ]);

        assert_eq!(graph.symbol_count(), 4);

        assert!(
            graph
                .add_relation("sym-method-save", "sym-class-user", RelationKind::DefinedIn)
                .expect("defined_in insertion should succeed")
        );
        assert!(
            graph
                .add_relation(
                    "sym-method-save",
                    "sym-method-validate",
                    RelationKind::Calls
                )
                .expect("calls insertion should succeed")
        );
        assert!(
            graph
                .add_relation("sym-method-save", "sym-const-limit", RelationKind::RefersTo)
                .expect("refers_to insertion should succeed")
        );

        // Duplicate edges with same relation are rejected deterministically.
        assert!(
            !graph
                .add_relation(
                    "sym-method-save",
                    "sym-method-validate",
                    RelationKind::Calls
                )
                .expect("duplicate calls edge should be deduplicated")
        );

        assert_eq!(graph.relation_count(), 3);

        let outgoing = graph.outgoing_relations("sym-method-save");
        assert_eq!(
            outgoing,
            vec![
                SymbolRelation {
                    from_symbol: "sym-method-save".to_string(),
                    to_symbol: "sym-class-user".to_string(),
                    relation: RelationKind::DefinedIn,
                },
                SymbolRelation {
                    from_symbol: "sym-method-save".to_string(),
                    to_symbol: "sym-const-limit".to_string(),
                    relation: RelationKind::RefersTo,
                },
                SymbolRelation {
                    from_symbol: "sym-method-save".to_string(),
                    to_symbol: "sym-method-validate".to_string(),
                    relation: RelationKind::Calls,
                },
            ]
        );

        let incoming = graph.incoming_relations("sym-method-validate");
        assert_eq!(
            incoming,
            vec![SymbolRelation {
                from_symbol: "sym-method-save".to_string(),
                to_symbol: "sym-method-validate".to_string(),
                relation: RelationKind::Calls,
            }]
        );

        let outgoing_neighbors = graph.outgoing_adjacency("sym-method-save");
        assert_eq!(
            outgoing_neighbors
                .iter()
                .map(|adjacent| (adjacent.relation, adjacent.symbol.symbol_id.clone()))
                .collect::<Vec<_>>(),
            vec![
                (RelationKind::DefinedIn, "sym-class-user".to_string()),
                (RelationKind::RefersTo, "sym-const-limit".to_string()),
                (RelationKind::Calls, "sym-method-validate".to_string()),
            ]
        );
    }

    #[test]
    fn relation_traversal_requires_pre_registered_symbols() {
        let mut graph = SymbolGraph::default();
        graph.register_symbol(symbol(
            "sym-existing",
            "existing",
            "function",
            "src/lib.rs",
            1,
        ));

        let from_error = graph
            .add_relation("sym-missing", "sym-existing", RelationKind::RefersTo)
            .expect_err("missing source symbol should fail relation insertion");
        assert_eq!(
            from_error,
            SymbolGraphError::UnknownFromSymbol("sym-missing".to_string())
        );

        let to_error = graph
            .add_relation("sym-existing", "sym-also-missing", RelationKind::RefersTo)
            .expect_err("missing target symbol should fail relation insertion");
        assert_eq!(
            to_error,
            SymbolGraphError::UnknownToSymbol("sym-also-missing".to_string())
        );
    }

    #[test]
    fn relation_traversal_register_symbol_upserts_existing_entry() {
        let mut graph = SymbolGraph::default();

        assert!(graph.register_symbol(symbol("sym-user", "User", "struct", "src/user.rs", 3,)));

        assert!(!graph.register_symbol(symbol(
            "sym-user",
            "UserRenamed",
            "struct",
            "src/user.rs",
            44,
        )));

        let symbol = graph
            .symbol("sym-user")
            .expect("registered symbol should be queryable");
        assert_eq!(symbol.display_name, "UserRenamed");
        assert_eq!(symbol.line, 44);
        assert_eq!(graph.symbol_count(), 1);
    }

    #[test]
    fn heuristic_relation_hints_are_confidence_ranked_and_deterministic() {
        let mut graph = SymbolGraph::default();
        graph.register_symbols([
            symbol("sym-target", "User", "class", "src/user.php", 3),
            symbol("sym-calls", "save", "method", "src/service.php", 11),
            symbol("sym-contains", "Service", "class", "src/service.php", 1),
            symbol("sym-defined-in", "User", "class", "src/user.php", 3),
        ]);

        assert!(
            graph
                .add_relation("sym-calls", "sym-target", RelationKind::Calls)
                .expect("calls relation should be accepted")
        );
        assert!(
            graph
                .add_relation("sym-contains", "sym-target", RelationKind::Contains)
                .expect("contains relation should be accepted")
        );
        assert!(
            graph
                .add_relation("sym-defined-in", "sym-target", RelationKind::DefinedIn)
                .expect("defined_in relation should be accepted")
        );

        let first = graph.heuristic_relation_hints_for_target("sym-target");
        let second = graph.heuristic_relation_hints_for_target("sym-target");

        assert_eq!(first, second, "hint ordering should be deterministic");
        assert_eq!(
            first
                .iter()
                .map(|hint| (hint.source_symbol.symbol_id.clone(), hint.confidence))
                .collect::<Vec<_>>(),
            vec![
                ("sym-calls".to_string(), HeuristicConfidence::High),
                ("sym-contains".to_string(), HeuristicConfidence::Medium),
                ("sym-defined-in".to_string(), HeuristicConfidence::Low),
            ]
        );
    }

    #[test]
    fn scip_ingest_maps_and_persists_normalized_records() {
        let mut graph = SymbolGraph::default();
        let payload = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#User", "range": [1, 18, 22], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Entity", "is_reference": true },
                    { "symbol": "scip-rust pkg a#Entity", "is_implementation": true }
                  ]
                }
              ]
            },
            {
              "relative_path": "src/base.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#Entity", "range": [0, 11, 17], "symbol_roles": 1 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg a#Entity", "display_name": "Entity", "kind": 7, "relationships": [] }
              ]
            }
          ]
        }"#;

        let summary = graph
            .ingest_scip_json("repo-001", "fixture:scip.json", payload)
            .expect("valid scip payload should ingest successfully");
        assert_eq!(summary.documents_ingested, 2);
        assert_eq!(summary.symbols_upserted, 2);
        assert_eq!(summary.occurrences_upserted, 3);
        assert_eq!(summary.relationships_upserted, 2);

        let counts = graph.precise_counts();
        assert_eq!(counts.symbols, 2);
        assert_eq!(counts.occurrences, 3);
        assert_eq!(counts.relationships, 2);

        let user_symbol = graph
            .precise_symbol("repo-001", "scip-rust pkg a#User")
            .expect("expected precise symbol");
        assert_eq!(user_symbol.display_name, "User");
        assert_eq!(user_symbol.kind, "struct");

        let user_occurrences =
            graph.precise_occurrences_for_symbol("repo-001", "scip-rust pkg a#User");
        assert_eq!(
            user_occurrences
                .iter()
                .map(|occurrence| {
                    (
                        occurrence.path.clone(),
                        occurrence.range.start_line,
                        occurrence.range.start_column,
                        occurrence.symbol_roles,
                    )
                })
                .collect::<Vec<_>>(),
            vec![
                ("src/a.rs".to_string(), 1, 8, 1),
                ("src/a.rs".to_string(), 2, 19, 8),
            ]
        );

        let user_references =
            graph.precise_references_for_symbol("repo-001", "scip-rust pkg a#User");
        assert_eq!(user_references.len(), 1);
        assert_eq!(user_references[0].range.start_line, 2);
        assert!(!user_references[0].is_definition());

        let relationships =
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#User");
        assert_eq!(
            relationships
                .iter()
                .map(|relationship| relationship.kind)
                .collect::<Vec<_>>(),
            vec![
                PreciseRelationshipKind::Reference,
                PreciseRelationshipKind::Implementation
            ]
        );
    }

    #[test]
    fn scip_protobuf_ingest_maps_and_persists_normalized_records() {
        let mut graph = SymbolGraph::default();
        let mut index = ScipIndexProto::new();

        let mut user_doc = ScipDocumentProto::new();
        user_doc.relative_path = "src/a.rs".to_owned();
        let mut user_definition = ScipOccurrenceProto::new();
        user_definition.symbol = "scip-rust pkg a#User".to_owned();
        user_definition.range = vec![0, 7, 11];
        user_definition.symbol_roles = 1;
        user_doc.occurrences.push(user_definition);

        let mut user_reference = ScipOccurrenceProto::new();
        user_reference.symbol = "scip-rust pkg a#User".to_owned();
        user_reference.range = vec![1, 18, 22];
        user_reference.symbol_roles = 8;
        user_doc.occurrences.push(user_reference);

        let mut user_symbol = ScipSymbolInformationProto::new();
        user_symbol.symbol = "scip-rust pkg a#User".to_owned();
        user_symbol.display_name = "User".to_owned();
        user_symbol.kind = EnumOrUnknown::from_i32(7);

        let mut relationship_reference = ScipRelationshipProto::new();
        relationship_reference.symbol = "scip-rust pkg a#Entity".to_owned();
        relationship_reference.is_reference = true;
        user_symbol.relationships.push(relationship_reference);

        let mut relationship_implementation = ScipRelationshipProto::new();
        relationship_implementation.symbol = "scip-rust pkg a#Entity".to_owned();
        relationship_implementation.is_implementation = true;
        user_symbol.relationships.push(relationship_implementation);
        user_doc.symbols.push(user_symbol);

        let mut entity_doc = ScipDocumentProto::new();
        entity_doc.relative_path = "src/base.rs".to_owned();
        let mut entity_occurrence = ScipOccurrenceProto::new();
        entity_occurrence.symbol = "scip-rust pkg a#Entity".to_owned();
        entity_occurrence.range = vec![0, 11, 17];
        entity_occurrence.symbol_roles = 1;
        entity_doc.occurrences.push(entity_occurrence);

        let mut entity_symbol = ScipSymbolInformationProto::new();
        entity_symbol.symbol = "scip-rust pkg a#Entity".to_owned();
        entity_symbol.display_name = "Entity".to_owned();
        entity_symbol.kind = EnumOrUnknown::from_i32(7);
        entity_doc.symbols.push(entity_symbol);

        index.documents.push(user_doc);
        index.documents.push(entity_doc);

        let payload = index
            .write_to_bytes()
            .expect("protobuf fixture payload should serialize");

        let summary = graph
            .ingest_scip_protobuf("repo-001", "fixture:scip.scip", &payload)
            .expect("valid protobuf scip payload should ingest successfully");
        assert_eq!(summary.documents_ingested, 2);
        assert_eq!(summary.symbols_upserted, 2);
        assert_eq!(summary.occurrences_upserted, 3);
        assert_eq!(summary.relationships_upserted, 2);

        let counts = graph.precise_counts();
        assert_eq!(counts.symbols, 2);
        assert_eq!(counts.occurrences, 3);
        assert_eq!(counts.relationships, 2);

        let relationships =
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#User");
        assert_eq!(
            relationships
                .iter()
                .map(|relationship| relationship.kind)
                .collect::<Vec<_>>(),
            vec![
                PreciseRelationshipKind::Reference,
                PreciseRelationshipKind::Implementation
            ]
        );
    }

    #[test]
    fn precise_navigation_symbol_selection_is_deterministic() {
        let mut graph = SymbolGraph::default();
        let payload = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [],
              "symbols": [
                { "symbol": "scip-rust pkg a#User", "display_name": "User", "kind": "struct", "relationships": [] },
                { "symbol": "scip-rust pkg a#user_lower", "display_name": "user", "kind": "struct", "relationships": [] }
              ]
            }
          ]
        }"#;
        graph
            .ingest_scip_json("repo-001", "fixture:precise-navigation.json", payload)
            .expect("fixture payload should ingest");

        let exact = graph
            .select_precise_symbol_for_navigation("repo-001", "scip-rust pkg a#User", "fallback")
            .expect("exact symbol query should resolve");
        assert_eq!(exact.symbol, "scip-rust pkg a#User");

        let case_insensitive = graph
            .select_precise_symbol_for_navigation("repo-001", "USER", "fallback")
            .expect("case-insensitive display-name query should resolve");
        assert_eq!(case_insensitive.symbol, "scip-rust pkg a#User");

        let fallback = graph
            .select_precise_symbol_for_navigation("repo-001", "missing", "user")
            .expect("fallback display-name query should resolve");
        assert_eq!(fallback.symbol, "scip-rust pkg a#user_lower");

        assert!(
            graph
                .select_precise_symbol_for_navigation("repo-001", "missing", "also-missing")
                .is_none(),
            "missing query and fallback should return None"
        );
    }

    #[test]
    fn precise_navigation_location_selection_prefers_containing_occurrence() {
        let mut graph = SymbolGraph::default();
        let payload = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#Entity", "range": [0, 13, 19], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#User", "range": [1, 18, 22], "symbol_roles": 8 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg a#User", "display_name": "User", "kind": "struct", "relationships": [] },
                { "symbol": "scip-rust pkg a#Entity", "display_name": "Entity", "kind": "struct", "relationships": [] }
              ]
            }
          ]
        }"#;
        graph
            .ingest_scip_json("repo-001", "fixture:precise-location.json", payload)
            .expect("fixture payload should ingest");

        let containing = graph
            .select_precise_symbol_for_location("repo-001", "src/a.rs", 1, Some(9))
            .expect("containing occurrence should resolve");
        assert_eq!(containing.symbol, "scip-rust pkg a#User");

        let later = graph
            .select_precise_symbol_for_location("repo-001", "src/a.rs", 1, Some(15))
            .expect("later containing occurrence should resolve");
        assert_eq!(later.symbol, "scip-rust pkg a#Entity");

        let reference = graph
            .select_precise_symbol_for_location("repo-001", "src/a.rs", 2, Some(20))
            .expect("reference occurrence should resolve");
        assert_eq!(reference.symbol, "scip-rust pkg a#User");
    }

    #[test]
    fn scip_ingest_returns_typed_invalid_input_and_preserves_state() {
        let mut graph = SymbolGraph::default();
        let valid = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                { "symbol": "scip-rust pkg a#User", "display_name": "User", "kind": "struct", "relationships": [] }
              ]
            }
          ]
        }"#;
        graph
            .ingest_scip_json("repo-001", "fixture:valid.json", valid)
            .expect("valid ingest should succeed");
        let before = graph.precise_counts();

        let invalid = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7], "symbol_roles": 8 }
              ],
              "symbols": []
            }
          ]
        }"#;
        let error = graph
            .ingest_scip_json("repo-001", "fixture:invalid-range.json", invalid)
            .expect_err("invalid range payload should fail with typed invalid-input error");
        assert_eq!(
            error,
            ScipIngestError::InvalidInput {
                diagnostic: ScipInvalidInputDiagnostic {
                    artifact_label: "fixture:invalid-range.json".to_string(),
                    code: ScipInvalidInputCode::InvalidRange,
                    message: "occurrence range for symbol 'scip-rust pkg a#User' in 'src/a.rs' must have 3 or 4 numbers".to_string(),
                    line: None,
                    column: None,
                }
            }
        );

        let after = graph.precise_counts();
        assert_eq!(
            before, after,
            "failed ingest must not mutate precise graph state"
        );
        assert_eq!(
            graph
                .precise_occurrences_for_symbol("repo-001", "scip-rust pkg a#User")
                .len(),
            1
        );
    }

    #[test]
    fn scip_ingest_rejects_payload_budget_overflow_with_typed_error() {
        let mut graph = SymbolGraph::default();
        let payload = br#"{
          "documents": [],
          "padding": "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx"
        }"#;

        let error = graph
            .ingest_scip_json_with_budgets(
                "repo-001",
                "fixture:payload-budget.json",
                payload,
                ScipResourceBudgets {
                    max_payload_bytes: 16,
                    max_documents: usize::MAX,
                    max_elapsed_ms: u64::MAX,
                },
            )
            .expect_err("oversized payload should fail with typed resource-budget error");
        assert_eq!(
            error,
            ScipIngestError::ResourceBudgetExceeded {
                diagnostic: ScipResourceBudgetDiagnostic {
                    artifact_label: "fixture:payload-budget.json".to_string(),
                    code: ScipResourceBudgetCode::PayloadBytes,
                    message: "scip payload bytes exceed configured budget".to_string(),
                    limit: 16,
                    actual: u64::try_from(payload.len()).unwrap_or(u64::MAX),
                },
            }
        );
        assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
    }

    #[test]
    fn scip_ingest_rejects_document_budget_overflow_with_typed_error() {
        let mut graph = SymbolGraph::default();
        let payload = br#"{
          "documents": [
            { "relative_path": "src/a.rs", "occurrences": [], "symbols": [] },
            { "relative_path": "src/b.rs", "occurrences": [], "symbols": [] }
          ]
        }"#;

        let error = graph
            .ingest_scip_json_with_budgets(
                "repo-001",
                "fixture:document-budget.json",
                payload,
                ScipResourceBudgets {
                    max_payload_bytes: usize::MAX,
                    max_documents: 1,
                    max_elapsed_ms: u64::MAX,
                },
            )
            .expect_err("document overflow should fail with typed resource-budget error");
        assert_eq!(
            error,
            ScipIngestError::ResourceBudgetExceeded {
                diagnostic: ScipResourceBudgetDiagnostic {
                    artifact_label: "fixture:document-budget.json".to_string(),
                    code: ScipResourceBudgetCode::Documents,
                    message: "scip document count exceeds configured budget".to_string(),
                    limit: 1,
                    actual: 2,
                },
            }
        );
        assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
    }

    #[test]
    fn scip_ingest_rejects_zero_elapsed_budget_with_typed_error() {
        let mut graph = SymbolGraph::default();
        let payload = br#"{
          "documents": []
        }"#;

        let error = graph
            .ingest_scip_json_with_budgets(
                "repo-001",
                "fixture:elapsed-budget.json",
                payload,
                ScipResourceBudgets {
                    max_payload_bytes: usize::MAX,
                    max_documents: usize::MAX,
                    max_elapsed_ms: 0,
                },
            )
            .expect_err("zero elapsed budget should fail deterministically");
        assert_eq!(
            error,
            ScipIngestError::ResourceBudgetExceeded {
                diagnostic: ScipResourceBudgetDiagnostic {
                    artifact_label: "fixture:elapsed-budget.json".to_string(),
                    code: ScipResourceBudgetCode::ElapsedMs,
                    message: "scip ingest elapsed time budget is zero".to_string(),
                    limit: 0,
                    actual: 0,
                },
            }
        );
        assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
    }

    #[test]
    fn scip_ingest_replaces_file_level_precise_occurrences() {
        let mut graph = SymbolGraph::default();
        let first = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#User", "range": [1, 7, 11], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Entity", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#;
        graph
            .ingest_scip_json("repo-001", "fixture:first.json", first)
            .expect("first ingest should succeed");

        let second = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [2, 7, 11], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;
        graph
            .ingest_scip_json("repo-001", "fixture:second.json", second)
            .expect("second ingest should succeed");

        let file_occurrences = graph.precise_occurrences_for_file("repo-001", "src/a.rs");
        assert_eq!(file_occurrences.len(), 1);
        assert_eq!(file_occurrences[0].range.start_line, 3);

        let relationships =
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#User");
        assert!(
            relationships.is_empty(),
            "file-level reingest should replace prior relationships"
        );
    }

    #[test]
    fn scip_incremental_update_replaces_only_target_file_and_preserves_unaffected_data() {
        let mut graph = SymbolGraph::default();
        let initial = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Base", "is_reference": true }
                  ]
                }
              ]
            },
            {
              "relative_path": "src/b.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg b#Service", "range": [0, 7, 14], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg b#Service",
                  "display_name": "Service",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg a#Base", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#;

        graph
            .ingest_scip_json("repo-001", "fixture:initial.json", initial)
            .expect("initial payload should ingest");

        let before_b_occurrences = graph.precise_occurrences_for_file("repo-001", "src/b.rs");
        let before_b_relationships =
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg b#Service");
        assert_eq!(before_b_occurrences.len(), 1);
        assert_eq!(before_b_relationships.len(), 1);
        assert!(
            graph
                .precise_symbol("repo-001", "scip-rust pkg a#User")
                .is_some(),
            "expected initial symbol for src/a.rs"
        );

        let incremental = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#Account", "range": [2, 7, 14], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#Account",
                  "display_name": "Account",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;

        graph
            .ingest_scip_json("repo-001", "fixture:incremental.json", incremental)
            .expect("incremental payload should ingest");

        // Updated file replaced.
        assert!(
            graph
                .precise_symbol("repo-001", "scip-rust pkg a#User")
                .is_none(),
            "old symbol from updated file should be removed"
        );
        assert!(
            graph
                .precise_symbol("repo-001", "scip-rust pkg a#Account")
                .is_some(),
            "new symbol from updated file should be present"
        );
        let a_occurrences = graph.precise_occurrences_for_file("repo-001", "src/a.rs");
        assert_eq!(a_occurrences.len(), 1);
        assert_eq!(a_occurrences[0].symbol, "scip-rust pkg a#Account");
        assert!(
            graph
                .precise_relationships_from_symbol("repo-001", "scip-rust pkg a#Account")
                .is_empty(),
            "updated file relationships should reflect replacement payload"
        );

        // Unaffected file preserved exactly.
        assert_eq!(
            graph.precise_occurrences_for_file("repo-001", "src/b.rs"),
            before_b_occurrences
        );
        assert_eq!(
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg b#Service"),
            before_b_relationships
        );
    }

    #[test]
    fn scip_incremental_update_is_deterministic_across_repeated_reingest() {
        let mut graph = SymbolGraph::default();
        let seed = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#User", "range": [0, 7, 11], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#User",
                  "display_name": "User",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            },
            {
              "relative_path": "src/b.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg b#Service", "range": [0, 7, 14], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg b#Service",
                  "display_name": "Service",
                  "kind": "struct",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;
        graph
            .ingest_scip_json("repo-001", "fixture:seed.json", seed)
            .expect("seed ingest should succeed");

        let incremental = br#"{
          "documents": [
            {
              "relative_path": "src/a.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg a#Account", "range": [2, 7, 14], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg a#Account", "range": [3, 10, 17], "symbol_roles": 8 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg a#Account",
                  "display_name": "Account",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg b#Service", "is_reference": true }
                  ]
                }
              ]
            }
          ]
        }"#;
        graph
            .ingest_scip_json("repo-001", "fixture:inc-1.json", incremental)
            .expect("first incremental ingest should succeed");

        let counts_after_first = graph.precise_counts();
        let file_a_after_first = graph.precise_occurrences_for_file("repo-001", "src/a.rs");
        let refs_after_first =
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#Account");

        graph
            .ingest_scip_json("repo-001", "fixture:inc-2.json", incremental)
            .expect("second incremental ingest should succeed");

        assert_eq!(graph.precise_counts(), counts_after_first);
        assert_eq!(
            graph.precise_occurrences_for_file("repo-001", "src/a.rs"),
            file_a_after_first
        );
        assert_eq!(
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg a#Account"),
            refs_after_first
        );
        assert!(
            graph
                .precise_symbol("repo-001", "scip-rust pkg a#User")
                .is_none(),
            "stale symbols must not reappear across repeated incremental updates"
        );
    }

    #[test]
    fn scip_overlay_ingest_preserves_overlapping_same_file_precise_data() {
        let mut graph = SymbolGraph::default();
        let canary = br#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "scip-rust pkg repo#Service", "range": [0, 10, 17], "symbol_roles": 1 },
                { "symbol": "scip-rust pkg repo#Impl", "range": [1, 11, 15], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "scip-rust pkg repo#Service",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                },
                {
                  "symbol": "scip-rust pkg repo#Impl",
                  "display_name": "Impl",
                  "kind": "struct",
                  "relationships": [
                    { "symbol": "scip-rust pkg repo#Service", "is_implementation": true }
                  ]
                }
              ]
            }
          ]
        }"#;
        graph
            .overlay_scip_json_with_budgets(
                "repo-001",
                "fixture:canary.json",
                canary,
                ScipResourceBudgets::default(),
            )
            .expect("canary overlay should ingest");

        let main = br#"{
          "documents": [
            {
              "relative_path": "src/lib.rs",
              "occurrences": [
                { "symbol": "rust-analyzer cargo repo 0.1.0 svc/Service#", "range": [0, 10, 17], "symbol_roles": 1 }
              ],
              "symbols": [
                {
                  "symbol": "rust-analyzer cargo repo 0.1.0 svc/Service#",
                  "display_name": "Service",
                  "kind": "trait",
                  "relationships": []
                }
              ]
            }
          ]
        }"#;
        graph
            .overlay_scip_json_with_budgets(
                "repo-001",
                "fixture:main.json",
                main,
                ScipResourceBudgets::default(),
            )
            .expect("main overlay should ingest");

        let matched =
            graph.matching_precise_symbols_for_navigation("repo-001", "Service", "Service");
        assert_eq!(matched.len(), 2);
        assert_eq!(
            matched[0].symbol,
            "rust-analyzer cargo repo 0.1.0 svc/Service#"
        );
        assert_eq!(matched[1].symbol, "scip-rust pkg repo#Service");
        assert!(
            graph
                .precise_symbol("repo-001", "scip-rust pkg repo#Service")
                .is_some(),
            "overlay ingest should preserve earlier same-file symbol namespace"
        );
        assert_eq!(
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg repo#Impl"),
            vec![PreciseRelationshipRecord {
                repository_id: "repo-001".to_owned(),
                from_symbol: "scip-rust pkg repo#Impl".to_owned(),
                to_symbol: "scip-rust pkg repo#Service".to_owned(),
                kind: PreciseRelationshipKind::Implementation,
            }]
        );
    }

    #[test]
    fn scip_fixture_matrix_definitions_and_references() {
        let mut graph = SymbolGraph::default();
        let payload = load_scip_fixture("matrix-definitions-references.json");

        let summary = graph
            .ingest_scip_json(
                "repo-001",
                "fixture:matrix-definitions-references.json",
                &payload,
            )
            .expect("fixture payload should ingest");
        assert_eq!(summary.documents_ingested, 2);
        assert_eq!(summary.symbols_upserted, 1);
        assert_eq!(summary.occurrences_upserted, 2);
        assert_eq!(summary.relationships_upserted, 0);

        let occurrences =
            graph.precise_occurrences_for_symbol("repo-001", "scip-rust pkg matrix#Thing");
        assert_eq!(occurrences.len(), 2);
        assert_eq!(
            occurrences
                .iter()
                .map(|occurrence| (occurrence.path.clone(), occurrence.range.start_line))
                .collect::<Vec<_>>(),
            vec![
                ("src/defs.rs".to_string(), 1),
                ("src/use.rs".to_string(), 3)
            ]
        );
        assert_eq!(
            graph
                .precise_references_for_symbol("repo-001", "scip-rust pkg matrix#Thing")
                .len(),
            1
        );
    }

    #[test]
    fn scip_fixture_matrix_relationship_expansion_and_dedup() {
        let mut graph = SymbolGraph::default();
        let payload = load_scip_fixture("matrix-relationships.json");

        graph
            .ingest_scip_json("repo-001", "fixture:matrix-relationships.json", &payload)
            .expect("fixture payload should ingest");

        let relationships =
            graph.precise_relationships_from_symbol("repo-001", "scip-rust pkg matrix#Thing");
        assert_eq!(relationships.len(), 4);
        assert_eq!(
            relationships
                .iter()
                .map(|relationship| relationship.kind)
                .collect::<Vec<_>>(),
            vec![
                PreciseRelationshipKind::Definition,
                PreciseRelationshipKind::Reference,
                PreciseRelationshipKind::Implementation,
                PreciseRelationshipKind::TypeDefinition,
            ]
        );
    }

    #[test]
    fn scip_fixture_matrix_role_bits_classification_edges() {
        let mut graph = SymbolGraph::default();
        let payload = load_scip_fixture("matrix-role-bits.json");

        graph
            .ingest_scip_json("repo-001", "fixture:matrix-role-bits.json", &payload)
            .expect("fixture payload should ingest");

        let occurrences =
            graph.precise_occurrences_for_symbol("repo-001", "scip-rust pkg matrix#Roleful");
        assert_eq!(occurrences.len(), 5);
        let definition_count = occurrences
            .iter()
            .filter(|occurrence| occurrence.is_definition())
            .count();
        assert_eq!(
            definition_count, 2,
            "roles 1 and 9 should be classified as definitions"
        );

        let references =
            graph.precise_references_for_symbol("repo-001", "scip-rust pkg matrix#Roleful");
        assert_eq!(references.len(), 3);
        assert_eq!(
            references
                .iter()
                .map(|occurrence| (occurrence.range.start_line, occurrence.symbol_roles))
                .collect::<Vec<_>>(),
            vec![(3, 2), (4, 4), (5, 0)]
        );
    }

    #[test]
    fn scip_fixture_matrix_invalid_range_returns_typed_diagnostic() {
        let mut graph = SymbolGraph::default();
        let payload = load_scip_fixture("matrix-invalid-range.json");

        let error = graph
            .ingest_scip_json("repo-001", "fixture:matrix-invalid-range.json", &payload)
            .expect_err("invalid fixture should return typed invalid-input error");
        assert_eq!(
            error,
            ScipIngestError::InvalidInput {
                diagnostic: ScipInvalidInputDiagnostic {
                    artifact_label: "fixture:matrix-invalid-range.json".to_string(),
                    code: ScipInvalidInputCode::InvalidRange,
                    message:
                        "occurrence range for symbol 'scip-rust pkg matrix#Broken' in 'src/invalid.rs' must have 3 or 4 numbers"
                            .to_string(),
                    line: None,
                    column: None,
                },
            }
        );
        assert_eq!(graph.precise_counts(), PreciseGraphCounts::default());
    }

    fn load_scip_fixture(file_name: &str) -> Vec<u8> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/scip")
            .join(file_name);
        fs::read(&path).expect("SCIP fixture must exist")
    }
}
