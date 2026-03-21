use super::*;

impl FriggMcpServer {
    pub(super) async fn read_file_impl(
        &self,
        params: ReadFileParams,
    ) -> Result<Json<ReadFileResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("read_file", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut resolved_repository_id: Option<String> = None;
                let mut resolved_path: Option<String> = None;
                let mut resolved_absolute_path: Option<String> = None;
                let mut effective_max_bytes: Option<usize> = None;
                let mut effective_line_start: Option<usize> = None;
                let mut effective_line_end: Option<usize> = None;
                let result = (|| -> Result<Json<ReadFileResponse>, ErrorData> {
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
                    let has_line_range = params_for_blocking.line_start.is_some()
                        || params_for_blocking.line_end.is_some();
                    if params_for_blocking.line_start == Some(0) {
                        return Err(Self::invalid_params(
                            "line_start must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.line_end == Some(0) {
                        return Err(Self::invalid_params(
                            "line_end must be greater than zero when provided",
                            None,
                        ));
                    }
                    if let (Some(line_start), Some(line_end)) =
                        (params_for_blocking.line_start, params_for_blocking.line_end)
                        && line_end < line_start
                    {
                        return Err(Self::invalid_params(
                            "line_end must be greater than or equal to line_start",
                            Some(json!({
                                "line_start": line_start,
                                "line_end": line_end,
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
                    let pre_read_bytes = pre_read_bytes.unwrap_or_else(|| snapshot.raw_bytes_len());
                    if !has_line_range {
                        let content = snapshot.read_file_content();
                        return Ok(Json(ReadFileResponse {
                            repository_id,
                            path: display_path,
                            bytes: pre_read_bytes,
                            content,
                        }));
                    }

                    let line_start = params_for_blocking.line_start.unwrap_or(1);
                    let requested_line_end = params_for_blocking.line_end;
                    let effective_end_hint = requested_line_end;
                    effective_line_start = Some(line_start);
                    effective_line_end = Some(effective_end_hint.unwrap_or(1));

                    let line_slice = snapshot
                        .read_line_slice_lossy(line_start, requested_line_end, max_bytes)
                        .map_err(|err| Self::map_lossy_line_slice_error(&path, err))?;
                    let sliced_content = line_slice.content;
                    let sliced_bytes = line_slice.bytes;
                    let total_lines = line_slice.total_lines;
                    let effective_end = requested_line_end
                        .unwrap_or(total_lines.max(1))
                        .min(total_lines.max(1));
                    effective_line_end = Some(effective_end);

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
                                "line_start": line_start,
                                "line_end": effective_end,
                                "total_lines": total_lines,
                            })),
                        ));
                    }

                    Ok(Json(ReadFileResponse {
                        repository_id,
                        path: display_path,
                        bytes: sliced_bytes,
                        content: sliced_content,
                    }))
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
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    "read_file",
                    execution_context_for_blocking.repository_hint.as_deref(),
                    json!({
                        "repository_id": execution_context_for_blocking.repository_hint,
                        "path": Self::bounded_text(&params_for_blocking.path),
                        "max_bytes": params_for_blocking.max_bytes,
                        "line_start": params_for_blocking.line_start,
                        "line_end": params_for_blocking.line_end,
                        "effective_max_bytes": effective_max_bytes,
                        "effective_line_start": effective_line_start,
                        "effective_line_end": effective_line_end,
                    }),
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
    ) -> Result<Json<ReadMatchResponse>, ErrorData> {
        let anchor = self
            .session_result_handle_match(&params.result_handle, &params.match_id)
            .ok_or_else(|| {
                Self::resource_not_found(
                    "result_handle or match_id not found",
                    Some(json!({
                        "result_handle": params.result_handle,
                        "match_id": params.match_id,
                    })),
                )
            })?;
        let before = params.before.unwrap_or(10).min(MAX_CONTEXT_LINES);
        let after = params.after.unwrap_or(10).min(MAX_CONTEXT_LINES);
        let line_start = anchor.line.saturating_sub(before).max(1);
        let line_end = anchor.line.saturating_add(after);
        let read_params = ReadFileParams {
            path: anchor.path.clone(),
            repository_id: Some(anchor.repository_id.clone()),
            max_bytes: None,
            line_start: Some(line_start),
            line_end: Some(line_end),
        };
        let read = self.read_file_impl(read_params).await?.0;
        Ok(Json(ReadMatchResponse {
            repository_id: read.repository_id,
            path: read.path,
            line: anchor.line,
            column: anchor.column,
            line_start,
            line_end,
            bytes: read.bytes,
            content: read.content,
        }))
    }

    pub(super) async fn explore_impl(
        &self,
        params: ExploreParams,
    ) -> Result<Json<ExploreResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("explore", params.repository_id.clone());
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

                let result = (|| -> Result<Json<ExploreResponse>, ErrorData> {
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
                        line_start: None,
                        line_end: None,
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
                        && (scan.total_lines == 0 || cursor.line > scan.total_lines)
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

                    Ok(Json(ExploreResponse {
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
                        },
                    }))
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
                "line_start is outside file bounds",
                Some(json!({
                    "line_start": line_start,
                    "line_end": line_end,
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
                "line_start": line_start,
                "line_end": line_end,
                "total_lines": total_lines,
            })),
        )
    }
}
