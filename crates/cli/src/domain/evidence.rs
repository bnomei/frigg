use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable product-ring vocabulary for Frigg's public framing.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum ProductRing {
    StableCore,
    OptionalAccelerator,
    AdvancedConsumer,
}

impl ProductRing {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::StableCore => "stable_core",
            Self::OptionalAccelerator => "optional_accelerator",
            Self::AdvancedConsumer => "advanced_consumer",
        }
    }
}

/// Stable evidence-channel vocabulary for retrieval and replay behavior.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum EvidenceChannel {
    LexicalManifest,
    GraphPrecise,
    Semantic,
    PathSurfaceWitness,
}

impl EvidenceChannel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::LexicalManifest => "lexical_manifest",
            Self::GraphPrecise => "graph_precise",
            Self::Semantic => "semantic",
            Self::PathSurfaceWitness => "path_surface_witness",
        }
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EvidenceDocumentRef {
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAnchorKind {
    TextSpan,
    Symbol,
    SemanticChunk,
    PathWitness,
}

#[derive(
    Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub struct EvidenceAnchor {
    pub kind: EvidenceAnchorKind,
    pub start_line: usize,
    pub start_column: usize,
    pub end_line: usize,
    pub end_column: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

impl EvidenceAnchor {
    pub fn new(
        kind: EvidenceAnchorKind,
        start_line: usize,
        start_column: usize,
        end_line: usize,
        end_column: usize,
    ) -> Self {
        Self {
            kind,
            start_line,
            start_column,
            end_line,
            end_column,
            detail: None,
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvidenceHit {
    pub channel: EvidenceChannel,
    pub document: EvidenceDocumentRef,
    pub anchor: EvidenceAnchor,
    pub raw_score: f32,
    pub excerpt: String,
    pub provenance_ids: Vec<String>,
}

impl EvidenceHit {
    pub fn single_provenance(
        channel: EvidenceChannel,
        document: EvidenceDocumentRef,
        anchor: EvidenceAnchor,
        raw_score: f32,
        excerpt: impl Into<String>,
        provenance_id: impl Into<String>,
    ) -> Self {
        Self {
            channel,
            document,
            anchor,
            raw_score,
            excerpt: excerpt.into(),
            provenance_ids: vec![provenance_id.into()],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ChannelHealthStatus {
    Disabled,
    Unavailable,
    Ok,
    Degraded,
    Filtered,
}

impl ChannelHealthStatus {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::Unavailable => "unavailable",
            Self::Ok => "ok",
            Self::Degraded => "degraded",
            Self::Filtered => "filtered",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelHealth {
    pub status: ChannelHealthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl ChannelHealth {
    pub fn new(status: ChannelHealthStatus, reason: Option<String>) -> Self {
        Self { status, reason }
    }

    pub fn ok() -> Self {
        Self::new(ChannelHealthStatus::Ok, None)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelDiagnostic {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelStats {
    pub candidate_count: usize,
    pub hit_count: usize,
    pub match_count: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ChannelResult {
    pub channel: EvidenceChannel,
    pub hits: Vec<EvidenceHit>,
    pub health: ChannelHealth,
    pub diagnostics: Vec<ChannelDiagnostic>,
    pub stats: ChannelStats,
}

impl ChannelResult {
    pub fn new(
        channel: EvidenceChannel,
        hits: Vec<EvidenceHit>,
        health: ChannelHealth,
        diagnostics: Vec<ChannelDiagnostic>,
        stats: ChannelStats,
    ) -> Self {
        Self {
            channel,
            hits,
            health,
            diagnostics,
            stats,
        }
    }
}

/// Coarse internal architecture layers for Frigg modules and future seams.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum FriggLayer {
    Engine,
    ApplicationRuntime,
    ExternalConsumer,
}

impl FriggLayer {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Engine => "engine",
            Self::ApplicationRuntime => "application_runtime",
            Self::ExternalConsumer => "external_consumer",
        }
    }
}

/// Public support-level vocabulary used by docs and future capability reporting.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
pub enum SupportLevel {
    FirstClass,
    Partial,
    Planned,
}

impl SupportLevel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::FirstClass => "first_class",
            Self::Partial => "partial",
            Self::Planned => "planned",
        }
    }
}
