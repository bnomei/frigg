use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::domain::{FriggError, FriggResult};
use rmcp::handler::server::wrapper::Parameters;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::types::{
    FindReferencesParams, ListRepositoriesParams, ReadFileParams, SearchSymbolParams,
    SearchTextParams,
};

use super::*;

struct ResolvedDeepSearchPlaybookStep<'a> {
    step: &'a DeepSearchPlaybookStep,
    tool: DeepSearchStepTool,
}

impl DeepSearchHarness {
    pub fn new(server: FriggMcpServer) -> Self {
        Self { server }
    }

    pub fn load_playbook(path: &Path) -> FriggResult<DeepSearchPlaybook> {
        let raw = fs::read_to_string(path).map_err(FriggError::Io)?;
        serde_json::from_str::<DeepSearchPlaybook>(&raw).map_err(|err| {
            FriggError::InvalidInput(format!(
                "failed to parse deep-search playbook {}: {err}",
                path.display()
            ))
        })
    }

    pub fn persist_trace_artifact(
        path: &Path,
        artifact: &DeepSearchTraceArtifact,
    ) -> FriggResult<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(FriggError::Io)?;
        }
        let raw = serde_json::to_string_pretty(artifact).map_err(|err| {
            FriggError::Internal(format!(
                "failed to serialize deep-search trace artifact: {err}"
            ))
        })?;
        fs::write(path, raw).map_err(FriggError::Io)?;
        Ok(())
    }

    pub fn load_trace_artifact(path: &Path) -> FriggResult<DeepSearchTraceArtifact> {
        let raw = fs::read_to_string(path).map_err(FriggError::Io)?;
        serde_json::from_str::<DeepSearchTraceArtifact>(&raw).map_err(|err| {
            FriggError::InvalidInput(format!(
                "failed to parse deep-search trace artifact {}: {err}",
                path.display()
            ))
        })
    }

    pub async fn run_playbook(
        &self,
        playbook: &DeepSearchPlaybook,
    ) -> FriggResult<DeepSearchTraceArtifact> {
        let resolved_steps = resolve_playbook_steps(playbook)?;
        let mut trace_steps = Vec::with_capacity(playbook.steps.len());
        for (step_index, step) in resolved_steps.into_iter().enumerate() {
            let params_json = canonical_json_string(&step.step.params)?;
            let outcome = self.run_resolved_step(&step).await;
            if let DeepSearchTraceOutcome::Err {
                message,
                error_code: Some(error_code),
                ..
            } = &outcome
            {
                if error_code == "invalid_params" {
                    return Err(FriggError::InvalidInput(format!(
                        "deep-search playbook step '{}' failed with invalid_params: {message}",
                        step.step.step_id
                    )));
                }
            }
            trace_steps.push(DeepSearchTraceStep {
                step_index,
                step_id: step.step.step_id.clone(),
                tool_name: step.tool.as_str().to_owned(),
                params_json,
                outcome,
            });
        }

        Ok(DeepSearchTraceArtifact {
            trace_schema: "frigg.deep_search.trace.v1".to_owned(),
            playbook_id: playbook.playbook_id.clone(),
            step_count: trace_steps.len(),
            steps: trace_steps,
        })
    }

    pub async fn replay_and_diff(
        &self,
        playbook: &DeepSearchPlaybook,
        expected: &DeepSearchTraceArtifact,
    ) -> FriggResult<DeepSearchReplayCheck> {
        let replayed = self.run_playbook(playbook).await?;
        let diff = diff_trace_artifacts(expected, &replayed);

        Ok(DeepSearchReplayCheck {
            matches: diff.is_none(),
            diff,
            replayed,
        })
    }

    pub async fn replay_from_artifact_path(
        &self,
        playbook: &DeepSearchPlaybook,
        artifact_path: &Path,
    ) -> FriggResult<DeepSearchReplayCheck> {
        let expected = Self::load_trace_artifact(artifact_path)?;
        self.replay_and_diff(playbook, &expected).await
    }

    pub fn compose_citation_payload(
        trace: &DeepSearchTraceArtifact,
        answer: impl Into<String>,
    ) -> FriggResult<DeepSearchCitationPayload> {
        let mut claims = Vec::new();
        let mut citations = Vec::new();

        for step in &trace.steps {
            let DeepSearchTraceOutcome::Ok { response_json } = &step.outcome else {
                continue;
            };
            let response = serde_json::from_str::<Value>(response_json).map_err(|err| {
                FriggError::InvalidInput(format!(
                    "failed to parse response_json for deep-search step {}: {err}",
                    step.step_id
                ))
            })?;
            let evidences = collect_step_evidence(step, &response)?;
            for evidence in evidences {
                let citation_id = format!("cit-{:03}", citations.len() + 1);
                let claim_id = format!("claim-{:03}", claims.len() + 1);
                claims.push(DeepSearchClaim {
                    claim_id,
                    text: evidence.claim_text,
                    citation_ids: vec![citation_id.clone()],
                });
                citations.push(DeepSearchCitation {
                    citation_id,
                    tool_call_id: step.step_id.clone(),
                    tool_name: step.tool_name.clone(),
                    repository_id: evidence.repository_id,
                    path: evidence.path,
                    span: evidence.span,
                });
            }
        }

        let answer = answer.into();
        let answer = if answer.trim().is_empty() {
            claims
                .iter()
                .map(|claim| claim.text.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            answer
        };

        Ok(DeepSearchCitationPayload {
            answer_schema: "frigg.deep_search.answer.v1".to_owned(),
            playbook_id: trace.playbook_id.clone(),
            answer,
            claims,
            citations,
        })
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(super) async fn run_step(&self, step: &DeepSearchPlaybookStep) -> DeepSearchTraceOutcome {
        let resolved = match resolve_step_tool(step) {
            Ok(tool) => ResolvedDeepSearchPlaybookStep { step, tool },
            Err(error) => {
                return DeepSearchTraceOutcome::Err {
                    code: "INVALID_PARAMS".to_owned(),
                    message: error.to_string(),
                    error_code: Some("invalid_params".to_owned()),
                };
            }
        };
        self.run_resolved_step(&resolved).await
    }

    async fn run_resolved_step(
        &self,
        step: &ResolvedDeepSearchPlaybookStep<'_>,
    ) -> DeepSearchTraceOutcome {
        let result = match step.tool {
            DeepSearchStepTool::ListRepositories => {
                let params = decode_params::<ListRepositoriesParams>(&step.step.params);
                match params {
                    Ok(params) => match self.server.list_repositories(Parameters(params)).await {
                        Ok(response) => serde_json::to_value(response.0).map_err(map_json_error),
                        Err(error) => Err(map_error_data(error)),
                    },
                    Err(err) => Err(err),
                }
            }
            DeepSearchStepTool::ReadFile => {
                let params = decode_params::<ReadFileParams>(&step.step.params);
                match params {
                    Ok(params) => match self.server.read_file(Parameters(params)).await {
                        Ok(response) => serde_json::to_value(response.0).map_err(map_json_error),
                        Err(error) => Err(map_error_data(error)),
                    },
                    Err(err) => Err(err),
                }
            }
            DeepSearchStepTool::SearchText => {
                let params = decode_params::<SearchTextParams>(&step.step.params);
                match params {
                    Ok(params) => match self.server.search_text(Parameters(params)).await {
                        Ok(response) => serde_json::to_value(response.0).map_err(map_json_error),
                        Err(error) => Err(map_error_data(error)),
                    },
                    Err(err) => Err(err),
                }
            }
            DeepSearchStepTool::SearchSymbol => {
                let params = decode_params::<SearchSymbolParams>(&step.step.params);
                match params {
                    Ok(params) => match self.server.search_symbol(Parameters(params)).await {
                        Ok(response) => serde_json::to_value(response.0).map_err(map_json_error),
                        Err(error) => Err(map_error_data(error)),
                    },
                    Err(err) => Err(err),
                }
            }
            DeepSearchStepTool::FindReferences => {
                let params = decode_params::<FindReferencesParams>(&step.step.params);
                match params {
                    Ok(params) => match self.server.find_references(Parameters(params)).await {
                        Ok(response) => serde_json::to_value(response.0).map_err(map_json_error),
                        Err(error) => Err(map_error_data(error)),
                    },
                    Err(err) => Err(err),
                }
            }
        };

        match result {
            Ok(response) => {
                let normalized = normalize_trace_response_for_tool(step.tool.as_str(), response);
                match canonical_json_string(&normalized) {
                    Ok(response_json) => DeepSearchTraceOutcome::Ok { response_json },
                    Err(err) => DeepSearchTraceOutcome::Err {
                        code: "INTERNAL_ERROR".to_owned(),
                        message: err.to_string(),
                        error_code: Some("internal".to_owned()),
                    },
                }
            }
            Err(err) => DeepSearchTraceOutcome::Err {
                code: err.code,
                message: err.message,
                error_code: err.error_code,
            },
        }
    }
}

pub(super) fn normalize_trace_response_for_tool(tool_name: &str, response: Value) -> Value {
    match tool_name {
        "list_repositories" => normalize_list_repositories_response(response),
        _ => response,
    }
}

fn normalize_list_repositories_response(response: Value) -> Value {
    let Some(repositories) = response.get("repositories").and_then(Value::as_array) else {
        return response;
    };

    let normalized = repositories
        .iter()
        .map(|repository| {
            let mut value = serde_json::Map::new();
            for field in ["repository_id", "display_name", "root_path"] {
                if let Some(entry) = repository.get(field) {
                    value.insert(field.to_owned(), canonicalize_json_value(entry));
                }
            }
            Value::Object(value)
        })
        .collect::<Vec<_>>();

    json!({ "repositories": normalized })
}

fn resolve_playbook_steps<'a>(
    playbook: &'a DeepSearchPlaybook,
) -> FriggResult<Vec<ResolvedDeepSearchPlaybookStep<'a>>> {
    playbook
        .steps
        .iter()
        .map(|step| {
            resolve_step_tool(step).map(|tool| ResolvedDeepSearchPlaybookStep { step, tool })
        })
        .collect()
}

