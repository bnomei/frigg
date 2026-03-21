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
    fn trim_provenance_storage_cache(
        &self,
        cache: &mut BTreeMap<ProvenanceStorageCacheKey, Arc<Storage>>,
    ) {
        while cache.len() > Self::PROVENANCE_STORAGE_CACHE_MAX_ENTRIES {
            let _ = cache.pop_first();
        }
    }

    fn provenance_payload(
        tool_name: &str,
        target_repository_id: &str,
        params: Value,
        source_refs: Value,
        outcome: Value,
        normalized_workload: Option<&NormalizedWorkloadMetadata>,
    ) -> Value {
        let mut payload = serde_json::Map::new();
        payload.insert("tool_name".to_owned(), Value::String(tool_name.to_owned()));
        payload.insert("params".to_owned(), params);
        payload.insert("source_refs".to_owned(), source_refs);
        payload.insert("outcome".to_owned(), outcome);
        payload.insert(
            "target_repository_id".to_owned(),
            Value::String(target_repository_id.to_owned()),
        );
        if let Some(metadata) = normalized_workload {
            payload.insert(
                "normalized_workload".to_owned(),
                serde_json::to_value(metadata).unwrap_or_else(|_| metadata.as_payload_value()),
            );
        }

        Value::Object(payload)
    }

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

    #[cfg(test)]
    pub(super) fn provenance_metadata_from_payload(
        payload: &Value,
    ) -> Option<NormalizedWorkloadMetadata> {
        payload.get("normalized_workload").and_then(|value| {
            serde_json::from_value::<NormalizedWorkloadMetadata>(value.clone()).ok()
        })
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

    fn provenance_single_repository_id_from_array<'a>(
        payload: &'a Value,
        field_name: &str,
    ) -> Option<&'a str> {
        match payload.get(field_name)?.as_array()?.as_slice() {
            [only] => only.as_str(),
            _ => None,
        }
    }

    fn provenance_repository_hint_from_source_refs(source_refs: &Value) -> Option<&str> {
        source_refs
            .get("resolved_repository_id")
            .and_then(Value::as_str)
            .or_else(|| source_refs.get("repository_id").and_then(Value::as_str))
            .or_else(|| {
                Self::provenance_single_repository_id_from_array(
                    source_refs,
                    "scoped_repository_ids",
                )
            })
            .or_else(|| {
                Self::provenance_single_repository_id_from_array(source_refs, "repository_ids")
            })
    }

    fn provenance_repository_hint_from_workload(
        normalized_workload_metadata: Option<&NormalizedWorkloadMetadata>,
    ) -> Option<&str> {
        normalized_workload_metadata.and_then(|metadata| {
            match metadata.repository_scope.repository_ids.as_slice() {
                [only] => Some(only.as_str()),
                _ => None,
            }
        })
    }

    fn resolved_provenance_repository_hint<'a>(
        repository_hint: Option<&'a str>,
        source_refs: &'a Value,
        normalized_workload_metadata: Option<&'a NormalizedWorkloadMetadata>,
    ) -> Option<&'a str> {
        repository_hint
            .or_else(|| Self::provenance_repository_hint_from_source_refs(source_refs))
            .or_else(|| {
                Self::provenance_repository_hint_from_workload(normalized_workload_metadata)
            })
    }

    fn default_provenance_target(&self) -> Option<(String, PathBuf)> {
        if let Some(workspace) = self.current_workspace() {
            return Some((workspace.repository_id, workspace.root));
        }

        self.attached_workspaces()
            .into_iter()
            .min_by(|left, right| left.repository_id.cmp(&right.repository_id))
            .or_else(|| {
                self.known_workspaces()
                    .into_iter()
                    .filter(|workspace| self.known_workspace_can_bootstrap_provenance(workspace))
                    .min_by(|left, right| left.repository_id.cmp(&right.repository_id))
            })
            .map(|workspace| (workspace.repository_id, workspace.root))
    }

    fn known_workspace_can_bootstrap_provenance(&self, workspace: &AttachedWorkspace) -> bool {
        if !workspace.db_path.exists() {
            return true;
        }

        let cache_key = ProvenanceStorageCacheKey {
            repository_id: workspace.repository_id.clone(),
            db_path: workspace.db_path.clone(),
        };
        self.cache_state
            .provenance_storage_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains_key(&cache_key)
    }

    fn provenance_target_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Option<(String, PathBuf)> {
        match repository_id {
            Some(repository_id) => self
                .attached_workspaces()
                .into_iter()
                .find(|workspace| workspace.repository_id == repository_id)
                .or_else(|| {
                    self.known_workspaces().into_iter().find(|workspace| {
                        workspace.repository_id == repository_id
                            && self.known_workspace_can_bootstrap_provenance(workspace)
                    })
                })
                .map(|workspace| (workspace.repository_id, workspace.root)),
            None => self.default_provenance_target(),
        }
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

    pub(super) fn provenance_outcome<T>(result: &Result<Json<T>, ErrorData>) -> Value {
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

    fn provenance_storage_for_target(
        &self,
        tool_name: &str,
        target_repository_id: &str,
        db_path: &Path,
    ) -> Result<Arc<Storage>, ErrorData> {
        let cache_key = ProvenanceStorageCacheKey {
            repository_id: target_repository_id.to_owned(),
            db_path: db_path.to_path_buf(),
        };
        if let Some(storage) = self
            .cache_state
            .provenance_storage_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(storage);
        }

        let storage = Arc::new(Storage::new(db_path));
        if let Err(err) = storage.initialize() {
            return Err(Self::provenance_persistence_error(
                ProvenancePersistenceStage::InitializeStorage,
                tool_name,
                Some(target_repository_id),
                Some(db_path),
                err,
            ));
        }

        let mut cache = self
            .cache_state
            .provenance_storage_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(storage) = cache.get(&cache_key).cloned() {
            return Ok(storage);
        }
        cache.insert(cache_key, storage.clone());
        self.trim_provenance_storage_cache(&mut cache);
        Ok(storage)
    }

    fn record_provenance_with_outcome_internal(
        &self,
        tool_name: &str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        outcome: Value,
        normalized_workload_metadata: Option<&NormalizedWorkloadMetadata>,
    ) -> Result<(), ErrorData> {
        if !self.provenance_state.enabled {
            return Ok(());
        }
        let repository_hint = Self::resolved_provenance_repository_hint(
            repository_hint,
            &source_refs,
            normalized_workload_metadata,
        );
        let Some((target_repository_id, target_root)) =
            self.provenance_target_for_repository(repository_hint)
        else {
            return Ok(());
        };

        let db_path = match ensure_provenance_db_parent_dir(&target_root) {
            Ok(path) => path,
            Err(err) => {
                return Err(Self::provenance_persistence_error(
                    ProvenancePersistenceStage::ResolveStoragePath,
                    tool_name,
                    Some(&target_repository_id),
                    None,
                    err,
                ));
            }
        };

        let storage =
            self.provenance_storage_for_target(tool_name, &target_repository_id, &db_path)?;

        let payload = Self::provenance_payload(
            tool_name,
            &target_repository_id,
            params,
            source_refs,
            outcome,
            normalized_workload_metadata,
        );
        let trace_id = Storage::new_provenance_trace_id(tool_name);
        if let Err(err) = storage.append_provenance_event(&trace_id, tool_name, &payload) {
            return Err(Self::provenance_persistence_error(
                ProvenancePersistenceStage::AppendEvent,
                tool_name,
                Some(&target_repository_id),
                Some(&db_path),
                err,
            ));
        }

        Ok(())
    }

    pub(super) fn record_provenance_with_outcome(
        &self,
        tool_name: &str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        outcome: Value,
    ) -> Result<(), ErrorData> {
        self.record_provenance_with_outcome_internal(
            tool_name,
            repository_hint,
            params,
            source_refs,
            outcome,
            None,
        )
    }

    pub(super) fn record_provenance_with_outcome_and_metadata(
        &self,
        tool_name: &str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        outcome: Value,
        normalized_workload_metadata: Option<NormalizedWorkloadMetadata>,
    ) -> Result<(), ErrorData> {
        self.record_provenance_with_outcome_internal(
            tool_name,
            repository_hint,
            params,
            source_refs,
            outcome,
            normalized_workload_metadata.as_ref(),
        )
    }

    pub(super) async fn record_provenance_blocking<T>(
        &self,
        tool_name: &'static str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        result: &Result<Json<T>, ErrorData>,
    ) -> Result<(), ErrorData> {
        if !self.provenance_state.enabled {
            return Ok(());
        }
        let server = self.clone();
        let repository_hint = repository_hint.map(str::to_owned);
        let outcome = Self::provenance_outcome(result);
        Self::run_blocking_task("record_provenance", move || {
            server.record_provenance_with_outcome_internal(
                tool_name,
                repository_hint.as_deref(),
                params,
                source_refs,
                outcome,
                None,
            )
        })
        .await?
    }

    pub(super) async fn record_provenance_blocking_with_metadata<T>(
        &self,
        tool_name: &'static str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        normalized_workload_metadata: Option<NormalizedWorkloadMetadata>,
        result: &Result<Json<T>, ErrorData>,
    ) -> Result<(), ErrorData> {
        if !self.provenance_state.enabled {
            return Ok(());
        }
        let server = self.clone();
        let repository_hint = repository_hint.map(str::to_owned);
        let outcome = Self::provenance_outcome(result);
        Self::run_blocking_task("record_provenance", move || {
            server.record_provenance_with_outcome_internal(
                tool_name,
                repository_hint.as_deref(),
                params,
                source_refs,
                outcome,
                normalized_workload_metadata.as_ref(),
            )
        })
        .await?
    }

    pub(super) fn finalize_with_provenance<T>(
        &self,
        tool_name: &str,
        result: Result<Json<T>, ErrorData>,
        provenance_result: Result<(), ErrorData>,
    ) -> Result<Json<T>, ErrorData> {
        match provenance_result {
            Ok(_) => result,
            Err(provenance_error) if self.provenance_state.best_effort => {
                warn!(
                    tool_name,
                    error = %provenance_error.message,
                    "provenance persistence failed in best-effort mode"
                );
                result
            }
            Err(provenance_error) => match result {
                Ok(_) => Err(provenance_error),
                Err(original_error) => {
                    warn!(
                        tool_name,
                        original_error_code = ?original_error.code,
                        provenance_error_code = ?provenance_error.code,
                        "provenance persistence failed but original request already returned typed error"
                    );
                    Err(original_error)
                }
            },
        }
    }

    pub(super) fn with_provenance_enabled(&self, provenance_enabled: bool) -> Self {
        let mut cloned = self.clone();
        cloned.provenance_state.enabled = provenance_enabled;
        cloned
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::searcher::{SearchStageAttribution, SearchStageSample};

    #[test]
    fn provenance_payload_round_trips_normalized_workload_metadata() {
        let metadata = FriggMcpServer::provenance_normalized_workload_metadata(
            "search_text",
            &["repo-a".to_owned(), "repo-b".to_owned()],
            crate::domain::WorkloadPrecisionMode::Heuristic,
            Some(crate::domain::WorkloadFallbackReason::ResourceBudget),
            Some("cache miss".to_owned()),
            None,
        )
        .with_stage_attribution(WorkloadStageAttribution::empty().with_scan(7, 8, 9));

        let payload = FriggMcpServer::provenance_payload(
            "search_text",
            "repo-a",
            json!({ "query": "q" }),
            json!({}),
            json!({ "status": "ok" }),
            Some(&metadata),
        );
        let parsed = FriggMcpServer::provenance_metadata_from_payload(&payload)
            .expect("normalized workload metadata should be parseable");
        assert_eq!(
            parsed.tool_class,
            crate::domain::WorkloadToolClass::LiteralLookup
        );
        assert_eq!(parsed.repository_scope.repository_count, 2);
        assert_eq!(
            parsed.repository_scope.scope,
            crate::domain::WorkloadRepositoryScopeKind::Multi
        );
        assert_eq!(
            parsed.precision_mode,
            crate::domain::WorkloadPrecisionMode::Heuristic
        );
        assert_eq!(
            parsed.fallback_reason,
            Some(crate::domain::WorkloadFallbackReason::ResourceBudget)
        );
        assert!(parsed.stage_attribution.is_some());
        assert_eq!(payload["target_repository_id"], json!("repo-a"));
    }

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

    #[test]
    fn provenance_payload_keeps_backward_compatible_fields() {
        let payload = FriggMcpServer::provenance_payload(
            "search_symbol",
            "repo-x",
            json!({ "repository_id": "repo-x" }),
            json!({ "diagnostics_count": 1 }),
            json!({ "status": "ok" }),
            None,
        );

        assert_eq!(payload["tool_name"], "search_symbol");
        assert_eq!(payload["target_repository_id"], "repo-x");
        assert_eq!(payload["params"], json!({ "repository_id": "repo-x" }));
        assert!(payload.get("normalized_workload").is_none());
    }
}
