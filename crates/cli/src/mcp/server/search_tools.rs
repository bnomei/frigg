use std::collections::BTreeMap;

use super::*;
use crate::domain::{ChannelHealthStatus, SourceClass};
use crate::mcp::types::{
    SearchHybridChannelDiagnostic, SearchHybridChannelMetadata, SearchHybridDiagnosticsSummary,
    SearchHybridLanguageCapabilityMetadata, SearchHybridMetadata, SearchHybridNavigationHint,
    SearchHybridSemanticAcceleratorMetadata, SearchHybridStageAttribution,
    SearchHybridUtilitySummary,
};
use crate::path_class::classify_repository_path;
use crate::searcher::{
    hybrid_match_definition_navigation_supported, hybrid_match_document_symbols_supported,
    hybrid_match_is_live_navigation_pivot, hybrid_match_source_class,
    hybrid_match_surface_families,
};

impl FriggMcpServer {
    pub(super) fn invalidate_repository_search_response_caches(&self, repository_id: &str) {
        let mut search_text_cache = self
            .cache_state
            .search_text_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = search_text_cache.len();
        search_text_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchTextResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(search_text_cache.len()),
        );

        let mut search_hybrid_cache = self
            .cache_state
            .search_hybrid_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = search_hybrid_cache.len();
        search_hybrid_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchHybridResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(search_hybrid_cache.len()),
        );

        let mut search_symbol_cache = self
            .cache_state
            .search_symbol_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = search_symbol_cache.len();
        search_symbol_cache.retain(|key, _| {
            !response_cache_scopes_include_repository(
                repository_id,
                &key.scoped_repository_ids,
                &key.freshness_scopes,
            )
        });
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchSymbolResponse,
            RuntimeCacheEvent::Invalidation,
            before.saturating_sub(search_symbol_cache.len()),
        );
    }

    pub(super) async fn search_text_impl(
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
                        return Ok(Json(cached.response));
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
                    };
                    response_source_refs = json!({
                        "scoped_repository_ids": scoped_repository_ids.clone(),
                        "freshness_basis": cache_freshness.basis.clone(),
                        "total_matches": response.total_matches,
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

                    Ok(Json(response))
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

    pub(super) async fn search_hybrid_impl(
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
                        return Ok(Json(cached.response));
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
                        semantic_requested,
                        semantic_enabled,
                        semantic_status: semantic_status.clone(),
                        semantic_reason: semantic_reason.clone(),
                        semantic_candidate_count,
                        semantic_hit_count,
                        semantic_match_count,
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

                    Ok(Json(response))
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

    pub(super) async fn search_symbol_impl(
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
                        return Ok(Json(cached.response));
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
                    Ok(Json(response))
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

    pub(super) async fn document_symbols_impl(
        &self,
        params: DocumentSymbolsParams,
    ) -> Result<Json<DocumentSymbolsResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("document_symbols", params.repository_id.clone());
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut resolved_repository_id: Option<String> = None;
            let mut resolved_path: Option<String> = None;
            let mut symbol_count = 0usize;

            let result = (|| -> Result<Json<DocumentSymbolsResponse>, ErrorData> {
                let read_params = ReadFileParams {
                    path: params_for_blocking.path.clone(),
                    repository_id: params_for_blocking.repository_id.clone(),
                    max_bytes: None,
                    line_start: None,
                    line_end: None,
                };
                let (repository_id, absolute_path, display_path) =
                    server.resolve_file_path(&read_params)?;
                resolved_repository_id = Some(repository_id.clone());
                resolved_path = Some(display_path.clone());

                let language =
                    supported_language_for_path(&absolute_path, LanguageCapability::DocumentSymbols)
                        .ok_or_else(|| {
                            Self::invalid_params(
                                LanguageCapability::DocumentSymbols
                                    .unsupported_file_message("document_symbols"),
                                Some(json!({
                                    "path": display_path.clone(),
                                    "supported_extensions": LanguageCapability::DocumentSymbols.supported_extensions(),
                                })),
                            )
                        })?;
                let metadata = fs::metadata(&absolute_path).map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to stat source file {}: {err}",
                            absolute_path.display()
                        ),
                        None,
                    )
                })?;
                let bytes = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
                if bytes > server.config.max_file_bytes {
                    return Err(Self::invalid_params(
                        format!("file exceeds max_bytes={}", server.config.max_file_bytes),
                        Some(json!({
                            "path": display_path.clone(),
                            "bytes": bytes,
                            "max_bytes": server.config.max_file_bytes,
                            "config_max_file_bytes": server.config.max_file_bytes,
                            "suggested_max_bytes": bytes.min(server.config.max_file_bytes),
                        })),
                    ));
                }
                let source = fs::read_to_string(&absolute_path).map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to read source file {}: {err}",
                            absolute_path.display()
                        ),
                        None,
                    )
                })?;
                let symbols = extract_symbols_from_source(language, &absolute_path, &source)
                    .map_err(Self::map_frigg_error)?;

                let outline =
                    Self::build_document_symbol_tree(&symbols, &repository_id, &display_path);
                symbol_count = outline.len();

                let metadata = if language == SymbolLanguage::Blade {
                    let blade_evidence = extract_blade_source_evidence_from_source(&source, &symbols);
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                        "blade": {
                            "relations_detected": blade_evidence.relations.len(),
                            "livewire_components": blade_evidence.livewire_components,
                            "wire_directives": blade_evidence.wire_directives,
                            "flux_components": blade_evidence.flux_components,
                            "flux_registry_version": FLUX_REGISTRY_VERSION,
                            "flux_hints": blade_evidence.flux_hints,
                        },
                    })
                } else if language == SymbolLanguage::Php {
                    let php_metadata = extract_php_source_evidence_from_source(
                        &absolute_path,
                        &source,
                        &symbols,
                    )
                    .ok()
                    .map(|evidence| {
                        json!({
                            "canonical_name_count": evidence.canonical_names_by_stable_id.len(),
                            "type_evidence_count": evidence.type_evidence.len(),
                            "target_evidence_count": evidence.target_evidence.len(),
                            "literal_evidence_count": evidence.literal_evidence.len(),
                        })
                    });
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                        "php": php_metadata,
                    })
                } else {
                    json!({
                        "source": "tree_sitter",
                        "language": language.as_str(),
                        "symbol_count": symbol_count,
                        "heuristic": false,
                    })
                };
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(DocumentSymbolsResponse {
                    symbols: outline,
                    metadata,
                    note,
                }))
            })();

            (result, resolved_repository_id, resolved_path, symbol_count)
        })
        .await?;

        let (result, resolved_repository_id, resolved_path, symbol_count) = execution;
        let provenance_result = self
            .record_provenance_blocking(
                "document_symbols",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "path": Self::bounded_text(&params.path),
                }),
                json!({
                    "resolved_repository_id": resolved_repository_id,
                    "resolved_path": resolved_path,
                    "symbol_count": symbol_count,
                }),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }

    pub(super) async fn search_structural_impl(
        &self,
        params: SearchStructuralParams,
    ) -> Result<Json<SearchStructuralResponse>, ErrorData> {
        let execution_context = self
            .read_only_tool_execution_context("search_structural", params.repository_id.clone());
        let params_for_blocking = params.clone();
        let server = self.clone();
        let execution = self.run_read_only_tool_blocking(&execution_context, move || {
            let mut scoped_repository_ids: Vec<String> = Vec::new();
            let mut effective_limit: Option<usize> = None;
            let mut language_filter: Option<String> = None;
            let mut files_scanned = 0usize;
            let mut files_matched = 0usize;
            let mut diagnostics_count = 0usize;
            let mut blade_relations_detected = 0usize;
            let mut blade_livewire_components = BTreeSet::new();
            let mut blade_wire_directives = BTreeSet::new();
            let mut blade_flux_components = BTreeSet::new();

            let result = (|| -> Result<Json<SearchStructuralResponse>, ErrorData> {
                let query = params_for_blocking.query.trim().to_owned();
                if query.is_empty() {
                    return Err(Self::invalid_params("query must not be empty", None));
                }
                if query.chars().count() > Self::SEARCH_STRUCTURAL_MAX_QUERY_CHARS {
                    return Err(Self::invalid_params(
                        "query exceeds structural search maximum length",
                        Some(json!({
                            "query_chars": query.chars().count(),
                            "max_query_chars": Self::SEARCH_STRUCTURAL_MAX_QUERY_CHARS,
                        })),
                    ));
                }

                let path_regex = match params_for_blocking.path_regex.as_ref() {
                    Some(raw) => Some(compile_safe_regex(raw).map_err(|err| {
                        Self::invalid_params(
                            format!("invalid path_regex: {err}"),
                            Some(json!({
                                "path_regex": raw,
                                "regex_error_code": err.code(),
                            })),
                        )
                    })?),
                    None => None,
                };

                let target_language =
                    Self::parse_symbol_language(params_for_blocking.language.as_deref())?;
                language_filter = target_language.map(|language| language.as_str().to_owned());
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

                let mut matches = Vec::new();
                for corpus in corpora {
                    for source_path in &corpus.source_paths {
                        let Some(language) = supported_language_for_path(
                            source_path,
                            LanguageCapability::StructuralSearch,
                        ) else {
                            continue;
                        };
                        if let Some(target_language) = target_language {
                            if language != target_language {
                                continue;
                            }
                        }
                        let display_path = Self::relative_display_path(&corpus.root, source_path);
                        if let Some(path_regex) = &path_regex
                            && !path_regex.is_match(&display_path)
                        {
                            continue;
                        }
                        files_scanned = files_scanned.saturating_add(1);

                        let source = match fs::read_to_string(source_path) {
                            Ok(source) => source,
                            Err(err) => {
                                diagnostics_count = diagnostics_count.saturating_add(1);
                                warn!(
                                    repository_id = corpus.repository_id,
                                    path = %source_path.display(),
                                    error = %err,
                                    "skipping source file for structural search"
                                );
                                continue;
                            }
                        };

                        let structural_matches =
                            search_structural_in_source(language, source_path, &source, &query)
                                .map_err(Self::map_frigg_error)?;
                        if language == SymbolLanguage::Blade {
                            let blade_evidence =
                                extract_blade_source_evidence_from_source(&source, &[]);
                            blade_relations_detected = blade_relations_detected
                                .saturating_add(blade_evidence.relations.len());
                            blade_livewire_components
                                .extend(blade_evidence.livewire_components.into_iter());
                            blade_wire_directives
                                .extend(blade_evidence.wire_directives.into_iter());
                            blade_flux_components
                                .extend(blade_evidence.flux_components.into_iter());
                        }
                        files_matched = files_matched
                            .saturating_add(usize::from(!structural_matches.is_empty()));

                        for structural_match in structural_matches {
                            matches.push(crate::mcp::types::StructuralMatch {
                                repository_id: corpus.repository_id.clone(),
                                path: display_path.clone(),
                                line: structural_match.span.start_line,
                                column: structural_match.span.start_column,
                                end_line: structural_match.span.end_line,
                                end_column: structural_match.span.end_column,
                                excerpt: structural_match.excerpt,
                            });
                        }
                    }
                }

                matches.sort_by(|left, right| {
                    left.repository_id
                        .cmp(&right.repository_id)
                        .then(left.path.cmp(&right.path))
                        .then(left.line.cmp(&right.line))
                        .then(left.column.cmp(&right.column))
                        .then(left.end_line.cmp(&right.end_line))
                        .then(left.end_column.cmp(&right.end_column))
                        .then(left.excerpt.cmp(&right.excerpt))
                });
                if matches.len() > limit {
                    matches.truncate(limit);
                }

                let metadata = if target_language == Some(SymbolLanguage::Blade) {
                    json!({
                        "source": "tree_sitter_query",
                        "language": language_filter.clone().unwrap_or_else(|| "mixed".to_owned()),
                        "heuristic": false,
                        "diagnostics_count": diagnostics_count,
                        "files_scanned": files_scanned,
                        "files_matched": files_matched,
                        "blade": {
                            "relations_detected": blade_relations_detected,
                            "livewire_components": blade_livewire_components.into_iter().collect::<Vec<_>>(),
                            "wire_directives": blade_wire_directives.into_iter().collect::<Vec<_>>(),
                            "flux_components": blade_flux_components.into_iter().collect::<Vec<_>>(),
                            "flux_registry_version": FLUX_REGISTRY_VERSION,
                        },
                    })
                } else {
                    json!({
                        "source": "tree_sitter_query",
                        "language": language_filter.clone().unwrap_or_else(|| "mixed".to_owned()),
                        "heuristic": false,
                        "diagnostics_count": diagnostics_count,
                        "files_scanned": files_scanned,
                        "files_matched": files_matched,
                    })
                };
                let (metadata, note) = Self::metadata_note_pair(metadata);
                Ok(Json(SearchStructuralResponse {
                    matches,
                    metadata,
                    note,
                }))
            })();

            (
                result,
                scoped_repository_ids,
                effective_limit,
                language_filter,
                files_scanned,
                files_matched,
                diagnostics_count,
            )
        })
        .await?;

        let (
            result,
            scoped_repository_ids,
            effective_limit,
            language_filter,
            files_scanned,
            files_matched,
            diagnostics_count,
        ) = execution;
        let provenance_result = self
            .record_provenance_blocking(
                "search_structural",
                execution_context.repository_hint.as_deref(),
                json!({
                    "repository_id": execution_context.repository_hint,
                    "query": Self::bounded_text(&params.query),
                    "language": params.language,
                    "path_regex": params.path_regex.map(|raw| Self::bounded_text(&raw)),
                    "limit": params.limit,
                    "effective_limit": effective_limit,
                }),
                json!({
                    "scoped_repository_ids": scoped_repository_ids,
                    "language_filter": language_filter,
                    "files_scanned": files_scanned,
                    "files_matched": files_matched,
                    "diagnostics_count": diagnostics_count,
                }),
                &result,
            )
            .await;
        self.finalize_read_only_tool(&execution_context, result, provenance_result)
    }
}