fn resolve_step_tool(step: &DeepSearchPlaybookStep) -> FriggResult<DeepSearchStepTool> {
    DeepSearchStepTool::from_tool_name(step.tool_name.as_str()).ok_or_else(|| {
        FriggError::InvalidInput(format!(
            "unsupported tool in playbook step '{}': {} (allowed tools: {})",
            step.step_id,
            step.tool_name,
            allowed_step_tools().join(", ")
        ))
    })
}

#[derive(Debug)]
pub(super) struct DeepSearchStepError {
    pub(super) code: String,
    pub(super) message: String,
    pub(super) error_code: Option<String>,
}

impl DeepSearchStepError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_PARAMS".to_owned(),
            message: message.into(),
            error_code: Some("invalid_params".to_owned()),
        }
    }
}

pub(super) fn decode_params<T>(value: &Value) -> Result<T, DeepSearchStepError>
where
    T: for<'de> Deserialize<'de>,
{
    serde_json::from_value::<T>(value.clone()).map_err(|err| {
        DeepSearchStepError::invalid_params(format!("invalid playbook step params: {err}"))
    })
}

fn map_error_data(error: rmcp::ErrorData) -> DeepSearchStepError {
    let error_code = error
        .data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
        .map(ToOwned::to_owned);

    DeepSearchStepError {
        code: format!("{:?}", error.code),
        message: error.message.to_string(),
        error_code,
    }
}

