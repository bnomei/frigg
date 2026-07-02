//! Search-oriented MCP tools: text, hybrid, symbol, structural, syntax inspection, and document outlines.

use super::*;
use crate::context_efficiency::ManifestMetadataSummary;
use crate::domain::{ChannelHealthStatus, SourceClass, model::TextMatch};
use crate::mcp::types::{
    ContextEfficiencyMetadata, ContextEfficiencyStageAttribution, SearchHybridChannelDiagnostic,
    SearchHybridChannelMetadata, SearchHybridDiagnosticsSummary,
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

#[cfg(test)]
pub(crate) use self::hybrid::SearchHybridWarningContext;

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
            lexical_backend: Some(Self::search_lexical_backend_metadata(backend)?),
            lexical_backend_note: note,
            context_efficiency: None,
        })
    }

    pub(super) fn search_text_context_efficiency_metadata(
        workspaces: &[AttachedWorkspace],
        matches: &[TextMatch],
        total_matches: usize,
    ) -> Result<ContextEfficiencyMetadata, ErrorData> {
        let mut summaries = BTreeMap::<String, ManifestMetadataSummary>::new();
        for workspace in workspaces {
            let storage = Storage::new(&workspace.db_path);
            if let Some(summary) = storage
                .load_latest_context_efficiency_manifest_summary_for_repository(
                    &workspace.repository_id,
                )
                .map_err(Self::map_frigg_error)?
            {
                summaries.insert(workspace.repository_id.clone(), summary);
            }
        }

        let indexed_readable_files = summaries
            .values()
            .map(|summary| summary.indexed_readable_files)
            .sum();
        let indexed_readable_bytes = summaries
            .values()
            .map(|summary| summary.indexed_readable_bytes)
            .fold(0_u64, u64::saturating_add);
        let indexed_min_mtime_ns = summaries
            .values()
            .filter_map(|summary| summary.min_mtime_ns)
            .min();
        let indexed_max_mtime_ns = summaries
            .values()
            .filter_map(|summary| summary.max_mtime_ns)
            .max();

        let mut returned_paths = BTreeSet::<(String, String)>::new();
        let mut returned_source_bytes_estimate = 0_u64;
        for matched in matches {
            returned_paths.insert((matched.repository_id.clone(), matched.path.clone()));
            returned_source_bytes_estimate = returned_source_bytes_estimate
                .saturating_add(matched.excerpt.len().try_into().unwrap_or(u64::MAX));
        }

        let mut paths_by_repository = BTreeMap::<String, Vec<String>>::new();
        for (repository_id, path) in &returned_paths {
            paths_by_repository
                .entry(repository_id.clone())
                .or_default()
                .push(path.clone());
        }
        let returned_unique_file_bytes = paths_by_repository
            .iter()
            .filter_map(|(repository_id, paths)| {
                summaries.get(repository_id).map(|summary| {
                    summary.returned_unique_file_bytes(paths.iter().map(String::as_str))
                })
            })
            .fold(0_u64, u64::saturating_add);
        let denominator = returned_source_bytes_estimate.max(1) as f64;

        Ok(ContextEfficiencyMetadata {
            indexed_readable_files,
            indexed_readable_bytes,
            indexed_min_mtime_ns,
            indexed_max_mtime_ns,
            candidate_input_count: None,
            candidate_output_count: None,
            returned_match_count: Some(total_matches),
            returned_unique_paths: Some(returned_paths.len()),
            returned_unique_file_bytes: Some(returned_unique_file_bytes),
            returned_source_bytes_estimate: Some(returned_source_bytes_estimate),
            narrowing_ratio_estimate: Some(indexed_readable_bytes as f64 / denominator),
            stage_attribution: None,
        })
    }

    pub(super) fn search_hybrid_context_efficiency_metadata(
        workspaces: &[AttachedWorkspace],
        matches: &[SearchHybridMatch],
        stage_attribution: Option<&crate::searcher::SearchStageAttribution>,
    ) -> Result<ContextEfficiencyMetadata, ErrorData> {
        let mut summaries = BTreeMap::<String, ManifestMetadataSummary>::new();
        for workspace in workspaces {
            let storage = Storage::new(&workspace.db_path);
            if let Some(summary) = storage
                .load_latest_context_efficiency_manifest_summary_for_repository(
                    &workspace.repository_id,
                )
                .map_err(Self::map_frigg_error)?
            {
                summaries.insert(workspace.repository_id.clone(), summary);
            }
        }

        let indexed_readable_files = summaries
            .values()
            .map(|summary| summary.indexed_readable_files)
            .sum();
        let indexed_readable_bytes = summaries
            .values()
            .map(|summary| summary.indexed_readable_bytes)
            .fold(0_u64, u64::saturating_add);
        let indexed_min_mtime_ns = summaries
            .values()
            .filter_map(|summary| summary.min_mtime_ns)
            .min();
        let indexed_max_mtime_ns = summaries
            .values()
            .filter_map(|summary| summary.max_mtime_ns)
            .max();

        let mut returned_paths = BTreeSet::<(String, String)>::new();
        let mut returned_source_bytes_estimate = 0_u64;
        for matched in matches {
            returned_paths.insert((matched.repository_id.clone(), matched.path.clone()));
            returned_source_bytes_estimate = returned_source_bytes_estimate
                .saturating_add(matched.excerpt.len().try_into().unwrap_or(u64::MAX));
        }

        let mut paths_by_repository = BTreeMap::<String, Vec<String>>::new();
        for (repository_id, path) in &returned_paths {
            paths_by_repository
                .entry(repository_id.clone())
                .or_default()
                .push(path.clone());
        }
        let returned_unique_file_bytes = paths_by_repository
            .iter()
            .filter_map(|(repository_id, paths)| {
                summaries.get(repository_id).map(|summary| {
                    summary.returned_unique_file_bytes(paths.iter().map(String::as_str))
                })
            })
            .fold(0_u64, u64::saturating_add);

        let candidate_input_count =
            stage_attribution.map(|attribution| attribution.candidate_intake.input_count);
        let candidate_output_count =
            stage_attribution.map(|attribution| attribution.candidate_intake.output_count);
        let stage_attribution =
            stage_attribution.map(|attribution| ContextEfficiencyStageAttribution {
                candidate_input_count: attribution.candidate_intake.input_count,
                candidate_output_count: attribution.candidate_intake.output_count,
            });
        let denominator = returned_source_bytes_estimate.max(1) as f64;

        Ok(ContextEfficiencyMetadata {
            indexed_readable_files,
            indexed_readable_bytes,
            indexed_min_mtime_ns,
            indexed_max_mtime_ns,
            candidate_input_count,
            candidate_output_count,
            returned_match_count: Some(matches.len()),
            returned_unique_paths: Some(returned_paths.len()),
            returned_unique_file_bytes: Some(returned_unique_file_bytes),
            returned_source_bytes_estimate: Some(returned_source_bytes_estimate),
            narrowing_ratio_estimate: Some(indexed_readable_bytes as f64 / denominator),
            stage_attribution,
        })
    }
}