impl FriggMcpServer {
    fn source_span_contains_symbol(parent: &SourceSpan, child: &SourceSpan) -> bool {
        parent.start_byte <= child.start_byte
            && child.end_byte <= parent.end_byte
            && (parent.start_byte < child.start_byte || child.end_byte < parent.end_byte)
    }

    fn build_document_symbol_tree(
        symbols: &[SymbolDefinition],
        repository_id: &str,
        display_path: &str,
    ) -> Vec<crate::mcp::types::DocumentSymbolItem> {
        #[derive(Clone)]
        struct PendingDocumentSymbolNode {
            item: crate::mcp::types::DocumentSymbolItem,
            span: SourceSpan,
            children: Vec<usize>,
        }

        fn materialize(
            nodes: &[PendingDocumentSymbolNode],
            index: usize,
        ) -> crate::mcp::types::DocumentSymbolItem {
            let mut item = nodes[index].item.clone();
            item.children = nodes[index]
                .children
                .iter()
                .map(|child_index| materialize(nodes, *child_index))
                .collect();
            item
        }

        let mut nodes: Vec<PendingDocumentSymbolNode> = Vec::with_capacity(symbols.len());
        let mut root_indices = Vec::new();
        let mut stack: Vec<usize> = Vec::new();

        for symbol in symbols {
            while let Some(parent_index) = stack.last().copied() {
                if Self::source_span_contains_symbol(&nodes[parent_index].span, &symbol.span) {
                    break;
                }
                stack.pop();
            }

            let container = stack
                .last()
                .map(|parent_index| nodes[*parent_index].item.symbol.clone());
            let node_index = nodes.len();
            nodes.push(PendingDocumentSymbolNode {
                item: crate::mcp::types::DocumentSymbolItem {
                    symbol: symbol.name.clone(),
                    kind: symbol.kind.as_str().to_owned(),
                    repository_id: repository_id.to_owned(),
                    path: display_path.to_owned(),
                    line: symbol.span.start_line,
                    column: symbol.span.start_column,
                    end_line: Some(symbol.span.end_line),
                    end_column: Some(symbol.span.end_column),
                    container,
                    children: Vec::new(),
                },
                span: symbol.span.clone(),
                children: Vec::new(),
            });

            if let Some(parent_index) = stack.last().copied() {
                nodes[parent_index].children.push(node_index);
            } else {
                root_indices.push(node_index);
            }
            stack.push(node_index);
        }

        root_indices
            .into_iter()
            .map(|index| materialize(&nodes, index))
            .collect()
    }