fn map_json_error(error: serde_json::Error) -> DeepSearchStepError {
    DeepSearchStepError {
        code: "INTERNAL_ERROR".to_owned(),
        message: format!("failed to serialize tool response as json: {error}"),
        error_code: Some("internal".to_owned()),
    }
}

#[derive(Debug, Clone)]
struct StepEvidence {
    claim_text: String,
    repository_id: String,
    path: String,
    span: DeepSearchFileSpan,
}

fn collect_step_evidence(
    step: &DeepSearchTraceStep,
    response: &Value,
) -> FriggResult<Vec<StepEvidence>> {
    match step.tool_name.as_str() {
        "list_repositories" => Ok(Vec::new()),
        "read_file" => {
            let context = format!("tool {} step {}", step.tool_name, step.step_id);
            let repository_id = required_string_field(response, "repository_id", &context)?;
            let path = required_string_field(response, "path", &context)?;
            let content_line_count = response
                .get("content")
                .and_then(Value::as_str)
                .map(|content| content.lines().count().max(1))
                .unwrap_or(1);
            Ok(vec![StepEvidence {
                claim_text: format!(
                    "Read file evidence from tool call {} at {}:{}.",
                    step.step_id, repository_id, path
                ),
                repository_id,
                path,
                span: DeepSearchFileSpan {
                    start_line: 1,
                    start_column: 1,
                    end_line: content_line_count,
                    end_column: 1,
                },
            }])
        }
        "search_text" => {
            let matches = required_matches_array(response, step)?;
            let mut evidences = Vec::with_capacity(matches.len());
            for (index, matched) in matches.iter().enumerate() {
                let context = format!(
                    "tool {} step {} match {}",
                    step.tool_name, step.step_id, index
                );
                let repository_id = required_string_field(matched, "repository_id", &context)?;
                let path = required_string_field(matched, "path", &context)?;
                let line = required_usize_field(matched, "line", &context)?;
                let column = required_usize_field(matched, "column", &context)?;
                let excerpt = optional_non_empty_string_field(matched, "excerpt")
                    .or_else(|| optional_non_empty_string_field(matched, "snippet"))
                    .map(truncate_claim_fragment)
                    .unwrap_or_else(|| "text match".to_owned());
                evidences.push(StepEvidence {
                    claim_text: format!(
                        "Text evidence from tool call {} at {}:{}:{}:{} ({excerpt}).",
                        step.step_id, repository_id, path, line, column
                    ),
                    repository_id,
                    path,
                    span: point_span(line, column),
                });
            }
            Ok(evidences)
        }
        "search_hybrid" => {
            let matches = required_matches_array(response, step)?;
            let mut evidences = Vec::with_capacity(matches.len());
            for (index, matched) in matches.iter().enumerate() {
                let context = format!(
                    "tool {} step {} match {}",
                    step.tool_name, step.step_id, index
                );
                let repository_id = required_string_field(matched, "repository_id", &context)?;
                let path = required_string_field(matched, "path", &context)?;
                let line = required_usize_field(matched, "line", &context)?;
                let column = required_usize_field(matched, "column", &context)?;
                let excerpt = optional_non_empty_string_field(matched, "excerpt")
                    .or_else(|| optional_non_empty_string_field(matched, "snippet"))
                    .map(truncate_claim_fragment)
                    .unwrap_or_else(|| "hybrid match".to_owned());
                evidences.push(StepEvidence {
                    claim_text: format!(
                        "Hybrid evidence from tool call {} at {}:{}:{}:{} ({excerpt}).",
                        step.step_id, repository_id, path, line, column
                    ),
                    repository_id,
                    path,
                    span: matched
                        .get("anchor")
                        .and_then(parse_anchor_span)
                        .unwrap_or_else(|| point_span(line, column)),
                });
            }
            Ok(evidences)
        }
        "search_symbol" => {
            let matches = required_matches_array(response, step)?;
            let mut evidences = Vec::with_capacity(matches.len());
            for (index, matched) in matches.iter().enumerate() {
                let context = format!(
                    "tool {} step {} match {}",
                    step.tool_name, step.step_id, index
                );
                let repository_id = required_string_field(matched, "repository_id", &context)?;
                let path = required_string_field(matched, "path", &context)?;
                let line = required_usize_field(matched, "line", &context)?;
                let symbol = matched
                    .get("symbol")
                    .and_then(Value::as_str)
                    .map(truncate_claim_fragment)
                    .unwrap_or_else(|| "symbol".to_owned());
                evidences.push(StepEvidence {
                    claim_text: format!(
                        "Symbol evidence from tool call {} for {} at {}:{}:{}.",
                        step.step_id, symbol, repository_id, path, line
                    ),
                    repository_id,
                    path,
                    span: point_span(line, 1),
                });
            }
            Ok(evidences)
        }
        "find_references" => {
            let matches = required_matches_array(response, step)?;
            let mut evidences = Vec::with_capacity(matches.len());
            for (index, matched) in matches.iter().enumerate() {
                let context = format!(
                    "tool {} step {} match {}",
                    step.tool_name, step.step_id, index
                );
                let repository_id = required_string_field(matched, "repository_id", &context)?;
                let path = required_string_field(matched, "path", &context)?;
                let line = required_usize_field(matched, "line", &context)?;
                let column = required_usize_field(matched, "column", &context)?;
                let symbol = matched
                    .get("symbol")
                    .and_then(Value::as_str)
                    .map(truncate_claim_fragment)
                    .unwrap_or_else(|| "symbol".to_owned());
                evidences.push(StepEvidence {
                    claim_text: format!(
                        "Reference evidence from tool call {} for {} at {}:{}:{}:{}.",
                        step.step_id, symbol, repository_id, path, line, column
                    ),
                    repository_id,
                    path,
                    span: point_span(line, column),
                });
            }
            Ok(evidences)
        }
        _ => Ok(Vec::new()),
    }
}

