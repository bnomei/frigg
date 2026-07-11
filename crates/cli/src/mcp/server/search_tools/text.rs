//! `search_text` implementation with repository scoping and freshness metadata.
//!
//! Lexical search over manifest-scoped files with repository freshness metadata for
//! response-cache eligibility.

use super::*;
use crate::mcp::server_cache::ContinuationBinding;
use crate::mcp::types::{
    ResultCompleteness, ResultIncompleteReason, ResultTruncationReason, ResultUnit,
};
use crate::searcher::{SearchTextExecutionOptions, SearchTextRowMode};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

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
                    if pattern_type == SearchPatternType::Regex {
                        Self::reject_empty_matching_search_query_regex(&server, &query)?;
                    }

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
                    effective_pattern_type = Some(pattern_type.clone());

                    let requested_limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
                    effective_limit = Some(requested_limit);

                    let scoped_execution_context = server.scoped_read_only_tool_execution_context(
                        execution_context_for_blocking.tool_name,
                        execution_context_for_blocking.repository_hint.clone(),
                        RepositoryResponseCacheFreshnessMode::ManifestOnly,
                    )?;
                    let scoped_workspaces = scoped_execution_context.scoped_workspaces.clone();
                    scoped_repository_ids = scoped_execution_context.scoped_repository_ids.clone();
                    let cache_freshness = scoped_execution_context.cache_freshness.clone();
                    let snapshot_fingerprints = cache_freshness
                        .scopes
                        .as_ref()
                        .map(|scopes| {
                            scopes
                                .iter()
                                .map(|scope| {
                                    format!("{}:{}", scope.repository_id, scope.snapshot_id)
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    let row_mode = if params_for_blocking.files_with_matches == Some(true)
                        || params_for_blocking.collapse_by_file == Some(true)
                    {
                        SearchTextRowMode::UniqueFile
                    } else if let Some(max_count_per_file) = params_for_blocking.max_count_per_file
                    {
                        SearchTextRowMode::PerFileCapped { max_count_per_file }
                    } else {
                        SearchTextRowMode::Occurrence
                    };
                    let unit = match row_mode {
                        SearchTextRowMode::UniqueFile => ResultUnit::File,
                        SearchTextRowMode::Occurrence | SearchTextRowMode::PerFileCapped { .. } => {
                            ResultUnit::Occurrence
                        }
                    };
                    let request_digest = Self::search_text_continuation_digest(
                        &query,
                        &pattern_type,
                        &params_for_blocking,
                        &scoped_repository_ids,
                        unit,
                    );
                    let resume_offset = match params_for_blocking.continuation.as_deref() {
                        Some(token) => server
                            .session_continuation_lookup(
                                token,
                                "search_text",
                                &request_digest,
                                &scoped_repository_ids,
                                &snapshot_fingerprints,
                                unit,
                            )
                            .map(|binding| binding.next_position)
                            .map_err(|error| {
                                Self::invalid_params(
                                    error.message.clone(),
                                    Some(json!({
                                        "continuation": error,
                                    })),
                                )
                            })?,
                        None => 0,
                    };
                    let (scoped_config, scoped_runtime_repository_ids, repository_id_map) =
                        server.scoped_search_config(&scoped_workspaces);

                    let searcher = server.runtime_text_searcher_with_repository_ids(
                        scoped_config,
                        scoped_runtime_repository_ids,
                    );
                    let execution_options = SearchTextExecutionOptions {
                        include_glob: glob_regex,
                        exclude_glob: exclude_glob_regex,
                        row_mode,
                    };
                    let search_output = match pattern_type {
                        SearchPatternType::Literal => searcher
                            .search_literal_with_execution_options_diagnostics(
                                SearchTextQuery {
                                    query,
                                    path_regex: explicit_path_regex.clone(),
                                    limit: usize::MAX,
                                },
                                SearchFilters {
                                    include_hidden,
                                    ..SearchFilters::default()
                                },
                                execution_options.clone(),
                            ),
                        SearchPatternType::Regex => searcher
                            .search_regex_with_execution_options_diagnostics(
                                SearchTextQuery {
                                    query,
                                    path_regex: explicit_path_regex,
                                    limit: usize::MAX,
                                },
                                SearchFilters {
                                    include_hidden,
                                    ..SearchFilters::default()
                                },
                                execution_options,
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
                    let total_matches = search_output.total_matches;
                    let metadata = Self::search_text_metadata(
                        search_output.lexical_backend,
                        search_output.lexical_backend_note.clone(),
                    );
                    let mut matches = search_output.matches;
                    for found in &mut matches {
                        if let Some(actual_repository_id) =
                            repository_id_map.get(&found.repository_id)
                        {
                            found.repository_id = actual_repository_id.clone();
                        }
                    }
                    let total_rows = search_output.total_rows;
                    let page = matches
                        .into_iter()
                        .skip(resume_offset)
                        .take(requested_limit)
                        .collect::<Vec<_>>();
                    let rows_omitted = total_rows
                        .is_some_and(|total| resume_offset.saturating_add(page.len()) < total);
                    let coverage_is_exact =
                        search_output.coverage == crate::searcher::SearchExecutionCoverage::Exact;
                    let mut incomplete_reasons = Vec::new();
                    if !coverage_is_exact {
                        incomplete_reasons.push(ResultIncompleteReason::DiagnosticCoverage);
                        if read_diagnostics_count > 0 {
                            incomplete_reasons.push(ResultIncompleteReason::UnreadableCandidate);
                        }
                        if walk_diagnostics_count > 0 {
                            incomplete_reasons.push(ResultIncompleteReason::WalkFailure);
                        }
                    }
                    let continuation = (rows_omitted
                        && coverage_is_exact
                        && !snapshot_fingerprints.is_empty()
                        && requested_limit > 0)
                        .then(|| {
                            server.store_session_continuation(ContinuationBinding {
                                tool: "search_text",
                                request_digest: request_digest.clone(),
                                repository_ids: scoped_repository_ids.clone(),
                                snapshot_fingerprints: snapshot_fingerprints.clone(),
                                unit,
                                next_position: resume_offset.saturating_add(page.len()),
                            })
                        });
                    let completeness = if params_for_blocking.count_only == Some(true) {
                        match total_matches {
                            Some(total) => ResultCompleteness::complete_count_only(
                                ResultUnit::Occurrence,
                                total,
                            ),
                            None => ResultCompleteness::try_new(
                                ResultUnit::Occurrence,
                                0,
                                None,
                                false,
                                false,
                                vec![],
                                incomplete_reasons.clone(),
                                None,
                            ),
                        }
                    } else {
                        ResultCompleteness::try_new(
                            unit,
                            page.len(),
                            total_rows,
                            coverage_is_exact && !rows_omitted,
                            rows_omitted,
                            rows_omitted
                                .then_some(ResultTruncationReason::PageLimit)
                                .into_iter()
                                .collect(),
                            incomplete_reasons,
                            continuation,
                        )
                    }
                    .map_err(|error| {
                        Self::invalid_params(
                            format!("invalid search completeness state: {error}"),
                            None,
                        )
                    })?;
                    let mut response = SearchTextResponse {
                        total_matches: total_matches.unwrap_or(0),
                        matches: page,
                        completeness,
                        result_handle: None,
                        handle_scope: None,
                        handle_expires: None,
                        count_only: params_for_blocking
                            .count_only
                            .filter(|value| *value)
                            .or(None),
                        latency_class: None,
                        metadata,
                        recovery: RecoveryFields::default(),
                    };
                    if params_for_blocking.count_only == Some(true) {
                        response.count_only = Some(true);
                    }
                    if response.total_matches == 0 {
                        let pattern_type_is_literal = !matches!(
                            params_for_blocking.pattern_type,
                            Some(SearchPatternType::Regex)
                        );
                        let mut scope = ZeroHitScope::default();
                        if let Some(path_regex) = params_for_blocking.path_regex.as_ref() {
                            scope = scope.with_path_regex(path_regex.clone());
                        }
                        if let Some(glob) = params_for_blocking.glob.as_ref() {
                            scope = scope.with_glob(glob.clone());
                        }
                        if let Some(repository_id) = params_for_blocking.repository_id.as_ref() {
                            scope = scope.with_repository_id(repository_id.clone());
                        }
                        response.recovery = RecoveryFields::for_zero_hit(ZeroHitInput {
                            tool: "search_text",
                            query: Some(params_for_blocking.query.as_str()),
                            pattern_type_is_literal: Some(pattern_type_is_literal),
                            scope: Some(scope).filter(|scope| !scope.is_empty()),
                            index: server.zero_hit_index_for_repositories(&scoped_repository_ids),
                            reason_override: None,
                        });
                        if let Some(glob) = params_for_blocking.glob.as_deref() {
                            response.recovery = response.recovery.with_non_recursive_glob_hint(
                                params_for_blocking.query.as_str(),
                                glob,
                            );
                        }
                    } else {
                        let mut scope = ZeroHitScope::default();
                        if let Some(path_regex) = params_for_blocking.path_regex.as_ref() {
                            scope = scope.with_path_regex(path_regex.clone());
                        }
                        if let Some(glob) = params_for_blocking.glob.as_ref() {
                            scope = scope.with_glob(glob.clone());
                        }
                        if let Some(repository_id) = params_for_blocking.repository_id.as_ref() {
                            scope = scope.with_repository_id(repository_id.clone());
                        }
                        if !scope.is_empty() {
                            response.recovery.scope = Some(scope);
                        }
                    }
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

    fn reject_empty_matching_search_query_regex(
        server: &FriggMcpServer,
        query: &str,
    ) -> Result<(), ErrorData> {
        let regex = server.compile_cached_safe_regex(query).map_err(|err| {
            Self::invalid_params(
                format!("invalid query regex: {err}"),
                Some(serde_json::json!({
                    "query": query,
                    "regex_error_code": err.code(),
                })),
            )
        })?;
        if regex.is_match("") {
            return Err(Self::invalid_params(
                "query regex must not match empty strings",
                Some(serde_json::json!({ "query": query })),
            ));
        }
        Ok(())
    }

    fn search_text_continuation_digest(
        query: &str,
        pattern_type: &SearchPatternType,
        params: &SearchTextParams,
        repository_ids: &[String],
        unit: ResultUnit,
    ) -> String {
        let normalized = json!({
            "query": query,
            "pattern_type": pattern_type,
            "repository_id": params.repository_id,
            "repository_ids": repository_ids,
            "path_regex": params.path_regex,
            "limit": params.limit,
            "context_lines": params.context_lines,
            "case_sensitive": params.case_sensitive,
            "ignore_case": params.ignore_case,
            "word": params.word,
            "files_with_matches": params.files_with_matches,
            "count_only": params.count_only,
            "glob": params.glob,
            "exclude_glob": params.exclude_glob,
            "include_hidden": params.include_hidden,
            "max_count_per_file": params.max_count_per_file,
            "collapse_by_file": params.collapse_by_file,
            "unit": unit,
        });
        let mut hasher = DefaultHasher::new();
        normalized.to_string().hash(&mut hasher);
        format!("search-text-v2:{:016x}", hasher.finish())
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
