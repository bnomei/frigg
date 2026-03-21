use super::*;

fn error_code_tag(error: &ErrorData) -> Option<&str> {
    error
        .data
        .as_ref()
        .and_then(|value| value.get("error_code"))
        .and_then(|value| value.as_str())
}

struct LoadedHeuristicReferences {
    references: Vec<HeuristicReference>,
    source_files_discovered: usize,
    source_read_diagnostics_count: usize,
    source_files_loaded: usize,
    source_bytes_loaded: u64,
}

impl FriggMcpServer {
    fn selected_target_reference_match(
        target_corpus: &RepositorySymbolCorpus,
        target_root: &Path,
        target_symbol: &SymbolDefinition,
        display_symbol: String,
        path: String,
        line: usize,
        column: usize,
        match_kind: ReferenceMatchKind,
        precision: Option<String>,
        fallback_reason: Option<String>,
    ) -> ReferenceMatch {
        let (container, signature) =
            Self::symbol_context_for_stable_id(target_corpus, &target_symbol.stable_id);
        ReferenceMatch {
            match_id: None,
            stable_symbol_id: Some(target_symbol.stable_id.clone()),
            repository_id: target_corpus.repository_id.clone(),
            symbol: display_symbol,
            path: if path.is_empty() {
                Self::relative_display_path(target_root, &target_symbol.path)
            } else {
                path
            },
            line,
            column,
            match_kind,
            precision,
            fallback_reason,
            container,
            signature,
            follow_up_structural: Vec::new(),
        }
    }

    fn heuristic_reference_probe_terms(
        target_corpus: &RepositorySymbolCorpus,
        target_symbol: &SymbolDefinition,
    ) -> Vec<String> {
        let mut terms = Vec::new();
        let mut seen = BTreeSet::new();
        let raw_name = target_symbol.name.trim();
        if !raw_name.is_empty() && seen.insert(raw_name.to_owned()) {
            terms.push(raw_name.to_owned());
        }
        if let Some(canonical_name) = target_corpus
            .canonical_symbol_name_by_stable_id
            .get(target_symbol.stable_id.as_str())
            .map(|value| value.trim())
            .filter(|value| !value.is_empty() && *value != raw_name)
            && seen.insert(canonical_name.to_owned())
        {
            terms.push(canonical_name.to_owned());
        }
        terms
    }

    fn heuristic_reference_candidate_paths(
        &self,
        target_corpus: &RepositorySymbolCorpus,
        target_symbol: &SymbolDefinition,
        resource_budgets: FindReferencesResourceBudgets,
    ) -> Vec<PathBuf> {
        let available_source_paths = target_corpus
            .source_paths
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut candidate_paths = BTreeSet::new();
        if available_source_paths.contains(&target_symbol.path) {
            candidate_paths.insert(target_symbol.path.clone());
        }

        let probe_terms = Self::heuristic_reference_probe_terms(target_corpus, target_symbol);
        if !probe_terms.is_empty() {
            let searcher = self.runtime_text_searcher((*self.config).clone());
            let limit = resource_budgets
                .source_max_files
                .saturating_mul(32)
                .clamp(256, 8_192);
            for term in probe_terms {
                let output = searcher.search_literal_with_filters_diagnostics(
                    SearchTextQuery {
                        query: term,
                        path_regex: None,
                        limit,
                    },
                    SearchFilters {
                        repository_id: Some(target_corpus.repository_id.clone()),
                        language: None,
                    },
                );
                let Ok(output) = output else {
                    continue;
                };
                for matched in output.matches {
                    let absolute_path = target_corpus.root.join(&matched.path);
                    if available_source_paths.contains(&absolute_path) {
                        candidate_paths.insert(absolute_path);
                    }
                }
            }
        }

        if candidate_paths.len() <= 1 {
            let full_sweep_limit = resource_budgets.source_max_files.min(64);
            if target_corpus.source_paths.len() <= full_sweep_limit {
                return target_corpus.source_paths.clone();
            }
        }

        candidate_paths.into_iter().collect()
    }