    pub(super) fn search_hybrid_semantic_status(
        status: ChannelHealthStatus,
    ) -> ChannelHealthStatus {
        match status {
            ChannelHealthStatus::Filtered => ChannelHealthStatus::Disabled,
            other => other,
        }
    }

    pub(super) fn search_hybrid_warning(
        semantic_status: Option<ChannelHealthStatus>,
        semantic_reason: Option<&str>,
        semantic_hit_count: Option<usize>,
        semantic_match_count: Option<usize>,
    ) -> Option<String> {
        match semantic_status {
            Some(ChannelHealthStatus::Disabled) => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "semantic retrieval is disabled; results are ranked from lexical and graph signals only ({reason})"
                ),
                _ => "semantic retrieval is disabled; results are ranked from lexical and graph signals only".to_owned(),
            }),
            Some(ChannelHealthStatus::Unavailable) => Some(match semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "semantic retrieval is unavailable; results are ranked from lexical and graph signals only ({reason})"
                ),
                _ => "semantic retrieval is unavailable; results are ranked from lexical and graph signals only".to_owned(),
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

    fn parse_symbol_language(value: Option<&str>) -> Result<Option<SymbolLanguage>, ErrorData> {
        let Some(value) = value else {
            return Ok(None);
        };
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err(Self::invalid_params("language must not be empty", None));
        }

        let language = parse_supported_language(&normalized, LanguageCapability::StructuralSearch)
            .ok_or_else(|| {
                Self::invalid_params(
                    format!("unsupported language `{value}` for structural search"),
                    Some(json!({
                        "language": value,
                        "supported_languages": LanguageCapability::StructuralSearch.supported_language_names(),
                    })),
                )
            })?;
        Ok(Some(language))
    }

    fn compile_cached_safe_regex(
        &self,
        raw: &str,
    ) -> Result<regex::Regex, crate::searcher::RegexSearchError> {
        if let Some(cached) = self
            .cache_state
            .compiled_safe_regex_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(raw)
            .cloned()
        {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Hit,
                1,
            );
            return Ok(cached);
        }
        self.record_runtime_cache_event(
            RuntimeCacheFamily::CompiledSafeRegex,
            RuntimeCacheEvent::Miss,
            1,
        );

        let compiled = compile_safe_regex(raw)?;
        let mut cache = self
            .cache_state
            .compiled_safe_regex_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(cached) = cache.get(raw).cloned() {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Hit,
                1,
            );
            return Ok(cached);
        }
        let inserted = cache.insert(raw.to_owned(), compiled.clone()).is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::CompiledSafeRegex,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_entry_limit(RuntimeCacheFamily::CompiledSafeRegex, &mut cache);
        Ok(compiled)
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

    fn search_hybrid_utility_summary(matches: &[SearchHybridMatch]) -> SearchHybridUtilitySummary {
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

    fn search_pattern_type_cache_key(pattern_type: &SearchPatternType) -> &'static str {
        match pattern_type {
            SearchPatternType::Literal => "literal",
            SearchPatternType::Regex => "regex",
        }
    }

    pub(super) fn cached_search_text_response(
        &self,
        cache_key: &SearchTextResponseCacheKey,
    ) -> Option<CachedSearchTextResponse> {
        let cached = self
            .cache_state
            .search_text_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchTextResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn cache_search_text_response(
        &self,
        cache_key: SearchTextResponseCacheKey,
        response: &SearchTextResponse,
        source_refs: &Value,
    ) {
        let mut cache = self
            .cache_state
            .search_text_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedSearchTextResponse {
                    response: response.clone(),
                    source_refs: source_refs.clone(),
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::SearchTextResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_entry_limit(RuntimeCacheFamily::SearchTextResponse, &mut cache);
    }

    pub(super) fn cached_search_hybrid_response(
        &self,
        cache_key: &SearchHybridResponseCacheKey,
    ) -> Option<CachedSearchHybridResponse> {
        let cached = self
            .cache_state
            .search_hybrid_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchHybridResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn cache_search_hybrid_response(
        &self,
        cache_key: SearchHybridResponseCacheKey,
        response: &SearchHybridResponse,
        source_refs: &Value,
    ) {
        let mut cache = self
            .cache_state
            .search_hybrid_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedSearchHybridResponse {
                    response: response.clone(),
                    source_refs: source_refs.clone(),
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::SearchHybridResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_entry_limit(
            RuntimeCacheFamily::SearchHybridResponse,
            &mut cache,
        );
    }

    pub(super) fn cached_search_symbol_response(
        &self,
        cache_key: &SearchSymbolResponseCacheKey,
    ) -> Option<CachedSearchSymbolResponse> {
        let cached = self
            .cache_state
            .search_symbol_response_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(cache_key)
            .cloned();
        self.record_runtime_cache_event(
            RuntimeCacheFamily::SearchSymbolResponse,
            if cached.is_some() {
                RuntimeCacheEvent::Hit
            } else {
                RuntimeCacheEvent::Miss
            },
            1,
        );
        cached
    }

    pub(super) fn cache_search_symbol_response(
        &self,
        cache_key: SearchSymbolResponseCacheKey,
        response: &SearchSymbolResponse,
        scoped_repository_ids: &[String],
        diagnostics_count: usize,
        manifest_walk_diagnostics_count: usize,
        manifest_read_diagnostics_count: usize,
        symbol_extraction_diagnostics_count: usize,
        effective_limit: usize,
    ) {
        let mut cache = self
            .cache_state
            .search_symbol_response_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let inserted = cache
            .insert(
                cache_key,
                CachedSearchSymbolResponse {
                    response: response.clone(),
                    scoped_repository_ids: scoped_repository_ids.to_owned(),
                    diagnostics_count,
                    manifest_walk_diagnostics_count,
                    manifest_read_diagnostics_count,
                    symbol_extraction_diagnostics_count,
                    effective_limit,
                },
            )
            .is_none();
        if inserted {
            self.record_runtime_cache_event(
                RuntimeCacheFamily::SearchSymbolResponse,
                RuntimeCacheEvent::Insert,
                1,
            );
        }
        self.trim_runtime_cache_to_entry_limit(
            RuntimeCacheFamily::SearchSymbolResponse,
            &mut cache,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn match_fixture(
        path: &str,
        source_class: Option<SourceClass>,
        surface_families: &[&str],
        pivotable: bool,
        document_symbols: bool,
        go_to_definition: bool,
    ) -> SearchHybridMatch {
        SearchHybridMatch {
            repository_id: "repo-001".to_string(),
            path: path.to_string(),
            line: 1,
            column: 1,
            excerpt: "fixture".to_string(),
            anchor: None,
            blended_score: 1.0,
            lexical_score: 1.0,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: vec![],
            graph_sources: vec![],
            semantic_sources: vec![],
            path_class: None,
            source_class,
            surface_families: surface_families
                .iter()
                .map(|value| (*value).to_string())
                .collect(),
            navigation_hint: Some(SearchHybridNavigationHint {
                pivotable,
                document_symbols,
                go_to_definition,
            }),
        }
    }

    #[test]
    fn search_hybrid_utility_summary_prefers_runtime_source_pivot() {
        let matches = vec![
            match_fixture(
                "README.md",
                Some(SourceClass::Project),
                &["docs"],
                false,
                false,
                false,
            ),
            match_fixture(
                "tests/runtime_test.rs",
                Some(SourceClass::Tests),
                &["tests"],
                true,
                true,
                false,
            ),
            match_fixture(
                "src/runtime/server.rs",
                Some(SourceClass::Runtime),
                &["runtime"],
                true,
                true,
                true,
            ),
        ];

        let summary = FriggMcpServer::search_hybrid_utility_summary(&matches);
        assert_eq!(summary.pivotable_match_count, 2);
        assert_eq!(summary.best_pivot_rank, Some(3));
        assert_eq!(
            summary.best_pivot_path.as_deref(),
            Some("src/runtime/server.rs")
        );
        assert!(summary.symbol_navigation_ready);
    }

    #[test]
    fn search_hybrid_utility_summary_reports_miss_without_pivotable_matches() {
        let matches = vec![
            match_fixture(
                "docs/overview.md",
                Some(SourceClass::Project),
                &["docs"],
                false,
                false,
                false,
            ),
            match_fixture(
                "package.json",
                Some(SourceClass::Project),
                &["package_surface"],
                false,
                false,
                false,
            ),
        ];

        let summary = FriggMcpServer::search_hybrid_utility_summary(&matches);
        assert_eq!(summary.pivotable_match_count, 0);
        assert_eq!(summary.best_pivot_rank, None);
        assert_eq!(summary.best_pivot_path, None);
        assert!(!summary.symbol_navigation_ready);
    }
}
