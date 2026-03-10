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
