use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::mcp::types::{
    SearchHybridExactPivotAssistance, SearchHybridQueryShape, SearchHybridRankReason,
};
use crate::path_class::classify_repository_path;
use crate::searcher::{SearchFilters, SearchTextQuery, TextSearcher};

#[derive(Debug, Clone, Default)]
struct SearchHybridExactPivotAssistInternal {
    applied: bool,
    symbol_hit_lines: BTreeMap<(String, String), BTreeSet<usize>>,
    text_hit_lines: BTreeMap<(String, String), BTreeSet<usize>>,
    exact_symbol_hit_count: usize,
    exact_text_hit_count: usize,
}

impl SearchHybridExactPivotAssistInternal {
    fn score_symbol(&self, repository_id: &str, path: &str, line: usize) -> u8 {
        let key = (repository_id.to_owned(), path.to_owned());
        match self.symbol_hit_lines.get(&key) {
            Some(lines) if lines.contains(&line) => 2,
            Some(_) => 1,
            None => 0,
        }
    }

    fn score_text(&self, repository_id: &str, path: &str, line: usize) -> u8 {
        let key = (repository_id.to_owned(), path.to_owned());
        match self.text_hit_lines.get(&key) {
            Some(lines) if lines.contains(&line) => 2,
            Some(_) => 1,
            None => 0,
        }
    }

    fn summary(&self, boosted_match_count: usize) -> SearchHybridExactPivotAssistance {
        SearchHybridExactPivotAssistance {
            applied: self.applied,
            exact_symbol_hit_count: self.exact_symbol_hit_count,
            exact_text_hit_count: self.exact_text_hit_count,
            boosted_match_count,
        }
    }
}

