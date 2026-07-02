//! Workload attribution helpers retained for MCP responses.

use super::*;
use crate::domain::{
    NormalizedWorkloadMetadata, WorkloadFallbackReason, WorkloadPrecisionMode,
    WorkloadStageAttribution,
};
use crate::searcher::SearchStageAttribution;

fn usize_from_u64(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

impl FriggMcpServer {
    pub(super) fn normalized_workload_from_search_stage_attribution(
        stage_attribution: &SearchStageAttribution,
    ) -> WorkloadStageAttribution {
        WorkloadStageAttribution::empty()
            .with_candidate_intake(
                usize_from_u64(stage_attribution.candidate_intake.elapsed_us),
                stage_attribution.candidate_intake.input_count,
                stage_attribution.candidate_intake.output_count,
            )
            .with_freshness_validation(
                usize_from_u64(stage_attribution.freshness_validation.elapsed_us),
                stage_attribution.freshness_validation.input_count,
                stage_attribution.freshness_validation.output_count,
            )
            .with_scan(
                usize_from_u64(stage_attribution.scan.elapsed_us),
                stage_attribution.scan.input_count,
                stage_attribution.scan.output_count,
            )
            .with_witness_scoring(
                usize_from_u64(stage_attribution.witness_scoring.elapsed_us),
                stage_attribution.witness_scoring.input_count,
                stage_attribution.witness_scoring.output_count,
            )
            .with_graph_expansion(
                usize_from_u64(stage_attribution.graph_expansion.elapsed_us),
                stage_attribution.graph_expansion.input_count,
                stage_attribution.graph_expansion.output_count,
            )
            .with_semantic_retrieval(
                usize_from_u64(stage_attribution.semantic_retrieval.elapsed_us),
                stage_attribution.semantic_retrieval.input_count,
                stage_attribution.semantic_retrieval.output_count,
            )
            .with_anchor_blending(
                usize_from_u64(stage_attribution.anchor_blending.elapsed_us),
                stage_attribution.anchor_blending.input_count,
                stage_attribution.anchor_blending.output_count,
            )
            .with_document_aggregation(
                usize_from_u64(stage_attribution.document_aggregation.elapsed_us),
                stage_attribution.document_aggregation.input_count,
                stage_attribution.document_aggregation.output_count,
            )
            .with_final_diversification(
                usize_from_u64(stage_attribution.final_diversification.elapsed_us),
                stage_attribution.final_diversification.input_count,
                stage_attribution.final_diversification.output_count,
            )
    }

    pub(super) fn provenance_normalized_workload_metadata(
        tool_name: &str,
        repository_ids: &[String],
        precision_mode: WorkloadPrecisionMode,
        fallback_reason: Option<WorkloadFallbackReason>,
        fallback_reason_detail: Option<String>,
        stage_attribution: Option<&SearchStageAttribution>,
    ) -> NormalizedWorkloadMetadata {
        let mut metadata = NormalizedWorkloadMetadata::from_repository_ids(
            tool_name,
            repository_ids,
            precision_mode,
        );
        if let Some(fallback_reason) = fallback_reason {
            metadata = metadata.with_fallback_reason(fallback_reason, fallback_reason_detail);
        } else {
            metadata = metadata.with_fallback_reason_detail(fallback_reason_detail);
        }
        if let Some(stage_attribution) = stage_attribution {
            metadata = metadata.with_stage_attribution(
                Self::normalized_workload_from_search_stage_attribution(stage_attribution),
            );
        }

        metadata
    }

    pub(super) fn provenance_precision_mode_from_label(
        precision_label: Option<&str>,
    ) -> WorkloadPrecisionMode {
        match precision_label.unwrap_or_default() {
            "exact" => WorkloadPrecisionMode::Exact,
            "precise" | "precise_partial" => WorkloadPrecisionMode::Precise,
            "heuristic" => WorkloadPrecisionMode::Heuristic,
            "fallback" => WorkloadPrecisionMode::Fallback,
            "unknown" => WorkloadPrecisionMode::Unknown,
            _ => WorkloadPrecisionMode::Unknown,
        }
    }

    pub(super) fn provenance_fallback_reason_from_label(
        fallback_label: Option<&str>,
    ) -> Option<WorkloadFallbackReason> {
        match fallback_label.unwrap_or_default() {
            "none" => Some(WorkloadFallbackReason::None),
            "precise_absent" => Some(WorkloadFallbackReason::PreciseAbsent),
            "resource_budget" => Some(WorkloadFallbackReason::ResourceBudget),
            "stage_filtered" => Some(WorkloadFallbackReason::StageFiltered),
            "semantic_unavailable" => Some(WorkloadFallbackReason::SemanticUnavailable),
            "timeout" => Some(WorkloadFallbackReason::Timeout),
            "unsupported_feature" => Some(WorkloadFallbackReason::UnsupportedFeature),
            _ => None,
        }
    }

    pub(super) fn bounded_text(value: &str) -> String {
        if value.chars().count() <= Self::PROVENANCE_MAX_TEXT_CHARS {
            return value.to_owned();
        }
        let mut bounded = value
            .chars()
            .take(Self::PROVENANCE_MAX_TEXT_CHARS)
            .collect::<String>();
        bounded.push_str("...");
        bounded
    }

    fn provenance_error_code(error: &ErrorData) -> String {
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code"))
            .and_then(|value| value.as_str())
            .unwrap_or("missing_error_code")
            .to_owned()
    }

    pub(super) fn provenance_outcome<T>(result: &Result<T, ErrorData>) -> Value {
        match result {
            Ok(_) => json!({
                "status": "ok",
            }),
            Err(error) => json!({
                "status": "error",
                "error_code": Self::provenance_error_code(error),
                "mcp_error_code": error.code,
            }),
        }
    }

    pub(super) fn record_provenance_with_outcome(
        &self,
        _tool_name: &str,
        _repository_hint: Option<&str>,
        _params: Value,
        _source_refs: Value,
        _outcome: Value,
    ) -> Result<(), ErrorData> {
        Ok(())
    }

    pub(super) fn record_provenance_with_outcome_and_metadata(
        &self,
        _tool_name: &str,
        _repository_hint: Option<&str>,
        _params: Value,
        _source_refs: Value,
        _outcome: Value,
        _normalized_workload_metadata: Option<NormalizedWorkloadMetadata>,
    ) -> Result<(), ErrorData> {
        Ok(())
    }

    pub(super) async fn record_provenance_blocking<T>(
        &self,
        _tool_name: &'static str,
        _repository_hint: Option<&str>,
        _params: Value,
        _source_refs: Value,
        _result: &Result<T, ErrorData>,
    ) -> Result<(), ErrorData> {
        Ok(())
    }

    pub(super) async fn record_provenance_blocking_with_metadata<T>(
        &self,
        _tool_name: &'static str,
        _repository_hint: Option<&str>,
        _params: Value,
        _source_refs: Value,
        _normalized_workload_metadata: Option<NormalizedWorkloadMetadata>,
        _result: &Result<T, ErrorData>,
    ) -> Result<(), ErrorData> {
        Ok(())
    }

    pub(super) fn finalize_with_provenance<T>(
        &self,
        _tool_name: &str,
        result: Result<T, ErrorData>,
        _provenance_result: Result<(), ErrorData>,
    ) -> Result<T, ErrorData> {
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::{SearchStageAttribution, SearchStageSample};

    #[test]
    fn provenance_precision_mode_from_label_maps_partial() {
        assert_eq!(
            FriggMcpServer::provenance_precision_mode_from_label(Some("precise_partial")),
            crate::domain::WorkloadPrecisionMode::Precise
        );
    }

    #[test]
    fn provenance_fallback_reason_from_label_maps_semantic_unavailable() {
        assert_eq!(
            FriggMcpServer::provenance_fallback_reason_from_label(Some("semantic_unavailable")),
            Some(crate::domain::WorkloadFallbackReason::SemanticUnavailable),
        );
    }

    #[test]
    fn normalized_stage_attribution_bounds_search_stage_attribution() {
        let search_attribution = SearchStageAttribution {
            candidate_intake: SearchStageSample::new(12, 3, 4),
            freshness_validation: SearchStageSample::new(13, 5, 6),
            scan: SearchStageSample::new(14, 7, 8),
            witness_scoring: SearchStageSample::new(15, 9, 10),
            graph_expansion: SearchStageSample::new(16, 11, 12),
            semantic_retrieval: SearchStageSample::new(17, 13, 14),
            anchor_blending: SearchStageSample::new(18, 15, 16),
            document_aggregation: SearchStageSample::new(19, 17, 18),
            final_diversification: SearchStageSample::new(20, 19, 20),
        };

        let converted =
            FriggMcpServer::normalized_workload_from_search_stage_attribution(&search_attribution);

        assert_eq!(converted.candidate_intake.elapsed_us, 12);
        assert_eq!(converted.freshness_validation.elapsed_us, 13);
        assert_eq!(converted.semantic_retrieval.output_count, 14);
        assert_eq!(converted.final_diversification.input_count, 19);
    }

    #[test]
    fn normalized_stage_attribution_bounds_elapsed_time() {
        let search_attribution = SearchStageAttribution {
            candidate_intake: SearchStageSample::new(u64::MAX, 1, 2),
            freshness_validation: SearchStageSample::new(u64::MAX, 3, 4),
            scan: SearchStageSample::new(5, 6, 7),
            witness_scoring: SearchStageSample::new(8, 9, 10),
            graph_expansion: SearchStageSample::new(11, 12, 13),
            semantic_retrieval: SearchStageSample::new(14, 15, 16),
            anchor_blending: SearchStageSample::new(17, 18, 19),
            document_aggregation: SearchStageSample::new(20, 21, 22),
            final_diversification: SearchStageSample::new(23, 24, 25),
        };

        let converted =
            FriggMcpServer::normalized_workload_from_search_stage_attribution(&search_attribution);

        assert_eq!(converted.candidate_intake.elapsed_us, u64::MAX);
        assert_eq!(converted.freshness_validation.elapsed_us, u64::MAX);
        assert_eq!(converted.scan.input_count, 6);
        assert_eq!(converted.scan.output_count, 7);
    }
}
