use super::*;
use crate::path_class::classify_repository_path;

impl FriggMcpServer {
    pub(crate) async fn search_hybrid_impl(
        &self,
        params: SearchHybridParams,
    ) -> Result<Json<SearchHybridResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("search_hybrid", params.repository_id.clone());
        let execution_context_for_blocking = execution_context.clone();
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self
            .run_read_only_tool_blocking(&execution_context, move || {
                let mut scoped_repository_ids: Vec<String> = Vec::new();
                let mut effective_limit: Option<usize> = None;
                let mut effective_weights: Option<SearchHybridChannelWeightsParams> = None;
                let mut diagnostics_count = 0usize;
                let mut walk_diagnostics_count = 0usize;
                let mut read_diagnostics_count = 0usize;
                let mut semantic_requested: Option<bool> = None;
                let mut semantic_enabled: Option<bool> = None;
                let mut semantic_status: Option<ChannelHealthStatus> = None;
                let mut semantic_reason: Option<String> = None;
                let mut semantic_candidate_count: Option<usize> = None;
                let mut semantic_hit_count: Option<usize> = None;
                let mut semantic_match_count: Option<usize> = None;
                let mut warning: Option<String> = None;
                let mut channel_metadata: Option<BTreeMap<String, SearchHybridChannelMetadata>> =
                    None;
                let mut stage_attribution: Option<crate::searcher::SearchStageAttribution> = None;
                let mut match_anchors: Option<Value> = None;
                let mut response_source_refs = json!({});
                let result = (|| -> Result<Json<SearchHybridResponse>, ErrorData> {
                    let query = params_for_blocking.query.trim().to_owned();
                    if query.is_empty() {
                        return Err(Self::invalid_params("query must not be empty", None));
                    }
                    let limit = params_for_blocking
                        .limit
                        .unwrap_or(server.config.max_search_results)
                        .min(server.config.max_search_results.max(1));
                    effective_limit = Some(limit);

                    let scoped_execution_context = server.scoped_read_only_tool_execution_context(
                        execution_context_for_blocking.tool_name,
                        execution_context_for_blocking.repository_hint.clone(),
                        RepositoryResponseCacheFreshnessMode::SemanticAware,
                    )?;
                    let scoped_workspaces = scoped_execution_context.scoped_workspaces.clone();
                    scoped_repository_ids = scoped_execution_context.scoped_repository_ids.clone();
                    let (scoped_config, repository_id_map) =
                        server.scoped_search_config(&scoped_workspaces);

                    let weights = {
                        let mut weights = HybridChannelWeights::default();
                        if let Some(overrides) = params_for_blocking.weights.clone() {
                            if let Some(lexical) = overrides.lexical {
                                weights.lexical = lexical;
                            }
                            if let Some(graph) = overrides.graph {
                                weights.graph = graph;
                            }
                            if let Some(semantic) = overrides.semantic {
                                weights.semantic = semantic;
                            }
                        }
                        effective_weights = Some(SearchHybridChannelWeightsParams {
                            lexical: Some(weights.lexical),
                            graph: Some(weights.graph),
                            semantic: Some(weights.semantic),
                        });
                        weights
                    };
                    let cache_freshness = scoped_execution_context.cache_freshness.clone();
                    let cache_key = cache_freshness.scopes.as_ref().map(|freshness_scopes| {
                        SearchHybridResponseCacheKey {
                            scoped_repository_ids: scoped_repository_ids.clone(),
                            freshness_scopes: freshness_scopes.clone(),
                            query: query.clone(),
                            language: params_for_blocking.language.clone(),
                            limit,
                            semantic: params_for_blocking.semantic,
                            lexical_weight_bits: weights.lexical.to_bits(),
                            graph_weight_bits: weights.graph.to_bits(),
                            semantic_weight_bits: weights.semantic.to_bits(),
                        }
                    });
                    if cache_key.is_none() {
                        server.record_runtime_cache_event(
                            RuntimeCacheFamily::SearchHybridResponse,
                            RuntimeCacheEvent::Bypass,
                            1,
                        );
                    }
                    if let Some(cache_key) = cache_key.as_ref()
                        && let Some(cached) = server.cached_search_hybrid_response(cache_key)
                    {
                        response_source_refs = cached.source_refs.clone();
                        return Ok(Json(server.present_search_hybrid_response(
                            cached.response,
                            params_for_blocking.response_mode,
                        )));
                    }

                    let searcher = server.runtime_text_searcher(scoped_config);
                    let search_output = searcher
                        .search_hybrid_with_filters(
                            SearchHybridQuery {
                                query,
                                limit,
                                weights,
                                semantic: params_for_blocking.semantic,
                            },
                            SearchFilters {
                                repository_id: None,
                                language: params_for_blocking.language.clone(),
                            },
                        )
                        .map_err(Self::map_frigg_error)?;

                    diagnostics_count = search_output.diagnostics.total_count();
                    walk_diagnostics_count = search_output
                        .diagnostics
                        .count_by_kind(SearchDiagnosticKind::Walk);
                    read_diagnostics_count = search_output
                        .diagnostics
                        .count_by_kind(SearchDiagnosticKind::Read);
                    semantic_requested = Some(
                        params_for_blocking
                            .semantic
                            .unwrap_or(server.config.semantic_runtime.enabled),
                    );
                    let semantic_channel = Self::search_hybrid_channel_result(
                        &search_output.channel_results,
                        EvidenceChannel::Semantic,
                    );
                    semantic_enabled =
                        Some(semantic_channel.is_some_and(|result| result.stats.match_count > 0));
                    semantic_status = semantic_channel
                        .map(|result| Self::search_hybrid_semantic_status(result.health.status));
                    semantic_reason =
                        semantic_channel.and_then(|result| result.health.reason.clone());
                    semantic_candidate_count =
                        semantic_channel.map(|result| result.stats.candidate_count);
                    semantic_hit_count = semantic_channel.map(|result| result.stats.hit_count);
                    semantic_match_count = semantic_channel.map(|result| result.stats.match_count);
                    warning = Self::search_hybrid_warning(
                        &params_for_blocking.query,
                        search_output.note.lexical_only_mode,
                        semantic_status,
                        semantic_reason.as_deref(),
                        semantic_hit_count,
                        semantic_match_count,
                    );
                    let semantic_language_capability =
                        params_for_blocking.language.as_deref().map(|raw_language| {
                            Self::search_hybrid_language_capability_metadata(
                                raw_language,
                                semantic_status,
                                semantic_reason.as_deref(),
                            )
                        });
                    channel_metadata = Some(Self::search_hybrid_channels_metadata(
                        &search_output.channel_results,
                    ));
                    stage_attribution = search_output.stage_attribution.clone();

                    let mut matches = search_output
                        .matches
                        .into_iter()
                        .map(Self::search_hybrid_match_from_evidence)
                        .collect::<Vec<_>>();
                    for found in &mut matches {
                        if let Some(actual_repository_id) =
                            repository_id_map.get(&found.repository_id)
                        {
                            found.repository_id = actual_repository_id.clone();
                        }
                    }
                    match_anchors = Some(Self::search_hybrid_provenance_match_summary(&matches));

                    let metadata = Some(SearchHybridMetadata {
                        channels: channel_metadata.clone().unwrap_or_default(),
                        lexical_backend: Self::search_lexical_backend_metadata(
                            search_output.note.lexical_backend,
                        ),
                        lexical_backend_note: search_output.note.lexical_backend_note.clone(),
                        semantic_requested,
                        semantic_enabled,
                        semantic_status,
                        semantic_reason: semantic_reason.clone(),
                        semantic_candidate_count,
                        semantic_hit_count,
                        semantic_match_count,
                        lexical_only_mode: Some(search_output.note.lexical_only_mode),
                        warning: warning.clone(),
                        diagnostics_count,
                        diagnostics: SearchHybridDiagnosticsSummary {
                            walk: walk_diagnostics_count,
                            read: read_diagnostics_count,
                            total: diagnostics_count,
                        },
                        stage_attribution: stage_attribution
                            .as_ref()
                            .map(SearchHybridStageAttribution::from),
                        semantic_capability: semantic_language_capability.clone(),
                        utility: Some(Self::search_hybrid_utility_summary(&matches)),
                        freshness_basis: serde_json::from_value(cache_freshness.basis.clone())
                            .expect("search_hybrid freshness basis should deserialize"),
                    });

                    let response = SearchHybridResponse {
                        matches,
                        result_handle: None,
                        semantic_requested: None,
                        semantic_enabled: None,
                        semantic_status: None,
                        semantic_reason: None,
                        semantic_hit_count: None,
                        semantic_match_count: None,
                        warning: None,
                        metadata,
                        note: None,
                    };
                    let mut response_source_refs_value = serde_json::to_value(
                        response
                            .metadata
                            .as_ref()
                            .expect("search_hybrid metadata should exist"),
                    )
                    .expect("search_hybrid metadata should serialize");
                    response_source_refs_value
                        .as_object_mut()
                        .expect("search_hybrid source refs should be an object")
                        .insert(
                            "scoped_repository_ids".to_owned(),
                            json!(scoped_repository_ids.clone()),
                        );
                    response_source_refs_value
                        .as_object_mut()
                        .expect("search_hybrid source refs should be an object")
                        .insert(
                            "matches".to_owned(),
                            match_anchors.clone().unwrap_or_else(|| json!([])),
                        );
                    response_source_refs = response_source_refs_value;
                    if let Some(cache_key) = cache_key {
                        server.cache_search_hybrid_response(
                            cache_key,
                            &response,
                            &response_source_refs,
                        );
                    }

                    Ok(Json(server.present_search_hybrid_response(
                        response,
                        params_for_blocking.response_mode,
                    )))
                })();
                let fallback_reason =
                    if matches!(semantic_status, Some(ChannelHealthStatus::Unavailable)) {
                        Self::provenance_fallback_reason_from_label(Some("semantic_unavailable"))
                    } else if matches!(semantic_status, Some(ChannelHealthStatus::Disabled)) {
                        Self::provenance_fallback_reason_from_label(Some("unsupported_feature"))
                    } else if matches!(semantic_status, Some(ChannelHealthStatus::Degraded)) {
                        Self::provenance_fallback_reason_from_label(Some("stage_filtered"))
                    } else {
                        Self::provenance_fallback_reason_from_label(None)
                    };
                let fallback_reason_detail = semantic_reason.clone();
                let precision_mode = if fallback_reason.is_some() {
                    WorkloadPrecisionMode::Fallback
                } else {
                    WorkloadPrecisionMode::Precise
                };
                let normalized_workload = FriggMcpServer::provenance_normalized_workload_metadata(
                    "search_hybrid",
                    &scoped_repository_ids,
                    precision_mode,
                    fallback_reason,
                    fallback_reason_detail,
                    stage_attribution.as_ref(),
                );
                let finalization = server.tool_execution_finalization(
                    response_source_refs.clone(),
                    Some(normalized_workload),
                );
                let provenance_result = server.record_provenance_with_outcome_and_metadata(
                    "search_hybrid",
                    execution_context_for_blocking.repository_hint.as_deref(),
                    json!({
                        "repository_id": execution_context_for_blocking.repository_hint.clone(),
                        "query": Self::bounded_text(&params_for_blocking.query),
                        "language": params_for_blocking
                            .language
                            .as_ref()
                            .map(|language| Self::bounded_text(language)),
                        "limit": params_for_blocking.limit,
                        "effective_limit": effective_limit,
                        "semantic": params_for_blocking.semantic,
                        "weights": effective_weights.clone(),
                    }),
                    finalization.source_refs,
                    Self::provenance_outcome(&result),
                    finalization.normalized_workload,
                );

                SearchHybridExecution {
                    result,
                    provenance_result,
                    scoped_repository_ids,
                    effective_limit,
                    effective_weights,
                    diagnostics_count,
                    walk_diagnostics_count,
                    read_diagnostics_count,
                    semantic_requested,
                    semantic_enabled,
                    semantic_status,
                    semantic_reason,
                    semantic_candidate_count,
                    semantic_hit_count,
                    semantic_match_count,
                    warning,
                    channel_metadata,
                    match_anchors,
                }
            })
            .await?;

