//! Search-oriented MCP tools: text, hybrid, symbol, structural, syntax inspection, and document outlines.

use super::*;
use crate::context_efficiency::{
    ContextEfficiencyLogMetrics, ContextEfficiencyLogRow, ManifestMetadataSummary,
};
use crate::domain::{ChannelHealthStatus, SourceClass, model::TextMatch};
use crate::mcp::types::{
    ContextEfficiencyMetadata, ContextEfficiencyStageAttribution, ResponseFreshnessBasisMetadata,
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
mod files;
mod hybrid;
mod inspect;
mod symbol;
mod text;

#[cfg(test)]
mod tests;

#[cfg(test)]
pub(crate) use self::hybrid::SearchHybridWarningContext;

impl FriggMcpServer {
    #[cfg(test)]
    pub(super) fn context_efficiency_metadata_for_controls<T>(
        include_context_efficiency: Option<bool>,
        log_enabled: bool,
        build: impl FnOnce() -> Result<T, ErrorData>,
    ) -> Result<Option<T>, ErrorData> {
        if include_context_efficiency == Some(true) {
            build().map(Some)
        } else if log_enabled {
            Ok(build().ok())
        } else {
            Ok(None)
        }
    }

    pub(super) fn context_efficiency_metadata_for_tool_observers(
        &self,
        execution_context: &ReadOnlyToolExecutionContext,
        include_context_efficiency: Option<bool>,
        log_enabled: bool,
        build: impl FnOnce() -> Result<ContextEfficiencyMetadata, ErrorData>,
    ) -> Result<Option<ContextEfficiencyMetadata>, ErrorData> {
        let display_enabled = self.tool_call_display_enabled();
        if include_context_efficiency == Some(true) {
            let metadata = build()?;
            execution_context.set_display_context_saved_percent(
                metadata.matched_file_context_saved_percent_estimate,
            );
            Ok(Some(metadata))
        } else if log_enabled || display_enabled {
            let metadata = build().ok();
            if let Some(metadata) = metadata.as_ref() {
                execution_context.set_display_context_saved_percent(
                    metadata.matched_file_context_saved_percent_estimate,
                );
            }
            Ok(metadata)
        } else {
            Ok(None)
        }
    }

    pub(crate) fn context_efficiency_log_metrics(
        metadata: &ContextEfficiencyMetadata,
    ) -> ContextEfficiencyLogMetrics {
        ContextEfficiencyLogMetrics {
            indexed_readable_files: metadata.indexed_readable_files,
            indexed_readable_bytes: metadata.indexed_readable_bytes,
            indexed_min_mtime_ns: metadata.indexed_min_mtime_ns,
            indexed_max_mtime_ns: metadata.indexed_max_mtime_ns,
            candidate_input_count: metadata.candidate_input_count,
            candidate_output_count: metadata.candidate_output_count,
            returned_match_count: metadata.returned_match_count,
            returned_unique_paths: metadata.returned_unique_paths,
            returned_unique_file_bytes: metadata.returned_unique_file_bytes,
            returned_source_bytes_estimate: metadata.returned_source_bytes_estimate,
            matched_file_context_saved_bytes_estimate: metadata
                .matched_file_context_saved_bytes_estimate,
            matched_file_context_saved_percent_estimate: metadata
                .matched_file_context_saved_percent_estimate,
            query_duration_ms: metadata.query_duration_ms,
            narrowing_ratio_estimate: metadata.narrowing_ratio_estimate,
        }
    }

    pub(super) fn context_efficiency_elapsed_ms(started_at: std::time::Instant) -> u64 {
        u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    fn signed_byte_delta(baseline: u64, returned: u64) -> i64 {
        if baseline >= returned {
            i64::try_from(baseline - returned).unwrap_or(i64::MAX)
        } else {
            -i64::try_from(returned - baseline).unwrap_or(i64::MAX)
        }
    }

    fn rounded_percent(value: f64) -> f64 {
        (value * 100.0).round() / 100.0
    }

    fn rounded_ratio_to_u64(numerator: u64, denominator: u64) -> u64 {
        let denominator = denominator.max(1);
        let rounded =
            (u128::from(numerator) + u128::from(denominator / 2)) / u128::from(denominator);
        rounded.min(u128::from(u64::MAX)) as u64
    }

    pub(super) fn finalize_context_efficiency_metadata(
        mut metadata: ContextEfficiencyMetadata,
    ) -> ContextEfficiencyMetadata {
        if let Some(returned_source_bytes) = metadata.returned_source_bytes_estimate {
            if let Some(returned_unique_file_bytes) = metadata.returned_unique_file_bytes
                && returned_unique_file_bytes > 0
            {
                let saved =
                    Self::signed_byte_delta(returned_unique_file_bytes, returned_source_bytes);
                metadata.matched_file_context_saved_bytes_estimate = Some(saved);
                metadata.matched_file_context_saved_percent_estimate = Some(Self::rounded_percent(
                    saved as f64 / returned_unique_file_bytes as f64 * 100.0,
                ));
                metadata.narrowing_ratio_estimate = Some(Self::rounded_ratio_to_u64(
                    returned_unique_file_bytes,
                    returned_source_bytes,
                ));
            }

            let corpus_saved =
                Self::signed_byte_delta(metadata.indexed_readable_bytes, returned_source_bytes);
            metadata.corpus_context_saved_bytes_estimate = Some(corpus_saved);
            if metadata.indexed_readable_bytes > 0 {
                let percent = corpus_saved as f64 / metadata.indexed_readable_bytes as f64 * 100.0;
                metadata.corpus_context_saved_percent_estimate =
                    Some(Self::rounded_percent(percent));
            }
            let corpus_ratio =
                Self::rounded_ratio_to_u64(metadata.indexed_readable_bytes, returned_source_bytes);
            metadata.corpus_narrowing_ratio_estimate = Some(corpus_ratio);
        }

        metadata
    }

    pub(crate) fn append_context_efficiency_log_for_workspaces(
        &self,
        tool_name: &str,
        workspaces: &[AttachedWorkspace],
        metadata: &ContextEfficiencyMetadata,
    ) {
        let log_enabled = crate::context_efficiency::context_efficiency_log_enabled();
        let session_id = self.session_state.display_session_id();
        Self::append_context_efficiency_log_for_workspaces_with_log_state(
            tool_name,
            workspaces,
            metadata,
            log_enabled,
            Some(session_id.as_str()),
        );
    }

    fn append_context_efficiency_log_for_workspaces_with_log_state(
        tool_name: &str,
        workspaces: &[AttachedWorkspace],
        metadata: &ContextEfficiencyMetadata,
        log_enabled: bool,
        session_id: Option<&str>,
    ) {
        if !log_enabled {
            return;
        }

        let metrics = Self::context_efficiency_log_metrics(metadata);
        for workspace in workspaces {
            let snapshot_id = Storage::new(&workspace.db_path)
                .load_latest_context_efficiency_manifest_summary_for_repository(
                    &workspace.runtime_repository_id,
                )
                .ok()
                .flatten()
                .map(|summary| summary.snapshot_id);
            let mut row = ContextEfficiencyLogRow::new(
                tool_name,
                workspace.repository_id.clone(),
                snapshot_id,
                metrics.clone(),
            );
            if let Some(session_id) = session_id
                .map(str::trim)
                .filter(|session_id| !session_id.is_empty())
            {
                row = row.with_session_id(session_id);
            }
            let _ = crate::context_efficiency::append_context_efficiency_log_row_if_enabled(
                &workspace.root,
                log_enabled,
                &row,
            );
        }
    }

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

    pub(super) fn response_freshness_basis_metadata(
        freshness_basis: &Value,
    ) -> ResponseFreshnessBasisMetadata {
        serde_json::from_value(freshness_basis.clone())
            .expect("response freshness basis should deserialize")
    }

    fn returned_unique_file_bytes_estimate(
        workspaces: &BTreeMap<String, &AttachedWorkspace>,
        summaries: &BTreeMap<String, ManifestMetadataSummary>,
        returned_source_bytes_by_path: &BTreeMap<(String, String), u64>,
    ) -> Option<u64> {
        if returned_source_bytes_by_path.is_empty() {
            return Some(0);
        }

        let mut total = 0_u64;
        for ((repository_id, path), returned_source_bytes) in returned_source_bytes_by_path {
            let known_file_bytes = summaries.get(repository_id).and_then(|summary| {
                summary.file_size_bytes_for_path(
                    workspaces
                        .get(repository_id)
                        .map(|workspace| workspace.root.as_path()),
                    path,
                )
            });
            total = total.saturating_add(known_file_bytes.unwrap_or(*returned_source_bytes));
        }
        Some(total)
    }

    pub(super) fn search_text_context_efficiency_metadata(
        workspaces: &[AttachedWorkspace],
        matches: &[TextMatch],
        total_matches: usize,
    ) -> Result<ContextEfficiencyMetadata, ErrorData> {
        let workspaces_by_repository = workspaces
            .iter()
            .map(|workspace| (workspace.repository_id.clone(), workspace))
            .collect::<BTreeMap<_, _>>();
        let mut summaries = BTreeMap::<String, ManifestMetadataSummary>::new();
        for workspace in workspaces {
            let storage = Storage::new(&workspace.db_path);
            if let Some(summary) = storage
                .load_latest_context_efficiency_manifest_summary_for_repository(
                    &workspace.runtime_repository_id,
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
        let mut returned_source_bytes_by_path = BTreeMap::<(String, String), u64>::new();
        let mut returned_source_bytes_estimate = 0_u64;
        for matched in matches {
            let key = (matched.repository_id.clone(), matched.path.clone());
            returned_paths.insert(key.clone());
            let returned_bytes = matched.excerpt.len().try_into().unwrap_or(u64::MAX);
            returned_source_bytes_estimate =
                returned_source_bytes_estimate.saturating_add(returned_bytes);
            let path_returned_bytes = returned_source_bytes_by_path.entry(key).or_default();
            *path_returned_bytes = path_returned_bytes.saturating_add(returned_bytes);
        }

        let returned_unique_file_bytes = Self::returned_unique_file_bytes_estimate(
            &workspaces_by_repository,
            &summaries,
            &returned_source_bytes_by_path,
        );

        Ok(Self::finalize_context_efficiency_metadata(
            ContextEfficiencyMetadata {
                indexed_readable_files,
                indexed_readable_bytes,
                indexed_min_mtime_ns,
                indexed_max_mtime_ns,
                candidate_input_count: None,
                candidate_output_count: None,
                returned_match_count: Some(total_matches),
                returned_unique_paths: Some(returned_paths.len()),
                returned_unique_file_bytes,
                returned_source_bytes_estimate: Some(returned_source_bytes_estimate),
                matched_file_context_saved_bytes_estimate: None,
                matched_file_context_saved_percent_estimate: None,
                corpus_context_saved_bytes_estimate: None,
                corpus_context_saved_percent_estimate: None,
                corpus_narrowing_ratio_estimate: None,
                query_duration_ms: None,
                narrowing_ratio_estimate: None,
                stage_attribution: None,
            },
        ))
    }

    pub(super) fn search_hybrid_context_efficiency_metadata(
        workspaces: &[AttachedWorkspace],
        matches: &[SearchHybridMatch],
        stage_attribution: Option<&crate::searcher::SearchStageAttribution>,
    ) -> Result<ContextEfficiencyMetadata, ErrorData> {
        let workspaces_by_repository = workspaces
            .iter()
            .map(|workspace| (workspace.repository_id.clone(), workspace))
            .collect::<BTreeMap<_, _>>();
        let mut summaries = BTreeMap::<String, ManifestMetadataSummary>::new();
        for workspace in workspaces {
            let storage = Storage::new(&workspace.db_path);
            if let Some(summary) = storage
                .load_latest_context_efficiency_manifest_summary_for_repository(
                    &workspace.runtime_repository_id,
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
        let mut returned_source_bytes_by_path = BTreeMap::<(String, String), u64>::new();
        let mut returned_source_bytes_estimate = 0_u64;
        for matched in matches {
            let key = (matched.repository_id.clone(), matched.path.clone());
            returned_paths.insert(key.clone());
            let returned_bytes = matched.excerpt.len().try_into().unwrap_or(u64::MAX);
            returned_source_bytes_estimate =
                returned_source_bytes_estimate.saturating_add(returned_bytes);
            let path_returned_bytes = returned_source_bytes_by_path.entry(key).or_default();
            *path_returned_bytes = path_returned_bytes.saturating_add(returned_bytes);
        }

        let returned_unique_file_bytes = Self::returned_unique_file_bytes_estimate(
            &workspaces_by_repository,
            &summaries,
            &returned_source_bytes_by_path,
        );

        let candidate_input_count =
            stage_attribution.map(|attribution| attribution.candidate_intake.input_count);
        let candidate_output_count =
            stage_attribution.map(|attribution| attribution.candidate_intake.output_count);
        let stage_attribution =
            stage_attribution.map(|attribution| ContextEfficiencyStageAttribution {
                candidate_input_count: attribution.candidate_intake.input_count,
                candidate_output_count: attribution.candidate_intake.output_count,
            });
        Ok(Self::finalize_context_efficiency_metadata(
            ContextEfficiencyMetadata {
                indexed_readable_files,
                indexed_readable_bytes,
                indexed_min_mtime_ns,
                indexed_max_mtime_ns,
                candidate_input_count,
                candidate_output_count,
                returned_match_count: Some(matches.len()),
                returned_unique_paths: Some(returned_paths.len()),
                returned_unique_file_bytes,
                returned_source_bytes_estimate: Some(returned_source_bytes_estimate),
                matched_file_context_saved_bytes_estimate: None,
                matched_file_context_saved_percent_estimate: None,
                corpus_context_saved_bytes_estimate: None,
                corpus_context_saved_percent_estimate: None,
                corpus_narrowing_ratio_estimate: None,
                query_duration_ms: None,
                narrowing_ratio_estimate: None,
                stage_attribution,
            },
        ))
    }
}
