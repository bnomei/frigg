//! `search_text` implementation with repository scoping and freshness metadata.
//!
//! Lexical search over manifest-scoped files with repository freshness metadata for
//! response-cache eligibility.

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
                    let query_started_at = std::time::Instant::now();
                    let context_efficiency_log_enabled =
                        crate::context_efficiency::context_efficiency_log_enabled();
                    let need_context_efficiency =
                        crate::context_efficiency::need_context_efficiency_with_log_state(
                            params_for_blocking.include_context_efficiency,
                            context_efficiency_log_enabled,
                        ) || server.tool_call_display_enabled();
                    let query = params_for_blocking.query.trim().to_owned();
                    if query.is_empty() {
                        return Err(Self::invalid_params("query must not be empty", None));
                    }
                    if params_for_blocking.max_count_per_file == Some(0) {
                        return Err(Self::invalid_params(
                            "max_count_per_file must be greater than zero when provided",
                            None,
                        ));
                    }
                    if params_for_blocking.files_with_matches == Some(true)
                        && params_for_blocking.count_only == Some(true)
                    {
                        return Err(Self::invalid_params(
                            "files_with_matches and count_only are mutually exclusive",
                            None,
                        ));
                    }

                    let (query, pattern_type) =
                        Self::normalize_search_text_rg_pattern(&params_for_blocking, query)?;

                    let explicit_path_regex =
                        Self::compile_optional_path_regex(&server, &params_for_blocking)?;
                    let glob_regex = Self::compile_optional_path_glob(
                        &server,
                        "glob",
                        &params_for_blocking.glob,
                    )?;
                    let exclude_glob_regex = Self::compile_optional_path_glob(
                        &server,
                        "exclude_glob",
                        &params_for_blocking.exclude_glob,
                    )?;
                    let include_hidden = params_for_blocking.include_hidden.unwrap_or(false);
                    let search_path_regex =
                        explicit_path_regex.clone().or_else(|| glob_regex.clone());
                    let needs_post_path_filter = (explicit_path_regex.is_some()
                        && glob_regex.is_some())
                        || exclude_glob_regex.is_some()
                        || !include_hidden;
                    effective_pattern_type = Some(pattern_type.clone());

                    let requested_limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
                    let limit = if params_for_blocking.context_lines.unwrap_or(0) > 0
                        || params_for_blocking.max_count_per_file.is_some()
                        || params_for_blocking.collapse_by_file == Some(true)
                        || params_for_blocking.files_with_matches == Some(true)
                        || params_for_blocking.count_only == Some(true)
                        || needs_post_path_filter
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
                    let (scoped_config, scoped_runtime_repository_ids, repository_id_map) =
                        server.scoped_search_config(&scoped_workspaces);

                    let searcher = server.runtime_text_searcher_with_repository_ids(
                        scoped_config,
                        scoped_runtime_repository_ids,
                    );
                    let search_output = match pattern_type {
                        SearchPatternType::Literal => searcher
                            .search_literal_with_filters_diagnostics(
                                SearchTextQuery {
                                    query,
                                    path_regex: search_path_regex.clone(),
                                    limit,
                                },
                                SearchFilters::default(),
                            ),
                        SearchPatternType::Regex => searcher.search_regex_with_filters_diagnostics(
                            SearchTextQuery {
                                query,
                                path_regex: search_path_regex,
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
                    if needs_post_path_filter {
                        matches.retain(|matched| {
                            Self::search_text_path_filter_allows(
                                &matched.path,
                                glob_regex.as_ref(),
                                exclude_glob_regex.as_ref(),
                                include_hidden,
                            )
                        });
                    }
                    let total_matches = if needs_post_path_filter {
                        matches.len()
                    } else {
                        search_output.total_matches
                    };
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
                    let compact_metadata_seed = response.metadata.clone();
                    response_source_refs = json!({
                        "scoped_repository_ids": scoped_repository_ids.clone(),
                        "freshness_basis": cache_freshness.basis.clone(),
                        "total_matches": response.total_matches,
                        "lexical_backend": response
                            .metadata
                            .as_ref()
                            .and_then(|metadata| metadata.lexical_backend.clone()),
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
                    let mut presented =
                        server.present_search_text_response(response, &params_for_blocking)?;
                    if need_context_efficiency {
                        let context_efficiency = server
                            .context_efficiency_metadata_for_tool_observers(
                                &execution_context_for_blocking,
                                params_for_blocking.include_context_efficiency,
                                context_efficiency_log_enabled,
                                || {
                                    Self::search_text_context_efficiency_metadata(
                                        &scoped_workspaces,
                                        &presented.matches,
                                        presented.total_matches,
                                    )
                                },
                            )?
                            .map(|mut metadata| {
                                metadata.query_duration_ms =
                                    Some(Self::context_efficiency_elapsed_ms(query_started_at));
                                metadata
                            });
                        if let Some(context_efficiency) = context_efficiency.as_ref() {
                            server.append_context_efficiency_log_for_workspaces(
                                "search_text",
                                &scoped_workspaces,
                                context_efficiency,
                            );
                        }
                        if params_for_blocking.include_context_efficiency == Some(true) {
                            presented
                                .metadata
                                .get_or_insert_with(|| {
                                    compact_metadata_seed.clone().unwrap_or(SearchTextMetadata {
                                        lexical_backend: None,
                                        lexical_backend_note: None,
                                        context_efficiency: None,
                                    })
                                })
                                .context_efficiency = context_efficiency;
                        }
                    }
                    if let Some(metadata) = &presented.metadata {
                        let mut metadata_value = serde_json::to_value(metadata)
                            .expect("search_text metadata should serialize");
                        metadata_value
                            .as_object_mut()
                            .expect("search_text metadata should be an object")
                            .remove("context_efficiency");
                        response_source_refs
                            .as_object_mut()
                            .expect("search_text source refs should be an object")
                            .insert("metadata".to_owned(), metadata_value);
                    }

                    Ok(Json(presented))
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
                        "case_sensitive": params_for_blocking.case_sensitive,
                        "ignore_case": params_for_blocking.ignore_case,
                        "word": params_for_blocking.word,
                        "files_with_matches": params_for_blocking.files_with_matches,
                        "count_only": params_for_blocking.count_only,
                        "glob": params_for_blocking
                            .glob
                            .as_ref()
                            .map(|raw| Self::bounded_text(raw)),
                        "exclude_glob": params_for_blocking
                            .exclude_glob
                            .as_ref()
                            .map(|raw| Self::bounded_text(raw)),
                        "include_hidden": params_for_blocking.include_hidden,
                        "max_count_per_file": params_for_blocking.max_count_per_file,
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

    fn normalize_search_text_rg_pattern(
        params: &SearchTextParams,
        query: String,
    ) -> Result<(String, SearchPatternType), ErrorData> {
        match (params.case_sensitive, params.ignore_case) {
            (Some(true), Some(true)) => {
                return Err(Self::invalid_params(
                    "case_sensitive=true conflicts with ignore_case=true",
                    None,
                ));
            }
            (Some(false), Some(false)) => {
                return Err(Self::invalid_params(
                    "case_sensitive=false conflicts with ignore_case=false",
                    None,
                ));
            }
            _ => {}
        }

        let pattern_type = params
            .pattern_type
            .clone()
            .unwrap_or(SearchPatternType::Literal);
        let ignore_case = params.ignore_case == Some(true) || params.case_sensitive == Some(false);
        let word = params.word == Some(true);
        if !ignore_case && !word {
            return Ok((query, pattern_type));
        }

        let mut pattern = match pattern_type {
            SearchPatternType::Literal => regex::escape(&query),
            SearchPatternType::Regex => query,
        };
        if word {
            pattern = format!(r"\b(?:{pattern})\b");
        }
        if ignore_case {
            pattern = format!("(?i:{pattern})");
        }
        Ok((pattern, SearchPatternType::Regex))
    }

    fn compile_optional_path_regex(
        server: &FriggMcpServer,
        params: &SearchTextParams,
    ) -> Result<Option<regex::Regex>, ErrorData> {
        match params.path_regex.clone() {
            Some(raw) => Ok(Some(server.compile_cached_safe_regex(&raw).map_err(
                |err| {
                    Self::invalid_params(
                        format!("invalid path_regex: {err}"),
                        Some(serde_json::json!({
                            "path_regex": raw,
                            "regex_error_code": err.code(),
                        })),
                    )
                },
            )?)),
            None => Ok(None),
        }
    }

    pub(super) fn compile_optional_path_glob(
        server: &FriggMcpServer,
        field: &'static str,
        glob: &Option<String>,
    ) -> Result<Option<regex::Regex>, ErrorData> {
        let Some(raw) = glob else {
            return Ok(None);
        };
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Err(Self::invalid_params(
                format!("{field} must not be empty when provided"),
                None,
            ));
        }
        let regex_source = Self::repository_glob_to_regex(trimmed);
        server
            .compile_cached_safe_regex(&regex_source)
            .map(Some)
            .map_err(|err| {
                Self::invalid_params(
                    format!("invalid {field}: {err}"),
                    Some(serde_json::json!({
                        field: raw,
                        "regex_error_code": err.code(),
                    })),
                )
            })
    }

    fn search_text_path_filter_allows(
        path: &str,
        glob_regex: Option<&regex::Regex>,
        exclude_glob_regex: Option<&regex::Regex>,
        include_hidden: bool,
    ) -> bool {
        if !include_hidden && Self::repository_path_is_hidden(path) {
            return false;
        }
        if glob_regex.is_some_and(|regex| !regex.is_match(path)) {
            return false;
        }
        if exclude_glob_regex.is_some_and(|regex| regex.is_match(path)) {
            return false;
        }
        true
    }

    pub(super) fn repository_path_is_hidden(path: &str) -> bool {
        path.split('/')
            .filter(|component| !component.is_empty())
            .any(|component| component.starts_with('.'))
    }

    pub(super) fn repository_glob_to_regex(glob: &str) -> String {
        let mut regex = String::new();
        if glob.contains('/') {
            regex.push('^');
        } else {
            regex.push_str(r"(?:^|.*/)");
        }

        let mut chars = glob.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '*' => {
                    if chars.peek() == Some(&'*') {
                        let _ = chars.next();
                        regex.push_str(".*");
                    } else {
                        regex.push_str("[^/]*");
                    }
                }
                '?' => regex.push_str("[^/]"),
                '/' => regex.push('/'),
                _ => regex.push_str(&regex::escape(&ch.to_string())),
            }
        }
        regex.push('$');
        regex
    }
}
