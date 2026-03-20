use std::collections::BTreeMap;

use super::*;
use crate::domain::{ChannelHealthStatus, SourceClass};
use crate::mcp::types::{
    SearchHybridChannelDiagnostic, SearchHybridChannelMetadata, SearchHybridDiagnosticsSummary,
    SearchHybridLanguageCapabilityMetadata, SearchHybridMetadata, SearchHybridNavigationHint,
    SearchHybridSemanticAcceleratorMetadata, SearchHybridStageAttribution,
    SearchHybridUtilitySummary,
};
use crate::searcher::{
    hybrid_match_definition_navigation_supported, hybrid_match_document_symbols_supported,
    hybrid_match_is_live_navigation_pivot, hybrid_match_source_class,
    hybrid_match_surface_families,
};

mod cache;
mod document_symbols;
mod hybrid;
mod inspect;
mod symbol;
mod text;

#[cfg(test)]
mod tests;
