use super::*;

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
            return "target_not_present_in_precise_graph";
        }
        PreciseCoverageMode::None => {
            return "no_usable_precise_data";
        }
        _ => {}
    }

    "precise_unavailable"
}

impl FriggMcpServer {
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
                                resolution_source =
                                    Some(resolved_target.resolution_source.to_owned());
                                let symbol_query = resolved_target.symbol_query;
                                target_selection_candidate_count =
                                    resolved_target.target.candidate_count;
                                target_selection_same_rank_count =
                                    resolved_target.target.selected_rank_candidate_count;
                                let target = resolved_target.target.candidate;
                                selected_symbol_id = Some(target.symbol.stable_id.clone());
                                let target_corpus = resolved_target.target.corpus;

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
                                            .map(|occurrence| NavigationLocation {
                                                symbol: if precise_target.display_name.is_empty() {
                                                    target.symbol.name.clone()
                                                } else {
                                                    precise_target.display_name.clone()
                                                },
                                                repository_id: target_corpus.repository_id.clone(),
                                                path: Self::canonicalize_navigation_path(
                                                    &target.root,
                                                    &occurrence.path,
                                                ),
                                                line: occurrence.range.start_line,
                                                column: occurrence.range.start_column,
                                                kind: Self::display_symbol_kind(
                                                    &precise_target.kind,
                                                ),
                                                precision: Some(
                                                    Self::precise_match_precision(precise_coverage)
                                                        .to_owned(),
                                                ),
                                                follow_up_structural: Vec::new(),
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
                                        mode: FriggMcpServer::navigation_mode_from_precision_label(
                                            Some(precision),
                                        ),
                                        metadata,
                                        note,
                                    })
                                } else {
                                    let mut matches = vec![NavigationLocation {
                                        symbol: target.symbol.name.clone(),
                                        repository_id: target_corpus.repository_id.clone(),
                                        path: Self::relative_display_path(
                                            &target.root,
                                            &target.symbol.path,
                                        ),
                                        line: target.symbol.line,
                                        column: 1,
                                        kind: Self::display_symbol_kind(
                                            target.symbol.kind.as_str(),
                                        ),
                                        precision: Some("heuristic".to_owned()),
                                        follow_up_structural: Vec::new(),
                                    }];
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
                                        mode: NavigationMode::HeuristicNoPrecise,
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
                            target_selection_candidate_count =
                                resolved_target.target.candidate_count;
                            target_selection_same_rank_count =
                                resolved_target.target.selected_rank_candidate_count;
                            let target = resolved_target.target.candidate;
                            selected_symbol_id = Some(target.symbol.stable_id.clone());
                            let target_corpus = resolved_target.target.corpus;

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
                                        .map(|occurrence| NavigationLocation {
                                            symbol: if precise_target.display_name.is_empty() {
                                                target.symbol.name.clone()
                                            } else {
                                                precise_target.display_name.clone()
                                            },
                                            repository_id: target_corpus.repository_id.clone(),
                                            path: Self::canonicalize_navigation_path(
                                                &target.root,
                                                &occurrence.path,
                                            ),
                                            line: occurrence.range.start_line,
                                            column: occurrence.range.start_column,
                                            kind: Self::display_symbol_kind(&precise_target.kind),
                                            precision: Some(
                                                Self::precise_match_precision(precise_coverage)
                                                    .to_owned(),
                                            ),
                                            follow_up_structural: Vec::new(),
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
                                    mode: FriggMcpServer::navigation_mode_from_precision_label(
                                        Some(precision),
                                    ),
                                    metadata,
                                    note,
                                })
                            } else {
                                let mut matches = vec![NavigationLocation {
                                    symbol: target.symbol.name.clone(),
                                    repository_id: target_corpus.repository_id.clone(),
                                    path: Self::relative_display_path(
                                        &target.root,
                                        &target.symbol.path,
                                    ),
                                    line: target.symbol.line,
                                    column: 1,
                                    kind: Self::display_symbol_kind(target.symbol.kind.as_str()),
                                    precision: Some("heuristic".to_owned()),
                                    follow_up_structural: Vec::new(),
                                }];
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
                                    mode: NavigationMode::HeuristicNoPrecise,
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
                        target_selection_candidate_count = resolved_target.target.candidate_count;
                        target_selection_same_rank_count =
                            resolved_target.target.selected_rank_candidate_count;
                        let target = resolved_target.target.candidate;
                        selected_symbol_id = Some(target.symbol.stable_id.clone());
                        let target_corpus = resolved_target.target.corpus;

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
                                    .map(|occurrence| NavigationLocation {
                                        symbol: if precise_target.display_name.is_empty() {
                                            target.symbol.name.clone()
                                        } else {
                                            precise_target.display_name.clone()
                                        },
                                        repository_id: target_corpus.repository_id.clone(),
                                        path: Self::canonicalize_navigation_path(
                                            &target.root,
                                            &occurrence.path,
                                        ),
                                        line: occurrence.range.start_line,
                                        column: occurrence.range.start_column,
                                        kind: Self::display_symbol_kind(&precise_target.kind),
                                        precision: Some(
                                            Self::precise_match_precision(precise_coverage)
                                                .to_owned(),
                                        ),
                                        follow_up_structural: Vec::new(),
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
                                mode: FriggMcpServer::navigation_mode_from_precision_label(Some(
                                    precision,
                                )),
                                metadata,
                                note,
                            })
                        } else {
                            let mut matches = vec![NavigationLocation {
                                symbol: target.symbol.name.clone(),
                                repository_id: target_corpus.repository_id.clone(),
                                path: Self::relative_display_path(
                                    &target.root,
                                    &target.symbol.path,
                                ),
                                line: target.symbol.line,
                                column: 1,
                                kind: Self::display_symbol_kind(target.symbol.kind.as_str()),
                                precision: Some("heuristic".to_owned()),
                                follow_up_structural: Vec::new(),
                            }];
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
                                mode: NavigationMode::HeuristicNoPrecise,
                                metadata,
                                note,
                            })
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

        let result = execution.result;
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
                    target_selection_candidate_count = resolved_target.target.candidate_count;
                    target_selection_same_rank_count =
                        resolved_target.target.selected_rank_candidate_count;
                    let target = resolved_target.target.candidate;
                    selected_symbol_id = Some(target.symbol.stable_id.clone());
                    let target_corpus = resolved_target.target.corpus;

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
                                .map(|occurrence| NavigationLocation {
                                    symbol: if precise_target.display_name.is_empty() {
                                        target.symbol.name.clone()
                                    } else {
                                        precise_target.display_name.clone()
                                    },
                                    repository_id: target_corpus.repository_id.clone(),
                                    path: Self::canonicalize_navigation_path(
                                        &target.root,
                                        &occurrence.path,
                                    ),
                                    line: occurrence.range.start_line,
                                    column: occurrence.range.start_column,
                                    kind: Self::display_symbol_kind(&precise_target.kind),
                                    precision: Some(
                                        Self::precise_match_precision(precise_coverage).to_owned(),
                                    ),
                                    follow_up_structural: Vec::new(),
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
                            mode: FriggMcpServer::navigation_mode_from_precision_label(Some(
                                precision,
                            )),
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

                    let mut matches = vec![NavigationLocation {
                        symbol: target.symbol.name.clone(),
                        repository_id: target_corpus.repository_id.clone(),
                        path: Self::relative_display_path(&target.root, &target.symbol.path),
                        line: target.symbol.line,
                        column: 1,
                        kind: Self::display_symbol_kind(target.symbol.kind.as_str()),
                        precision: Some("heuristic".to_owned()),
                        follow_up_structural: Vec::new(),
                    }];
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
                        mode: NavigationMode::HeuristicNoPrecise,
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

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }
}
