use super::*;

impl FriggMcpServer {
    pub(crate) async fn search_symbol_impl(
        &self,
        params: SearchSymbolParams,
    ) -> Result<Json<SearchSymbolResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("search_symbol", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut scoped_repository_ids: Vec<String> = Vec::new();
                let mut diagnostics_count = 0usize;
                let mut manifest_walk_diagnostics_count = 0usize;
                let mut manifest_read_diagnostics_count = 0usize;
                let mut symbol_extraction_diagnostics_count = 0usize;
                let mut effective_limit: Option<usize> = None;
                let result = (|| -> Result<Json<SearchSymbolResponse>, ErrorData> {
                    let query = params_for_blocking.query.trim().to_owned();
                    if query.is_empty() {
                        return Err(Self::invalid_params("query must not be empty", None));
                    }

                    let path_regex = match params_for_blocking.path_regex.clone() {
                        Some(raw) => Some(compile_safe_regex(&raw).map_err(|err| {
                            Self::invalid_params(
                                format!("invalid path_regex: {err}"),
                                Some(serde_json::json!({
                                    "path_regex": raw,
                                    "regex_error_code": err.code(),
                                })),
                            )
                        })?),
                        None => None,
                    };
                    let path_class_filter = params_for_blocking.path_class;
                    let query_lower = query.to_ascii_lowercase();
                    let query_looks_canonical =
                        query.contains('\\') || query.contains("::") || query.contains('$');
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
                    effective_limit = Some(limit);
                    let scoped_execution_context = server.scoped_read_only_tool_execution_context(
                        execution_context_for_blocking.tool_name,
                        execution_context_for_blocking.repository_hint.clone(),
                        RepositoryResponseCacheFreshnessMode::ManifestOnly,
                    )?;
                    let scoped_workspaces = scoped_execution_context.scoped_workspaces;
                    let scoped_repository_ids_for_cache = scoped_workspaces
                        .iter()
                        .map(|workspace| workspace.repository_id.clone())
                        .collect::<Vec<_>>();
                    let cache_freshness = scoped_execution_context.cache_freshness;
                    let cache_key = cache_freshness.scopes.as_ref().map(|freshness_scopes| {
                        SearchSymbolResponseCacheKey {
                            scoped_repository_ids: scoped_repository_ids_for_cache,
                            freshness_scopes: freshness_scopes.clone(),
                            query: query.clone(),
                            path_class: path_class_filter.map(|value| value.as_str().to_owned()),
                            path_regex: params_for_blocking.path_regex.clone(),
                            limit,
                        }
                    });
                    if cache_key.is_none() {
                        server.record_runtime_cache_event(
                            RuntimeCacheFamily::SearchSymbolResponse,
                            RuntimeCacheEvent::Bypass,
                            1,
                        );
                    }
                    if let Some(cache_key) = cache_key.as_ref()
                        && let Some(cached) = server.cached_search_symbol_response(cache_key)
                    {
                        scoped_repository_ids = cached.scoped_repository_ids;
                        diagnostics_count = cached.diagnostics_count;
                        manifest_walk_diagnostics_count = cached.manifest_walk_diagnostics_count;
                        manifest_read_diagnostics_count = cached.manifest_read_diagnostics_count;
                        symbol_extraction_diagnostics_count =
                            cached.symbol_extraction_diagnostics_count;
                        effective_limit = Some(cached.effective_limit);
                        return Ok(Json(server.present_search_symbol_response(
                            cached.response,
                            params_for_blocking.response_mode,
                        )));
                    }

                    let corpora = server.collect_repository_symbol_corpora(
                        params_for_blocking.repository_id.as_deref(),
                    )?;
                    scoped_repository_ids = corpora
                        .iter()
                        .map(|corpus| corpus.repository_id.clone())
                        .collect::<Vec<_>>();
                    manifest_walk_diagnostics_count = corpora
                        .iter()
                        .map(|corpus| corpus.diagnostics.manifest_walk_count)
                        .sum::<usize>();
                    manifest_read_diagnostics_count = corpora
                        .iter()
                        .map(|corpus| corpus.diagnostics.manifest_read_count)
                        .sum::<usize>();
                    symbol_extraction_diagnostics_count = corpora
                        .iter()
                        .map(|corpus| corpus.diagnostics.symbol_extraction_count)
                        .sum::<usize>();
                    diagnostics_count = manifest_walk_diagnostics_count
                        + manifest_read_diagnostics_count
                        + symbol_extraction_diagnostics_count;

                    let mut ranked_matches: Vec<RankedSymbolMatch> = Vec::new();
                    for corpus in &corpora {
                        if query_looks_canonical {
                            if let Some(symbol_indices) =
                                corpus.symbol_indices_by_canonical_name.get(&query)
                            {
                                for symbol_index in symbol_indices {
                                    if let Some(candidate) = Self::build_ranked_symbol_match(
                                        corpus,
                                        *symbol_index,
                                        0,
                                        path_class_filter,
                                        path_regex.as_ref(),
                                    ) {
                                        ranked_matches.push(candidate);
                                    }
                                }
                            }
                            if let Some(symbol_indices) = corpus
                                .symbol_indices_by_lower_canonical_name
                                .get(&query_lower)
                            {
                                for symbol_index in symbol_indices {
                                    if corpus
                                        .canonical_symbol_name_by_stable_id
                                        .get(corpus.symbols[*symbol_index].stable_id.as_str())
                                        .is_some_and(|canonical| canonical != &query)
                                    {
                                        if let Some(candidate) = Self::build_ranked_symbol_match(
                                            corpus,
                                            *symbol_index,
                                            1,
                                            path_class_filter,
                                            path_regex.as_ref(),
                                        ) {
                                            ranked_matches.push(candidate);
                                        }
                                    }
                                }
                            }
                            let canonical_matches: std::collections::btree_map::Range<
                                '_,
                                String,
                                Vec<usize>,
                            > = corpus
                                .symbol_indices_by_lower_canonical_name
                                .range(query_lower.clone()..);
                            for (canonical_name, symbol_indices) in canonical_matches {
                                if !canonical_name.starts_with(&query_lower) {
                                    break;
                                }
                                if canonical_name == &query_lower {
                                    continue;
                                }
                                for symbol_index in symbol_indices {
                                    if let Some(candidate) = Self::build_ranked_symbol_match(
                                        corpus,
                                        *symbol_index,
                                        2,
                                        path_class_filter,
                                        path_regex.as_ref(),
                                    ) {
                                        ranked_matches.push(candidate);
                                    }
                                }
                            }
                        }

                        let name_rank_offset = if query_looks_canonical { 3 } else { 0 };
                        if let Some(symbol_indices) = corpus.symbol_indices_by_name.get(&query) {
                            for symbol_index in symbol_indices {
                                if let Some(candidate) = Self::build_ranked_symbol_match(
                                    corpus,
                                    *symbol_index,
                                    name_rank_offset,
                                    path_class_filter,
                                    path_regex.as_ref(),
                                ) {
                                    ranked_matches.push(candidate);
                                }
                            }
                        }
                        if let Some(symbol_indices) =
                            corpus.symbol_indices_by_lower_name.get(&query_lower)
                        {
                            for symbol_index in symbol_indices {
                                if corpus.symbols[*symbol_index].name != query {
                                    if let Some(candidate) = Self::build_ranked_symbol_match(
                                        corpus,
                                        *symbol_index,
                                        name_rank_offset + 1,
                                        path_class_filter,
                                        path_regex.as_ref(),
                                    ) {
                                        ranked_matches.push(candidate);
                                    }
                                }
                            }
                        }
                        let normalized_matches: std::collections::btree_map::Range<
                            '_,
                            String,
                            Vec<usize>,
                        > = corpus
                            .symbol_indices_by_lower_name
                            .range(query_lower.clone()..);
                        for (normalized_name, symbol_indices) in normalized_matches {
                            if !normalized_name.starts_with(&query_lower) {
                                break;
                            }
                            if normalized_name == &query_lower {
                                continue;
                            }
                            for symbol_index in symbol_indices {
                                if let Some(candidate) = Self::build_ranked_symbol_match(
                                    corpus,
                                    *symbol_index,
                                    name_rank_offset + 2,
                                    path_class_filter,
                                    path_regex.as_ref(),
                                ) {
                                    ranked_matches.push(candidate);
                                }
                            }
                        }
                    }
                    if ranked_matches.len() < limit {
                        let infix_limit = limit.saturating_sub(ranked_matches.len());
                        let mut infix_matches = Vec::new();
                        for corpus in &corpora {
                            for (symbol_index, symbol) in corpus.symbols.iter().enumerate() {
                                if Self::symbol_name_match_rank(&symbol.name, &query, &query_lower)
                                    != Some(3)
                                {
                                    continue;
                                }
                                if let Some(candidate) = Self::build_ranked_symbol_match(
                                    corpus,
                                    symbol_index,
                                    if query_looks_canonical { 6 } else { 3 },
                                    path_class_filter,
                                    path_regex.as_ref(),
                                ) {
                                    Self::retain_bounded_ranked_symbol_match(
                                        &mut infix_matches,
                                        infix_limit,
                                        candidate,
                                    );
                                }
                            }
                        }
                        ranked_matches.extend(infix_matches);
                    }

                    Self::sort_ranked_symbol_matches(&mut ranked_matches);
                    Self::dedup_ranked_symbol_matches(&mut ranked_matches);
                    let matches = ranked_matches
                        .into_iter()
                        .take(limit)
                        .map(|ranked| ranked.matched)
                        .collect::<Vec<_>>();

                    let metadata = json!({
                        "source": "tree_sitter",
                        "freshness_basis": cache_freshness.basis.clone(),
                        "diagnostics_count": diagnostics_count,
                        "diagnostics": {
                            "manifest_walk": manifest_walk_diagnostics_count,
                            "manifest_read": manifest_read_diagnostics_count,
                            "symbol_extraction": symbol_extraction_diagnostics_count,
                            "total": diagnostics_count,
                        },
                        "heuristic": false,
                        "path_class": path_class_filter.map(|value| value.as_str()),
                        "path_regex": params_for_blocking.path_regex.clone(),
                        "path_class_sort": "runtime_first",
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    let response = SearchSymbolResponse {
                        matches,
                        result_handle: None,
                        metadata,
                        note,
                    };
                    if let Some(cache_key) = cache_key {
                        server.cache_search_symbol_response(
                            cache_key,
                            &response,
                            &scoped_repository_ids,
                            diagnostics_count,
                            manifest_walk_diagnostics_count,
                            manifest_read_diagnostics_count,
                            symbol_extraction_diagnostics_count,
                            limit,
                        );
                    }
                    Ok(Json(server.present_search_symbol_response(
                        response,
                        params_for_blocking.response_mode,
                    )))
                })();

                SearchSymbolExecution {
                    result,
                    scoped_repository_ids,
                    diagnostics_count,
                    manifest_walk_diagnostics_count,
                    manifest_read_diagnostics_count,
                    symbol_extraction_diagnostics_count,
                    effective_limit,
                }
            })
            .await?;

        let result = execution.result;
        let metadata = execution_context.normalized_workload(
            &execution.scoped_repository_ids,
            WorkloadPrecisionMode::Precise,
        );
        let provenance_result = self
            .record_provenance_blocking_with_metadata(
                "search_symbol",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "query": Self::bounded_text(&params.query),
                    "path_class": params.path_class.map(|value| value.as_str().to_owned()),
                    "path_regex": params.path_regex.map(|value| Self::bounded_text(&value)),
                    "limit": params.limit,
                    "effective_limit": execution.effective_limit,
                }),
                json!({
                    "scoped_repository_ids": execution.scoped_repository_ids,
                    "diagnostics_count": execution.diagnostics_count,
                    "diagnostics": {
                        "manifest_walk": execution.manifest_walk_diagnostics_count,
                        "manifest_read": execution.manifest_read_diagnostics_count,
                        "symbol_extraction": execution.symbol_extraction_diagnostics_count,
                        "total": execution.diagnostics_count,
                    },
                }),
                Some(metadata),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }
}