fn required_matches_array<'a>(
    response: &'a Value,
    step: &DeepSearchTraceStep,
) -> FriggResult<&'a Vec<Value>> {
    response
        .get("matches")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            FriggError::InvalidInput(format!(
                "tool {} step {} response is missing matches[] for citation composition",
                step.tool_name, step.step_id
            ))
        })
}

fn required_string_field(value: &Value, field: &str, context: &str) -> FriggResult<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| {
            FriggError::InvalidInput(format!(
                "{context} is missing required string field '{field}' for citation composition"
            ))
        })
}

fn optional_non_empty_string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
}

fn required_usize_field(value: &Value, field: &str, context: &str) -> FriggResult<usize> {
    let raw = value.get(field).and_then(Value::as_u64).ok_or_else(|| {
        FriggError::InvalidInput(format!(
            "{context} is missing required numeric field '{field}' for citation composition"
        ))
    })?;
    let normalized = usize::try_from(raw).map_err(|_| {
        FriggError::InvalidInput(format!(
            "{context} field '{field}' value {raw} exceeds usize bounds"
        ))
    })?;

    Ok(normalized.max(1))
}

fn point_span(line: usize, column: usize) -> DeepSearchFileSpan {
    DeepSearchFileSpan {
        start_line: line.max(1),
        start_column: column.max(1),
        end_line: line.max(1),
        end_column: column.max(1),
    }
}