pub(crate) struct SearchHybridWarningContext<'a> {
    pub(crate) lexical_only_mode: bool,
    pub(crate) query_shape: SearchHybridQueryShape,
    pub(crate) semantic_status: Option<ChannelHealthStatus>,
    pub(crate) semantic_reason: Option<&'a str>,
    pub(crate) semantic_hit_count: Option<usize>,
    pub(crate) semantic_match_count: Option<usize>,
    pub(crate) exact_pivot_assistance: Option<&'a SearchHybridExactPivotAssistance>,
    pub(crate) witness_demotion_applied: bool,
}

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
                let mut query_shape: Option<SearchHybridQueryShape> = None;
                let mut exact_pivot_assistance: Option<SearchHybridExactPivotAssistance> = None;
                let mut witness_demotion_applied: Option<bool> = None;
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
                    let detected_query_shape = Self::search_hybrid_query_shape(&query);
                    query_shape = Some(detected_query_shape);
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
                                query: query.clone(),
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
                    let exact_pivot_assist =
                        if Self::search_hybrid_should_run_exact_pivot_assistance(
                            &matches,
                            search_output.note.lexical_only_mode,
                            detected_query_shape,
                        ) {
                            Self::search_hybrid_exact_pivot_assistance(
                                &server,
                                &query,
                                params_for_blocking.repository_id.as_deref(),
                                params_for_blocking.language.as_deref(),
                                &searcher,
                                &matches,
                            )?
                        } else {
                            None
                        };
                    let (boosted_match_count, witness_demotion_was_applied) =
                        Self::search_hybrid_apply_guardrails(
                            &mut matches,
                            search_output.note.lexical_only_mode,
                            exact_pivot_assist.as_ref(),
                        );
                    exact_pivot_assistance = exact_pivot_assist
                        .as_ref()
                        .map(|assist| assist.summary(boosted_match_count));
                    witness_demotion_applied = Some(witness_demotion_was_applied);
                    warning = Self::search_hybrid_warning(
                        &params_for_blocking.query,
                        SearchHybridWarningContext {
                            lexical_only_mode: search_output.note.lexical_only_mode,
                            query_shape: detected_query_shape,
                            semantic_status,
                            semantic_reason: semantic_reason.as_deref(),
                            semantic_hit_count,
                            semantic_match_count,
                            exact_pivot_assistance: exact_pivot_assistance.as_ref(),
                            witness_demotion_applied: witness_demotion_was_applied,
                        },
                    );
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
                        query_shape,
                        warning: warning.clone(),
                        exact_pivot_assistance: exact_pivot_assistance.clone(),
                        witness_demotion_applied,
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
        context: SearchHybridWarningContext<'_>,
    ) -> Option<String> {
        let broad_natural_language = context.lexical_only_mode
            && Self::search_hybrid_query_looks_broad_natural_language(query);
        let code_shaped_exact_assist = context.lexical_only_mode
            && context.query_shape == SearchHybridQueryShape::CodeShaped
            && context
                .exact_pivot_assistance
                .is_some_and(|assistance| assistance.applied);
        let base = match context.semantic_status {
            Some(ChannelHealthStatus::Disabled) => Some(match context.semantic_reason {
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
            Some(ChannelHealthStatus::Unavailable) => Some(match context.semantic_reason {
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
            Some(ChannelHealthStatus::Degraded) => Some(match context.semantic_reason {
                Some(reason) if !reason.trim().is_empty() => format!(
                    "semantic retrieval is degraded; semantic contribution may be partial ({reason})"
                ),
                _ => "semantic retrieval is degraded; semantic contribution may be partial".to_owned(),
            }),
            Some(ChannelHealthStatus::Ok) if context.semantic_hit_count == Some(0) => Some(
                "semantic retrieval completed successfully but retained no query-relevant semantic hits; results are ranked from lexical and graph signals only"
                    .to_owned(),
            ),
            Some(ChannelHealthStatus::Ok)
                if context.semantic_hit_count.unwrap_or(0) > 0
                    && context.semantic_match_count == Some(0) =>
            {
                Some(
                    "semantic retrieval retained semantic hits, but none contributed to the returned top results; ranking is effectively lexical and graph for this result set"
                        .to_owned(),
                )
            }
            _ => None,
        }?;

        let mut warning = base;
        if code_shaped_exact_assist {
            let boosted = context
                .exact_pivot_assistance
                .map(|assistance| assistance.boosted_match_count)
                .unwrap_or(0);
            if boosted > 0 {
                warning.push_str(
                    "; code-shaped exact symbol/text pivots were preferred for direct matches",
                );
            } else {
                warning.push_str("; code-shaped exact symbol/text pivots were checked");
            }
        }
        if context.lexical_only_mode && context.witness_demotion_applied {
            warning.push_str("; weak witness-only matches were demoted");
        }
        Some(warning)
    }

    fn search_hybrid_query_shape(query: &str) -> SearchHybridQueryShape {
        if Self::search_hybrid_query_looks_broad_natural_language(query) {
            SearchHybridQueryShape::BroadNaturalLanguage
        } else if Self::search_hybrid_query_looks_code_shaped(query) {
            SearchHybridQueryShape::CodeShaped
        } else {
            SearchHybridQueryShape::Neutral
        }
    }

    fn search_hybrid_query_looks_code_shaped(query: &str) -> bool {
        let trimmed = query.trim();
        if trimmed.is_empty() || Self::search_hybrid_query_looks_broad_natural_language(trimmed) {
            return false;
        }
        if trimmed.contains("::")
            || trimmed.contains("->")
            || trimmed.contains("=>")
            || trimmed.contains('/')
            || trimmed.contains('\\')
            || trimmed.contains('#')
            || trimmed.contains('$')
            || trimmed.contains('(')
            || trimmed.contains(')')
            || trimmed.contains('[')
            || trimmed.contains(']')
            || trimmed.contains('{')
            || trimmed.contains('}')
        {
            return true;
        }

        let tokens = trimmed.split_whitespace().collect::<Vec<_>>();
        if tokens.len() > 3 {
            return false;
        }

        tokens.iter().any(|token| {
            let cleaned = token.trim_matches(|ch: char| {
                !ch.is_ascii_alphanumeric() && !matches!(ch, '_' | '-' | '.')
            });
            if cleaned.is_empty() {
                return false;
            }
            cleaned.contains('_')
                || cleaned.contains('.')
                || cleaned.chars().any(|ch| ch.is_ascii_digit())
                || cleaned.chars().any(|ch| ch.is_ascii_uppercase())
                || cleaned.contains('-')
        })
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

    fn search_hybrid_exact_pivot_assistance(
        server: &Self,
        query: &str,
        repository_id: Option<&str>,
        language: Option<&str>,
        searcher: &TextSearcher,
        matches: &[SearchHybridMatch],
    ) -> Result<Option<SearchHybridExactPivotAssistInternal>, ErrorData> {
        let relevant_paths = matches
            .iter()
            .map(|matched| (matched.repository_id.clone(), matched.path.clone()))
            .collect::<BTreeSet<_>>();
        if relevant_paths.is_empty() {
            return Ok(None);
        }
        let mut relevant_paths_by_path: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
        for key in &relevant_paths {
            relevant_paths_by_path
                .entry(key.1.clone())
                .or_default()
                .push(key.clone());
        }

        let mut assistance = SearchHybridExactPivotAssistInternal {
            applied: true,
            ..Default::default()
        };
        let normalized_language = language
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase());
        let query_lower = query.to_ascii_lowercase();
        let query_looks_canonical =
            query.contains('\\') || query.contains("::") || query.contains('$');

        for corpus in server.collect_repository_symbol_corpora(repository_id)? {
            let mut matched_indices = BTreeSet::new();
            if query_looks_canonical {
                if let Some(indices) = corpus.symbol_indices_by_canonical_name.get(query) {
                    matched_indices.extend(indices.iter().copied());
                }
                if let Some(indices) = corpus
                    .symbol_indices_by_lower_canonical_name
                    .get(&query_lower)
                {
                    matched_indices.extend(indices.iter().copied());
                }
            }
            if let Some(indices) = corpus.symbol_indices_by_name.get(query) {
                matched_indices.extend(indices.iter().copied());
            }
            if let Some(indices) = corpus.symbol_indices_by_lower_name.get(&query_lower) {
                matched_indices.extend(indices.iter().copied());
            }

            for symbol_index in matched_indices {
                let symbol = &corpus.symbols[symbol_index];
                if !Self::search_hybrid_symbol_matches_language(
                    symbol.language,
                    normalized_language.as_deref(),
                ) {
                    continue;
                }
                let path = Self::relative_display_path(&corpus.root, &symbol.path);
                let key = (corpus.repository_id.clone(), path);
                if !relevant_paths.contains(&key) {
                    continue;
                }
                let lines = assistance.symbol_hit_lines.entry(key).or_default();
                if lines.insert(symbol.line) {
                    assistance.exact_symbol_hit_count =
                        assistance.exact_symbol_hit_count.saturating_add(1);
                }
            }
        }

        let text_hits = searcher
            .search_literal_with_filters_diagnostics(
                SearchTextQuery {
                    query: query.to_owned(),
                    path_regex: None,
                    limit: 32,
                },
                SearchFilters {
                    repository_id: None,
                    language: normalized_language.clone(),
                },
            )
            .map_err(Self::map_frigg_error)?;
        for text_match in text_hits.matches {
            let Some(relevant_keys) = relevant_paths_by_path.get(&text_match.path) else {
                continue;
            };
            for key in relevant_keys {
                let lines = assistance.text_hit_lines.entry(key.clone()).or_default();
                if lines.insert(text_match.line) {
                    assistance.exact_text_hit_count =
                        assistance.exact_text_hit_count.saturating_add(1);
                }
            }
        }

        Ok(Some(assistance))
    }

    fn search_hybrid_symbol_matches_language(
        symbol_language: SymbolLanguage,
        normalized_language: Option<&str>,
    ) -> bool {
        let Some(normalized_language) = normalized_language else {
            return true;
        };
        SymbolLanguage::parse_alias(normalized_language)
            .is_some_and(|language| language == symbol_language)
    }

    fn search_hybrid_source_is_witness_only(source: &str) -> bool {
        source.starts_with("path_witness:")
            || source.starts_with("path_surface_witness:")
            || source.starts_with("witness:")
    }

    fn search_hybrid_should_run_exact_pivot_assistance(
        _matches: &[SearchHybridMatch],
        lexical_only_mode: bool,
        query_shape: SearchHybridQueryShape,
    ) -> bool {
        lexical_only_mode && query_shape == SearchHybridQueryShape::CodeShaped
    }

    fn search_hybrid_match_has_strong_lexical_anchor(matched: &SearchHybridMatch) -> bool {
        matched.lexical_score > 0.0
            && !matched.lexical_sources.is_empty()
            && matched
                .lexical_sources
                .iter()
                .any(|source| !Self::search_hybrid_source_is_witness_only(source))
    }

    fn search_hybrid_match_is_weak_witness_only(
        matched: &SearchHybridMatch,
        exact_symbol_score: u8,
        exact_text_score: u8,
    ) -> bool {
        exact_symbol_score == 0
            && exact_text_score == 0
            && matched.graph_score <= 0.0
            && matched.semantic_score <= 0.0
            && !matched.lexical_sources.is_empty()
            && matched
                .lexical_sources
                .iter()
                .all(|source| Self::search_hybrid_source_is_witness_only(source))
    }

    fn search_hybrid_rank_reasons(
        matched: &SearchHybridMatch,
        exact_symbol_score: u8,
        exact_text_score: u8,
        weak_witness_only: bool,
    ) -> Vec<SearchHybridRankReason> {
        let mut reasons = Vec::new();
        if exact_symbol_score > 0 {
            reasons.push(SearchHybridRankReason::ExactSymbolMatch);
        }
        if exact_text_score > 0 {
            reasons.push(SearchHybridRankReason::ExactTextMatch);
        }
        if Self::search_hybrid_match_has_strong_lexical_anchor(matched) {
            reasons.push(SearchHybridRankReason::StrongLexicalAnchor);
        }
        if matched.graph_score > 0.0 && !matched.graph_sources.is_empty() {
            reasons.push(SearchHybridRankReason::GraphAdjacency);
        }
        if matched.semantic_score > 0.0 && !matched.semantic_sources.is_empty() {
            reasons.push(SearchHybridRankReason::SemanticContribution);
        }
        if weak_witness_only {
            reasons.push(SearchHybridRankReason::WitnessOnlyFallback);
        }
        reasons.truncate(3);
        reasons
    }

    fn search_hybrid_apply_guardrails(
        matches: &mut Vec<SearchHybridMatch>,
        lexical_only_mode: bool,
        exact_pivot_assist: Option<&SearchHybridExactPivotAssistInternal>,
    ) -> (usize, bool) {
        #[derive(Debug)]
        struct GuardedHybridMatch {
            original_index: usize,
            matched: SearchHybridMatch,
            exact_symbol_score: u8,
            exact_text_score: u8,
            strong_lexical_anchor: bool,
            graph_adjacency: bool,
            weak_witness_only: bool,
        }

        let mut guarded_matches = matches
            .drain(..)
            .enumerate()
            .map(|(original_index, mut matched)| {
                let exact_symbol_score = exact_pivot_assist
                    .map(|assist| {
                        assist.score_symbol(&matched.repository_id, &matched.path, matched.line)
                    })
                    .unwrap_or(0);
                let exact_text_score = exact_pivot_assist
                    .map(|assist| {
                        assist.score_text(&matched.repository_id, &matched.path, matched.line)
                    })
                    .unwrap_or(0);
                let strong_lexical_anchor =
                    Self::search_hybrid_match_has_strong_lexical_anchor(&matched);
                let graph_adjacency =
                    matched.graph_score > 0.0 && !matched.graph_sources.is_empty();
                let weak_witness_only = Self::search_hybrid_match_is_weak_witness_only(
                    &matched,
                    exact_symbol_score,
                    exact_text_score,
                );
                matched.rank_reasons = Self::search_hybrid_rank_reasons(
                    &matched,
                    exact_symbol_score,
                    exact_text_score,
                    weak_witness_only,
                );
                GuardedHybridMatch {
                    original_index,
                    matched,
                    exact_symbol_score,
                    exact_text_score,
                    strong_lexical_anchor,
                    graph_adjacency,
                    weak_witness_only,
                }
            })
            .collect::<Vec<_>>();

        let boosted_match_count = guarded_matches
            .iter()
            .filter(|matched| matched.exact_symbol_score > 0 || matched.exact_text_score > 0)
            .count();
        let witness_demotion_applied = lexical_only_mode
            && boosted_match_count > 0
            && guarded_matches
                .iter()
                .any(|matched| matched.weak_witness_only);

        if lexical_only_mode && boosted_match_count > 0 {
            guarded_matches.sort_by(|left, right| {
                right
                    .exact_symbol_score
                    .cmp(&left.exact_symbol_score)
                    .then(right.exact_text_score.cmp(&left.exact_text_score))
                    .then(right.strong_lexical_anchor.cmp(&left.strong_lexical_anchor))
                    .then(right.graph_adjacency.cmp(&left.graph_adjacency))
                    .then(left.weak_witness_only.cmp(&right.weak_witness_only))
                    .then_with(|| {
                        right
                            .matched
                            .blended_score
                            .partial_cmp(&left.matched.blended_score)
                            .unwrap_or(Ordering::Equal)
                    })
                    .then_with(|| {
                        right
                            .matched
                            .lexical_score
                            .partial_cmp(&left.matched.lexical_score)
                            .unwrap_or(Ordering::Equal)
                    })
                    .then_with(|| {
                        right
                            .matched
                            .graph_score
                            .partial_cmp(&left.matched.graph_score)
                            .unwrap_or(Ordering::Equal)
                    })
                    .then(left.matched.repository_id.cmp(&right.matched.repository_id))
                    .then(left.matched.path.cmp(&right.matched.path))
                    .then(left.matched.line.cmp(&right.matched.line))
                    .then(left.matched.column.cmp(&right.matched.column))
                    .then(left.original_index.cmp(&right.original_index))
            });
        }

        matches.extend(guarded_matches.into_iter().map(|matched| matched.matched));
        (boosted_match_count, witness_demotion_applied)
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
            rank_reasons: Vec::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hybrid_match_fixture(
        path: &str,
        line: usize,
        lexical_sources: &[&str],
        excerpt: &str,
    ) -> SearchHybridMatch {
        SearchHybridMatch {
            match_id: None,
            repository_id: "repo-001".to_owned(),
            path: path.to_owned(),
            line,
            column: 1,
            excerpt: excerpt.to_owned(),
            anchor: None,
            blended_score: 1.0,
            lexical_score: 1.0,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: lexical_sources
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            graph_sources: vec![],
            semantic_sources: vec![],
            path_class: None,
            source_class: Some(SourceClass::Runtime),
            surface_families: vec!["runtime".to_owned()],
            navigation_hint: Some(SearchHybridNavigationHint {
                pivotable: true,
                document_symbols: true,
                go_to_definition: true,
            }),
            rank_reasons: vec![],
        }
    }

    #[test]
    fn search_hybrid_query_shape_distinguishes_broad_and_code_shaped_queries() {
        assert_eq!(
            FriggMcpServer::search_hybrid_query_shape(
                "where is capture request flow handled after tool layer",
            ),
            SearchHybridQueryShape::BroadNaturalLanguage
        );
        assert_eq!(
            FriggMcpServer::search_hybrid_query_shape("setNavigationContext"),
            SearchHybridQueryShape::CodeShaped
        );
        assert_eq!(
            FriggMcpServer::search_hybrid_query_shape("runtime capture flow"),
            SearchHybridQueryShape::Neutral
        );
    }

    #[test]
    fn search_hybrid_apply_guardrails_prefers_exact_direct_matches_over_witnesses() {
        let mut matches = vec![
            hybrid_match_fixture(
                "tests/capture_screen_flow.rs",
                1,
                &["path_surface_witness:tests/capture_screen_flow.rs:1:1"],
                "fn smoke_test() {}",
            ),
            hybrid_match_fixture(
                "src/lib.rs",
                3,
                &["literal:src/lib.rs:3:1"],
                "pub fn capture_screen() {}",
            ),
        ];
        let mut exact_pivot_assist = SearchHybridExactPivotAssistInternal {
            applied: true,
            ..Default::default()
        };
        exact_pivot_assist.text_hit_lines.insert(
            ("repo-001".to_owned(), "src/lib.rs".to_owned()),
            BTreeSet::from([3usize]),
        );
        exact_pivot_assist.exact_text_hit_count = 1;

        let (boosted_match_count, witness_demotion_applied) =
            FriggMcpServer::search_hybrid_apply_guardrails(
                &mut matches,
                true,
                Some(&exact_pivot_assist),
            );

        assert_eq!(boosted_match_count, 1);
        assert!(witness_demotion_applied);
        assert_eq!(matches[0].path, "src/lib.rs");
        assert!(
            matches[0]
                .rank_reasons
                .contains(&SearchHybridRankReason::ExactTextMatch)
        );
        assert!(
            matches[0]
                .rank_reasons
                .contains(&SearchHybridRankReason::StrongLexicalAnchor)
        );
        assert_eq!(matches[1].path, "tests/capture_screen_flow.rs");
        assert_eq!(
            matches[1].rank_reasons,
            vec![SearchHybridRankReason::WitnessOnlyFallback]
        );
    }
}
