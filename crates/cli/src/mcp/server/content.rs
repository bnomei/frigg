//! `read_file`, `read_match`, and `explore` implementations with path containment and runtime
//! file-content window reuse.
//!
//! Enforces workspace path containment on reads and reuses file-content windows within a tool
//! execution scope to limit duplicate IO.

use super::presentation::SessionResultHandleLookup;
use super::*;
use crate::mcp::types::ContextEfficiencyMetadata;
use serde::Serialize;

#[derive(Clone)]
pub(super) struct ReadFileProvenanceContext {
    tool_name: &'static str,
    extra_params: Value,
}

impl ReadFileProvenanceContext {
    pub(super) fn read_file() -> Self {
        Self {
            tool_name: "read_file",
            extra_params: Value::Null,
        }
    }

    pub(super) fn read_match(result_handle: &str, match_id: &str) -> Self {
        Self {
            tool_name: "read_match",
            extra_params: json!({
                "result_handle": result_handle,
                "match_id": match_id,
            }),
        }
    }

    fn merge_extra_params(&self, params: &mut Value) {
        let (Some(target), Some(extra)) = (params.as_object_mut(), self.extra_params.as_object())
        else {
            return;
        };
        for (key, value) in extra {
            target.insert(key.clone(), value.clone());
        }
    }
}

impl FriggMcpServer {
    pub(super) async fn read_file_impl(
        &self,
        params: ReadFileParams,
    ) -> Result<ReadFileResponse, ErrorData> {
        self.read_file_impl_with_provenance(params, ReadFileProvenanceContext::read_file())
            .await
    }

