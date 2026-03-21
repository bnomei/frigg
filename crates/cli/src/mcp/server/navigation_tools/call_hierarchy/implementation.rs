use super::*;

impl FriggMcpServer {
    pub(in crate::mcp::server) async fn find_implementations_impl(
        &self,
        params: FindImplementationsParams,
    ) -> Result<Json<FindImplementationsResponse>, ErrorData> {
        let execution_context = self
            .read_only_tool_execution_context("find_implementations", params.repository_id.clone());
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
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
            let mut fallback_reason: Option<String> = None;
            (|| -> Result<Json<FindImplementationsResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                let include_follow_up_structural =
                    params_for_blocking.include_follow_up_structural == Some(true);
                effective_limit = Some(limit);

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

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;
                let precise_targets = Self::matching_precise_symbols_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );

                let mut precise_matches = Vec::new();
                for precise_target in &precise_targets {
                    let matches = Self::precise_implementation_matches_for_symbol(
                        graph.as_ref(),
                        &target_corpus.repository_id,
                        &target.root,
                        precise_coverage,
                        precise_target,
                    );
                    if !matches.is_empty() {
                        selected_precise_symbol = Some(precise_target.symbol.clone());
                        precise_matches = matches;
                        break;
                    }
                }
                if precise_matches.is_empty() {
                    for precise_target in &precise_targets {
                        let matches = Self::precise_implementation_matches_from_occurrences(
                            graph.as_ref(),
                            target_corpus.as_ref(),
                            &target.root,
                            &target.symbol.name,
                            precise_coverage,
                            precise_target,
                        );
                        if !matches.is_empty() {
                            selected_precise_symbol = Some(precise_target.symbol.clone());
                            precise_matches = matches;
                            break;
                        }
                    }
                }
                if precise_matches.is_empty() {
                    precise_matches = Self::heuristic_implementation_matches_from_symbols(
                        &target.symbol,
                        target_corpus.as_ref(),
                        &target.root,
                    );
                    resolution_precision = Some("heuristic".to_owned());
                    fallback_reason = precise_matches
                        .iter()
                        .find_map(|implementation_match| implementation_match.fallback_reason.clone())
                        .or_else(|| Some("precise_absent".to_owned()));
                    for implementation_match in &mut precise_matches {
                        if implementation_match.fallback_reason.is_none() {
                            implementation_match.fallback_reason = fallback_reason.clone();
                        }
                    }
                } else {
                    resolution_precision =
                        Some(Self::precise_resolution_precision(precise_coverage).to_owned());
                }
                if precise_matches.len() > limit {
                    precise_matches.truncate(limit);
                }
                if include_follow_up_structural {
                    Self::populate_implementation_match_follow_up_structural(
                        &target.root,
                        &mut precise_matches,
                    );
                }
                match_count = precise_matches.len();

                let metadata = json!({
                    "precision": resolution_precision.clone().unwrap_or_else(|| "heuristic".to_owned()),
                    "heuristic": resolution_precision.as_deref() == Some("heuristic"),
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "target_precise_symbol": selected_precise_symbol.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "fallback_reason": fallback_reason,
                    "precise_absence_reason": fallback_reason.as_ref().map(|_| {
                        Self::precise_absence_reason(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            0,
                        )
                    }),
                    "scoped_repository_ids": scoped_repository_ids.clone(),
                    "precise_artifacts_ingested": precise_artifacts_ingested,
                    "precise_artifacts_failed": precise_artifacts_failed,
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "implementation_count",
                        precise_matches.len(),
                    ),
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(FindImplementationsResponse {
                    matches: precise_matches,
                    mode: Self::navigation_mode_from_precision_label(
                        resolution_precision.as_deref(),
                    ),
                    metadata,
                    note,
                }))
            })()
        });
        execution.await?
    }

    pub(in crate::mcp::server) async fn incoming_calls_impl(
        &self,
        params: IncomingCallsParams,
    ) -> Result<Json<IncomingCallsResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("incoming_calls", params.repository_id.clone());
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_failed = 0usize;
            (|| -> Result<Json<IncomingCallsResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                let include_follow_up_structural =
                    params_for_blocking.include_follow_up_structural == Some(true);
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
                let target_resolution = resolved_target.target;
                target_selection_candidate_count = target_resolution.candidate_count;
                target_selection_same_rank_count = target_resolution.selected_rank_candidate_count;
                let target = target_resolution.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());

                let target_corpus = target_resolution.corpus;
                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;

                let mut precise_matches = Vec::new();
                let precise_targets = Self::matching_precise_symbols_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                for precise_target in &precise_targets {
                    let matches = Self::precise_incoming_matches_from_relationships(
                        graph.as_ref(),
                        &target_corpus.repository_id,
                        &target.root,
                        &target.symbol.name,
                        precise_coverage,
                        precise_target,
                    );
                    if !matches.is_empty() {
                        selected_precise_symbol = Some(precise_target.symbol.clone());
                        precise_matches = matches;
                        break;
                    }
                }
                if precise_matches.is_empty() {
                    for precise_target in &precise_targets {
                        let matches = Self::precise_incoming_matches_from_occurrences(
                            graph.as_ref(),
                            target_corpus.as_ref(),
                            &target.root,
                            &target.symbol.name,
                            precise_coverage,
                            precise_target,
                            &target.symbol.stable_id,
                        );
                        if !matches.is_empty() {
                            selected_precise_symbol = Some(precise_target.symbol.clone());
                            precise_matches = matches;
                            break;
                        }
                    }
                }

                if !precise_matches.is_empty() {
                    if precise_matches.len() > limit {
                        precise_matches.truncate(limit);
                    }
                    if include_follow_up_structural {
                        Self::populate_call_hierarchy_match_follow_up_structural(
                            &target.root,
                            &mut precise_matches,
                        );
                    }
                    let precision = Self::precise_resolution_precision(precise_coverage).to_owned();
                    resolution_precision = Some(precision.clone());
                    let availability = Self::call_hierarchy_availability(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        precise_matches.len(),
                        0,
                    );
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "availability": availability.clone(),
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise_artifacts_ingested": precise_artifacts_ingested,
                        "precise_artifacts_failed": precise_artifacts_failed,
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "incoming_count",
                            precise_matches.len(),
                        ),
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(IncomingCallsResponse {
                        matches: precise_matches,
                        mode: Self::navigation_mode_from_precision_label(
                            resolution_precision.as_deref(),
                        ),
                        availability: Some(availability),
                        metadata,
                        note,
                    }));
                }

                if precise_coverage == PreciseCoverageMode::Full {
                    let precision = Self::precise_resolution_precision(precise_coverage).to_owned();
                    resolution_precision = Some(precision.clone());
                    let availability = Self::call_hierarchy_availability(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                        0,
                    );
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "availability": availability.clone(),
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "precise_artifacts_ingested": precise_artifacts_ingested,
                        "precise_artifacts_failed": precise_artifacts_failed,
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "incoming_count",
                            0,
                        ),
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(IncomingCallsResponse {
                        matches: Vec::new(),
                        mode: Self::navigation_mode_from_precision_label(
                            resolution_precision.as_deref(),
                        ),
                        availability: Some(availability),
                        metadata,
                        note,
                    }));
                }

                let mut matches = graph
                    .incoming_adjacency(&target.symbol.stable_id)
                    .into_iter()
                    .filter(|adjacent| Self::is_heuristic_call_relation(adjacent.relation))
                    .map(|adjacent| CallHierarchyMatch {
                        source_symbol: adjacent.symbol.display_name,
                        target_symbol: target.symbol.name.clone(),
                        repository_id: target_corpus.repository_id.clone(),
                        path: Self::canonicalize_navigation_path(
                            &target.root,
                            &adjacent.symbol.path,
                        ),
                        line: adjacent.symbol.line,
                        column: 1,
                        relation: adjacent.relation.as_str().to_owned(),
                        precision: Some("heuristic".to_owned()),
                        call_path: None,
                        call_line: None,
                        call_column: None,
                        call_end_line: None,
                        call_end_column: None,
                        follow_up_structural: Vec::new(),
                    })
                    .collect::<Vec<_>>();
                Self::sort_call_hierarchy_matches(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }
                if include_follow_up_structural {
                    Self::populate_call_hierarchy_match_follow_up_structural(
                        &target.root,
                        &mut matches,
                    );
                }

                resolution_precision = Some("heuristic".to_owned());
                let fallback_reason = "precise_absent";
                let availability = NavigationAvailability {
                    status: "heuristic".to_owned(),
                    reason: Some(
                        Self::precise_absence_reason(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            0,
                        )
                        .to_owned(),
                    ),
                    precise_required_for_complete_results: true,
                };
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": fallback_reason,
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                    ),
                    "availability": availability.clone(),
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "target_precise_symbol": selected_precise_symbol.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "precise_artifacts_ingested": precise_artifacts_ingested,
                    "precise_artifacts_failed": precise_artifacts_failed,
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "incoming_count",
                        0,
                    ),
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(IncomingCallsResponse {
                    matches,
                    mode: Self::navigation_mode_from_precision_label(
                        resolution_precision.as_deref(),
                    ),
                    availability: Some(availability),
                    metadata,
                    note,
                }))
            })()
        });
        execution.await?
    }

    pub(in crate::mcp::server) async fn outgoing_calls_impl(
        &self,
        params: OutgoingCallsParams,
    ) -> Result<Json<OutgoingCallsResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("outgoing_calls", params.repository_id.clone());
        let resource_budgets = self.find_references_resource_budgets();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_failed = 0usize;
            (|| -> Result<Json<OutgoingCallsResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                let include_follow_up_structural =
                    params_for_blocking.include_follow_up_structural == Some(true);
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
                let target_resolution = resolved_target.target;
                target_selection_candidate_count = target_resolution.candidate_count;
                target_selection_same_rank_count = target_resolution.selected_rank_candidate_count;
                let target = target_resolution.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());

                let target_corpus = target_resolution.corpus;
                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                precise_artifacts_ingested = cached_precise_graph.ingest_stats.artifacts_ingested;
                precise_artifacts_failed = cached_precise_graph.ingest_stats.artifacts_failed;

                let mut precise_matches = Vec::new();
                let mut precise_occurrence_cache = std::collections::BTreeMap::<
                    String,
                    Vec<crate::graph::PreciseOccurrenceRecord>,
                >::new();
                let mut precise_source_cache =
                    std::collections::BTreeMap::<String, Option<String>>::new();
                let precise_targets = Self::matching_precise_symbols_for_resolved_target(
                    graph.as_ref(),
                    &target_corpus.repository_id,
                    &target.root,
                    &symbol_query,
                    &target.symbol,
                );
                for precise_target in &precise_targets {
                    let matches = Self::precise_outgoing_matches_from_occurrences(
                        graph.as_ref(),
                        target_corpus.as_ref(),
                        &target.root,
                        &target.symbol.name,
                        precise_coverage,
                        precise_target,
                        &target.symbol.stable_id,
                        &mut precise_occurrence_cache,
                        &mut precise_source_cache,
                    );
                    if !matches.is_empty() {
                        selected_precise_symbol = Some(precise_target.symbol.clone());
                        precise_matches = matches;
                        break;
                    }
                }

                if !precise_matches.is_empty() {
                    if precise_matches.len() > limit {
                        precise_matches.truncate(limit);
                    }
                    if include_follow_up_structural {
                        Self::populate_call_hierarchy_match_follow_up_structural(
                            &target.root,
                            &mut precise_matches,
                        );
                    }
                    let precision = Self::precise_resolution_precision(precise_coverage).to_owned();
                    resolution_precision = Some(precision.clone());
                    let availability = Self::call_hierarchy_availability(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        precise_matches.len(),
                        0,
                    );
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "availability": availability.clone(),
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "scoped_repository_ids": scoped_repository_ids.clone(),
                        "precise_artifacts_ingested": precise_artifacts_ingested,
                        "precise_artifacts_failed": precise_artifacts_failed,
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "outgoing_count",
                            precise_matches.len(),
                        ),
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(OutgoingCallsResponse {
                        matches: precise_matches,
                        mode: Self::navigation_mode_from_precision_label(
                            resolution_precision.as_deref(),
                        ),
                        availability: Some(availability),
                        metadata,
                        note,
                    }));
                }

                if precise_coverage == PreciseCoverageMode::Full {
                    let precision = Self::precise_resolution_precision(precise_coverage).to_owned();
                    resolution_precision = Some(precision.clone());
                    let availability = Self::call_hierarchy_availability(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                        0,
                    );
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "availability": availability.clone(),
                        "target_symbol_id": target.symbol.stable_id.clone(),
                        "target_precise_symbol": selected_precise_symbol.clone(),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "scoped_repository_ids": scoped_repository_ids.clone(),
                        "precise_artifacts_ingested": precise_artifacts_ingested,
                        "precise_artifacts_failed": precise_artifacts_failed,
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            "outgoing_count",
                            0,
                        ),
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    return Ok(Json(OutgoingCallsResponse {
                        matches: Vec::new(),
                        mode: Self::navigation_mode_from_precision_label(
                            resolution_precision.as_deref(),
                        ),
                        availability: Some(availability),
                        metadata,
                        note,
                    }));
                }

                let mut source_cache: std::collections::BTreeMap<String, Option<String>> =
                    std::collections::BTreeMap::new();
                let mut call_target_cache: std::collections::BTreeMap<
                    String,
                    Option<std::collections::BTreeSet<String>>,
                > = std::collections::BTreeMap::new();
                let mut matches = graph
                    .outgoing_adjacency(&target.symbol.stable_id)
                    .into_iter()
                    .filter(|adjacent| {
                        Self::is_heuristic_call_relation(adjacent.relation)
                            && Self::is_heuristic_callable_kind(&adjacent.symbol.kind)
                            && Self::heuristic_symbol_body_has_call_like_reference(
                                &target.root,
                                &target.symbol,
                                adjacent.symbol.display_name.as_str(),
                                &mut source_cache,
                                &mut call_target_cache,
                            )
                    })
                    .map(|adjacent| CallHierarchyMatch {
                        source_symbol: target.symbol.name.clone(),
                        target_symbol: adjacent.symbol.display_name,
                        repository_id: target_corpus.repository_id.clone(),
                        path: Self::canonicalize_navigation_path(
                            &target.root,
                            &adjacent.symbol.path,
                        ),
                        line: adjacent.symbol.line,
                        column: 1,
                        relation: adjacent.relation.as_str().to_owned(),
                        precision: Some("heuristic".to_owned()),
                        call_path: None,
                        call_line: None,
                        call_column: None,
                        call_end_line: None,
                        call_end_column: None,
                        follow_up_structural: Vec::new(),
                    })
                    .collect::<Vec<_>>();
                Self::sort_call_hierarchy_matches(&mut matches);
                if matches.len() > limit {
                    matches.truncate(limit);
                }
                if include_follow_up_structural {
                    Self::populate_call_hierarchy_match_follow_up_structural(
                        &target.root,
                        &mut matches,
                    );
                }

                resolution_precision = Some("heuristic".to_owned());
                let fallback_reason = "precise_absent";
                let availability = NavigationAvailability {
                    status: "heuristic".to_owned(),
                    reason: Some(
                        Self::precise_absence_reason(
                            precise_coverage,
                            &cached_precise_graph.ingest_stats,
                            0,
                        )
                        .to_owned(),
                    ),
                    precise_required_for_complete_results: true,
                };
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": fallback_reason,
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        0,
                    ),
                    "availability": availability.clone(),
                    "target_symbol_id": target.symbol.stable_id.clone(),
                    "target_precise_symbol": selected_precise_symbol.clone(),
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "scoped_repository_ids": scoped_repository_ids.clone(),
                    "precise_artifacts_ingested": precise_artifacts_ingested,
                    "precise_artifacts_failed": precise_artifacts_failed,
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &cached_precise_graph.ingest_stats,
                        "outgoing_count",
                        0,
                    ),
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(OutgoingCallsResponse {
                    matches,
                    mode: Self::navigation_mode_from_precision_label(
                        resolution_precision.as_deref(),
                    ),
                    availability: Some(availability),
                    metadata,
                    note,
                }))
            })()
        });
        execution.await?
    }

    fn heuristic_symbol_body_has_call_like_reference(
        root: &Path,
        source_symbol: &SymbolDefinition,
        target_name: &str,
        source_cache: &mut std::collections::BTreeMap<String, Option<String>>,
        call_target_cache: &mut std::collections::BTreeMap<
            String,
            Option<std::collections::BTreeSet<String>>,
        >,
    ) -> bool {
        if target_name.trim().is_empty() {
            return false;
        }
        let call_targets = call_target_cache
            .entry(source_symbol.stable_id.clone())
            .or_insert_with(|| {
                Self::heuristic_symbol_body_call_targets(root, source_symbol, source_cache)
            });
        call_targets
            .as_ref()
            .is_some_and(|targets| targets.contains(target_name))
    }

    fn heuristic_symbol_body_call_targets(
        root: &Path,
        source_symbol: &SymbolDefinition,
        source_cache: &mut std::collections::BTreeMap<String, Option<String>>,
    ) -> Option<std::collections::BTreeSet<String>> {
        let relative_path = Self::relative_display_path(root, &source_symbol.path);
        let source = source_cache
            .entry(relative_path)
            .or_insert_with(|| fs::read_to_string(&source_symbol.path).ok())
            .as_deref()?;
        let start = source_symbol.span.start_byte.min(source.len());
        let end = source_symbol.span.end_byte.min(source.len());
        let body = source.get(start..end)?;
        let bytes = body.as_bytes();
        let mut targets = std::collections::BTreeSet::new();
        let mut index = 0usize;
        while index < bytes.len() {
            let byte = bytes[index];
            if !(byte.is_ascii_alphabetic() || byte == b'_') {
                index = index.saturating_add(1);
                continue;
            }

            let start_index = index;
            index = index.saturating_add(1);
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric() || bytes[index] == b'_')
            {
                index = index.saturating_add(1);
            }

            let Some(name) = body.get(start_index..index) else {
                continue;
            };
            let suffix = body.get(index..).unwrap_or_default();
            let is_call = match source_symbol.language {
                SymbolLanguage::Rust => rust_source_suffix_looks_like_call(suffix),
                _ => suffix.trim_start().starts_with('('),
            };
            if is_call {
                targets.insert(name.to_owned());
            }
        }

        Some(targets)
    }
}