fn parse_anchor_span(value: &Value) -> Option<DeepSearchFileSpan> {
    Some(DeepSearchFileSpan {
        start_line: usize::try_from(value.get("start_line")?.as_u64()?).ok()?,
        start_column: usize::try_from(value.get("start_column")?.as_u64()?).ok()?,
        end_line: usize::try_from(value.get("end_line")?.as_u64()?).ok()?,
        end_column: usize::try_from(value.get("end_column")?.as_u64()?).ok()?,
    })
}

fn truncate_claim_fragment(raw: &str) -> String {
    let max_chars = 120usize;
    if raw.chars().count() <= max_chars {
        return raw.to_owned();
    }

    let mut truncated = raw.chars().take(max_chars).collect::<String>();
    truncated.push_str("...");
    truncated
}

pub(super) fn diff_trace_artifacts(
    expected: &DeepSearchTraceArtifact,
    actual: &DeepSearchTraceArtifact,
) -> Option<String> {
    if expected.trace_schema != actual.trace_schema {
        return Some(format!(
            "trace_schema mismatch: expected '{}' but got '{}'",
            expected.trace_schema, actual.trace_schema
        ));
    }
    if expected.playbook_id != actual.playbook_id {
        return Some(format!(
            "playbook_id mismatch: expected '{}' but got '{}'",
            expected.playbook_id, actual.playbook_id
        ));
    }
    if expected.steps.len() != expected.step_count {
        return Some(format!(
            "expected trace steps length mismatch: step_count={} steps_len={}",
            expected.step_count,
            expected.steps.len()
        ));
    }
    if actual.steps.len() != actual.step_count {
        return Some(format!(
            "actual trace steps length mismatch: step_count={} steps_len={}",
            actual.step_count,
            actual.steps.len()
        ));
    }
    if expected.step_count != actual.step_count {
        return Some(format!(
            "step_count mismatch: expected {} but got {}",
            expected.step_count, actual.step_count
        ));
    }
    for (index, (expected_step, actual_step)) in
        expected.steps.iter().zip(actual.steps.iter()).enumerate()
    {
        if expected_step != actual_step {
            return Some(format!(
                "step[{index}] mismatch for tool '{}': expected={} actual={}",
                expected_step.tool_name,
                serialize_step_for_diff(expected_step),
                serialize_step_for_diff(actual_step)
            ));
        }
    }

    None
}

fn serialize_step_for_diff(step: &DeepSearchTraceStep) -> String {
    serde_json::to_string(step).unwrap_or_else(|_| "{\"serialization\":\"failed\"}".to_owned())
}

fn canonical_json_string(value: &Value) -> FriggResult<String> {
    let canonical = canonicalize_json_value(value);
    serde_json::to_string(&canonical).map_err(|err| {
        FriggError::Internal(format!(
            "failed to serialize canonical deep-search json payload: {err}"
        ))
    })
}

fn canonicalize_json_value(value: &Value) -> Value {
    match value {
        Value::Null => Value::Null,
        Value::Bool(flag) => Value::Bool(*flag),
        Value::Number(number) => Value::Number(number.clone()),
        Value::String(string) => Value::String(string.clone()),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .map(canonicalize_json_value)
                .collect::<Vec<_>>(),
        ),
        Value::Object(map) => {
            let mut ordered = BTreeMap::new();
            for (key, value) in map {
                ordered.insert(key.clone(), canonicalize_json_value(value));
            }

            let mut normalized = serde_json::Map::new();
            for (key, value) in ordered {
                normalized.insert(key, value);
            }
            Value::Object(normalized)
        }
    }
}
