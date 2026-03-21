use super::*;

fn error_code_tag(error: &ErrorData) -> Option<&str> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
}

fn precise_absence_reason(
    coverage_mode: PreciseCoverageMode,
    stats: &PreciseIngestStats,
    precise_match_count: usize,
) -> &'static str {
    if stats.artifacts_discovered == 0 {
        return "no_scip_artifacts_discovered";
    }

    match coverage_mode {
        PreciseCoverageMode::Partial if precise_match_count == 0 => {
            return "precise_partial_non_authoritative_absence";
        }
        PreciseCoverageMode::None if stats.artifacts_failed > 0 => {
            return "scip_artifact_ingest_failed";
        }
        PreciseCoverageMode::Full | PreciseCoverageMode::Partial
            if stats.artifacts_ingested > 0 && precise_match_count == 0 =>
        {
            return "required_precise_matches_not_present_in_precise_graph";
        }
        PreciseCoverageMode::None => {
            return "no_usable_precise_data";
        }
        _ => {}
    }

    "precise_unavailable"
}

fn route_definition_name_regex(route_name: &str) -> Option<regex::Regex> {
    if route_name.is_empty()
        || route_name.contains('\n')
        || route_name.contains('\r')
        || route_name.contains('*')
    {
        return None;
    }

    let escaped = regex::escape(route_name);
    regex::Regex::new(&format!(
        r#"->\s*name\s*\(\s*(?:"(?P<double>{escaped})"|'(?P<single>{escaped})')\s*\)"#
    ))
    .ok()
}

impl FriggMcpServer {
    fn selected_target_navigation_location(
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
        target_symbol: &SymbolDefinition,
        display_symbol: String,
        path: String,
        line: usize,
        column: usize,
        kind: Option<String>,
        precision: Option<String>,
    ) -> NavigationLocation {
        let (container, signature) =
            Self::symbol_context_for_stable_id(target_corpus, &target_symbol.stable_id);
        NavigationLocation {
            match_id: None,
            stable_symbol_id: Some(target_symbol.stable_id.clone()),
            symbol: display_symbol,
            repository_id: target_corpus.repository_id.clone(),
            path: if path.is_empty() {
                Self::relative_display_path(target_root, &target_symbol.path)
            } else {
                path
            },
            line,
            column,
            kind,
            container,
            signature,
            precision,
            follow_up_structural: Vec::new(),
        }
    }

    fn route_helper_definition_matches(
        corpora: &[Arc<RepositorySymbolCorpus>],
        route_name: &str,
        include_follow_up_structural: bool,
        limit: usize,
    ) -> Vec<NavigationLocation> {
        let Some(route_name_regex) = route_definition_name_regex(route_name) else {
            return Vec::new();
        };

        let mut matches = Vec::new();
        for corpus in corpora {
            let mut corpus_matches = Vec::new();
            for path in &corpus.source_paths {
                if supported_language_for_path(path, LanguageCapability::StructuralSearch)
                    != Some(SymbolLanguage::Php)
                {
                    continue;
                }

                let relative_path = Self::relative_display_path(&corpus.root, path);
                if !relative_path.starts_with("routes/") {
                    continue;
                }

                let Ok(source) = fs::read_to_string(path) else {
                    continue;
                };

                for capture in route_name_regex.captures_iter(&source) {
                    let Some(route_literal) =
                        capture.name("single").or_else(|| capture.name("double"))
                    else {
                        continue;
                    };
                    let (line, column) =
                        crate::indexer::line_column_for_offset(&source, route_literal.start());
                    corpus_matches.push(NavigationLocation {
                        match_id: None,
                        stable_symbol_id: None,
                        symbol: route_name.to_owned(),
                        repository_id: corpus.repository_id.clone(),
                        path: relative_path.clone(),
                        line,
                        column,
                        kind: Some("route".to_owned()),
                        container: None,
                        signature: None,
                        precision: Some("heuristic".to_owned()),
                        follow_up_structural: Vec::new(),
                    });
                }
            }

            if include_follow_up_structural {
                Self::populate_navigation_location_follow_up_structural(
                    &corpus.root,
                    &mut corpus_matches,
                );
            }
            matches.extend(corpus_matches);
        }

        Self::sort_navigation_locations(&mut matches);
        matches.dedup_by(|left, right| {
            left.repository_id == right.repository_id
                && left.path == right.path
                && left.line == right.line
                && left.column == right.column
                && left.symbol == right.symbol
                && left.kind == right.kind
                && left.precision == right.precision
        });
        if matches.len() > limit {
            matches.truncate(limit);
        }
        matches
    }

