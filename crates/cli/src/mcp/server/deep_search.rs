use super::*;

impl FriggMcpServer {
    pub(super) async fn deep_search_run_impl(
        &self,
        playbook: DeepSearchPlaybook,
    ) -> Result<Json<DeepSearchRunResponse>, ErrorData> {
        let playbook_id = Self::bounded_text(&playbook.playbook_id);
        let step_count = playbook.steps.len();
        let step_tools = playbook
            .steps
            .iter()
            .map(|step| step.tool_name.clone())
            .collect::<Vec<_>>();
        let harness = DeepSearchHarness::new(self.with_provenance_enabled(false));
        let internal_result = harness.run_playbook(&playbook).await;
        let budget_metadata = internal_result
            .as_ref()
            .ok()
            .map(Self::deep_search_budget_metadata_from_trace)
            .unwrap_or_else(|| json!({ "resource_budgets": [], "resource_usage": [] }));
        let result: Result<Json<DeepSearchRunResponse>, ErrorData> = internal_result
            .map(|trace_artifact| Json(trace_artifact.into()))
            .map_err(Self::map_frigg_error);
        let provenance_result = self
            .record_provenance_blocking(
                "deep_search_run",
                None,
                json!({
                    "playbook_id": playbook_id,
                    "step_count": step_count,
                    "step_tools": step_tools,
                }),
                json!({
                    "resource_budgets": budget_metadata["resource_budgets"].clone(),
                    "resource_usage": budget_metadata["resource_usage"].clone(),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("deep_search_run", result, provenance_result)
    }

    pub(super) async fn deep_search_replay_impl(
        &self,
        params: DeepSearchReplayParams,
    ) -> Result<Json<DeepSearchReplayResponse>, ErrorData> {
        let playbook_id = Self::bounded_text(&params.playbook.playbook_id);
        let step_count = params.playbook.steps.len();
        let step_tools = params
            .playbook
            .steps
            .iter()
            .map(|step| step.tool_name.clone())
            .collect::<Vec<_>>();
        let expected_trace_schema =
            Self::bounded_text(&params.expected_trace_artifact.trace_schema);
        let expected_step_count = params.expected_trace_artifact.step_count;
        let (playbook, expected_trace_artifact) = params.into_internal();
        let harness = DeepSearchHarness::new(self.with_provenance_enabled(false));
        let internal_result = harness
            .replay_and_diff(&playbook, &expected_trace_artifact)
            .await;
        let budget_metadata = internal_result
            .as_ref()
            .ok()
            .map(|replay| Self::deep_search_budget_metadata_from_trace(&replay.replayed))
            .unwrap_or_else(|| json!({ "resource_budgets": [], "resource_usage": [] }));
        let result: Result<Json<DeepSearchReplayResponse>, ErrorData> = internal_result
            .map(|replay| Json(replay.into()))
            .map_err(Self::map_frigg_error);
        let provenance_result = self
            .record_provenance_blocking(
                "deep_search_replay",
                None,
                json!({
                    "playbook_id": playbook_id,
                    "step_count": step_count,
                    "step_tools": step_tools,
                    "expected_trace_schema": expected_trace_schema,
                    "expected_step_count": expected_step_count,
                }),
                json!({
                    "matches": result.as_ref().ok().map(|response| response.0.matches),
                    "diff": result
                        .as_ref()
                        .ok()
                        .and_then(|response| response.0.diff.as_ref().map(|diff| Self::bounded_text(diff))),
                    "resource_budgets": budget_metadata["resource_budgets"].clone(),
                    "resource_usage": budget_metadata["resource_usage"].clone(),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("deep_search_replay", result, provenance_result)
    }

    pub(super) async fn deep_search_compose_citations_impl(
        &self,
        params: DeepSearchComposeCitationsParams,
    ) -> Result<Json<DeepSearchComposeCitationsResponse>, ErrorData> {
        let playbook_id = Self::bounded_text(&params.trace_artifact.playbook_id);
        let trace_schema = Self::bounded_text(&params.trace_artifact.trace_schema);
        let step_count = params.trace_artifact.step_count;
        let answer = params.answer;
        let answer_supplied = answer
            .as_ref()
            .map(|candidate| !candidate.trim().is_empty())
            .unwrap_or(false);

        let trace_artifact = params.trace_artifact.into();
        let budget_metadata = Self::deep_search_budget_metadata_from_trace(&trace_artifact);
        let result: Result<Json<DeepSearchComposeCitationsResponse>, ErrorData> =
            DeepSearchHarness::compose_citation_payload(
                &trace_artifact,
                answer.unwrap_or_default(),
            )
            .map(|citation_payload| Json(citation_payload.into()))
            .map_err(Self::map_frigg_error);
        let provenance_result = self
            .record_provenance_blocking(
                "deep_search_compose_citations",
                None,
                json!({
                    "playbook_id": playbook_id,
                    "trace_schema": trace_schema,
                    "step_count": step_count,
                    "answer_supplied": answer_supplied,
                }),
                json!({
                    "claims_count": result
                        .as_ref()
                        .ok()
                        .map(|response| response.0.citation_payload.claims.len()),
                    "citations_count": result
                        .as_ref()
                        .ok()
                        .map(|response| response.0.citation_payload.citations.len()),
                    "resource_budgets": budget_metadata["resource_budgets"].clone(),
                    "resource_usage": budget_metadata["resource_usage"].clone(),
                }),
                &result,
            )
            .await;
        self.finalize_with_provenance("deep_search_compose_citations", result, provenance_result)
    }

    fn deep_search_budget_metadata_from_trace(trace: &DeepSearchTraceArtifact) -> Value {
        let mut resource_budgets = Vec::new();
        let mut resource_usage = Vec::new();

        for step in &trace.steps {
            let DeepSearchTraceOutcome::Ok { response_json } = &step.outcome else {
                continue;
            };
            let Ok(response) = serde_json::from_str::<Value>(response_json) else {
                continue;
            };
            let Some(note_raw) = response.get("note").and_then(Value::as_str) else {
                continue;
            };
            let Ok(note) = serde_json::from_str::<Value>(note_raw) else {
                continue;
            };
            let Some(note) = note.as_object() else {
                continue;
            };

            if let Some(step_budgets) = note.get("resource_budgets").cloned() {
                resource_budgets.push(json!({
                    "step_id": step.step_id,
                    "tool_name": step.tool_name,
                    "resource_budgets": step_budgets,
                }));
            }
            if let Some(step_usage) = note.get("resource_usage").cloned() {
                resource_usage.push(json!({
                    "step_id": step.step_id,
                    "tool_name": step.tool_name,
                    "resource_usage": step_usage,
                }));
            }
        }

        json!({
            "resource_budgets": resource_budgets,
            "resource_usage": resource_usage,
        })
    }
}
