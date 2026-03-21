use super::*;
use crate::domain::{ChannelHealthStatus, SourceClass};
use crate::mcp::types::{
    SearchHybridChannelDiagnostic, SearchHybridChannelMetadata, SearchHybridDiagnosticsSummary,
    SearchHybridLanguageCapabilityMetadata, SearchHybridMetadata, SearchHybridNavigationHint,
    SearchHybridSemanticAcceleratorMetadata, SearchHybridStageAttribution,
    SearchHybridUtilitySummary, SearchLexicalBackendMetadata, SearchTextMetadata,
};
use crate::searcher::{
    SearchLexicalBackend, hybrid_match_definition_navigation_supported,
    hybrid_match_document_symbols_supported, hybrid_match_is_live_navigation_pivot,
    hybrid_match_source_class, hybrid_match_surface_families,
};

mod cache;
mod document_symbols;
mod hybrid;
mod inspect;
mod symbol;
mod text;

#[cfg(test)]
mod tests;

impl FriggMcpServer {
    pub(super) fn search_lexical_backend_metadata(
        backend: Option<SearchLexicalBackend>,
    ) -> Option<SearchLexicalBackendMetadata> {
        match backend? {
            SearchLexicalBackend::Native => Some(SearchLexicalBackendMetadata::Native),
            SearchLexicalBackend::Ripgrep => Some(SearchLexicalBackendMetadata::Ripgrep),
            SearchLexicalBackend::Mixed => Some(SearchLexicalBackendMetadata::Mixed),
        }
    }

    pub(super) fn search_text_metadata(
        backend: Option<SearchLexicalBackend>,
        note: Option<String>,
    ) -> Option<SearchTextMetadata> {
        Some(SearchTextMetadata {
            lexical_backend: Self::search_lexical_backend_metadata(backend)?,
            lexical_backend_note: note,
        })
    }
}