    pub(in crate::mcp::server) async fn go_to_definition_impl(
        &self,
        params: GoToDefinitionParams,
    ) -> Result<Json<GoToDefinitionResponse>, ErrorData> {
        struct GoToDefinitionExecution {
            result: Result<Json<GoToDefinitionResponse>, ErrorData>,
            provenance_result: Result<(), ErrorData>,
        }

        let execution_context =
            self.read_only_tool_execution_context("go_to_definition", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut scoped_repository_ids: Vec<String> = Vec::new();
                let mut selected_symbol_id: Option<String> = None;
                let mut selected_precise_symbol: Option<String> = None;
                let mut resolution_precision: Option<String> = None;
                let mut resolution_source: Option<String> = None;
                let mut target_selection_candidate_count = 0usize;
                let mut target_selection_same_rank_count = 0usize;
                let mut effective_limit: Option<usize> = None;
                let mut precise_artifacts_ingested = 0usize;
                let mut precise_artifacts_failed = 0usize;
                let mut match_count = 0usize;
                let result = (|| -> Result<Json<GoToDefinitionResponse>, ErrorData> {
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
                    let include_follow_up_structural =
                        params_for_blocking.include_follow_up_structural == Some(true);
                    effective_limit = Some(limit);
                    let scoped_execution_context = server.scoped_read_only_tool_execution_context(
                        execution_context_for_blocking.tool_name,
                        execution_context_for_blocking.repository_hint.clone(),
                        RepositoryResponseCacheFreshnessMode::ManifestOnly,
                    )?;
                    let scoped_repository_ids_for_cache =
                        scoped_execution_context.scoped_repository_ids.clone();
                    let cache_freshness = scoped_execution_context.cache_freshness.clone();
                    let cache_key = cache_freshness.scopes.as_ref().map(|freshness_scopes| {
                        GoToDefinitionResponseCacheKey {
                            scoped_repository_ids: scoped_repository_ids_for_cache.clone(),
                            freshness_scopes: freshness_scopes.clone(),
                            repository_id: params_for_blocking.repository_id.clone(),
                            symbol: params_for_blocking.symbol.clone(),
                            path: params_for_blocking.path.clone(),
                            line: params_for_blocking.line,
                            column: params_for_blocking.column,
                            include_follow_up_structural,
                            limit,
                        }
                    });
                    if cache_key.is_none() {
                        server.record_runtime_cache_event(
                            RuntimeCacheFamily::GoToDefinitionResponse,
                            RuntimeCacheEvent::Bypass,
                            1,
                        );
                    }
                    if let Some(cache_key) = cache_key.as_ref()
                        && let Some(cached) = server.cached_go_to_definition_response(cache_key)
                    {
                        scoped_repository_ids = cached.scoped_repository_ids;
                        selected_symbol_id = cached.selected_symbol_id;
                        selected_precise_symbol = cached.selected_precise_symbol;
                        resolution_precision = cached.resolution_precision;
                        resolution_source = cached.resolution_source;
                        effective_limit = Some(cached.effective_limit);
                        precise_artifacts_ingested = cached.precise_artifacts_ingested;
                        precise_artifacts_failed = cached.precise_artifacts_failed;
                        match_count = cached.match_count;
                        return Ok(Json(cached.response));
                    }

                    let response = if params_for_blocking.symbol.is_none() {
                        if let (Some(path), Some(line)) = (
                            params_for_blocking.path.as_deref(),
                            params_for_blocking.line,
                        ) {
                            let corpora = server.collect_repository_symbol_corpora(
                                params_for_blocking.repository_id.as_deref(),
                            )?;
                            scoped_repository_ids = corpora
                                .iter()
                                .map(|corpus| corpus.repository_id.clone())
                                .collect::<Vec<_>>();

                            let location_hint = params_for_blocking.column.and_then(|column| {
                                Self::navigation_symbol_query_token_from_location(
                                    &corpora, path, line, column,
                                )
                            });

                            if let Some(token_hint) = location_hint.as_ref()
                                && token_hint.resolution_source == "location_token_php_helper"
                            {
                                if let Some(helper_kind) = token_hint.helper_kind
                                    && let Some(direct_precise_target) = server
                                        .select_direct_precise_navigation_target_for_php_helper(
                                            &corpora,
                                            &token_hint.symbol_query,
                                            helper_kind,
                                            resource_budgets,
                                        )?
                                {
                                    resolution_source =
                                        Some(token_hint.resolution_source.to_owned());
                                    selected_precise_symbol = Some(
                                        direct_precise_target.precise_target.symbol.clone(),
                                    );
                                    precise_artifacts_ingested =
                                        direct_precise_target.ingest_stats.artifacts_ingested;
                                    precise_artifacts_failed =
                                        direct_precise_target.ingest_stats.artifacts_failed;
                                    let mut precise_matches =
                                        Self::direct_precise_definition_matches_for_target(
                                            &direct_precise_target,
                                            &token_hint.symbol_query,
                                        );
                                    if precise_matches.len() > limit {
                                        precise_matches.truncate(limit);
                                    }
                                    if include_follow_up_structural {
                                        Self::populate_navigation_location_follow_up_structural(
                                            &direct_precise_target.root,
                                            &mut precise_matches,
                                        );
                                    }
                                    if !precise_matches.is_empty() {
                                        let precision = Self::precise_resolution_precision(
                                            direct_precise_target.coverage_mode,
                                        );
                                        resolution_precision = Some(precision.to_owned());
                                        match_count = precise_matches.len();
                                        let metadata = json!({
                                            "precision": precision,
                                            "heuristic": false,
                                            "target_precise_symbol": selected_precise_symbol.clone(),
                                            "resolution_source": resolution_source.clone(),
                                            "precise": Self::precise_note_with_count(
                                                direct_precise_target.coverage_mode,
                                                &direct_precise_target.ingest_stats,
                                                "definition_count",
                                                precise_matches.len(),
                                            )
                                        });
                                        let metadata = Self::metadata_with_freshness_basis(
                                            metadata,
                                            &cache_freshness.basis,
                                        );
                                        let (metadata, note) =
                                            Self::metadata_note_pair(metadata);
                                        return Ok(Json(GoToDefinitionResponse {
                                            matches: precise_matches,
                                            result_handle: None,
                                            mode: FriggMcpServer::navigation_mode_from_precision_label(
                                                Some(precision),
                                            ),
                                            target_selection: None,
                                            metadata,
                                            note,
                                        }));
                                    }
                                }

                                if token_hint.helper_kind == Some(NavigationPhpHelperKind::Route) {
                                    let route_matches = Self::route_helper_definition_matches(
                                        &corpora,
                                        &token_hint.symbol_query,
                                        include_follow_up_structural,
                                        limit,
                                    );
                                    if !route_matches.is_empty() {
                                        resolution_source = Some(
                                            "location_token_php_helper_route_source".to_owned(),
                                        );
                                        resolution_precision = Some("heuristic".to_owned());
                                        match_count = route_matches.len();
                                        let metadata = json!({
                                            "precision": "heuristic",
                                            "heuristic": true,
                                            "fallback_reason": "route_helper_source",
                                            "resolution_source": resolution_source.clone(),
                                            "helper_kind": "route",
                                            "target_route_name": token_hint.symbol_query,
                                        });
                                        let metadata = Self::metadata_with_freshness_basis(
                                            metadata,
                                            &cache_freshness.basis,
                                        );
                                        let (metadata, note) =
                                            Self::metadata_note_pair(metadata);
                                        return Ok(Json(GoToDefinitionResponse {
                                            matches: route_matches,
                                            result_handle: None,
                                            mode: NavigationMode::HeuristicNoPrecise,
                                            target_selection: None,
                                            metadata,
                                            note,
                                        }));
                                    }
                                }
                            }

                            if let Some((response, repository_id, precise_symbol, precision)) =
                                server.try_precise_definition_fast_path(
                                    params_for_blocking.repository_id.as_deref(),
                                    path,
                                    line,
                                    params_for_blocking.column,
                                    include_follow_up_structural,
                                    limit,
                                )?
                            {
                                scoped_repository_ids = vec![repository_id];
                                selected_precise_symbol = Some(precise_symbol);
                                resolution_source = Some("location_precise_cache".to_owned());
                                resolution_precision = Some(precision);
                                match_count = response.0.matches.len();
                                response
                            } else {

                                let resolved_target =
                                    Self::resolve_navigation_target_from_location_hint(
                                        &corpora,
                                        path,
                                        line,
                                        params_for_blocking.column,
                                        params_for_blocking.repository_id.as_deref(),
                                        location_hint,
                                    )?;
                                resolution_source =
                                    Some(resolved_target.resolution_source.to_owned());
                                let symbol_query = resolved_target.symbol_query;
                                let target_selection = Some(
                                    Self::navigation_target_selection_summary_for_selection(
                                        &corpora,
                                        &symbol_query,
                                        &resolved_target.selection,
                                    ),
                                );
                                let target_resolution = match resolved_target.selection {
                                    NavigationTargetSelection::Resolved(target_resolution) => {
                                        target_resolution
                                    }
                                    NavigationTargetSelection::DisambiguationRequired(_) => {
                                        let metadata = json!({
                                            "precision": "unavailable",
                                            "heuristic": false,
                                            "disambiguation_required": true,
                                            "resolution_source": resolution_source.clone(),
                                            "target_selection": Self::navigation_target_selection_summary_value(
                                                target_selection
                                                    .as_ref()
                                                    .expect("target selection summary should be present"),
                                            ),
                                        });
                                        let metadata = Self::metadata_with_freshness_basis(
                                            metadata,
                                            &cache_freshness.basis,
                                        );
                                        let (metadata, note) = Self::metadata_note_pair(metadata);
                                        return Ok(Json(GoToDefinitionResponse {
                                            matches: Vec::new(),
                                            result_handle: None,
                                            mode: NavigationMode::UnavailableNoPrecise,
                                            target_selection,
                                            metadata,
                                            note,
                                        }));
                                    }
                                };
                                target_selection_candidate_count =
                                    target_resolution.candidate_count;
                                target_selection_same_rank_count =
                                    target_resolution.selected_rank_candidate_count;
                                let target = target_resolution.candidate;
                                selected_symbol_id = Some(target.symbol.stable_id.clone());
                                let target_corpus = target_resolution.corpus;

                                let cached_precise_graph = server.precise_graph_for_corpus(
                                    target_corpus.as_ref(),
                                    resource_budgets,
                                )?;
                                let precise_coverage = cached_precise_graph.coverage_mode;
                                let graph = cached_precise_graph.graph;
                                precise_artifacts_ingested =
                                    cached_precise_graph.ingest_stats.artifacts_ingested;
                                precise_artifacts_failed =
                                    cached_precise_graph.ingest_stats.artifacts_failed;
                                let precise_target =
                                    Self::select_precise_symbol_for_resolved_target(
                                        graph.as_ref(),
                                        &target_corpus.repository_id,
                                        &target.root,
                                        &symbol_query,
                                        &target.symbol,
                                    );
                                if let Some(precise_target) = &precise_target {
                                    selected_precise_symbol = Some(precise_target.symbol.clone());
                                }

                                let mut precise_matches = precise_target
                                    .as_ref()
                                    .map(|precise_target| {
                                        graph
                                            .precise_occurrences_for_symbol(
                                                &target_corpus.repository_id,
                                                &precise_target.symbol,
                                            )
                                            .into_iter()
                                            .filter(|occurrence| occurrence.is_definition())
                                            .map(|occurrence| {
                                                Self::selected_target_navigation_location(
                                                    target_corpus.as_ref(),
                                                    &target.root,
                                                    &target.symbol,
                                                    if precise_target.display_name.is_empty() {
                                                        target.symbol.name.clone()
                                                    } else {
                                                        precise_target.display_name.clone()
                                                    },
                                                    Self::canonicalize_navigation_path(
                                                        &target.root,
                                                        &occurrence.path,
                                                    ),
                                                    occurrence.range.start_line,
                                                    occurrence.range.start_column,
                                                    Self::display_symbol_kind(
                                                        &precise_target.kind,
                                                    ),
                                                    Some(
                                                        Self::precise_match_precision(
                                                            precise_coverage,
                                                        )
                                                        .to_owned(),
                                                    ),
                                                )
                                            })
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                Self::sort_navigation_locations(&mut precise_matches);
                                if precise_matches.len() > limit {
                                    precise_matches.truncate(limit);
                                }
                                if include_follow_up_structural {
                                    Self::populate_navigation_location_follow_up_structural(
                                        &target.root,
                                        &mut precise_matches,
                                    );
                                }

                                if !precise_matches.is_empty() {
                                    let precision =
                                        Self::precise_resolution_precision(precise_coverage);
                                    resolution_precision = Some(precision.to_owned());
                                    match_count = precise_matches.len();
                                    let metadata = json!({
                                        "precision": precision,
                                        "heuristic": false,
                                        "target_symbol_id": target.symbol.stable_id.clone(),
                                        "target_precise_symbol": selected_precise_symbol.clone(),
                                        "resolution_source": resolution_source.clone(),
                                        "target_selection": Self::navigation_target_selection_note(
                                            &symbol_query,
                                            &target,
                                            target_selection_candidate_count,
                                            target_selection_same_rank_count,
                                        ),
                                        "precise": Self::precise_note_with_count(
                                            precise_coverage,
                                            &cached_precise_graph.ingest_stats,
                                            "definition_count",
                                            precise_matches.len(),
                                        )
                                    });
                                    let metadata = Self::metadata_with_freshness_basis(
                                        metadata,
                                        &cache_freshness.basis,
                                    );
                                    let (metadata, note) = Self::metadata_note_pair(metadata);
                                    Json(GoToDefinitionResponse {
                                        matches: precise_matches,
                                        result_handle: None,
                                        mode: FriggMcpServer::navigation_mode_from_precision_label(
                                            Some(precision),
                                        ),
                                        target_selection: target_selection.clone(),
                                        metadata,
                                        note,
                                    })
                                } else {
                                    let mut matches = vec![Self::selected_target_navigation_location(
                                        target_corpus.as_ref(),
                                        &target.root,
                                        &target.symbol,
                                        target.symbol.name.clone(),
                                        String::new(),
                                        target.symbol.line,
                                        1,
                                        Self::display_symbol_kind(target.symbol.kind.as_str()),
                                        Some("heuristic".to_owned()),
                                    )];
                                    Self::sort_navigation_locations(&mut matches);
                                    if matches.len() > limit {
                                        matches.truncate(limit);
                                    }
                                    if include_follow_up_structural {
                                        Self::populate_navigation_location_follow_up_structural(
                                            &target.root,
                                            &mut matches,
                                        );
                                    }

                                    resolution_precision = Some("heuristic".to_owned());
                                    match_count = matches.len();
                                    let metadata = json!({
                                        "precision": "heuristic",
                                        "heuristic": true,
                                        "fallback_reason": "precise_absent",
                                        "precise_absence_reason": precise_absence_reason(
                                            precise_coverage,
                                            &cached_precise_graph.ingest_stats,
                                            0,
                                        ),
                                        "target_symbol_id": target.symbol.stable_id.clone(),
                                        "resolution_source": resolution_source.clone(),
                                        "target_selection": Self::navigation_target_selection_note(
                                            &symbol_query,
                                            &target,
                                            target_selection_candidate_count,
                                            target_selection_same_rank_count,
                                        ),
                                        "precise": Self::precise_note_with_count(
                                            precise_coverage,
                                            &cached_precise_graph.ingest_stats,
                                            "definition_count",
                                            0,
                                        )
                                    });
                                    let metadata = Self::metadata_with_freshness_basis(
                                        metadata,
                                        &cache_freshness.basis,
                                    );
                                    let (metadata, note) = Self::metadata_note_pair(metadata);
                                    Json(GoToDefinitionResponse {
                                        matches,
                                        result_handle: None,
                                        mode: NavigationMode::HeuristicNoPrecise,
                                        target_selection: target_selection.clone(),
                                        metadata,
                                        note,
                                    })
                                }
                            }
                        } else {
                            let corpora = server.collect_repository_symbol_corpora(
                                params_for_blocking.repository_id.as_deref(),
                            )?;
                            scoped_repository_ids = corpora
                                .iter()
                                .map(|corpus| corpus.repository_id.clone())
                                .collect::<Vec<_>>();

                            let resolved_target = Self::resolve_navigation_target(
                                &corpora,
                                params_for_blocking.symbol.as_deref(),
                                params_for_blocking.path.as_deref(),
                                params_for_blocking.line,
                                params_for_blocking.column,
                                params_for_blocking.repository_id.as_deref(),
                            )?;
                            resolution_source = Some(resolved_target.resolution_source.to_owned());
                            let symbol_query = resolved_target.symbol_query;
                            let target_selection = Some(
                                Self::navigation_target_selection_summary_for_selection(
                                    &corpora,
                                    &symbol_query,
                                    &resolved_target.selection,
                                ),
                            );
                            let target_resolution = match resolved_target.selection {
                                NavigationTargetSelection::Resolved(target_resolution) => {
                                    target_resolution
                                }
                                NavigationTargetSelection::DisambiguationRequired(_) => {
                                    let metadata = json!({
                                        "precision": "unavailable",
                                        "heuristic": false,
                                        "disambiguation_required": true,
                                        "resolution_source": resolution_source.clone(),
                                        "target_selection": Self::navigation_target_selection_summary_value(
                                            target_selection
                                                .as_ref()
                                                .expect("target selection summary should be present"),
                                        ),
                                    });
                                    let metadata = Self::metadata_with_freshness_basis(
                                        metadata,
                                        &cache_freshness.basis,
                                    );
                                    let (metadata, note) = Self::metadata_note_pair(metadata);
                                    return Ok(Json(GoToDefinitionResponse {
                                        matches: Vec::new(),
                                        result_handle: None,
                                        mode: NavigationMode::UnavailableNoPrecise,
                                        target_selection,
                                        metadata,
                                        note,
                                    }));
                                }
                            };
                            target_selection_candidate_count =
                                target_resolution.candidate_count;
                            target_selection_same_rank_count =
                                target_resolution.selected_rank_candidate_count;
                            let target = target_resolution.candidate;
                            selected_symbol_id = Some(target.symbol.stable_id.clone());
                            let target_corpus = target_resolution.corpus;

                            let cached_precise_graph = server.precise_graph_for_corpus(
                                target_corpus.as_ref(),
                                resource_budgets,
                            )?;
                            let precise_coverage = cached_precise_graph.coverage_mode;
                            let graph = cached_precise_graph.graph;
                            precise_artifacts_ingested =
                                cached_precise_graph.ingest_stats.artifacts_ingested;
                            precise_artifacts_failed =
                                cached_precise_graph.ingest_stats.artifacts_failed;
                            let precise_target = Self::select_precise_symbol_for_resolved_target(
                                graph.as_ref(),
                                &target_corpus.repository_id,
                                &target.root,
                                &symbol_query,
                                &target.symbol,
                            );
                            if let Some(precise_target) = &precise_target {
                                selected_precise_symbol = Some(precise_target.symbol.clone());
                            }

                            let mut precise_matches = precise_target
                                .as_ref()
                                .map(|precise_target| {
                                    graph
                                        .precise_occurrences_for_symbol(
                                            &target_corpus.repository_id,
                                            &precise_target.symbol,
                                        )
                                        .into_iter()
                                        .filter(|occurrence| occurrence.is_definition())
                                        .map(|occurrence| {
                                            Self::selected_target_navigation_location(
                                                target_corpus.as_ref(),
                                                &target.root,
                                                &target.symbol,
                                                if precise_target.display_name.is_empty() {
                                                    target.symbol.name.clone()
                                                } else {
                                                    precise_target.display_name.clone()
                                                },
                                                Self::canonicalize_navigation_path(
                                                    &target.root,
                                                    &occurrence.path,
                                                ),
                                                occurrence.range.start_line,
                                                occurrence.range.start_column,
                                                Self::display_symbol_kind(&precise_target.kind),
                                                Some(
                                                    Self::precise_match_precision(
                                                        precise_coverage,
                                                    )
                                                    .to_owned(),
                                                ),
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            Self::sort_navigation_locations(&mut precise_matches);
                            if precise_matches.len() > limit {
                                precise_matches.truncate(limit);
                            }
                            if include_follow_up_structural {
                                Self::populate_navigation_location_follow_up_structural(
                                    &target.root,
                                    &mut precise_matches,
                                );
                            }

                            if !precise_matches.is_empty() {
                                let precision =
                                    Self::precise_resolution_precision(precise_coverage);
                                resolution_precision = Some(precision.to_owned());
                                match_count = precise_matches.len();
                                let metadata = json!({
                                    "precision": precision,
                                    "heuristic": false,
                                    "target_symbol_id": target.symbol.stable_id.clone(),
                                    "target_precise_symbol": selected_precise_symbol.clone(),
                                    "resolution_source": resolution_source.clone(),
                                    "target_selection": Self::navigation_target_selection_note(
                                        &symbol_query,
                                        &target,
                                        target_selection_candidate_count,
                                        target_selection_same_rank_count,
                                    ),
                                    "precise": Self::precise_note_with_count(
                                        precise_coverage,
                                        &cached_precise_graph.ingest_stats,
                                        "definition_count",
                                        precise_matches.len(),
                                    )
                                });
                                let metadata = Self::metadata_with_freshness_basis(
                                    metadata,
                                    &cache_freshness.basis,
                                );
                                let (metadata, note) = Self::metadata_note_pair(metadata);
                                Json(GoToDefinitionResponse {
                                    matches: precise_matches,
                                    result_handle: None,
                                    mode: FriggMcpServer::navigation_mode_from_precision_label(
                                        Some(precision),
                                    ),
                                    target_selection: target_selection.clone(),
                                    metadata,
                                    note,
                                })
                            } else {
                                let mut matches = vec![Self::selected_target_navigation_location(
                                    target_corpus.as_ref(),
                                    &target.root,
                                    &target.symbol,
                                    target.symbol.name.clone(),
                                    String::new(),
                                    target.symbol.line,
                                    1,
                                    Self::display_symbol_kind(target.symbol.kind.as_str()),
                                    Some("heuristic".to_owned()),
                                )];
                                Self::sort_navigation_locations(&mut matches);
                                if matches.len() > limit {
                                    matches.truncate(limit);
                                }
                                if include_follow_up_structural {
                                    Self::populate_navigation_location_follow_up_structural(
                                        &target.root,
                                        &mut matches,
                                    );
                                }

                                resolution_precision = Some("heuristic".to_owned());
                                match_count = matches.len();
                                let metadata = json!({
                                    "precision": "heuristic",
                                    "heuristic": true,
                                    "fallback_reason": "precise_absent",
                                    "precise_absence_reason": precise_absence_reason(
                                        precise_coverage,
                                        &cached_precise_graph.ingest_stats,
                                        0,
                                    ),
                                    "target_symbol_id": target.symbol.stable_id.clone(),
                                    "resolution_source": resolution_source.clone(),
                                    "target_selection": Self::navigation_target_selection_note(
                                        &symbol_query,
                                        &target,
                                        target_selection_candidate_count,
                                        target_selection_same_rank_count,
                                    ),
                                    "precise": Self::precise_note_with_count(
                                        precise_coverage,
                                        &cached_precise_graph.ingest_stats,
                                        "definition_count",
                                        0,
                                    )
                                });
                                let metadata = Self::metadata_with_freshness_basis(
                                    metadata,
                                    &cache_freshness.basis,
                                );
                                let (metadata, note) = Self::metadata_note_pair(metadata);
                                Json(GoToDefinitionResponse {
                                    matches,
                                    result_handle: None,
                                    mode: NavigationMode::HeuristicNoPrecise,
                                    target_selection: target_selection.clone(),
                                    metadata,
                                    note,
                                })
                            }
                        }
                    } else {
                        let corpora = server.collect_repository_symbol_corpora(
                            params_for_blocking.repository_id.as_deref(),
                        )?;
                        scoped_repository_ids = corpora
                            .iter()
                            .map(|corpus| corpus.repository_id.clone())
                            .collect::<Vec<_>>();

                        match Self::resolve_navigation_target(
                            &corpora,
                            params_for_blocking.symbol.as_deref(),
                            params_for_blocking.path.as_deref(),
                            params_for_blocking.line,
                            params_for_blocking.column,
                            params_for_blocking.repository_id.as_deref(),
                        ) {
                            Ok(resolved_target) => {
                                resolution_source =
                                    Some(resolved_target.resolution_source.to_owned());
                                let symbol_query = resolved_target.symbol_query;
                                let target_selection = Some(
                                    Self::navigation_target_selection_summary_for_selection(
                                        &corpora,
                                        &symbol_query,
                                        &resolved_target.selection,
                                    ),
                                );
                                let target_resolution = match resolved_target.selection {
                                    NavigationTargetSelection::Resolved(target_resolution) => {
                                        target_resolution
                                    }
                                    NavigationTargetSelection::DisambiguationRequired(_) => {
                                        let metadata = json!({
                                            "precision": "unavailable",
                                            "heuristic": false,
                                            "disambiguation_required": true,
                                            "resolution_source": resolution_source.clone(),
                                            "target_selection": Self::navigation_target_selection_summary_value(
                                                target_selection
                                                    .as_ref()
                                                    .expect("target selection summary should be present"),
                                            ),
                                        });
                                        let metadata = Self::metadata_with_freshness_basis(
                                            metadata,
                                            &cache_freshness.basis,
                                        );
                                        let (metadata, note) = Self::metadata_note_pair(metadata);
                                        return Ok(Json(GoToDefinitionResponse {
                                            matches: Vec::new(),
                                            result_handle: None,
                                            mode: NavigationMode::UnavailableNoPrecise,
                                            target_selection,
                                            metadata,
                                            note,
                                        }));
                                    }
                                };
                                target_selection_candidate_count =
                                    target_resolution.candidate_count;
                                target_selection_same_rank_count =
                                    target_resolution.selected_rank_candidate_count;
                                let target = target_resolution.candidate;
                                selected_symbol_id = Some(target.symbol.stable_id.clone());
                                let target_corpus = target_resolution.corpus;

                                let cached_precise_graph = server
                                    .precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                                let precise_coverage = cached_precise_graph.coverage_mode;
                                let graph = cached_precise_graph.graph;
                                precise_artifacts_ingested =
                                    cached_precise_graph.ingest_stats.artifacts_ingested;
                                precise_artifacts_failed =
                                    cached_precise_graph.ingest_stats.artifacts_failed;
                                let precise_target = Self::select_precise_symbol_for_resolved_target(
                                    graph.as_ref(),
                                    &target_corpus.repository_id,
                                    &target.root,
                                    &symbol_query,
                                    &target.symbol,
                                );
                                if let Some(precise_target) = &precise_target {
                                    selected_precise_symbol = Some(precise_target.symbol.clone());
                                }

                                let mut precise_matches = precise_target
                                    .as_ref()
                                    .map(|precise_target| {
                                        graph
                                        .precise_occurrences_for_symbol(
                                            &target_corpus.repository_id,
                                            &precise_target.symbol,
                                        )
                                        .into_iter()
                                        .filter(|occurrence| occurrence.is_definition())
                                        .map(|occurrence| {
                                            Self::selected_target_navigation_location(
                                                target_corpus.as_ref(),
                                                &target.root,
                                                &target.symbol,
                                                if precise_target.display_name.is_empty() {
                                                    target.symbol.name.clone()
                                                } else {
                                                    precise_target.display_name.clone()
                                                },
                                                Self::canonicalize_navigation_path(
                                                    &target.root,
                                                    &occurrence.path,
                                                ),
                                                occurrence.range.start_line,
                                                occurrence.range.start_column,
                                                Self::display_symbol_kind(&precise_target.kind),
                                                Some(
                                                    Self::precise_match_precision(
                                                        precise_coverage,
                                                    )
                                                    .to_owned(),
                                                ),
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                Self::sort_navigation_locations(&mut precise_matches);
                                if precise_matches.len() > limit {
                                    precise_matches.truncate(limit);
                                }
                                if include_follow_up_structural {
                                    Self::populate_navigation_location_follow_up_structural(
                                        &target.root,
                                        &mut precise_matches,
                                    );
                                }

                                if !precise_matches.is_empty() {
                                    let precision = Self::precise_resolution_precision(precise_coverage);
                                    resolution_precision = Some(precision.to_owned());
                                    match_count = precise_matches.len();
                                    let metadata = json!({
                                        "precision": precision,
                                        "heuristic": false,
                                        "target_symbol_id": target.symbol.stable_id.clone(),
                                        "target_precise_symbol": selected_precise_symbol.clone(),
                                        "resolution_source": resolution_source.clone(),
                                        "target_selection": Self::navigation_target_selection_note(
                                            &symbol_query,
                                            &target,
                                            target_selection_candidate_count,
                                            target_selection_same_rank_count,
                                        ),
                                        "precise": Self::precise_note_with_count(
                                            precise_coverage,
                                            &cached_precise_graph.ingest_stats,
                                            "definition_count",
                                            precise_matches.len(),
                                        )
                                    });
                                    let metadata = Self::metadata_with_freshness_basis(
                                        metadata,
                                        &cache_freshness.basis,
                                    );
                                    let (metadata, note) = Self::metadata_note_pair(metadata);
                                    Json(GoToDefinitionResponse {
                                        matches: precise_matches,
                                        result_handle: None,
                                        mode: FriggMcpServer::navigation_mode_from_precision_label(Some(
                                            precision,
                                        )),
                                        target_selection: target_selection.clone(),
                                        metadata,
                                        note,
                                    })
                                } else {
                                    let mut matches = vec![Self::selected_target_navigation_location(
                                        target_corpus.as_ref(),
                                        &target.root,
                                        &target.symbol,
                                        target.symbol.name.clone(),
                                        String::new(),
                                        target.symbol.line,
                                        1,
                                        Self::display_symbol_kind(target.symbol.kind.as_str()),
                                        Some("heuristic".to_owned()),
                                    )];
                                    Self::sort_navigation_locations(&mut matches);
                                    if matches.len() > limit {
                                        matches.truncate(limit);
                                    }
                                    if include_follow_up_structural {
                                        Self::populate_navigation_location_follow_up_structural(
                                            &target.root,
                                            &mut matches,
                                        );
                                    }

                                    resolution_precision = Some("heuristic".to_owned());
                                    match_count = matches.len();
                                    let metadata = json!({
                                        "precision": "heuristic",
                                        "heuristic": true,
                                        "fallback_reason": "precise_absent",
                                        "precise_absence_reason": precise_absence_reason(
                                            precise_coverage,
                                            &cached_precise_graph.ingest_stats,
                                            0,
                                        ),
                                        "target_symbol_id": target.symbol.stable_id.clone(),
                                        "resolution_source": resolution_source.clone(),
                                        "target_selection": Self::navigation_target_selection_note(
                                            &symbol_query,
                                            &target,
                                            target_selection_candidate_count,
                                            target_selection_same_rank_count,
                                        ),
                                        "precise": Self::precise_note_with_count(
                                            precise_coverage,
                                            &cached_precise_graph.ingest_stats,
                                            "definition_count",
                                            0,
                                        )
                                    });
                                    let metadata = Self::metadata_with_freshness_basis(
                                        metadata,
                                        &cache_freshness.basis,
                                    );
                                    let (metadata, note) = Self::metadata_note_pair(metadata);
                                    Json(GoToDefinitionResponse {
                                        matches,
                                        result_handle: None,
                                        mode: NavigationMode::HeuristicNoPrecise,
                                        target_selection: target_selection.clone(),
                                        metadata,
                                        note,
                                    })
                                }
                            }
                            Err(error) if error_code_tag(&error) == Some("resource_not_found") => {
                                let symbol_query = params_for_blocking
                                    .symbol
                                    .as_deref()
                                    .unwrap_or_default()
                                    .trim()
                                    .to_owned();
                                if let Some(direct_precise_target) = server
                                    .select_direct_precise_navigation_target(
                                        &corpora,
                                        &symbol_query,
                                        resource_budgets,
                                    )?
                                {
                                    resolution_source = Some("symbol_precise_direct".to_owned());
                                    selected_precise_symbol =
                                        Some(direct_precise_target.precise_target.symbol.clone());
                                    precise_artifacts_ingested =
                                        direct_precise_target.ingest_stats.artifacts_ingested;
                                    precise_artifacts_failed =
                                        direct_precise_target.ingest_stats.artifacts_failed;
                                    let mut precise_matches =
                                        Self::direct_precise_definition_matches_for_target(
                                            &direct_precise_target,
                                            &symbol_query,
                                        );
                                    if precise_matches.len() > limit {
                                        precise_matches.truncate(limit);
                                    }
                                    if include_follow_up_structural {
                                        Self::populate_navigation_location_follow_up_structural(
                                            &direct_precise_target.root,
                                            &mut precise_matches,
                                        );
                                    }
                                    let precision = Self::precise_resolution_precision(
                                        direct_precise_target.coverage_mode,
                                    );
                                    resolution_precision = Some(precision.to_owned());
                                    match_count = precise_matches.len();
                                    let metadata = json!({
                                        "precision": precision,
                                        "heuristic": false,
                                        "target_precise_symbol": selected_precise_symbol.clone(),
                                        "resolution_source": resolution_source.clone(),
                                        "precise": Self::precise_note_with_count(
                                            direct_precise_target.coverage_mode,
                                            &direct_precise_target.ingest_stats,
                                            "definition_count",
                                            precise_matches.len(),
                                        )
                                    });
                                    let metadata = Self::metadata_with_freshness_basis(
                                        metadata,
                                        &cache_freshness.basis,
                                    );
                                    let (metadata, note) = Self::metadata_note_pair(metadata);
                                    Json(GoToDefinitionResponse {
                                        matches: precise_matches,
                                        result_handle: None,
                                        mode: FriggMcpServer::navigation_mode_from_precision_label(
                                            Some(precision),
                                        ),
                                        target_selection: None,
                                        metadata,
                                        note,
                                    })
                                } else {
                                    return Err(error);
                                }
                            }
                            Err(error) => return Err(error),
                        }
                    };

                    if let Some(cache_key) = cache_key {
                        server.cache_go_to_definition_response(
                            cache_key,
                            &response.0,
                            &scoped_repository_ids,
                            selected_symbol_id.as_deref(),
                            selected_precise_symbol.as_deref(),
                            resolution_precision.as_deref(),
                            resolution_source.as_deref(),
                            limit,
                            precise_artifacts_ingested,
                            precise_artifacts_failed,
                            match_count,
                        );
                    }
                    Ok(response)
                })();
                let precision_mode = FriggMcpServer::provenance_precision_mode_from_label(
                    resolution_precision.as_deref(),
                );
                let fallback_reason = if precision_mode == WorkloadPrecisionMode::Heuristic {
                    Some(WorkloadFallbackReason::PreciseAbsent)
                } else {
                    None
                };
                let metadata = FriggMcpServer::provenance_normalized_workload_metadata(
                    "go_to_definition",
                    &scoped_repository_ids,
                    precision_mode,
                    fallback_reason,
                    None,
                    None,
                );
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    execution_context_for_blocking.tool_name,
                    execution_context_for_blocking.repository_hint.as_deref(),
                    json!({
                        "symbol": params_for_blocking
                            .symbol
                            .as_ref()
                            .map(|symbol| Self::bounded_text(symbol)),
                        "repository_id": execution_context_for_blocking.repository_hint,
                        "path": params_for_blocking
                            .path
                            .as_ref()
                            .map(|path| Self::bounded_text(path)),
                        "line": params_for_blocking.line,
                        "column": params_for_blocking.column,
                        "limit": params_for_blocking.limit,
                        "effective_limit": effective_limit,
                    }),
                    json!({
                        "scoped_repository_ids": scoped_repository_ids,
                        "total_matches": match_count,
                        "selected_symbol_id": selected_symbol_id,
                        "selected_precise_symbol": selected_precise_symbol,
                        "resolution_precision": resolution_precision,
                        "resolution_source": resolution_source,
                        "precise_artifacts_ingested": precise_artifacts_ingested,
                        "precise_artifacts_failed": precise_artifacts_failed,
                        "match_count": match_count,
                    }),
                    Self::provenance_outcome(&result),
                    Some(metadata),
                );

                GoToDefinitionExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result.map(|Json(response)| {
            Json(self.present_go_to_definition_response(response, params.response_mode))
        });
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    pub(in crate::mcp::server) async fn find_declarations_impl(
        &self,
        params: FindDeclarationsParams,
    ) -> Result<Json<FindDeclarationsResponse>, ErrorData> {
        struct FindDeclarationsExecution {
            result: Result<Json<FindDeclarationsResponse>, ErrorData>,
            provenance_result: Result<(), ErrorData>,
        }

        let execution_context = self
            .read_only_tool_execution_context("find_declarations", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut scoped_repository_ids: Vec<String> = Vec::new();
                let mut selected_symbol_id: Option<String> = None;
                let mut selected_precise_symbol: Option<String> = None;
                let mut resolution_precision: Option<String> = None;
                let mut resolution_source: Option<String> = None;
                let mut target_selection_candidate_count = 0usize;
                let mut target_selection_same_rank_count = 0usize;
                let mut effective_limit: Option<usize> = None;
                let mut precise_artifacts_ingested = 0usize;
                let mut precise_artifacts_failed = 0usize;
                let mut match_count = 0usize;

                let result = (|| -> Result<Json<FindDeclarationsResponse>, ErrorData> {
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
                    let include_follow_up_structural =
                        params_for_blocking.include_follow_up_structural == Some(true);
                    effective_limit = Some(limit);
                    let scoped_execution_context = server.scoped_read_only_tool_execution_context(
                        execution_context_for_blocking.tool_name,
                        execution_context_for_blocking.repository_hint.clone(),
                        RepositoryResponseCacheFreshnessMode::ManifestOnly,
                    )?;
                    let scoped_repository_ids_for_cache =
                        scoped_execution_context.scoped_repository_ids.clone();
                    let cache_freshness = scoped_execution_context.cache_freshness.clone();
                    let cache_key = cache_freshness.scopes.as_ref().map(|freshness_scopes| {
                        FindDeclarationsResponseCacheKey {
                            scoped_repository_ids: scoped_repository_ids_for_cache.clone(),
                            freshness_scopes: freshness_scopes.clone(),
                            repository_id: params_for_blocking.repository_id.clone(),
                            symbol: params_for_blocking.symbol.clone(),
                            path: params_for_blocking.path.clone(),
                            line: params_for_blocking.line,
                            column: params_for_blocking.column,
                            include_follow_up_structural,
                            limit,
                        }
                    });
                    if cache_key.is_none() {
                        server.record_runtime_cache_event(
                            RuntimeCacheFamily::FindDeclarationsResponse,
                            RuntimeCacheEvent::Bypass,
                            1,
                        );
                    }
                    if let Some(cache_key) = cache_key.as_ref()
                        && let Some(cached) = server.cached_find_declarations_response(cache_key)
                    {
                        scoped_repository_ids = cached.scoped_repository_ids;
                        selected_symbol_id = cached.selected_symbol_id;
                        selected_precise_symbol = cached.selected_precise_symbol;
                        resolution_precision = cached.resolution_precision;
                        resolution_source = cached.resolution_source;
                        effective_limit = Some(cached.effective_limit);
                        precise_artifacts_ingested = cached.precise_artifacts_ingested;
                        precise_artifacts_failed = cached.precise_artifacts_failed;
                        match_count = cached.match_count;
                        return Ok(Json(cached.response));
                    }

                    let corpora = server.collect_repository_symbol_corpora(
                        params_for_blocking.repository_id.as_deref(),
                    )?;
                    scoped_repository_ids = corpora
                        .iter()
                        .map(|corpus| corpus.repository_id.clone())
                        .collect::<Vec<_>>();

                    let resolved_target = Self::resolve_navigation_target(
                        &corpora,
                        params_for_blocking.symbol.as_deref(),
                        params_for_blocking.path.as_deref(),
                        params_for_blocking.line,
                        params_for_blocking.column,
                        params_for_blocking.repository_id.as_deref(),
                    )?;
                    resolution_source = Some(resolved_target.resolution_source.to_owned());
                    let symbol_query = resolved_target.symbol_query;
                    let target_selection =
                        Some(Self::navigation_target_selection_summary_for_selection(
                            &corpora,
                            &symbol_query,
                            &resolved_target.selection,
                        ));
                    let target_resolution = match resolved_target.selection {
                        NavigationTargetSelection::Resolved(target_resolution) => target_resolution,
                        NavigationTargetSelection::DisambiguationRequired(_) => {
                            let metadata = json!({
                                "precision": "unavailable",
                                "heuristic": false,
                                "disambiguation_required": true,
                                "declaration_mode": "definition_anchor_v1",
                                "resolution_source": resolution_source.clone(),
                                "target_selection": Self::navigation_target_selection_summary_value(
                                    target_selection
                                        .as_ref()
                                        .expect("target selection summary should be present"),
                                ),
                            });
                            let metadata = Self::metadata_with_freshness_basis(
                                metadata,
                                &cache_freshness.basis,
                            );
                            let (metadata, note) = Self::metadata_note_pair(metadata);
                            let response = FindDeclarationsResponse {
                                matches: Vec::new(),
                                result_handle: None,
                                mode: NavigationMode::UnavailableNoPrecise,
                                target_selection,
                                metadata,
                                note,
                            };
                            if let Some(cache_key) = cache_key.clone() {
                                server.cache_find_declarations_response(
                                    cache_key,
                                    &response,
                                    &scoped_repository_ids,
                                    selected_symbol_id.as_deref(),
                                    selected_precise_symbol.as_deref(),
                                    resolution_precision.as_deref(),
                                    resolution_source.as_deref(),
                                    limit,
                                    precise_artifacts_ingested,
                                    precise_artifacts_failed,
                                    match_count,
                                );
                            }
                            return Ok(Json(response));
                        }
                    };
                    target_selection_candidate_count = target_resolution.candidate_count;
                    target_selection_same_rank_count =
                        target_resolution.selected_rank_candidate_count;
                    let target = target_resolution.candidate;
                    selected_symbol_id = Some(target.symbol.stable_id.clone());
                    let target_corpus = target_resolution.corpus;

                    let cached_precise_graph = server
                        .precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                    let precise_coverage = cached_precise_graph.coverage_mode;
                    let graph = cached_precise_graph.graph;
                    precise_artifacts_ingested =
                        cached_precise_graph.ingest_stats.artifacts_ingested;
                    precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                    let precise_target = Self::select_precise_symbol_for_resolved_target(
                        graph.as_ref(),
                        &target_corpus.repository_id,
                        &target.root,
                        &symbol_query,
                        &target.symbol,
                    );
                    if let Some(precise_target) = &precise_target {
                        selected_precise_symbol = Some(precise_target.symbol.clone());
                    }

                    let mut precise_matches = precise_target
                        .as_ref()
                        .map(|precise_target| {
                            graph
                                .precise_occurrences_for_symbol(
                                    &target_corpus.repository_id,
                                    &precise_target.symbol,
                                )
                                .into_iter()
                                .filter(|occurrence| occurrence.is_definition())
                                .map(|occurrence| {
                                    Self::selected_target_navigation_location(
                                        target_corpus.as_ref(),
                                        &target.root,
                                        &target.symbol,
                                        if precise_target.display_name.is_empty() {
                                            target.symbol.name.clone()
                                        } else {
                                            precise_target.display_name.clone()
                                        },
                                        Self::canonicalize_navigation_path(
                                            &target.root,
                                            &occurrence.path,
                                        ),
                                        occurrence.range.start_line,
                                        occurrence.range.start_column,
                                        Self::display_symbol_kind(&precise_target.kind),
                                        Some(
                                            Self::precise_match_precision(precise_coverage)
                                                .to_owned(),
                                        ),
                                    )
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    Self::sort_navigation_locations(&mut precise_matches);
                    if precise_matches.len() > limit {
                        precise_matches.truncate(limit);
                    }
                    if include_follow_up_structural {
                        Self::populate_navigation_location_follow_up_structural(
                            &target.root,
                            &mut precise_matches,
                        );
                    }

                    if !precise_matches.is_empty() {
                        let precision = Self::precise_resolution_precision(precise_coverage);
                        resolution_precision = Some(precision.to_owned());
                        match_count = precise_matches.len();
                        let metadata = json!({
                            "precision": precision,
                            "heuristic": false,
                            "declaration_mode": "definition_anchor_v1",
                            "target_symbol_id": target.symbol.stable_id.clone(),
                            "target_precise_symbol": selected_precise_symbol.clone(),
                            "resolution_source": resolution_source.clone(),
                            "target_selection": Self::navigation_target_selection_note(
                                &symbol_query,
                                &target,
                                target_selection_candidate_count,
                                target_selection_same_rank_count,
                            ),
                            "precise": Self::precise_note_with_count(
                                precise_coverage,
                                &cached_precise_graph.ingest_stats,
                                "declaration_count",
                                precise_matches.len(),
                            )
                        });
                        let metadata =
                            Self::metadata_with_freshness_basis(metadata, &cache_freshness.basis);
                        let (metadata, note) = Self::metadata_note_pair(metadata);
                        let response = FindDeclarationsResponse {
                            matches: precise_matches,
                            result_handle: None,
                            mode: FriggMcpServer::navigation_mode_from_precision_label(Some(
                                precision,
                            )),
                            target_selection: target_selection.clone(),
                            metadata,
                            note,
                        };
                        if let Some(cache_key) = cache_key.clone() {
                            server.cache_find_declarations_response(
                                cache_key,
                                &response,
                                &scoped_repository_ids,
                                selected_symbol_id.as_deref(),
                                selected_precise_symbol.as_deref(),
                                resolution_precision.as_deref(),
                                resolution_source.as_deref(),
                                limit,
                                precise_artifacts_ingested,
                                precise_artifacts_failed,
                                match_count,
                            );
                        }
                        return Ok(Json(response));
                    }

                    let mut matches = vec![Self::selected_target_navigation_location(
                        target_corpus.as_ref(),
                        &target.root,
                        &target.symbol,
                        target.symbol.name.clone(),
                        String::new(),
                        target.symbol.line,
                        1,
                        Self::display_symbol_kind(target.symbol.kind.as_str()),
                        Some("heuristic".to_owned()),
                    )];
                    Self::sort_navigation_locations(&mut matches);
                    if matches.len() > limit {
                        matches.truncate(limit);
                    }
                    if include_follow_up_structural {
                        Self::populate_navigation_location_follow_up_structural(
                            &target.root,
                            &mut matches,
                        );
                    }

                    resolution_precision = Some("heuristic".to_owned());
                    match_count = matches.len();
                    let metadata = json!({
                        "precision": "heuristic",
                        "heuristic": true,
                        "fallback_reason": "precise_absent",
                        "precise_absence_reason": precise_absence_reason(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            0,
                        ),
                        "declaration_mode": "definition_anchor_v1",
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "declaration_count",
                            0,
                        )
                    });
                    let metadata =
                        Self::metadata_with_freshness_basis(metadata, &cache_freshness.basis);
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    let response = FindDeclarationsResponse {
                        matches,
                        result_handle: None,
                        mode: NavigationMode::HeuristicNoPrecise,
                        target_selection: target_selection.clone(),
                        metadata,
                        note,
                    };
                    if let Some(cache_key) = cache_key {
                        server.cache_find_declarations_response(
                            cache_key,
                            &response,
                            &scoped_repository_ids,
                            selected_symbol_id.as_deref(),
                            selected_precise_symbol.as_deref(),
                            resolution_precision.as_deref(),
                            resolution_source.as_deref(),
                            limit,
                            precise_artifacts_ingested,
                            precise_artifacts_failed,
                            match_count,
                        );
                    }
                    Ok(Json(response))
                })();

                let precision_mode = FriggMcpServer::provenance_precision_mode_from_label(
                    resolution_precision.as_deref(),
                );
                let fallback_reason = if precision_mode == WorkloadPrecisionMode::Heuristic {
                    Some(WorkloadFallbackReason::PreciseAbsent)
                } else {
                    None
                };
                let finalization = server.tool_execution_finalization(
                    json!({
                        "scoped_repository_ids": scoped_repository_ids,
                        "selected_symbol_id": selected_symbol_id,
                        "selected_precise_symbol": selected_precise_symbol,
                        "resolution_precision": resolution_precision,
                        "resolution_source": resolution_source,
                        "precise_artifacts_ingested": precise_artifacts_ingested,
                        "precise_artifacts_failed": precise_artifacts_failed,
                        "match_count": match_count,
                    }),
                    Some(FriggMcpServer::provenance_normalized_workload_metadata(
                        execution_context_for_blocking.tool_name,
                        &scoped_repository_ids,
                        precision_mode,
                        fallback_reason,
                        None,
                        None,
                    )),
                );
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    execution_context_for_blocking.tool_name,
                    execution_context_for_blocking.repository_hint.as_deref(),
                    json!({
                        "symbol": params_for_blocking
                            .symbol
                            .as_ref()
                            .map(|symbol| Self::bounded_text(symbol)),
                        "repository_id": execution_context_for_blocking.repository_hint,
                        "path": params_for_blocking
                            .path
                            .as_ref()
                            .map(|path| Self::bounded_text(path)),
                        "line": params_for_blocking.line,
                        "column": params_for_blocking.column,
                        "limit": params_for_blocking.limit,
                        "effective_limit": effective_limit,
                    }),
                    finalization.source_refs,
                    Self::provenance_outcome(&result),
                    finalization.normalized_workload,
                );

                FindDeclarationsExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result.map(|Json(response)| {
            Json(self.present_find_declarations_response(response, params.response_mode))
        });
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }
}
