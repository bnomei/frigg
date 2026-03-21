use super::*;

impl FriggMcpServer {
    pub(crate) async fn search_text_impl(
        &self,
        params: SearchTextParams,
    ) -> Result<Json<SearchTextResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("search_text", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut scoped_repository_ids: Vec<String> = Vec::new();
                let mut effective_limit: Option<usize> = None;
                let mut effective_pattern_type: Option<SearchPatternType> = None;
                let mut diagnostics_count = 0usize;
                let mut walk_diagnostics_count = 0usize;
                let mut read_diagnostics_count = 0usize;
                let mut response_source_refs = json!({});
                let result = (|| -> Result<Json<SearchTextResponse>, ErrorData> {
                    let query = params_for_blocking.query.trim().to_owned();
                    if query.is_empty() {
                        return Err(Self::invalid_params("query must not be empty", None));
                    }
                    if params_for_blocking.max_matches_per_file == Some(0) {
                        return Err(Self::invalid_params(
                            "max_matches_per_file must be greater than zero when provided",
                            None,
                        ));
                    }

                    let path_regex = match params_for_blocking.path_regex.clone() {
                        Some(raw) => {
                            Some(server.compile_cached_safe_regex(&raw).map_err(|err| {
                                Self::invalid_params(
                                    format!("invalid path_regex: {err}"),
                                    Some(serde_json::json!({
                                        "path_regex": raw,
                                        "regex_error_code": err.code(),
                                    })),
                                )
                            })?)
                        }
                        None => None,
                    };

                    let pattern_type = params_for_blocking
                        .pattern_type
                        .clone()
                        .unwrap_or(SearchPatternType::Literal);
                    effective_pattern_type = Some(pattern_type.clone());

                    let requested_limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
                    let limit = if params_for_blocking.context_lines.unwrap_or(0) > 0
                        || params_for_blocking.max_matches_per_file.is_some()
                        || params_for_blocking.collapse_by_file == Some(true)
                    {
                        server.config.max_search_results.max(requested_limit)
                    } else {
                        requested_limit
                    };
                    effective_limit = Some(limit);

                    let scoped_execution_context = server.scoped_read_only_tool_execution_context(
                        execution_context_for_blocking.tool_name,
                        execution_context_for_blocking.repository_hint.clone(),
                        RepositoryResponseCacheFreshnessMode::ManifestOnly,
                    )?;
                    let scoped_workspaces = scoped_execution_context.scoped_workspaces.clone();
                    scoped_repository_ids = scoped_execution_context.scoped_repository_ids.clone();
                    let cache_freshness = scoped_execution_context.cache_freshness.clone();
                    let cache_key = cache_freshness.scopes.as_ref().map(|freshness_scopes| {
                        SearchTextResponseCacheKey {
                            scoped_repository_ids: scoped_repository_ids.clone(),
                            freshness_scopes: freshness_scopes.clone(),
                            query: query.clone(),
                            pattern_type: Self::search_pattern_type_cache_key(&pattern_type),
                            path_regex: params_for_blocking.path_regex.clone(),
                            limit,
                        }
                    });
                    if cache_key.is_none() {
                        server.record_runtime_cache_event(
                            RuntimeCacheFamily::SearchTextResponse,
                            RuntimeCacheEvent::Bypass,
                            1,
                        );
                    }
                    if let Some(cache_key) = cache_key.as_ref()
                        && let Some(cached) = server.cached_search_text_response(cache_key)
                    {
                        response_source_refs = cached.source_refs.clone();
                        diagnostics_count = cached
                            .source_refs
                            .get("diagnostics_count")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize;
                        walk_diagnostics_count = cached
                            .source_refs
                            .get("diagnostics")
                            .and_then(|value| value.get("walk"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize;
                        read_diagnostics_count = cached
                            .source_refs
                            .get("diagnostics")
                            .and_then(|value| value.get("read"))
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as usize;
                        return Ok(Json(server.present_search_text_response(
                            cached.response,
                            &params_for_blocking,
                        )?));
                    }
                    let (scoped_config, repository_id_map) =
                        server.scoped_search_config(&scoped_workspaces);

                    let searcher = server.runtime_text_searcher(scoped_config);
                    let search_output = match pattern_type {
                        SearchPatternType::Literal => searcher
                            .search_literal_with_filters_diagnostics(
                                SearchTextQuery {
                                    query,
                                    path_regex,
                                    limit,
                                },
                                SearchFilters::default(),
                            ),
                        SearchPatternType::Regex => searcher.search_regex_with_filters_diagnostics(
                            SearchTextQuery {
                                query,
                                path_regex,
                                limit,
                            },
                            SearchFilters::default(),
                        ),
                    }
                    .map_err(Self::map_frigg_error)?;
                    diagnostics_count = search_output.diagnostics.total_count();
                    walk_diagnostics_count = search_output
                        .diagnostics
                        .count_by_kind(SearchDiagnosticKind::Walk);
                    read_diagnostics_count = search_output
                        .diagnostics
                        .count_by_kind(SearchDiagnosticKind::Read);
                    let mut matches = search_output.matches;
                    let total_matches = search_output.total_matches;
                    let metadata = Self::search_text_metadata(
                        search_output.lexical_backend,
                        search_output.lexical_backend_note.clone(),
                    );
                    for found in &mut matches {
                        if let Some(actual_repository_id) =
                            repository_id_map.get(&found.repository_id)
                        {
                            found.repository_id = actual_repository_id.clone();
                        }
                    }
                    let response = SearchTextResponse {
                        total_matches,
                        matches,
                        result_handle: None,
                        metadata,
                    };
                    response_source_refs = json!({
                        "scoped_repository_ids": scoped_repository_ids.clone(),
                        "freshness_basis": cache_freshness.basis.clone(),
                        "total_matches": response.total_matches,
                        "lexical_backend": response
                            .metadata
                            .as_ref()
                            .map(|metadata| metadata.lexical_backend.clone()),
                        "lexical_backend_note": response
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.lexical_backend_note.clone()),
                        "diagnostics_count": diagnostics_count,
                        "diagnostics": {
                            "walk": walk_diagnostics_count,
                            "read": read_diagnostics_count,
                            "total": diagnostics_count,
                        },
                    });
                    if let Some(cache_key) = cache_key {
                        server.cache_search_text_response(
                            cache_key,
                            &response,
                            &response_source_refs,
                        );
                    }

                    Ok(Json(server.present_search_text_response(
                        response,
                        &params_for_blocking,
                    )?))
                })();

                let total_matches = result
                    .as_ref()
                    .map(|response| response.0.total_matches)
                    .unwrap_or(0);
                let normalized_workload = execution_context_for_blocking
                    .normalized_workload(&scoped_repository_ids, WorkloadPrecisionMode::Exact);
                let finalization = server.tool_execution_finalization(
                    response_source_refs.clone(),
                    Some(normalized_workload),
                );
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    "search_text",
                    execution_context_for_blocking.repository_hint.as_deref(),
                    json!({
                        "repository_id": execution_context_for_blocking.repository_hint,
                        "query": Self::bounded_text(&params_for_blocking.query),
                        "pattern_type": effective_pattern_type.clone(),
                        "path_regex": params_for_blocking
                            .path_regex
                            .as_ref()
                            .map(|raw| Self::bounded_text(raw)),
                        "limit": params_for_blocking.limit,
                        "effective_limit": effective_limit,
                    }),
                    finalization.source_refs,
                    Self::provenance_outcome(&result),
                    finalization.normalized_workload,
                );

                SearchTextExecution {
                    result,
                    provenance_result,
                    scoped_repository_ids,
                    total_matches,
                    effective_limit,
                    effective_pattern_type,
                    diagnostics_count,
                    walk_diagnostics_count,
                    read_diagnostics_count,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }
}