    pub(super) async fn read_file_impl_with_provenance(
        &self,
        params: ReadFileParams,
        provenance: ReadFileProvenanceContext,
    ) -> Result<ReadFileResponse, ErrorData> {
        let execution_context = self
            .read_only_tool_execution_context(provenance.tool_name, params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let provenance_for_blocking = provenance.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut resolved_repository_id: Option<String> = None;
                let mut resolved_path: Option<String> = None;
                let mut resolved_absolute_path: Option<String> = None;
                let mut effective_max_bytes: Option<usize> = None;
                let mut effective_start_line: Option<usize> = None;
                let mut effective_end_line: Option<usize> = None;
                let result = (|| -> Result<ReadFileResponse, ErrorData> {
                    let query_started_at = std::time::Instant::now();
                    let context_efficiency_log_enabled =
                        crate::context_efficiency::context_efficiency_log_enabled();
                    let need_context_efficiency =
                        crate::context_efficiency::need_context_efficiency_with_log_state(
                            params_for_blocking.include_context_efficiency,
                            context_efficiency_log_enabled,
                        ) || server.tool_call_display_enabled();
                    let requested_max_bytes = params_for_blocking
                        .max_bytes
                        .unwrap_or(server.config.max_file_bytes);
                    if requested_max_bytes == 0 {
                        return Err(Self::invalid_params(
                            "max_bytes must be greater than zero",
                            None,
                        ));
                    }

                    let max_bytes = requested_max_bytes.min(server.config.max_file_bytes);
                    effective_max_bytes = Some(max_bytes);
                    let has_line_range = params_for_blocking.start_line.is_some()
                        || params_for_blocking.end_line.is_some()
                        || params_for_blocking.line_count.is_some();
                    if params_for_blocking.start_line == Some(0) {
                        return Err(Self::invalid_params(
                            "start_line must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.end_line == Some(0) {
                        return Err(Self::invalid_params(
                            "end_line must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.line_count == Some(0) {
                        return Err(Self::invalid_params(
                            "line_count must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.end_line.is_some()
                        && params_for_blocking.line_count.is_some()
                    {
                        return Err(Self::invalid_params(
                            "end_line and line_count are mutually exclusive",
                            None,
                        ));
                    }
                    if let (Some(start_line), Some(end_line)) =
                        (params_for_blocking.start_line, params_for_blocking.end_line)
                        && end_line < start_line
                    {
                        return Err(Self::invalid_params(
                            "end_line must be greater than or equal to start_line",
                            Some(json!({
                                "start_line": start_line,
                                "end_line": end_line,
                            })),
                        ));
                    }

                    let (repository_id, path, display_path) =
                        server.resolve_file_path(&params_for_blocking)?;
                    resolved_repository_id = Some(repository_id.clone());
                    resolved_path = Some(display_path.clone());
                    resolved_absolute_path = Some(path.display().to_string());

                    let workspace = server
                        .attached_workspaces_for_repository(Some(repository_id.as_str()))?
                        .into_iter()
                        .find(|workspace| workspace.repository_id == repository_id)
                        .ok_or_else(|| {
                            Self::resource_not_found(
                                "repository_id not found",
                                Some(json!({ "repository_id": repository_id })),
                            )
                        })?;
                    let pre_read_bytes = if !has_line_range {
                        let metadata = fs::metadata(&path).map_err(|err| {
                            Self::internal(
                                format!("failed to stat file {}: {err}", path.display()),
                                None,
                            )
                        })?;
                        Some(usize::try_from(metadata.len()).unwrap_or(usize::MAX))
                    } else {
                        None
                    };
                    if let Some(pre_read_bytes) = pre_read_bytes
                        && pre_read_bytes > max_bytes
                    {
                        let suggested_max_bytes = pre_read_bytes.min(server.config.max_file_bytes);
                        return Err(Self::invalid_params(
                            format!("file exceeds max_bytes={max_bytes}"),
                            Some(json!({
                                "path": display_path.clone(),
                                "bytes": pre_read_bytes,
                                "max_bytes": max_bytes,
                                "requested_max_bytes": requested_max_bytes,
                                "config_max_file_bytes": server.config.max_file_bytes,
                                "suggested_max_bytes": suggested_max_bytes,
                            })),
                        ));
                    }
                    let snapshot = server.file_content_snapshot_for_workspace(&workspace, &path)?;
                    let _pre_read_bytes =
                        pre_read_bytes.unwrap_or_else(|| snapshot.raw_bytes_len());
                    if !has_line_range {
                        let post_read_bytes = snapshot.raw_bytes_len();
                        if post_read_bytes > max_bytes {
                            let suggested_max_bytes = post_read_bytes.min(server.config.max_file_bytes);
                            return Err(Self::invalid_params(
                                format!("file exceeds max_bytes={max_bytes}"),
                                Some(json!({
                                    "path": display_path.clone(),
                                    "bytes": post_read_bytes,
                                    "max_bytes": max_bytes,
                                    "requested_max_bytes": requested_max_bytes,
                                    "config_max_file_bytes": server.config.max_file_bytes,
                                    "suggested_max_bytes": suggested_max_bytes,
                                })),
                            ));
                        }
                        let content = snapshot.read_file_content();
                        let total_lines = if content.is_empty() {
                            0
                        } else {
                            content.lines().count().max(1)
                        };
                        let mut response = ReadFileResponse {
                            repository_id,
                            path: display_path,
                            start_line: Some(1),
                            end_line: Some(total_lines.max(1)),
                            bytes: post_read_bytes,
                            content,
                            context_efficiency: None,
                        };
                        if need_context_efficiency {
                            let context_efficiency = server
                                .context_efficiency_metadata_for_tool_observers(
                                    &execution_context_for_blocking,
                                    params_for_blocking.include_context_efficiency,
                                    context_efficiency_log_enabled,
                                    || {
                                        server.read_surface_context_efficiency_metadata(
                                            &response.repository_id,
                                            &response.path,
                                            response.bytes,
                                            None,
                                            Some(Self::context_efficiency_elapsed_ms(
                                                query_started_at,
                                            )),
                                        )
                                    },
                                )?;
                            if let Some(context_efficiency) = context_efficiency.as_ref() {
                                server.append_context_efficiency_log_for_workspaces(
                                    provenance_for_blocking.tool_name,
                                    std::slice::from_ref(&workspace),
                                    context_efficiency,
                                );
                            }
                            if params_for_blocking.include_context_efficiency == Some(true) {
                                response.context_efficiency = context_efficiency;
                            }
                        }
                        return Ok(response);
                    }

                    let start_line = params_for_blocking.start_line.unwrap_or(1);
                    let requested_line_end = match params_for_blocking.end_line {
                        Some(end_line) => Some(end_line),
                        None => params_for_blocking.line_count.map(|line_count| {
                            start_line.saturating_add(line_count.saturating_sub(1))
                        }),
                    };
                    let effective_end_hint = requested_line_end;
                    effective_start_line = Some(start_line);
                    effective_end_line = Some(effective_end_hint.unwrap_or(1));

                    let line_slice = snapshot
                        .read_line_slice_lossy(start_line, requested_line_end, max_bytes)
                        .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
                    let sliced_content = line_slice.content;
                    let sliced_bytes = line_slice.bytes;
                    let total_lines = line_slice.total_lines;
                    let effective_end = requested_line_end
                        .unwrap_or(total_lines.max(1))
                        .min(total_lines.max(1));
                    effective_end_line = Some(effective_end);

                    if sliced_bytes > max_bytes {
                        let suggested_max_bytes = sliced_bytes.min(server.config.max_file_bytes);
                        return Err(Self::invalid_params(
                            format!("selected line range exceeds max_bytes={max_bytes}"),
                            Some(json!({
                                "path": display_path.clone(),
                                "bytes": sliced_bytes,
                                "max_bytes": max_bytes,
                                "requested_max_bytes": requested_max_bytes,
                                "config_max_file_bytes": server.config.max_file_bytes,
                                "suggested_max_bytes": suggested_max_bytes,
                                "start_line": start_line,
                                "end_line": effective_end,
                                "total_lines": total_lines,
                            })),
                        ));
                    }

                    let mut response = ReadFileResponse {
                        repository_id,
                        path: display_path,
                        start_line: Some(start_line),
                        end_line: Some(effective_end),
                        bytes: sliced_bytes,
                        content: sliced_content,
                        context_efficiency: None,
                    };
                    if need_context_efficiency {
                        let context_efficiency = server
                            .context_efficiency_metadata_for_tool_observers(
                                &execution_context_for_blocking,
                                params_for_blocking.include_context_efficiency,
                                context_efficiency_log_enabled,
                                || {
                                    server.read_surface_context_efficiency_metadata(
                                        &response.repository_id,
                                        &response.path,
                                        response.bytes,
                                        None,
                                        Some(Self::context_efficiency_elapsed_ms(query_started_at)),
                                    )
                                },
                            )?;
                        if let Some(context_efficiency) = context_efficiency.as_ref() {
                            server.append_context_efficiency_log_for_workspaces(
                                provenance_for_blocking.tool_name,
                                std::slice::from_ref(&workspace),
                                context_efficiency,
                            );
                        }
                        if params_for_blocking.include_context_efficiency == Some(true) {
                            response.context_efficiency = context_efficiency;
                        }
                    }
                    Ok(response)
                })();
                let repository_ids = resolved_repository_id
                    .clone()
                    .or_else(|| execution_context_for_blocking.repository_hint.clone())
                    .into_iter()
                    .collect::<Vec<_>>();
                let normalized_workload = (!repository_ids.is_empty()).then(|| {
                    execution_context_for_blocking
                        .normalized_workload(&repository_ids, WorkloadPrecisionMode::Exact)
                });
                let finalization = server.tool_execution_finalization(
                    json!({
                        "resolved_repository_id": resolved_repository_id.clone(),
                        "resolved_path": resolved_path
                            .clone()
                            .map(|path| Self::bounded_text(&path)),
                        "resolved_absolute_path": resolved_absolute_path
                            .clone()
                            .map(|path| Self::bounded_text(&path)),
                    }),
                    normalized_workload,
                );
                let mut provenance_params = json!({
                    "repository_id": execution_context_for_blocking.repository_hint,
                    "path": Self::bounded_text(&params_for_blocking.path),
                    "max_bytes": params_for_blocking.max_bytes,
                    "start_line": params_for_blocking.start_line,
                    "end_line": params_for_blocking.end_line,
                    "line_count": params_for_blocking.line_count,
                    "effective_max_bytes": effective_max_bytes,
                    "effective_start_line": effective_start_line,
                    "effective_end_line": effective_end_line,
                });
                provenance_for_blocking.merge_extra_params(&mut provenance_params);
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    provenance_for_blocking.tool_name,
                    execution_context_for_blocking.repository_hint.as_deref(),
                    provenance_params,
                    finalization.source_refs,
                    Self::provenance_outcome(&result),
                    finalization.normalized_workload,
                );

                ReadFileExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    pub(super) async fn read_match_impl(
        &self,
        params: ReadMatchParams,
    ) -> Result<ReadMatchResponse, ErrorData> {
        let started_at = Instant::now();
        let anchor = match self
            .session_result_handle_lookup(&params.result_handle, &params.match_id)
        {
            SessionResultHandleLookup::Found(anchor) => anchor,
            SessionResultHandleLookup::StaleHandle => {
                let recovery = RecoveryFields::stale_handle(
                    Some(params.result_handle.as_str()),
                    Some(params.match_id.as_str()),
                );
                crate::mcp::routing_stats::record_handle_failure();
                let message = recovery
                    .message
                    .clone()
                    .unwrap_or_else(|| "result_handle not found".to_owned());
                let result: Result<ReadMatchResponse, ErrorData> = Err(Self::resource_not_found(
                    message,
                    Some(json!({
                        "error_code": recovery.error_code.clone().unwrap_or_else(|| "STALE_HANDLE".to_owned()),
                        "result_handle": params.result_handle,
                        "match_id": params.match_id,
                        "correction_hint": recovery.correction_hint,
                        "related_tools": recovery.related_tools,
                        "suggested_next": recovery.suggested_next,
                    })),
                ));
                return self.finalize_with_provenance_timed(
                    "read_match",
                    started_at,
                    result,
                    Ok(()),
                    None,
                );
            }
            SessionResultHandleLookup::MixedHandle {
                foreign_handle_has_match,
                foreign_handle,
            } => {
                let recovery = RecoveryFields::mixed_handle(
                    Some(params.result_handle.as_str()),
                    Some(params.match_id.as_str()),
                );
                crate::mcp::routing_stats::record_handle_failure();
                let message = if foreign_handle_has_match {
                    format!(
                        "match_id {:?} does not belong to result_handle {:?} (belongs to another handle{}).",
                        params.match_id,
                        params.result_handle,
                        foreign_handle
                            .as_ref()
                            .map(|handle| format!(" {handle:?}"))
                            .unwrap_or_default()
                    )
                } else {
                    recovery
                        .message
                        .clone()
                        .unwrap_or_else(|| "match_id does not belong to result_handle".to_owned())
                };
                let result: Result<ReadMatchResponse, ErrorData> = Err(Self::resource_not_found(
                    message,
                    Some(json!({
                        "error_code": recovery.error_code.clone().unwrap_or_else(|| "MIXED_HANDLE".to_owned()),
                        "result_handle": params.result_handle,
                        "match_id": params.match_id,
                        "foreign_handle": foreign_handle,
                        "correction_hint": recovery.correction_hint,
                        "related_tools": recovery.related_tools,
                        "suggested_next": recovery.suggested_next,
                    })),
                ));
                return self.finalize_with_provenance_timed(
                    "read_match",
                    started_at,
                    result,
                    Ok(()),
                    None,
                );
            }
        };
        let before = params.before.unwrap_or(10).min(MAX_CONTEXT_LINES);
        let after = params.after.unwrap_or(10).min(MAX_CONTEXT_LINES);
        let line_start = anchor.line.saturating_sub(before).max(1);
        let line_end = anchor.line.saturating_add(after);
        let read_params = ReadFileParams {
            path: anchor.path.clone(),
            repository_id: Some(anchor.repository_id.clone()),
            max_bytes: None,
            start_line: Some(line_start),
            end_line: Some(line_end),
            line_count: None,
            presentation_mode: Some(ReadPresentationMode::Json),
            include_context_efficiency: params.include_context_efficiency,
        };
        let read = self
            .read_file_impl_with_provenance(
                read_params,
                ReadFileProvenanceContext::read_match(&params.result_handle, &params.match_id),
            )
            .await?;
        let effective_end_line = {
            let read_params = ReadFileParams {
                path: read.path.clone(),
                repository_id: Some(read.repository_id.clone()),
                max_bytes: None,
                start_line: Some(line_start),
                end_line: Some(line_end),
                line_count: None,
                presentation_mode: Some(ReadPresentationMode::Json),
                include_context_efficiency: None,
            };
            let (repository_id, path, _) = self.resolve_file_path(&read_params)?;
            let workspace = self
                .attached_workspaces_for_repository(Some(repository_id.as_str()))?
                .into_iter()
                .find(|workspace| workspace.repository_id == repository_id)
                .ok_or_else(|| {
                    Self::resource_not_found(
                        "repository_id not found",
                        Some(json!({ "repository_id": repository_id })),
                    )
                })?;
            let line_slice = self
                .file_content_snapshot_for_workspace(&workspace, &path)?
                .read_line_slice_lossy(line_start, Some(line_end), self.config.max_file_bytes)
                .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
            if line_slice.total_lines == 0 {
                0
            } else {
                line_end.min(line_slice.total_lines)
            }
        };
        Ok(ReadMatchResponse {
            repository_id: read.repository_id,
            path: read.path,
            line: anchor.line,
            column: anchor.column,
            start_line: line_start,
            end_line: effective_end_line,
            bytes: read.bytes,
            content: read.content,
            context_efficiency: read.context_efficiency,
        })
    }

    pub(super) async fn explore_impl(
        &self,
        params: ExploreParams,
    ) -> Result<ExploreResponse, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("explore", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut resolved_repository_id: Option<String> = None;
                let mut resolved_path: Option<String> = None;
                let mut resolved_absolute_path: Option<String> = None;
                let mut effective_context_lines: Option<usize> = None;
                let mut effective_max_matches: Option<usize> = None;
                let mut scan_scope = None;
                let mut total_matches = 0usize;
                let mut truncated = false;

                let result = (|| -> Result<ExploreResponse, ErrorData> {
                    let query_started_at = std::time::Instant::now();
                    let context_efficiency_log_enabled =
                        crate::context_efficiency::context_efficiency_log_enabled();
                    let need_context_efficiency =
                        crate::context_efficiency::need_context_efficiency_with_log_state(
                            params_for_blocking.include_context_efficiency,
                            context_efficiency_log_enabled,
                        ) || server.tool_call_display_enabled();
                    let requested_context_lines = params_for_blocking
                        .context_lines
                        .unwrap_or(DEFAULT_CONTEXT_LINES);
                    let context_lines = requested_context_lines.min(MAX_CONTEXT_LINES);
                    effective_context_lines = Some(context_lines);

                    let requested_max_matches = params_for_blocking
                        .max_matches
                        .unwrap_or(DEFAULT_MAX_MATCHES);
                    if requested_max_matches == 0 {
                        return Err(Self::invalid_params(
                            "max_matches must be greater than zero",
                            None,
                        ));
                    }
                    let max_matches =
                        requested_max_matches.min(server.config.max_search_results.max(1));
                    effective_max_matches = Some(max_matches);

                    let operation = params_for_blocking.operation;
                    let query = params_for_blocking
                        .query
                        .as_ref()
                        .map(|value| value.trim().to_owned());
                    let anchor = params_for_blocking.anchor.clone();
                    let resume_from = params_for_blocking.resume_from.clone();

                    let (
                        matcher,
                        response_query,
                        response_pattern_type,
                        scope,
                        include_scope_content,
                    ) = match operation {
                        ExploreOperation::Probe => {
                            if anchor.is_some() {
                                return Err(Self::invalid_params(
                                    "anchor is not allowed for probe",
                                    None,
                                ));
                            }
                            let Some(query) = query.clone().filter(|value| !value.is_empty())
                            else {
                                return Err(Self::invalid_params("query must not be empty", None));
                            };
                            if let Some(cursor) = resume_from.as_ref() {
                                validate_cursor(cursor).map_err(|message| {
                                    Self::invalid_params(
                                        message,
                                        Some(json!({ "resume_from": cursor })),
                                    )
                                })?;
                            }

                            let pattern_type = params_for_blocking
                                .pattern_type
                                .clone()
                                .unwrap_or(SearchPatternType::Literal);
                            let matcher = match pattern_type.clone() {
                                SearchPatternType::Literal => {
                                    ExploreMatcher::Literal(query.clone())
                                }
                                SearchPatternType::Regex => {
                                    let regex = compile_safe_regex(&query).map_err(|err| {
                                        Self::invalid_params(
                                            format!("invalid query regex: {err}"),
                                            Some(json!({
                                                "query": query,
                                                "regex_error_code": err.code(),
                                            })),
                                        )
                                    })?;
                                    if regex.is_match("") {
                                        return Err(Self::invalid_params(
                                            "query regex must not match empty strings",
                                            Some(json!({ "query": query })),
                                        ));
                                    }
                                    ExploreMatcher::Regex(regex)
                                }
                            };

                            (
                                Some(matcher),
                                Some(query),
                                Some(pattern_type),
                                ExploreScopeRequest {
                                    start_line: resume_from
                                        .as_ref()
                                        .map(|cursor| cursor.line)
                                        .unwrap_or(1),
                                    end_line: None,
                                },
                                false,
                            )
                        }
                        ExploreOperation::Zoom => {
                            if params_for_blocking.query.is_some() {
                                return Err(Self::invalid_params(
                                    "query is not allowed for zoom",
                                    None,
                                ));
                            }
                            if params_for_blocking.pattern_type.is_some() {
                                return Err(Self::invalid_params(
                                    "pattern_type is not allowed for zoom",
                                    None,
                                ));
                            }
                            if resume_from.is_some() {
                                return Err(Self::invalid_params(
                                    "resume_from is not allowed for zoom",
                                    None,
                                ));
                            }
                            let Some(anchor) = anchor.as_ref() else {
                                return Err(Self::invalid_params(
                                    "anchor is required for zoom",
                                    None,
                                ));
                            };
                            validate_anchor(anchor).map_err(|message| {
                                Self::invalid_params(message, Some(json!({ "anchor": anchor })))
                            })?;
                            let scope_window = line_window_around_anchor(anchor, context_lines);
                            (
                                None,
                                None,
                                None,
                                ExploreScopeRequest {
                                    start_line: scope_window.start_line,
                                    end_line: Some(scope_window.end_line),
                                },
                                true,
                            )
                        }
                        ExploreOperation::Refine => {
                            let Some(anchor) = anchor.as_ref() else {
                                return Err(Self::invalid_params(
                                    "anchor is required for refine",
                                    None,
                                ));
                            };
                            validate_anchor(anchor).map_err(|message| {
                                Self::invalid_params(message, Some(json!({ "anchor": anchor })))
                            })?;
                            let Some(query) = query.clone().filter(|value| !value.is_empty())
                            else {
                                return Err(Self::invalid_params("query must not be empty", None));
                            };
                            let scope_window = line_window_around_anchor(anchor, context_lines);
                            if let Some(cursor) = resume_from.as_ref() {
                                validate_cursor(cursor).map_err(|message| {
                                    Self::invalid_params(
                                        message,
                                        Some(json!({ "resume_from": cursor })),
                                    )
                                })?;
                                if cursor.line < scope_window.start_line
                                    || cursor.line > scope_window.end_line
                                {
                                    return Err(Self::invalid_params(
                                        "resume_from must stay within the refine scan scope",
                                        Some(json!({
                                            "resume_from": cursor,
                                            "scan_scope": scope_window.clone(),
                                        })),
                                    ));
                                }
                            }

                            let pattern_type = params_for_blocking
                                .pattern_type
                                .clone()
                                .unwrap_or(SearchPatternType::Literal);
                            let matcher = match pattern_type.clone() {
                                SearchPatternType::Literal => {
                                    ExploreMatcher::Literal(query.clone())
                                }
                                SearchPatternType::Regex => {
                                    let regex = compile_safe_regex(&query).map_err(|err| {
                                        Self::invalid_params(
                                            format!("invalid query regex: {err}"),
                                            Some(json!({
                                                "query": query,
                                                "regex_error_code": err.code(),
                                            })),
                                        )
                                    })?;
                                    if regex.is_match("") {
                                        return Err(Self::invalid_params(
                                            "query regex must not match empty strings",
                                            Some(json!({ "query": query })),
                                        ));
                                    }
                                    ExploreMatcher::Regex(regex)
                                }
                            };

                            (
                                Some(matcher),
                                Some(query),
                                Some(pattern_type),
                                ExploreScopeRequest {
                                    start_line: scope_window.start_line,
                                    end_line: Some(scope_window.end_line),
                                },
                                true,
                            )
                        }
                    };

                    let read_params = ReadFileParams {
                        path: params_for_blocking.path.clone(),
                        repository_id: params_for_blocking.repository_id.clone(),
                        max_bytes: None,
                        start_line: None,
                        end_line: None,
                        line_count: None,
                        presentation_mode: Some(ReadPresentationMode::Json),
                        include_context_efficiency: None,
                    };
                    let (repository_id, path, display_path) =
                        server.resolve_file_path(&read_params)?;
                    resolved_repository_id = Some(repository_id.clone());
                    resolved_path = Some(display_path.clone());
                    resolved_absolute_path = Some(path.display().to_string());

                    let workspace = server
                        .attached_workspaces_for_repository(Some(repository_id.as_str()))?
                        .into_iter()
                        .find(|workspace| workspace.repository_id == repository_id)
                        .ok_or_else(|| {
                            Self::resource_not_found(
                                "repository_id not found",
                                Some(json!({ "repository_id": repository_id })),
                            )
                        })?;
                    let snapshot = server.file_content_snapshot_for_workspace(&workspace, &path)?;
                    let scan = snapshot.scan_file_scope_lossy(
                        scope,
                        matcher.as_ref(),
                        max_matches,
                        resume_from.as_ref(),
                        include_scope_content,
                        include_scope_content.then_some(server.config.max_file_bytes),
                    );

                    if let Some(anchor) = anchor.as_ref()
                        && (scan.total_lines == 0 || anchor.end_line > scan.total_lines)
                    {
                        return Err(Self::invalid_params(
                            "anchor is outside file bounds",
                            Some(json!({
                                "anchor": anchor,
                                "total_lines": scan.total_lines,
                            })),
                        ));
                    }
                    if let Some(cursor) = resume_from.as_ref()
                        && cursor.line > scan.total_lines
                        && !(scan.total_lines == 0 && cursor.line == 1)
                    {
                        return Err(Self::invalid_params(
                            "resume_from is outside file bounds",
                            Some(json!({
                                "resume_from": cursor,
                                "total_lines": scan.total_lines,
                            })),
                        ));
                    }

                    let window = if include_scope_content {
                        if !scan.scope_within_budget {
                            return Err(Self::line_slice_budget_error(
                                &display_path,
                                scan.scope_bytes.unwrap_or(0),
                                server.config.max_file_bytes,
                                scope.start_line,
                                scan.effective_scope.end_line,
                                scan.total_lines,
                            ));
                        }

                        Some(ExploreWindow {
                            start_line: scan.effective_scope.start_line,
                            end_line: scan.effective_scope.end_line,
                            bytes: scan.scope_bytes.unwrap_or(0),
                            content: scan.scope_content.clone().unwrap_or_default(),
                        })
                    } else {
                        None
                    };

                    let mut matches = Vec::with_capacity(scan.matches.len());
                    for (index, matched) in scan.matches.iter().enumerate() {
                        let match_window =
                            line_window_around_anchor(&matched.anchor, context_lines);
                        let match_window_slice = snapshot
                            .read_line_slice_lossy(
                                match_window.start_line,
                                Some(match_window.end_line),
                                server.config.max_file_bytes,
                            )
                            .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
                        if match_window_slice.bytes > server.config.max_file_bytes {
                            return Err(Self::line_slice_budget_error(
                                &display_path,
                                match_window_slice.bytes,
                                server.config.max_file_bytes,
                                match_window.start_line,
                                match_window.end_line.min(
                                    match_window_slice.total_lines.max(match_window.start_line),
                                ),
                                match_window_slice.total_lines,
                            ));
                        }
                        let match_window_end = if match_window_slice.total_lines == 0 {
                            0
                        } else {
                            match_window.end_line.min(match_window_slice.total_lines)
                        };

                        matches.push(ExploreMatch {
                            match_id: format!("match-{index:04}"),
                            start_line: matched.start_line,
                            start_column: matched.start_column,
                            end_line: matched.end_line,
                            end_column: matched.end_column,
                            excerpt: matched.excerpt.clone(),
                            window: ExploreWindow {
                                start_line: match_window.start_line,
                                end_line: match_window_end,
                                bytes: match_window_slice.bytes,
                                content: match_window_slice.content,
                            },
                            anchor: matched.anchor.clone(),
                        });
                    }

                    scan_scope = Some(scan.effective_scope.clone());
                    total_matches = scan.total_matches;
                    truncated = scan.truncated;

                    let context_efficiency = if need_context_efficiency {
                        let returned_source_bytes_estimate = if let Some(window) = window.as_ref() {
                            window.bytes
                        } else {
                            matches
                                .iter()
                                .map(|matched| matched.window.bytes)
                                .fold(0usize, usize::saturating_add)
                        };
                        let context_efficiency = server
                            .context_efficiency_metadata_for_tool_observers(
                                &execution_context_for_blocking,
                                params_for_blocking.include_context_efficiency,
                                context_efficiency_log_enabled,
                                || {
                                    server.read_surface_context_efficiency_metadata(
                                        &repository_id,
                                        &display_path,
                                        returned_source_bytes_estimate,
                                        Some(scan.total_matches),
                                        Some(Self::context_efficiency_elapsed_ms(query_started_at)),
                                    )
                                },
                            )?;
                        if let Some(context_efficiency) = context_efficiency.as_ref() {
                            server.append_context_efficiency_log_for_workspaces(
                                "explore",
                                std::slice::from_ref(&workspace),
                                context_efficiency,
                            );
                        }
                        if params_for_blocking.include_context_efficiency == Some(true) {
                            context_efficiency
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    Ok(ExploreResponse {
                        repository_id,
                        path: display_path,
                        operation,
                        query: response_query,
                        pattern_type: response_pattern_type,
                        total_lines: scan.total_lines,
                        scan_scope: scan.effective_scope,
                        window,
                        total_matches: scan.total_matches,
                        matches,
                        truncated: scan.truncated,
                        resume_from: scan.resume_from,
                        metadata: ExploreMetadata {
                            lossy_utf8: scan.lossy_utf8,
                            effective_context_lines: context_lines,
                            effective_max_matches: max_matches,
                            context_efficiency,
                        },
                    })
                })();

                ExploreExecution {
                    result,
                    resolved_repository_id,
                    resolved_path,
                    resolved_absolute_path,
                    effective_context_lines,
                    effective_max_matches,
                    scan_scope,
                    total_matches,
                    truncated,
                }
            })
            .await?;

        let result = execution.result;
        let repository_ids = execution
            .resolved_repository_id
            .clone()
            .or_else(|| execution_context.repository_hint.clone())
            .into_iter()
            .collect::<Vec<_>>();
        let metadata =
            execution_context.normalized_workload(&repository_ids, WorkloadPrecisionMode::Exact);
        let provenance_result = self
            .record_provenance_blocking_with_metadata(
                "explore",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "path": Self::bounded_text(&params.path),
                    "operation": params.operation,
                    "query": params.query.as_ref().map(|value| Self::bounded_text(value)),
                    "pattern_type": params.pattern_type,
                    "context_lines": params.context_lines,
                    "max_matches": params.max_matches,
                    "resume_from": params.resume_from,
                    "effective_context_lines": execution.effective_context_lines,
                    "effective_max_matches": execution.effective_max_matches,
                }),
                json!({
                    "resolved_repository_id": execution.resolved_repository_id,
                    "resolved_path": execution
                        .resolved_path
                        .map(|path| Self::bounded_text(&path)),
                    "resolved_absolute_path": execution
                        .resolved_absolute_path
                        .map(|path| Self::bounded_text(&path)),
                    "scan_scope": execution.scan_scope,
                    "total_matches": execution.total_matches,
                    "truncated": execution.truncated,
                }),
                Some(metadata),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    pub(super) fn read_presentation_mode(
        mode: Option<ReadPresentationMode>,
    ) -> ReadPresentationMode {
        mode.unwrap_or(ReadPresentationMode::Text)
    }

    pub(super) fn explore_presentation_mode(
        params: &ExploreParams,
    ) -> Result<ReadPresentationMode, ErrorData> {
        match params.presentation_mode {
            Some(ReadPresentationMode::Json) => Ok(ReadPresentationMode::Json),
            Some(mode @ (ReadPresentationMode::Text | ReadPresentationMode::Citation))
                if matches!(
                    params.operation,
                    ExploreOperation::Probe | ExploreOperation::Refine
                ) =>
            {
                Err(Self::invalid_params(
                    "presentation_mode=text and presentation_mode=citation are only supported for zoom",
                    Some(json!({
                        "operation": params.operation,
                        "presentation_mode": mode,
                    })),
                ))
            }
            Some(ReadPresentationMode::Text) => Ok(ReadPresentationMode::Text),
            Some(ReadPresentationMode::Citation) => Ok(ReadPresentationMode::Citation),
            None if params.operation == ExploreOperation::Zoom => Ok(ReadPresentationMode::Text),
            None => Ok(ReadPresentationMode::Json),
        }
    }

    pub(super) fn present_read_file_result(
        &self,
        params: &ReadFileParams,
        response: ReadFileResponse,
    ) -> Result<CallToolResult, ErrorData> {
        match Self::read_presentation_mode(params.presentation_mode) {
            ReadPresentationMode::Json => Self::structured_tool_result(&response),
            ReadPresentationMode::Text => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                Ok(Self::text_read_surface_result(response.content))
            }
            ReadPresentationMode::Citation => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                let start_line = response.start_line.unwrap_or(1);
                Ok(Self::text_read_surface_result(Self::format_citation_text(
                    start_line,
                    &response.content,
                )))
            }
        }
    }

    pub(super) fn present_read_match_result(
        &self,
        params: &ReadMatchParams,
        response: ReadMatchResponse,
    ) -> Result<CallToolResult, ErrorData> {
        match Self::read_presentation_mode(params.presentation_mode) {
            ReadPresentationMode::Json => Self::structured_tool_result(&response),
            ReadPresentationMode::Text => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                Ok(Self::text_read_surface_result(response.content))
            }
            ReadPresentationMode::Citation => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                Ok(Self::text_read_surface_result(Self::format_citation_text(
                    response.start_line,
                    &response.content,
                )))
            }
        }
    }

    pub(super) fn present_explore_result(
        &self,
        params: &ExploreParams,
        response: ExploreResponse,
    ) -> Result<CallToolResult, ErrorData> {
        let mode = Self::explore_presentation_mode(params)?;
        match mode {
            ReadPresentationMode::Json => Self::structured_tool_result(&response),
            ReadPresentationMode::Text | ReadPresentationMode::Citation => {
                Self::reject_text_context_efficiency(params.include_context_efficiency)?;
                let Some(window) = response.window else {
                    return Err(Self::internal(
                        "explore zoom response missing window",
                        Some(json!({
                            "operation": response.operation,
                            "path": response.path,
                        })),
                    ));
                };
                let content = if mode == ReadPresentationMode::Citation {
                    Self::format_citation_text(window.start_line, &window.content)
                } else {
                    window.content
                };
                Ok(Self::text_read_surface_result(content))
            }
        }
    }

    /// Format source as `LINE|content` lines for citation-trained agents.
    pub(super) fn format_citation_text(start_line: usize, content: &str) -> String {
        if content.is_empty() {
            return String::new();
        }
        let ends_with_newline = content.ends_with('\n');
        let mut out = String::with_capacity(content.len().saturating_add(content.len() / 8 + 8));
        for (offset, line) in content.lines().enumerate() {
            let line_no = start_line.saturating_add(offset);
            out.push_str(&line_no.to_string());
            out.push('|');
            out.push_str(line);
            out.push('\n');
        }
        if !ends_with_newline && out.ends_with('\n') {
            out.pop();
        }
        out
    }

    fn read_surface_context_efficiency_metadata(
        &self,
        repository_id: &str,
        path: &str,
        returned_source_bytes_estimate: usize,
        returned_match_count: Option<usize>,
        query_duration_ms: Option<u64>,
    ) -> Result<ContextEfficiencyMetadata, ErrorData> {
        let workspace = self
            .attached_workspaces_for_repository(Some(repository_id))?
            .into_iter()
            .find(|workspace| workspace.repository_id == repository_id);
        let summary = workspace
            .as_ref()
            .map(|workspace| {
                if !workspace.db_path.exists() {
                    return Ok(None);
                }
                Storage::new(&workspace.db_path)
                    .load_latest_context_efficiency_manifest_summary_for_repository(
                        &workspace.runtime_repository_id,
                    )
                    .map_err(Self::map_frigg_error)
            })
            .transpose()?
            .flatten();

        let indexed_readable_files = summary
            .as_ref()
            .map(|summary| summary.indexed_readable_files)
            .unwrap_or(0);
        let indexed_readable_bytes = summary
            .as_ref()
            .map(|summary| summary.indexed_readable_bytes)
            .unwrap_or(0);
        let indexed_min_mtime_ns = summary.as_ref().and_then(|summary| summary.min_mtime_ns);
        let indexed_max_mtime_ns = summary.as_ref().and_then(|summary| summary.max_mtime_ns);
        let returned_source_bytes_estimate =
            u64::try_from(returned_source_bytes_estimate).unwrap_or(u64::MAX);
        let returned_unique_file_bytes = summary
            .as_ref()
            .and_then(|summary| {
                summary.file_size_bytes_for_path(
                    workspace.as_ref().map(|workspace| workspace.root.as_path()),
                    path,
                )
            })
            .or(Some(returned_source_bytes_estimate));

        Ok(Self::finalize_context_efficiency_metadata(
            ContextEfficiencyMetadata {
                indexed_readable_files,
                indexed_readable_bytes,
                indexed_min_mtime_ns,
                indexed_max_mtime_ns,
                candidate_input_count: None,
                candidate_output_count: None,
                returned_match_count,
                returned_unique_paths: Some(1),
                returned_unique_file_bytes,
                returned_source_bytes_estimate: Some(returned_source_bytes_estimate),
                matched_file_context_saved_bytes_estimate: None,
                matched_file_context_saved_percent_estimate: None,
                corpus_context_saved_bytes_estimate: None,
                corpus_context_saved_percent_estimate: None,
                corpus_narrowing_ratio_estimate: None,
                query_duration_ms,
                narrowing_ratio_estimate: None,
                stage_attribution: None,
            },
        ))
    }

    fn structured_tool_result<T: Serialize>(value: &T) -> Result<CallToolResult, ErrorData> {
        serde_json::to_value(value)
            .map(CallToolResult::structured)
            .map_err(|err| {
                Self::internal(
                    format!("failed to serialize structured tool result: {err}"),
                    None,
                )
            })
    }

    fn reject_text_context_efficiency(
        include_context_efficiency: Option<bool>,
    ) -> Result<(), ErrorData> {
        if include_context_efficiency == Some(true) {
            return Err(Self::invalid_params(
                "include_context_efficiency requires presentation_mode=json",
                Some(json!({
                    "include_context_efficiency": true,
                    "presentation_mode": "text_or_citation",
                    "supported_presentation_mode": ReadPresentationMode::Json,
                })),
            ));
        }
        Ok(())
    }

    fn text_read_surface_result(content: String) -> CallToolResult {
        // Text mode is intentionally just the selected source bytes. Do not prepend path/line
        // headers or attach `structuredContent`: callers that need metadata must use JSON mode.
        CallToolResult::success(vec![ContentBlock::text(content)])
    }

    pub(super) fn map_lossy_line_slice_error(path: &Path, error: LossyLineSliceError) -> ErrorData {
        match error {
            LossyLineSliceError::Io(err) => Self::internal(
                format!("failed to read file {}: {err}", path.display()),
                None,
            ),
            LossyLineSliceError::LineStartOutside {
                line_start,
                line_end,
                total_lines,
            } => Self::invalid_params(
                "start_line is outside file bounds",
                Some(json!({
                    "start_line": line_start,
                    "end_line": line_end,
                    "total_lines": total_lines,
                })),
            ),
        }
    }

    fn line_slice_budget_error(
        path: &str,
        bytes: usize,
        max_bytes: usize,
        line_start: usize,
        line_end: usize,
        total_lines: usize,
    ) -> ErrorData {
        Self::invalid_params(
            format!("selected line range exceeds max_bytes={max_bytes}"),
            Some(json!({
                "path": path,
                "bytes": bytes,
                "max_bytes": max_bytes,
                "config_max_file_bytes": max_bytes,
                "start_line": line_start,
                "end_line": line_end,
                "total_lines": total_lines,
            })),
        )
    }
}