    fn load_heuristic_references(
        &self,
        target_corpus: &RepositorySymbolCorpus,
        target_symbol: &SymbolDefinition,
        graph: &SymbolGraph,
        heuristic_scip_signature: String,
        resource_budgets: FindReferencesResourceBudgets,
    ) -> Result<LoadedHeuristicReferences, ErrorData> {
        let mut resolver = HeuristicReferenceResolver::new(
            &target_corpus.repository_id,
            &target_symbol.stable_id,
            &target_corpus.symbols,
            graph,
        )
        .ok_or_else(|| {
            Self::internal(
                "failed to initialize heuristic resolver for selected symbol",
                Some(json!({
                    "repository_id": target_corpus.repository_id,
                    "symbol_id": target_symbol.stable_id,
                })),
            )
        })?;
        let heuristic_cache_key = HeuristicReferenceCacheKey {
            repository_id: target_corpus.repository_id.clone(),
            symbol_id: target_symbol.stable_id.clone(),
            corpus_signature: target_corpus.root_signature.clone(),
            scip_signature: heuristic_scip_signature,
        };

        if let Some(cached) = self.cached_heuristic_references(&heuristic_cache_key) {
            return Ok(LoadedHeuristicReferences {
                references: (*cached.references).clone(),
                source_files_discovered: cached.source_files_discovered,
                source_read_diagnostics_count: cached.source_read_diagnostics_count,
                source_files_loaded: cached.source_files_loaded,
                source_bytes_loaded: cached.source_bytes_loaded,
            });
        }

        let candidate_source_paths = self.heuristic_reference_candidate_paths(
            target_corpus,
            target_symbol,
            resource_budgets,
        );
        let source_files_discovered = candidate_source_paths.len();
        let mut source_read_diagnostics_count = 0usize;
        let mut source_files_loaded = 0usize;
        let mut source_bytes_loaded = 0u64;
        let source_started_at = Instant::now();
        let source_max_elapsed = Duration::from_millis(resource_budgets.source_max_elapsed_ms);
        let source_max_file_bytes = Self::usize_to_u64(resource_budgets.source_max_file_bytes);
        let source_max_total_bytes = Self::usize_to_u64(resource_budgets.source_max_total_bytes);

        for (index, path) in candidate_source_paths.iter().enumerate() {
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
                let projected_total = source_bytes_loaded.saturating_add(pre_read_bytes);
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
                    let projected_total = source_bytes_loaded.saturating_add(source_bytes);
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

        let references = resolver.finish();
        self.cache_heuristic_references(
            heuristic_cache_key,
            references.clone(),
            source_files_discovered,
            source_read_diagnostics_count,
            source_files_loaded,
            source_bytes_loaded,
        );
        Ok(LoadedHeuristicReferences {
            references,
            source_files_discovered,
            source_read_diagnostics_count,
            source_files_loaded,
            source_bytes_loaded,
        })
    }

    pub(in crate::mcp::server) async fn find_references_impl(
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
                let include_definition = params_for_blocking.include_definition.unwrap_or(true);
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
                    match Self::resolve_navigation_target(
                        &corpora,
                        params_for_blocking.symbol.as_deref(),
                        None,
                        None,
                        None,
                        params_for_blocking.repository_id.as_deref(),
                    ) {
                        Ok(resolved_target) => resolved_target,
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
                                precise_artifacts_discovered =
                                    direct_precise_target.ingest_stats.artifacts_discovered;
                                precise_artifacts_discovered_bytes =
                                    direct_precise_target.ingest_stats.artifacts_discovered_bytes;
                                precise_artifacts_ingested =
                                    direct_precise_target.ingest_stats.artifacts_ingested;
                                precise_artifacts_ingested_bytes =
                                    direct_precise_target.ingest_stats.artifacts_ingested_bytes;
                                precise_artifacts_failed =
                                    direct_precise_target.ingest_stats.artifacts_failed;
                                precise_artifacts_failed_bytes =
                                    direct_precise_target.ingest_stats.artifacts_failed_bytes;
                                source_files_discovered =
                                    direct_precise_target.source_files_discovered;

                                let precise_references = if include_definition {
                                    direct_precise_target.graph.precise_occurrences_for_symbol(
                                        &direct_precise_target.repository_id,
                                        &direct_precise_target.precise_target.symbol,
                                    )
                                } else {
                                    direct_precise_target.graph.precise_references_for_symbol(
                                        &direct_precise_target.repository_id,
                                        &direct_precise_target.precise_target.symbol,
                                    )
                                };
                                precise_reference_count = precise_references.len();
                                total_matches = precise_reference_count;

                                let display_symbol = if direct_precise_target
                                    .precise_target
                                    .display_name
                                    .is_empty()
                                {
                                    symbol_query.clone()
                                } else {
                                    direct_precise_target.precise_target.display_name.clone()
                                };
                                let mut matches = precise_references
                                    .into_iter()
                                    .take(limit)
                                    .map(|reference| ReferenceMatch {
                                        match_id: None,
                                        stable_symbol_id: None,
                                        repository_id: direct_precise_target.repository_id.clone(),
                                        symbol: display_symbol.clone(),
                                        path: Self::canonicalize_navigation_path(
                                            &direct_precise_target.root,
                                            &reference.path,
                                        ),
                                        line: reference.range.start_line,
                                        column: reference.range.start_column,
                                        match_kind: if reference.is_definition() {
                                            ReferenceMatchKind::Definition
                                        } else {
                                            ReferenceMatchKind::Reference
                                        },
                                        precision: Some(
                                            Self::precise_match_precision(
                                                direct_precise_target.coverage_mode,
                                            )
                                            .to_owned(),
                                        ),
                                        fallback_reason: None,
                                        container: None,
                                        signature: None,
                                        follow_up_structural: Vec::new(),
                                    })
                                    .collect::<Vec<_>>();
                                if params_for_blocking.include_follow_up_structural == Some(true) {
                                    Self::populate_reference_match_follow_up_structural(
                                        &direct_precise_target.root,
                                        &mut matches,
                                    );
                                }

                                let precision = Self::precise_resolution_precision(
                                    direct_precise_target.coverage_mode,
                                );
                                resolution_precision = Some(precision.to_owned());
                                let metadata = json!({
                                    "precision": precision,
                                    "heuristic": false,
                                    "target_precise_symbol": direct_precise_target.precise_target.symbol,
                                    "resolution_source": resolution_source.clone(),
                                    "diagnostics_count": diagnostics_count,
                                    "diagnostics": {
                                        "manifest_walk": manifest_walk_diagnostics_count,
                                        "manifest_read": manifest_read_diagnostics_count,
                                        "symbol_extraction": symbol_extraction_diagnostics_count,
                                        "source_read": source_read_diagnostics_count,
                                        "total": diagnostics_count,
                                    },
                                    "precise": Self::precise_note_with_count(
                                        direct_precise_target.coverage_mode,
                                        &direct_precise_target.ingest_stats,
                                        "reference_count",
                                        precise_reference_count,
                                    ),
                                    "resource_budgets": resource_budget_metadata_for_blocking.clone(),
                                    "resource_usage": {
                                        "scip": {
                                            "artifacts_discovered": direct_precise_target.ingest_stats.artifacts_discovered,
                                            "artifacts_discovered_bytes": direct_precise_target.ingest_stats.artifacts_discovered_bytes,
                                            "artifacts_ingested": direct_precise_target.ingest_stats.artifacts_ingested,
                                            "artifacts_ingested_bytes": direct_precise_target.ingest_stats.artifacts_ingested_bytes,
                                            "artifacts_failed": direct_precise_target.ingest_stats.artifacts_failed,
                                            "artifacts_failed_bytes": direct_precise_target.ingest_stats.artifacts_failed_bytes,
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
                                    result_handle: None,
                                    mode: FriggMcpServer::navigation_mode_from_precision_label(
                                        Some(precision),
                                    ),
                                    target_selection: None,
                                    metadata,
                                    note,
                                }));
                            }
                            return Err(error);
                        }
                        Err(error) => return Err(error),
                    }
                };
                resolution_source = Some(resolved_target.resolution_source.to_owned());
                let symbol_query = resolved_target.symbol_query;
                let target_selection = Some(Self::navigation_target_selection_summary_for_selection(
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
                            "resolution_source": resolution_source.clone(),
                            "target_selection": Self::navigation_target_selection_summary_value(
                                target_selection
                                    .as_ref()
                                    .expect("target selection summary should be present"),
                            ),
                            "diagnostics_count": diagnostics_count,
                            "diagnostics": {
                                "manifest_walk": manifest_walk_diagnostics_count,
                                "manifest_read": manifest_read_diagnostics_count,
                                "symbol_extraction": symbol_extraction_diagnostics_count,
                                "source_read": source_read_diagnostics_count,
                                "total": diagnostics_count,
                            },
                            "resource_budgets": resource_budget_metadata_for_blocking.clone(),
                        });
                        let (metadata, note) = Self::metadata_note_pair(metadata);
                        return Ok(Json(FindReferencesResponse {
                            total_matches: 0,
                            matches: Vec::new(),
                            result_handle: None,
                            mode: NavigationMode::UnavailableNoPrecise,
                            target_selection,
                            metadata,
                            note,
                        }));
                    }
                };
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
                        if include_definition {
                            graph.precise_occurrences_for_symbol(
                                &target_corpus.repository_id,
                                &precise_target.symbol,
                            )
                        } else {
                            graph.precise_references_for_symbol(
                                &target_corpus.repository_id,
                                &precise_target.symbol,
                            )
                        }
                    })
                    .unwrap_or_default();
                precise_reference_count = precise_references.len();

                if !precise_references.is_empty() {
                    let mut matches = precise_references
                        .into_iter()
                        .take(limit)
                        .map(|reference| {
                            let reference_path = PathBuf::from(&reference.path);
                            let absolute_path = if reference_path.is_absolute() {
                                reference_path
                            } else {
                                target.root.join(reference_path)
                            };

                            Self::selected_target_reference_match(
                                target_corpus.as_ref(),
                                &target.root,
                                &target.symbol,
                                precise_target
                                    .as_ref()
                                    .map(|selected| selected.display_name.clone())
                                    .filter(|display_name| !display_name.is_empty())
                                    .unwrap_or_else(|| target.symbol.name.clone()),
                                Self::relative_display_path(&target.root, &absolute_path),
                                reference.range.start_line,
                                reference.range.start_column,
                                if reference.is_definition() {
                                    ReferenceMatchKind::Definition
                                } else {
                                    ReferenceMatchKind::Reference
                                },
                                Some(Self::precise_match_precision(precise_coverage).to_owned()),
                                None,
                            )
                        })
                        .collect::<Vec<_>>();
                    let should_supplement_php_method_references =
                        target.symbol.language == SymbolLanguage::Php
                            && target.symbol.kind.as_str() == "method";
                    let mut heuristic_supplement_match_count = 0usize;
                    let mut heuristic_supplement_error: Option<Value> = None;
                    if should_supplement_php_method_references {
                        match server.load_heuristic_references(
                            target_corpus.as_ref(),
                            &target.symbol,
                            graph.as_ref(),
                            heuristic_scip_signature.clone(),
                            resource_budgets,
                        ) {
                            Ok(loaded) => {
                                source_files_discovered = loaded.source_files_discovered;
                                source_read_diagnostics_count =
                                    loaded.source_read_diagnostics_count;
                                source_files_loaded = loaded.source_files_loaded;
                                source_bytes_loaded = loaded.source_bytes_loaded;
                                diagnostics_count += source_read_diagnostics_count;

                                let mut existing_match_locations = matches
                                    .iter()
                                    .map(|matched| {
                                        (
                                            matched.repository_id.clone(),
                                            matched.path.clone(),
                                            matched.line,
                                            matched.column,
                                        )
                                    })
                                    .collect::<BTreeSet<_>>();
                                let mut supplemental_matches = loaded
                                    .references
                                    .into_iter()
                                    .filter(|reference| {
                                        matches!(
                                            supported_language_for_path(
                                                &reference.path,
                                                LanguageCapability::StructuralSearch,
                                            ),
                                            Some(SymbolLanguage::Php | SymbolLanguage::Blade)
                                        )
                                    })
                                    .filter_map(|reference| {
                                        let relative_path = Self::relative_display_path(
                                            &target.root,
                                            &reference.path,
                                        );
                                        let key = (
                                            reference.repository_id.clone(),
                                            relative_path.clone(),
                                            reference.line,
                                            reference.column,
                                        );
                                        if !existing_match_locations.insert(key) {
                                            return None;
                                        }

                                        let (container, signature) = Self::symbol_context_for_stable_id(
                                            target_corpus.as_ref(),
                                            &target.symbol.stable_id,
                                        );
                                        Some(ReferenceMatch {
                                            match_id: None,
                                            stable_symbol_id: Some(target.symbol.stable_id.clone()),
                                            repository_id: reference.repository_id,
                                            symbol: reference.symbol_name,
                                            path: relative_path,
                                            line: reference.line,
                                            column: reference.column,
                                            match_kind: ReferenceMatchKind::Reference,
                                            precision: Some("heuristic".to_owned()),
                                            fallback_reason: Some(
                                                "precise_supplemented".to_owned(),
                                            ),
                                            container,
                                            signature,
                                            follow_up_structural: Vec::new(),
                                        })
                                    })
                                    .collect::<Vec<_>>();
                                heuristic_supplement_match_count = supplemental_matches.len();
                                let remaining = limit.saturating_sub(matches.len());
                                if supplemental_matches.len() > remaining {
                                    supplemental_matches.truncate(remaining);
                                }
                                matches.extend(supplemental_matches);
                            }
                            Err(error) => {
                                heuristic_supplement_error = Some(json!({
                                    "code": error_code_tag(&error),
                                    "message": error.message,
                                    "data": error.data,
                                }));
                            }
                        }
                    }
                    if params_for_blocking.include_follow_up_structural == Some(true) {
                        Self::populate_reference_match_follow_up_structural(
                            &target.root,
                            &mut matches,
                        );
                    }
                    total_matches = precise_reference_count + heuristic_supplement_match_count;

                    let precision = if heuristic_supplement_match_count > 0 {
                        "precise_partial"
                    } else {
                        Self::precise_resolution_precision(precise_coverage)
                    };
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
                        "heuristic_supplement": if should_supplement_php_method_references {
                            Some(json!({
                                "eligible": true,
                                "applied": heuristic_supplement_match_count > 0,
                                "match_count": heuristic_supplement_match_count,
                                "error": heuristic_supplement_error,
                            }))
                        } else {
                            None
                        },
                    });
                    let (metadata, note) = Self::metadata_note_pair(metadata);

                    return Ok(Json(FindReferencesResponse {
                        total_matches,
                        matches,
                        result_handle: None,
                        mode: FriggMcpServer::navigation_mode_from_precision_label(Some(
                            precision,
                        )),
                        target_selection: target_selection.clone(),
                        metadata,
                        note,
                    }));
                }

                let loaded = server.load_heuristic_references(
                    target_corpus.as_ref(),
                    &target.symbol,
                    graph.as_ref(),
                    heuristic_scip_signature,
                    resource_budgets,
                )?;
                source_files_discovered = loaded.source_files_discovered;
                source_read_diagnostics_count = loaded.source_read_diagnostics_count;
                source_files_loaded = loaded.source_files_loaded;
                source_bytes_loaded = loaded.source_bytes_loaded;
                let all_references = loaded.references;
                total_matches = all_references.len() + usize::from(include_definition);
                let references = all_references.into_iter().take(limit).collect::<Vec<_>>();

                let mut high_confidence = 0usize;
                let mut medium_confidence = 0usize;
                let mut low_confidence = 0usize;
                let mut graph_evidence = 0usize;
                let mut lexical_evidence = 0usize;

                let mut matches = Vec::new();
                if include_definition {
                    matches.push(Self::selected_target_reference_match(
                        target_corpus.as_ref(),
                        &target.root,
                        &target.symbol,
                        target.symbol.name.clone(),
                        String::new(),
                        target.symbol.line,
                        1,
                        ReferenceMatchKind::Definition,
                        Some("heuristic".to_owned()),
                        Some("precise_absent".to_owned()),
                    ));
                }

                matches.extend(references.iter().map(|reference| {
                        match reference.confidence {
                            HeuristicReferenceConfidence::High => high_confidence += 1,
                            HeuristicReferenceConfidence::Medium => medium_confidence += 1,
                            HeuristicReferenceConfidence::Low => low_confidence += 1,
                        }
                        match &reference.evidence {
                            HeuristicReferenceEvidence::GraphRelation { .. } => graph_evidence += 1,
                            HeuristicReferenceEvidence::LexicalToken => lexical_evidence += 1,
                        }

                        Self::selected_target_reference_match(
                            target_corpus.as_ref(),
                            &target.root,
                            &target.symbol,
                            reference.symbol_name.clone(),
                            Self::relative_display_path(&target.root, &reference.path),
                            reference.line,
                            reference.column,
                            ReferenceMatchKind::Reference,
                            Some("heuristic".to_owned()),
                            Some("precise_absent".to_owned()),
                        )
                    }));
                if matches.len() > limit {
                    matches.truncate(limit);
                }
                if params_for_blocking.include_follow_up_structural == Some(true) {
                    Self::populate_reference_match_follow_up_structural(&target.root, &mut matches);
                }

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
                    result_handle: None,
                    mode: NavigationMode::HeuristicNoPrecise,
                    target_selection: target_selection.clone(),
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

        let result = execution.result.map(|Json(response)| {
            Json(self.present_find_references_response(response, params.response_mode))
        });
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }
}
