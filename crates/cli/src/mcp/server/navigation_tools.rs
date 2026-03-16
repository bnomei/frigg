use super::*;
use crate::domain::WorkloadFallbackReason;

impl FriggMcpServer {
    pub(super) async fn find_references_impl(
        &self,
        params: FindReferencesParams,
    ) -> Result<Json<FindReferencesResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("find_references", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let resource_budgets = self.find_references_resource_budgets();
        let resource_budget_metadata = Self::find_references_budget_metadata(resource_budgets);
        let params_for_blocking = params.clone();
        let server = self.clone();
        let resource_budget_metadata_for_blocking = resource_budget_metadata.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut total_matches = 0usize;
            let mut selected_symbol_id: Option<String> = None;
            let mut selected_precise_symbol: Option<String> = None;
            let mut resolution_precision: Option<String> = None;
            let mut resolution_source: Option<String> = None;
            let mut diagnostics_count = 0usize;
            let mut manifest_walk_diagnostics_count = 0usize;
            let mut manifest_read_diagnostics_count = 0usize;
            let mut symbol_extraction_diagnostics_count = 0usize;
            let mut source_read_diagnostics_count = 0usize;
            let mut precise_artifacts_discovered = 0usize;
            let mut precise_artifacts_discovered_bytes = 0u64;
            let mut precise_artifacts_ingested = 0usize;
            let mut precise_artifacts_ingested_bytes = 0u64;
            let mut precise_artifacts_failed = 0usize;
            let mut precise_artifacts_failed_bytes = 0u64;
            let mut precise_reference_count = 0usize;
            let mut source_files_discovered = 0usize;
            let mut source_files_loaded = 0usize;
            let mut source_bytes_loaded = 0u64;
            let mut effective_limit: Option<usize> = None;
            let mut target_selection_candidate_count = 0usize;
            let mut target_selection_same_rank_count = 0usize;
            let result = (|| -> Result<Json<FindReferencesResponse>, ErrorData> {
                let limit = params_for_blocking
                    .limit
                    .unwrap_or(server.config.max_search_results)
                    .min(server.config.max_search_results.max(1));
                effective_limit = Some(limit);

                let corpora = server
                    .collect_repository_symbol_corpora(params_for_blocking.repository_id.as_deref())?;
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

                let resolve_by_location = params_for_blocking.path.is_some()
                    || params_for_blocking.line.is_some()
                    || params_for_blocking.column.is_some();
                let resolved_target = if resolve_by_location {
                    Self::resolve_navigation_target(
                        &corpora,
                        None,
                        params_for_blocking.path.as_deref(),
                        params_for_blocking.line,
                        params_for_blocking.column,
                        params_for_blocking.repository_id.as_deref(),
                    )?
                } else {
                    Self::resolve_navigation_target(
                        &corpora,
                        params_for_blocking.symbol.as_deref(),
                        None,
                        None,
                        None,
                        params_for_blocking.repository_id.as_deref(),
                    )?
                };
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                let target_resolution = Self::resolve_navigation_symbol_target(
                    &corpora,
                    &symbol_query,
                    params_for_blocking.repository_id.as_deref(),
                )?;
                target_selection_candidate_count = target_resolution.candidate_count;
                target_selection_same_rank_count = target_resolution.selected_rank_candidate_count;
                let target = target_resolution.candidate;
                selected_symbol_id = Some(target.symbol.stable_id.clone());

                let target_corpus = target_resolution.corpus;
                source_files_discovered = target_corpus.source_paths.len();

                let cached_precise_graph =
                    server.precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                let heuristic_scip_signature =
                    Self::scip_signature(&cached_precise_graph.discovery.artifact_digests);
                let precise_coverage = cached_precise_graph.coverage_mode;
                let graph = cached_precise_graph.graph;
                let target_precise_stats = cached_precise_graph.ingest_stats;
                precise_artifacts_discovered = target_precise_stats.artifacts_discovered;
                precise_artifacts_discovered_bytes = target_precise_stats.artifacts_discovered_bytes;
                precise_artifacts_ingested = target_precise_stats.artifacts_ingested;
                precise_artifacts_ingested_bytes = target_precise_stats.artifacts_ingested_bytes;
                precise_artifacts_failed = target_precise_stats.artifacts_failed;
                precise_artifacts_failed_bytes = target_precise_stats.artifacts_failed_bytes;

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

                let precise_references = precise_target
                    .as_ref()
                    .map(|precise_target| {
                        graph.precise_references_for_symbol(
                            &target_corpus.repository_id,
                            &precise_target.symbol,
                        )
                    })
                    .unwrap_or_default();
                precise_reference_count = precise_references.len();

                if !precise_references.is_empty() {
                    let matches = precise_references
                        .into_iter()
                        .take(limit)
                        .map(|reference| {
                            let reference_path = PathBuf::from(&reference.path);
                            let absolute_path = if reference_path.is_absolute() {
                                reference_path
                            } else {
                                target.root.join(reference_path)
                            };

                            ReferenceMatch {
                                repository_id: target_corpus.repository_id.clone(),
                                symbol: precise_target
                                    .as_ref()
                                    .map(|selected| selected.display_name.clone())
                                    .filter(|display_name| !display_name.is_empty())
                                    .unwrap_or_else(|| target.symbol.name.clone()),
                                path: Self::relative_display_path(&target.root, &absolute_path),
                                line: reference.range.start_line,
                                column: reference.range.start_column,
                            }
                        })
                        .collect::<Vec<_>>();
                    total_matches = precise_reference_count;

                    let precision = Self::precise_resolution_precision(precise_coverage);
                    resolution_precision = Some(precision.to_owned());
                    let metadata = json!({
                        "precision": precision,
                        "heuristic": false,
                        "target_symbol_id": target.symbol.stable_id,
                        "target_precise_symbol": precise_target
                            .as_ref()
                            .map(|selected| selected.symbol.clone()),
                        "resolution_source": resolution_source.clone(),
                        "target_selection": Self::navigation_target_selection_note(
                            &symbol_query,
                            &target,
                            target_selection_candidate_count,
                            target_selection_same_rank_count,
                        ),
                        "diagnostics_count": diagnostics_count,
                        "diagnostics": {
                            "manifest_walk": manifest_walk_diagnostics_count,
                            "manifest_read": manifest_read_diagnostics_count,
                            "symbol_extraction": symbol_extraction_diagnostics_count,
                            "source_read": source_read_diagnostics_count,
                            "total": diagnostics_count,
                        },
                        "precise": Self::precise_note_with_count(
                            precise_coverage,
                            &target_precise_stats,
                            "reference_count",
                            precise_reference_count,
                        ),
                        "resource_budgets": resource_budget_metadata_for_blocking.clone(),
                        "resource_usage": {
                            "scip": {
                                "artifacts_discovered": target_precise_stats.artifacts_discovered,
                                "artifacts_discovered_bytes": target_precise_stats.artifacts_discovered_bytes,
                                "artifacts_ingested": target_precise_stats.artifacts_ingested,
                                "artifacts_ingested_bytes": target_precise_stats.artifacts_ingested_bytes,
                                "artifacts_failed": target_precise_stats.artifacts_failed,
                                "artifacts_failed_bytes": target_precise_stats.artifacts_failed_bytes,
                            },
                            "source": {
                                "files_discovered": source_files_discovered,
                                "files_loaded": source_files_loaded,
                                "bytes_loaded": source_bytes_loaded,
                            },
                        },
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);

                    return Ok(Json(FindReferencesResponse {
                        total_matches,
                        matches,
                        metadata,
                        note,
                    }));
                }

                let mut resolver = HeuristicReferenceResolver::new(
                    &target_corpus.repository_id,
                    &target.symbol.stable_id,
                    &target_corpus.symbols,
                    graph.as_ref(),
                )
                .ok_or_else(|| {
                    Self::internal(
                        "failed to initialize heuristic resolver for selected symbol",
                        Some(json!({
                            "repository_id": target_corpus.repository_id,
                            "symbol_id": target.symbol.stable_id,
                        })),
                    )
                })?;
                let heuristic_cache_key = HeuristicReferenceCacheKey {
                    repository_id: target_corpus.repository_id.clone(),
                    symbol_id: target.symbol.stable_id.clone(),
                    corpus_signature: target_corpus.root_signature.clone(),
                    scip_signature: heuristic_scip_signature,
                };

                let all_references = if let Some(cached) =
                    server.cached_heuristic_references(&heuristic_cache_key)
                {
                    source_read_diagnostics_count = cached.source_read_diagnostics_count;
                    source_files_loaded = cached.source_files_loaded;
                    source_bytes_loaded = cached.source_bytes_loaded;
                    (*cached.references).clone()
                } else {
                    let source_started_at = Instant::now();
                    let source_max_elapsed =
                        Duration::from_millis(resource_budgets.source_max_elapsed_ms);
                    let source_max_file_bytes =
                        Self::usize_to_u64(resource_budgets.source_max_file_bytes);
                    let source_max_total_bytes =
                        Self::usize_to_u64(resource_budgets.source_max_total_bytes);

                    for (index, path) in target_corpus.source_paths.iter().enumerate() {
                        if source_started_at.elapsed() > source_max_elapsed {
                            let elapsed_ms =
                                u64::try_from(source_started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                            return Err(Self::find_references_resource_budget_error(
                                "source",
                                "source_elapsed_ms",
                                "find_references source processing exceeded time budget",
                                json!({
                                    "repository_id": target_corpus.repository_id,
                                    "actual": elapsed_ms,
                                    "limit": resource_budgets.source_max_elapsed_ms,
                                    "files_loaded": Self::usize_to_u64(source_files_loaded),
                                    "bytes_loaded": source_bytes_loaded,
                                }),
                            ));
                        }

                        if index >= resource_budgets.source_max_files {
                            return Err(Self::find_references_resource_budget_error(
                                "source",
                                "source_file_count",
                                "find_references source file count exceeds configured budget",
                                json!({
                                    "repository_id": target_corpus.repository_id,
                                    "actual": Self::usize_to_u64(index.saturating_add(1)),
                                    "limit": Self::usize_to_u64(resource_budgets.source_max_files),
                                }),
                            ));
                        }

                        let metadata = match fs::metadata(path) {
                            Ok(metadata) => Some(metadata),
                            Err(err) => {
                                source_read_diagnostics_count += 1;
                                warn!(
                                    repository_id = target_corpus.repository_id,
                                    path = %path.display(),
                                    error = %err,
                                    "skipping source file while resolving heuristic references"
                                );
                                None
                            }
                        };

                        if let Some(metadata) = metadata {
                            let pre_read_bytes = metadata.len();
                            if pre_read_bytes > source_max_file_bytes {
                                return Err(Self::find_references_resource_budget_error(
                                    "source",
                                    "source_file_bytes",
                                    "find_references source file exceeds per-file byte budget",
                                    json!({
                                        "repository_id": target_corpus.repository_id,
                                        "path": path.display().to_string(),
                                        "actual": pre_read_bytes,
                                        "limit": source_max_file_bytes,
                                    }),
                                ));
                            }
                            let projected_total =
                                source_bytes_loaded.saturating_add(pre_read_bytes);
                            if projected_total > source_max_total_bytes {
                                return Err(Self::find_references_resource_budget_error(
                                    "source",
                                    "source_total_bytes",
                                    "find_references source bytes exceed configured budget",
                                    json!({
                                        "repository_id": target_corpus.repository_id,
                                        "path": path.display().to_string(),
                                        "actual": projected_total,
                                        "limit": source_max_total_bytes,
                                    }),
                                ));
                            }
                        }

                        match fs::read_to_string(path) {
                            Ok(source) => {
                                let source_bytes = Self::usize_to_u64(source.len());
                                if source_bytes > source_max_file_bytes {
                                    return Err(Self::find_references_resource_budget_error(
                                        "source",
                                        "source_file_bytes",
                                        "find_references source file exceeds per-file byte budget",
                                        json!({
                                            "repository_id": target_corpus.repository_id,
                                            "path": path.display().to_string(),
                                            "actual": source_bytes,
                                            "limit": source_max_file_bytes,
                                        }),
                                    ));
                                }
                                let projected_total =
                                    source_bytes_loaded.saturating_add(source_bytes);
                                if projected_total > source_max_total_bytes {
                                    return Err(Self::find_references_resource_budget_error(
                                        "source",
                                        "source_total_bytes",
                                        "find_references source bytes exceed configured budget",
                                        json!({
                                            "repository_id": target_corpus.repository_id,
                                            "path": path.display().to_string(),
                                            "actual": projected_total,
                                            "limit": source_max_total_bytes,
                                        }),
                                    ));
                                }

                                resolver.ingest_source(path, &source);
                                source_files_loaded = source_files_loaded.saturating_add(1);
                                source_bytes_loaded = projected_total;
                            }
                            Err(err) => {
                                source_read_diagnostics_count += 1;
                                warn!(
                                    repository_id = target_corpus.repository_id,
                                    path = %path.display(),
                                    error = %err,
                                    "skipping source file while resolving heuristic references"
                                );
                            }
                        }
                    }

                    let all_references = resolver.finish();
                    server.cache_heuristic_references(
                        heuristic_cache_key,
                        all_references.clone(),
                        source_read_diagnostics_count,
                        source_files_loaded,
                        source_bytes_loaded,
                    );
                    all_references
                };
                total_matches = all_references.len();
                let references = all_references.into_iter().take(limit).collect::<Vec<_>>();

                let mut high_confidence = 0usize;
                let mut medium_confidence = 0usize;
                let mut low_confidence = 0usize;
                let mut graph_evidence = 0usize;
                let mut lexical_evidence = 0usize;

                let matches = references
                    .iter()
                    .map(|reference| {
                        match reference.confidence {
                            HeuristicReferenceConfidence::High => high_confidence += 1,
                            HeuristicReferenceConfidence::Medium => medium_confidence += 1,
                            HeuristicReferenceConfidence::Low => low_confidence += 1,
                        }
                        match &reference.evidence {
                            HeuristicReferenceEvidence::GraphRelation { .. } => graph_evidence += 1,
                            HeuristicReferenceEvidence::LexicalToken => lexical_evidence += 1,
                        }

                        ReferenceMatch {
                            repository_id: reference.repository_id.clone(),
                            symbol: reference.symbol_name.clone(),
                            path: Self::relative_display_path(&target.root, &reference.path),
                            line: reference.line,
                            column: reference.column,
                        }
                    })
                    .collect::<Vec<_>>();

                diagnostics_count += source_read_diagnostics_count;
                let metadata = json!({
                    "precision": "heuristic",
                    "heuristic": true,
                    "fallback_reason": "precise_absent",
                    "precise_absence_reason": Self::precise_absence_reason(
                        precise_coverage,
                        &target_precise_stats,
                        precise_reference_count,
                    ),
                    "target_symbol_id": target.symbol.stable_id,
                    "resolution_source": resolution_source.clone(),
                    "target_selection": Self::navigation_target_selection_note(
                        &symbol_query,
                        &target,
                        target_selection_candidate_count,
                        target_selection_same_rank_count,
                    ),
                    "confidence": {
                        "high": high_confidence,
                        "medium": medium_confidence,
                        "low": low_confidence,
                    },
                    "evidence": {
                        "graph_relation": graph_evidence,
                        "lexical_token": lexical_evidence,
                    },
                    "diagnostics_count": diagnostics_count,
                    "diagnostics": {
                        "manifest_walk": manifest_walk_diagnostics_count,
                        "manifest_read": manifest_read_diagnostics_count,
                        "symbol_extraction": symbol_extraction_diagnostics_count,
                        "source_read": source_read_diagnostics_count,
                        "total": diagnostics_count,
                    },
                    "precise": Self::precise_note_with_count(
                        precise_coverage,
                        &target_precise_stats,
                        "reference_count",
                        precise_reference_count,
                    ),
                    "resource_budgets": resource_budget_metadata_for_blocking.clone(),
                    "resource_usage": {
                        "scip": {
                            "artifacts_discovered": target_precise_stats.artifacts_discovered,
                            "artifacts_discovered_bytes": target_precise_stats.artifacts_discovered_bytes,
                            "artifacts_ingested": target_precise_stats.artifacts_ingested,
                            "artifacts_ingested_bytes": target_precise_stats.artifacts_ingested_bytes,
                            "artifacts_failed": target_precise_stats.artifacts_failed,
                            "artifacts_failed_bytes": target_precise_stats.artifacts_failed_bytes,
                        },
                        "source": {
                            "files_discovered": source_files_discovered,
                            "files_loaded": source_files_loaded,
                            "bytes_loaded": source_bytes_loaded,
                        },
                    },
                });
                let (metadata, note) = Self::metadata_note_pair(metadata);
                resolution_precision = Some("heuristic".to_owned());

                Ok(Json(FindReferencesResponse {
                    total_matches,
                    matches,
                    metadata,
                    note,
                }))
            })();
            let precision_mode =
                FriggMcpServer::provenance_precision_mode_from_label(resolution_precision.as_deref());
            let fallback_reason = if precision_mode == WorkloadPrecisionMode::Heuristic {
                Some(WorkloadFallbackReason::PreciseAbsent)
            } else {
                None
            };
            let metadata = FriggMcpServer::provenance_normalized_workload_metadata(
                execution_context_for_blocking.tool_name,
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
                    "repository_id": execution_context_for_blocking.repository_hint,
                    "symbol": params_for_blocking
                        .symbol
                        .as_ref()
                        .map(|symbol| Self::bounded_text(symbol)),
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
                    "scoped_repository_ids": scoped_repository_ids.clone(),
                    "total_matches": total_matches,
                    "selected_symbol_id": selected_symbol_id.clone(),
                    "selected_precise_symbol": selected_precise_symbol.clone(),
                    "resolution_precision": resolution_precision.clone(),
                    "resolution_source": resolution_source.clone(),
                    "diagnostics_count": diagnostics_count,
                    "diagnostics": {
                        "manifest_walk": manifest_walk_diagnostics_count,
                        "manifest_read": manifest_read_diagnostics_count,
                        "symbol_extraction": symbol_extraction_diagnostics_count,
                        "source_read": source_read_diagnostics_count,
                        "total": diagnostics_count,
                    },
                    "precise_artifacts_discovered": precise_artifacts_discovered,
                    "precise_artifacts_discovered_bytes": precise_artifacts_discovered_bytes,
                    "precise_artifacts_ingested": precise_artifacts_ingested,
                    "precise_artifacts_ingested_bytes": precise_artifacts_ingested_bytes,
                    "precise_artifacts_failed": precise_artifacts_failed,
                    "precise_artifacts_failed_bytes": precise_artifacts_failed_bytes,
                    "precise_reference_count": precise_reference_count,
                    "resource_budgets": resource_budget_metadata_for_blocking.clone(),
                    "source_files_discovered": source_files_discovered,
                    "source_files_loaded": source_files_loaded,
                    "source_bytes_loaded": source_bytes_loaded,
                }),
                Self::provenance_outcome(&result),
                Some(metadata),
            );

            FindReferencesExecution {
                result,
                provenance_result,
                scoped_repository_ids,
                total_matches,
                selected_symbol_id,
                selected_precise_symbol,
                resolution_precision,
                resolution_source,
                diagnostics_count,
                manifest_walk_diagnostics_count,
                manifest_read_diagnostics_count,
                symbol_extraction_diagnostics_count,
                source_read_diagnostics_count,
                precise_artifacts_discovered,
                precise_artifacts_discovered_bytes,
                precise_artifacts_ingested,
                precise_artifacts_ingested_bytes,
                precise_artifacts_failed,
                precise_artifacts_failed_bytes,
                precise_reference_count,
                source_files_discovered,
                source_files_loaded,
                source_bytes_loaded,
                effective_limit,
            }
        })
        .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    pub(super) async fn go_to_definition_impl(
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
                            limit,
                        }
                    });
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
                                            })
                                            .collect::<Vec<_>>()
                                    })
                                    .unwrap_or_default();
                                Self::sort_navigation_locations(&mut precise_matches);
                                if precise_matches.len() > limit {
                                    precise_matches.truncate(limit);
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
                                    }];
                                    Self::sort_navigation_locations(&mut matches);
                                    if matches.len() > limit {
                                        matches.truncate(limit);
                                    }

                                    resolution_precision = Some("heuristic".to_owned());
                                    match_count = matches.len();
                                    let metadata = json!({
                                        "precision": "heuristic",
                                        "heuristic": true,
                                        "fallback_reason": "precise_absent",
                                        "precise_absence_reason": Self::precise_absence_reason(
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
                                        })
                                        .collect::<Vec<_>>()
                                })
                                .unwrap_or_default();
                            Self::sort_navigation_locations(&mut precise_matches);
                            if precise_matches.len() > limit {
                                precise_matches.truncate(limit);
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
                                }];
                                Self::sort_navigation_locations(&mut matches);
                                if matches.len() > limit {
                                    matches.truncate(limit);
                                }

                                resolution_precision = Some("heuristic".to_owned());
                                match_count = matches.len();
                                let metadata = json!({
                                    "precision": "heuristic",
                                    "heuristic": true,
                                    "fallback_reason": "precise_absent",
                                    "precise_absence_reason": Self::precise_absence_reason(
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
                                    })
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        Self::sort_navigation_locations(&mut precise_matches);
                        if precise_matches.len() > limit {
                            precise_matches.truncate(limit);
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
                            }];
                            Self::sort_navigation_locations(&mut matches);
                            if matches.len() > limit {
                                matches.truncate(limit);
                            }

                            resolution_precision = Some("heuristic".to_owned());
                            match_count = matches.len();
                            let metadata = json!({
                                "precision": "heuristic",
                                "heuristic": true,
                                "fallback_reason": "precise_absent",
                                "precise_absence_reason": Self::precise_absence_reason(
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

    pub(super) async fn find_declarations_impl(
        &self,
        params: FindDeclarationsParams,
    ) -> Result<Json<FindDeclarationsResponse>, ErrorData> {
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
                            limit,
                        }
                    });
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
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    Self::sort_navigation_locations(&mut precise_matches);
                    if precise_matches.len() > limit {
                        precise_matches.truncate(limit);
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
                    }];
                    Self::sort_navigation_locations(&mut matches);
                    if matches.len() > limit {
                        matches.truncate(limit);
                    }

                    resolution_precision = Some("heuristic".to_owned());
                    match_count = matches.len();
                    let metadata = json!({
                        "precision": "heuristic",
                        "heuristic": true,
                        "fallback_reason": "precise_absent",
                        "precise_absence_reason": Self::precise_absence_reason(
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

                NavigationToolExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    pub(super) async fn find_implementations_impl(
        &self,
        params: FindImplementationsParams,
    ) -> Result<Json<FindImplementationsResponse>, ErrorData> {
        let execution_context = self
            .read_only_tool_execution_context("find_implementations", params.repository_id.clone());
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

                let result = (|| -> Result<Json<FindImplementationsResponse>, ErrorData> {
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
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

                    let cached_precise_graph = server
                        .precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                    let precise_coverage = cached_precise_graph.coverage_mode;
                    let graph = cached_precise_graph.graph;
                    precise_artifacts_ingested =
                        cached_precise_graph.ingest_stats.artifacts_ingested;
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
                    if precise_matches.len() > limit {
                        precise_matches.truncate(limit);
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
                                "implementation_count",
                                precise_matches.len(),
                            )
                        });
                        let (metadata, note) = Self::metadata_note_pair(metadata);
                        return Ok(Json(FindImplementationsResponse {
                            matches: precise_matches,
                            metadata,
                            note,
                        }));
                    }

                    let mut matches = graph
                        .incoming_adjacency(&target.symbol.stable_id)
                        .into_iter()
                        .filter(|adjacent| {
                            matches!(
                                adjacent.relation,
                                RelationKind::Implements | RelationKind::Extends
                            )
                        })
                        .map(|adjacent| ImplementationMatch {
                            symbol: adjacent.symbol.display_name,
                            kind: Self::display_symbol_kind(&adjacent.symbol.kind),
                            repository_id: target_corpus.repository_id.clone(),
                            path: Self::canonicalize_navigation_path(
                                &target.root,
                                &adjacent.symbol.path,
                            ),
                            line: adjacent.symbol.line,
                            column: 1,
                            relation: Some(adjacent.relation.as_str().to_owned()),
                            precision: Some("heuristic".to_owned()),
                            fallback_reason: Some("precise_absent".to_owned()),
                        })
                        .collect::<Vec<_>>();
                    matches.extend(Self::heuristic_implementation_matches_from_symbols(
                        &target.symbol,
                        target_corpus.as_ref(),
                        &target.root,
                    ));
                    Self::sort_implementation_matches(&mut matches);
                    matches.dedup_by(|left, right| {
                        left.repository_id == right.repository_id
                            && left.path == right.path
                            && left.line == right.line
                            && left.column == right.column
                            && left.symbol == right.symbol
                            && left.kind == right.kind
                            && left.relation == right.relation
                            && left.precision == right.precision
                            && left.fallback_reason == right.fallback_reason
                    });
                    if matches.len() > limit {
                        matches.truncate(limit);
                    }

                    resolution_precision = Some("heuristic".to_owned());
                    match_count = matches.len();
                    let metadata = json!({
                        "precision": "heuristic",
                        "heuristic": true,
                        "fallback_reason": "precise_absent",
                        "precise_absence_reason": Self::precise_absence_reason(
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
                            "implementation_count",
                            matches.len(),
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    Ok(Json(FindImplementationsResponse {
                        matches,
                        metadata,
                        note,
                    }))
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

                NavigationToolExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    pub(super) async fn incoming_calls_impl(
        &self,
        params: IncomingCallsParams,
    ) -> Result<Json<IncomingCallsResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("incoming_calls", params.repository_id.clone());
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

                let result = (|| -> Result<Json<IncomingCallsResponse>, ErrorData> {
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
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

                    let cached_precise_graph = server
                        .precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                    let precise_coverage = cached_precise_graph.coverage_mode;
                    let graph = cached_precise_graph.graph;
                    precise_artifacts_ingested =
                        cached_precise_graph.ingest_stats.artifacts_ingested;
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
                    if precise_matches.len() > limit {
                        precise_matches.truncate(limit);
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
                                "incoming_count",
                                precise_matches.len(),
                            )
                        });
                        let (metadata, note) = Self::metadata_note_pair(metadata);
                        return Ok(Json(IncomingCallsResponse {
                            matches: precise_matches,
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
                        })
                        .collect::<Vec<_>>();
                    Self::sort_call_hierarchy_matches(&mut matches);
                    if matches.len() > limit {
                        matches.truncate(limit);
                    }

                    resolution_precision = Some("heuristic".to_owned());
                    match_count = matches.len();
                    let metadata = json!({
                        "precision": "heuristic",
                        "heuristic": true,
                        "fallback_reason": "precise_absent",
                        "precise_absence_reason": Self::precise_absence_reason(
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
                            "incoming_count",
                            0,
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    Ok(Json(IncomingCallsResponse {
                        matches,
                        metadata,
                        note,
                    }))
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

                NavigationToolExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    pub(super) async fn outgoing_calls_impl(
        &self,
        params: OutgoingCallsParams,
    ) -> Result<Json<OutgoingCallsResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("outgoing_calls", params.repository_id.clone());
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

                let result = (|| -> Result<Json<OutgoingCallsResponse>, ErrorData> {
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
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

                    let cached_precise_graph = server
                        .precise_graph_for_corpus(target_corpus.as_ref(), resource_budgets)?;
                    let precise_coverage = cached_precise_graph.coverage_mode;
                    let graph = cached_precise_graph.graph;
                    precise_artifacts_ingested =
                        cached_precise_graph.ingest_stats.artifacts_ingested;
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
                        let mut matches = graph
                            .precise_relationships_from_symbol(
                                &target_corpus.repository_id,
                                &precise_target.symbol,
                            )
                            .into_iter()
                            .filter(|relationship| {
                                relationship.kind == PreciseRelationshipKind::Reference
                            })
                            .filter_map(|relationship| {
                                let callee_symbol = graph
                                    .precise_symbol(
                                        &target_corpus.repository_id,
                                        &relationship.to_symbol,
                                    )?
                                    .clone();
                                if !Self::is_precise_callable_kind(&callee_symbol.kind) {
                                    return None;
                                }
                                let callee_definition =
                                    Self::precise_definition_occurrence_for_symbol(
                                        graph.as_ref(),
                                        &target_corpus.repository_id,
                                        &relationship.to_symbol,
                                    )?;
                                Some(CallHierarchyMatch {
                                    source_symbol: if precise_target.display_name.is_empty() {
                                        target.symbol.name.clone()
                                    } else {
                                        precise_target.display_name.clone()
                                    },
                                    target_symbol: if callee_symbol.display_name.is_empty() {
                                        callee_symbol.symbol
                                    } else {
                                        callee_symbol.display_name
                                    },
                                    repository_id: target_corpus.repository_id.clone(),
                                    path: Self::canonicalize_navigation_path(
                                        &target.root,
                                        &callee_definition.path,
                                    ),
                                    line: callee_definition.range.start_line,
                                    column: callee_definition.range.start_column,
                                    relation: "calls".to_owned(),
                                    precision: Some(
                                        Self::precise_match_precision(precise_coverage).to_owned(),
                                    ),
                                    call_path: None,
                                    call_line: None,
                                    call_column: None,
                                    call_end_line: None,
                                    call_end_column: None,
                                })
                            })
                            .collect::<Vec<_>>();
                        Self::sort_call_hierarchy_matches(&mut matches);
                        if !matches.is_empty() {
                            selected_precise_symbol = Some(precise_target.symbol.clone());
                            precise_matches = matches;
                            break;
                        }
                    }
                    if precise_matches.is_empty() {
                        for precise_target in &precise_targets {
                            let matches = Self::precise_outgoing_matches_from_occurrences(
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
                    if precise_matches.len() > limit {
                        precise_matches.truncate(limit);
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
                                "outgoing_count",
                                precise_matches.len(),
                            )
                        });
                        let (metadata, note) = Self::metadata_note_pair(metadata);
                        return Ok(Json(OutgoingCallsResponse {
                            matches: precise_matches,
                            metadata,
                            note,
                        }));
                    }

                    let mut matches = graph
                        .outgoing_adjacency(&target.symbol.stable_id)
                        .into_iter()
                        .filter(|adjacent| {
                            Self::is_heuristic_call_relation(adjacent.relation)
                                && Self::is_heuristic_callable_kind(&adjacent.symbol.kind)
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
                        })
                        .collect::<Vec<_>>();
                    Self::sort_call_hierarchy_matches(&mut matches);
                    if matches.len() > limit {
                        matches.truncate(limit);
                    }

                    resolution_precision = Some("heuristic".to_owned());
                    match_count = matches.len();
                    let metadata = json!({
                        "precision": "heuristic",
                        "heuristic": true,
                        "fallback_reason": "precise_absent",
                        "precise_absence_reason": Self::precise_absence_reason(
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
                            "outgoing_count",
                            0,
                        )
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);
                    Ok(Json(OutgoingCallsResponse {
                        matches,
                        metadata,
                        note,
                    }))
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

                NavigationToolExecution {
                    result,
                    provenance_result,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }
}