        let result = execution.result;
        self.finalize_read_only_tool(&execution_context, result, execution.provenance_result)
    }

    pub(crate) fn search_hybrid_semantic_status(
        status: ChannelHealthStatus,
    ) -> ChannelHealthStatus {
        match status {
            ChannelHealthStatus::Filtered => ChannelHealthStatus::Disabled,
            other => other,
        }
    }

    pub(crate) fn search_hybrid_warning(
        query: &str,
        lexical_only_mode: bool,
        semantic_status: Option<ChannelHealthStatus>,
        semantic_reason: Option<&str>,
        semantic_hit_count: Option<usize>,
        semantic_match_count: Option<usize>,
    ) -> Option<String> {
        let broad_natural_language =
            lexical_only_mode && Self::search_hybrid_query_looks_broad_natural_language(query);
        match semantic_status {
            Some(ChannelHealthStatus::Disabled) => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "{} ({reason})",
                    if broad_natural_language {
                        "semantic retrieval is disabled; broad natural-language ranking is weaker in lexical-only mode, so use results as candidate pivots and switch to exact tools"
                    } else {
                        "semantic retrieval is disabled; results are ranked from lexical and graph signals only"
                    }
                ),
                _ => {
                    if broad_natural_language {
                        "semantic retrieval is disabled; broad natural-language ranking is weaker in lexical-only mode, so use results as candidate pivots and switch to exact tools".to_owned()
                    } else {
                        "semantic retrieval is disabled; results are ranked from lexical and graph signals only".to_owned()
                    }
                }
            }),
            Some(ChannelHealthStatus::Unavailable) => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "{} ({reason})",
                    if broad_natural_language {
                        "semantic retrieval is unavailable; broad natural-language ranking is weaker in lexical-only mode, so use results as candidate pivots and switch to exact tools"
                    } else {
                        "semantic retrieval is unavailable; results are ranked from lexical and graph signals only"
                    }
                ),
                _ => {
                    if broad_natural_language {
                        "semantic retrieval is unavailable; broad natural-language ranking is weaker in lexical-only mode, so use results as candidate pivots and switch to exact tools".to_owned()
                    } else {
                        "semantic retrieval is unavailable; results are ranked from lexical and graph signals only".to_owned()
                    }
                }
            }),
            Some(ChannelHealthStatus::Degraded) => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "semantic retrieval is degraded; semantic contribution may be partial ({reason})"
                ),
                _ => "semantic retrieval is degraded; semantic contribution may be partial".to_owned(),
            }),
            Some(ChannelHealthStatus::Ok) if semantic_hit_count == Some(0) => Some(
                "semantic retrieval completed successfully but retained no query-relevant semantic hits; results are ranked from lexical and graph signals only"
                    .to_owned(),
            ),
            Some(ChannelHealthStatus::Ok)
                if semantic_hit_count.unwrap_or(0) > 0
                    && semantic_match_count == Some(0) =>
            {
                Some(
                    "semantic retrieval retained semantic hits, but none contributed to the returned top results; ranking is effectively lexical and graph for this result set"
                        .to_owned(),
                )
            }
            _ => None,
        }
    }

    fn search_hybrid_query_looks_broad_natural_language(query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.len() < 18 || !trimmed.contains(char::is_whitespace) {
            return false;
        }
        if trimmed.contains("::")
            || trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.contains('_')
            || trimmed.contains('.')
            || trimmed.contains('#')
            || trimmed.contains("->")
        {
            return false;
        }

        let mut token_count = 0usize;
        let mut alphabetic_like_count = 0usize;
        for token in trimmed.split_whitespace() {
            token_count += 1;
            let cleaned = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '-');
            if !cleaned.is_empty()
                && cleaned
                    .chars()
                    .all(|ch| ch.is_ascii_alphabetic() || ch == '-')
            {
                alphabetic_like_count += 1;
            }
        }

        token_count >= 4 && alphabetic_like_count + 1 >= token_count
    }

    fn search_hybrid_semantic_accelerator_state(
        language: SymbolLanguage,
        semantic_status: Option<ChannelHealthStatus>,
        semantic_reason: Option<&str>,
    ) -> &'static str {
        if language
            .capability_tier(LanguageSupportCapability::SemanticChunking)
            .as_str()
            == "unsupported"
        {
            return "unsupported_language";
        }

        match semantic_status {
            Some(ChannelHealthStatus::Disabled) => match semantic_reason {
                Some("semantic channel disabled by request toggle") => "disabled_by_request",
                _ => "disabled_in_config",
            },
            Some(ChannelHealthStatus::Unavailable) => "repository_unavailable",
            Some(ChannelHealthStatus::Degraded) => "degraded_runtime",
            Some(ChannelHealthStatus::Ok) => "active",
            _ => "eligible",
        }
    }

    fn search_hybrid_language_capability_metadata(
        raw_language: &str,
        semantic_status: Option<ChannelHealthStatus>,
        semantic_reason: Option<&str>,
    ) -> SearchHybridLanguageCapabilityMetadata {
        let requested_language = raw_language.trim().to_ascii_lowercase();
        let Some(language) = SymbolLanguage::parse_alias(&requested_language) else {
            return SearchHybridLanguageCapabilityMetadata {
                requested_language,
                display_name: None,
                semantic_chunking: "unknown_filter_value".to_owned(),
                semantic_accelerator: SearchHybridSemanticAcceleratorMetadata {
                    tier: "unknown_filter_value".to_owned(),
                    state: "unknown_filter_value".to_owned(),
                    status: None,
                    reason: None,
                },
                capabilities: BTreeMap::new(),
            };
        };

        let mut capabilities = BTreeMap::new();
        for capability in LanguageSupportCapability::ALL {
            capabilities.insert(
                capability.as_str().to_owned(),
                language.capability_tier(capability).as_str().to_owned(),
            );
        }

        SearchHybridLanguageCapabilityMetadata {
            requested_language: language.as_str().to_owned(),
            display_name: Some(language.display_name().to_owned()),
            semantic_chunking: language
                .capability_tier(LanguageSupportCapability::SemanticChunking)
                .as_str()
                .to_owned(),
            semantic_accelerator: SearchHybridSemanticAcceleratorMetadata {
                tier: language
                    .capability_tier(LanguageSupportCapability::SemanticChunking)
                    .as_str()
                    .to_owned(),
                state: Self::search_hybrid_semantic_accelerator_state(
                    language,
                    semantic_status,
                    semantic_reason,
                )
                .to_owned(),
                status: semantic_status,
                reason: semantic_reason.map(ToOwned::to_owned),
            },
            capabilities,
        }
    }

    fn search_hybrid_channel_result(
        channel_results: &[ChannelResult],
        channel: EvidenceChannel,
    ) -> Option<&ChannelResult> {
        channel_results
            .iter()
            .find(|result| result.channel == channel)
    }

    fn search_hybrid_channels_metadata(
        channel_results: &[ChannelResult],
    ) -> BTreeMap<String, SearchHybridChannelMetadata> {
        let mut channels = BTreeMap::new();
        for result in channel_results {
            let diagnostics = result
                .diagnostics
                .iter()
                .map(|diagnostic| SearchHybridChannelDiagnostic {
                    code: diagnostic.code.clone(),
                    message: Self::bounded_text(&diagnostic.message),
                })
                .collect::<Vec<_>>();
            channels.insert(
                result.channel.as_str().to_owned(),
                SearchHybridChannelMetadata {
                    status: result.health.status,
                    reason: result
                        .health
                        .reason
                        .as_ref()
                        .map(|reason| Self::bounded_text(reason)),
                    candidate_count: result.stats.candidate_count,
                    hit_count: result.stats.hit_count,
                    match_count: result.stats.match_count,
                    diagnostic_count: result.diagnostics.len(),
                    diagnostics,
                },
            );
        }
        channels
    }

    fn search_hybrid_provenance_match_summary(matches: &[SearchHybridMatch]) -> Value {
        json!({
            "total_matches": matches.len(),
            "top_matches": matches
                .iter()
                .take(Self::PROVENANCE_MATCH_SAMPLE_LIMIT)
                .map(|matched| {
                    json!({
                        "repository_id": matched.repository_id,
                        "path": matched.path,
                        "line": matched.line,
                        "column": matched.column,
                        "anchor": matched.anchor,
                    })
                })
                .collect::<Vec<_>>(),
        })
    }

    fn search_hybrid_match_from_evidence(
        evidence: crate::searcher::HybridRankedEvidence,
    ) -> SearchHybridMatch {
        let path = evidence.document.path.clone();
        SearchHybridMatch {
            match_id: None,
            repository_id: evidence.document.repository_id,
            path: path.clone(),
            line: evidence.anchor.start_line,
            column: evidence.anchor.start_column,
            excerpt: evidence.excerpt,
            anchor: Some(evidence.anchor),
            blended_score: evidence.blended_score,
            lexical_score: evidence.lexical_score,
            graph_score: evidence.graph_score,
            semantic_score: evidence.semantic_score,
            lexical_sources: evidence.lexical_sources,
            graph_sources: evidence.graph_sources,
            semantic_sources: evidence.semantic_sources,
            path_class: Some(classify_repository_path(&path)),
            source_class: Some(hybrid_match_source_class(&path)),
            surface_families: hybrid_match_surface_families(&path),
            navigation_hint: Some(SearchHybridNavigationHint {
                pivotable: hybrid_match_is_live_navigation_pivot(&path),
                document_symbols: hybrid_match_document_symbols_supported(&path),
                go_to_definition: hybrid_match_definition_navigation_supported(&path),
            }),
        }
    }

    pub(crate) fn search_hybrid_utility_summary(
        matches: &[SearchHybridMatch],
    ) -> SearchHybridUtilitySummary {
        let pivotable_match_count = matches
            .iter()
            .filter(|matched| {
                matched
                    .navigation_hint
                    .as_ref()
                    .is_some_and(|hint| hint.pivotable)
            })
            .count();
        let best_pivot = matches
            .iter()
            .enumerate()
            .filter(|(_, matched)| {
                matched
                    .navigation_hint
                    .as_ref()
                    .is_some_and(|hint| hint.pivotable)
            })
            .max_by_key(|(index, matched)| {
                (
                    Self::search_hybrid_live_pivot_priority(matched),
                    usize::MAX.saturating_sub(*index),
                )
            });
        let (best_pivot_rank, best_pivot_path, best_pivot_repository_id, symbol_navigation_ready) =
            if let Some((index, matched)) = best_pivot {
                (
                    Some(index + 1),
                    Some(matched.path.clone()),
                    Some(matched.repository_id.clone()),
                    matched
                        .navigation_hint
                        .as_ref()
                        .is_some_and(|hint| hint.document_symbols || hint.go_to_definition),
                )
            } else {
                (None, None, None, false)
            };
        SearchHybridUtilitySummary {
            pivotable_match_count,
            best_pivot_rank,
            best_pivot_path,
            best_pivot_repository_id,
            symbol_navigation_ready,
        }
    }

    fn search_hybrid_live_pivot_priority(matched: &SearchHybridMatch) -> (u8, u8, u8, u8, u8, u8) {
        let source_priority = match matched.source_class {
            Some(SourceClass::Runtime) => 5,
            Some(SourceClass::Support) => 4,
            Some(SourceClass::Tests) => 3,
            Some(SourceClass::Project) => 2,
            _ => 0,
        };
        let runtime_family = matched
            .surface_families
            .iter()
            .any(|family| family == "runtime") as u8;
        let entrypoint_family = matched
            .surface_families
            .iter()
            .any(|family| family == "entrypoint") as u8;
        let tests_family = matched
            .surface_families
            .iter()
            .any(|family| family == "tests") as u8;
        let looks_like_test = Self::search_hybrid_path_looks_like_test(&matched.path) as u8;
        let navigation_hint =
            matched
                .navigation_hint
                .clone()
                .unwrap_or(SearchHybridNavigationHint {
                    pivotable: false,
                    document_symbols: false,
                    go_to_definition: false,
                });
        (
            source_priority,
            runtime_family,
            entrypoint_family,
            navigation_hint.document_symbols as u8,
            navigation_hint.go_to_definition as u8,
            tests_family.saturating_sub(looks_like_test),
        )
    }

    fn search_hybrid_path_looks_like_test(path: &str) -> bool {
        let lower = path.to_ascii_lowercase();
        [
            "/test/",
            "/tests/",
            "/spec/",
            "/specs/",
            ".test.",
            ".spec.",
            "_test.",
            "test_",
            "/__tests__/",
        ]
        .iter()
        .any(|marker| lower.contains(marker))
    }
}
