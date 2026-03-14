use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

use aho_corasick::AhoCorasickBuilder;
use regex::{Regex, escape};

use crate::domain::{
    ChannelHealth, ChannelHealthStatus, ChannelResult, ChannelStats, EvidenceChannel, FriggError,
    FriggResult, model::TextMatch,
};
use crate::languages::{LanguageSupportCapability, SymbolLanguage};
use crate::settings::SemanticRuntimeCredentials;

use super::{
    HYBRID_LEXICAL_RECALL_MAX_TOKENS, HybridPathWitnessQueryContext, HybridRankingIntent,
    SearchExecutionDiagnostics, SearchExecutionOutput, SearchFilters, SearchHybridExecutionOutput,
    SearchHybridQuery, SearchStageAttribution, SearchStageSample, SearchTextQuery, TextSearcher,
    build_hybrid_lexical_hits_with_intent, build_hybrid_lexical_recall_regex,
    build_hybrid_path_witness_hits_with_intent, empty_channel_result,
    hybrid_execution_note_from_channel_results, hybrid_lexical_recall_tokens,
    hybrid_path_has_exact_stem_match, hybrid_query_exact_terms, match_count_for_hits,
    merge_hybrid_lexical_search_output, normalize_search_filters,
    rank_hybrid_anchor_evidence_for_query_with_witness, retain_semantic_hits_for_query,
    search_diagnostics_to_channel_diagnostics, search_graph_channel_hits,
    search_semantic_channel_hits, sort_search_diagnostics_deterministically,
};

pub(super) fn search_hybrid_with_filters_using_executor(
    searcher: &TextSearcher,
    query: SearchHybridQuery,
    filters: SearchFilters,
    credentials: &SemanticRuntimeCredentials,
    semantic_executor: &dyn super::SemanticRuntimeQueryEmbeddingExecutor,
) -> FriggResult<SearchHybridExecutionOutput> {
    let query_text = query.query.trim().to_owned();
    let ranking_intent = HybridRankingIntent::from_query(&query_text);
    let prefer_graph_over_path_witness = prefers_graph_over_path_witness(&ranking_intent);
    let wants_path_witness_recall = ranking_intent.wants_path_witness_recall();
    let exact_terms = hybrid_query_exact_terms(&query_text);
    if query_text.is_empty() {
        return Err(FriggError::InvalidInput(
            "hybrid search query must not be empty".to_owned(),
        ));
    }
    if query.limit == 0 {
        return Ok(SearchHybridExecutionOutput::default());
    }

    let lexical_limit = if wants_path_witness_recall {
        if prefer_graph_over_path_witness {
            // Graph-oriented witness queries only need a small lexical seed set before exact-stem
            // graph fallback takes over, so a larger pre-graph lexical pool mostly burns scan time
            // without changing the final graph-ranked top-k.
            query.limit.saturating_add(2).max(8)
        } else {
            // Path-witness ranking gets its breadth from the witness channel itself, so a large
            // lexical pool mostly adds repeated token scans before the file-surface merge.
            query.limit.saturating_mul(2).max(16)
        }
    } else if prefers_compact_lexical_seed_set(&ranking_intent, &exact_terms) {
        // Generic single-term hybrid queries are effectively lexical-only; carrying a 30-result
        // working set through the scan engine costs more than it helps when the final top-k is 5.
        query.limit.saturating_add(7).max(12)
    } else {
        // Non-witness hybrid ranking still benefits from a broader candidate pool than the final
        // top-k, but collecting the full max_search_results set is wasted work for small limits.
        query
            .limit
            .saturating_mul(6)
            .max(30)
            .min(searcher.config.max_search_results.max(1))
    };
    let widen_lexical_working_set = wants_path_witness_recall
        || ranking_intent.wants_docs
        || ranking_intent.wants_contracts
        || ranking_intent.wants_error_taxonomy
        || ranking_intent.wants_tool_contracts
        || exact_terms.len() >= 4;
    let lexical_working_limit = if widen_lexical_working_set {
        lexical_limit
            .saturating_mul(4)
            .clamp(lexical_limit, lexical_limit.max(128))
    } else {
        lexical_limit
    };
    let path_witness_working_limit = if wants_path_witness_recall {
        query.limit.saturating_mul(4).max(32)
    } else {
        lexical_limit
    };
    let semantic_limit = query.limit.max(searcher.config.max_search_results);
    let lexical_seed_terms = if wants_path_witness_recall {
        hybrid_lexical_recall_tokens(&query_text)
    } else {
        Vec::new()
    };
    let path_witness_query_context =
        wants_path_witness_recall.then(|| HybridPathWitnessQueryContext::new(&query_text));
    let normalized_filters = normalize_search_filters(filters.clone())?;
    let candidate_universe_build = searcher.build_candidate_universe_with_attribution(
        &SearchTextQuery {
            query: String::new(),
            path_regex: None,
            limit: lexical_limit,
        },
        &normalized_filters,
    );
    let candidate_repository_count = candidate_universe_build.repository_count;
    let candidate_file_count = candidate_universe_build.candidate_count;
    let manifest_backed_repository_count =
        candidate_universe_build.manifest_backed_repository_count;
    let candidate_intake_elapsed_us = candidate_universe_build.candidate_intake_elapsed_us;
    let freshness_validation_elapsed_us = candidate_universe_build.freshness_validation_elapsed_us;
    let candidate_universe = candidate_universe_build.universe;
    let path_witness_lexical_universe = path_witness_query_context.as_ref().and_then(|context| {
        searcher.build_overlay_aware_path_witness_seed_universe(
            &candidate_universe,
            &normalized_filters,
            &ranking_intent,
            context,
            path_witness_working_limit,
        )
    });
    let lexical_candidate_universe = path_witness_lexical_universe
        .as_ref()
        .unwrap_or(&candidate_universe);
    let mut witness_scoring_elapsed_us = 0_u64;
    let mut witness_output = SearchExecutionOutput::default();
    let lexical_seeded_with_terms = !lexical_seed_terms.is_empty();
    let scan_started_at = Instant::now();
    let mut lexical_output = if lexical_seeded_with_terms {
        // Witness-oriented queries behave like bags of anchor terms more often than contiguous
        // phrases, so a single streaming multi-literal pass is cheaper than repeated token scans.
        search_case_insensitive_recall_terms_with_universe(
            searcher,
            &lexical_seed_terms,
            lexical_working_limit,
            lexical_candidate_universe,
            true,
        )?
    } else if prefers_compact_lexical_seed_set(&ranking_intent, &exact_terms) {
        searcher.search_literal_prefix_with_candidate_universe(
            &SearchTextQuery {
                query: query_text.clone(),
                path_regex: None,
                limit: lexical_working_limit,
            },
            &candidate_universe,
        )?
    } else {
        searcher.search_literal_with_candidate_universe(
            &SearchTextQuery {
                query: query_text.clone(),
                path_regex: None,
                limit: lexical_working_limit,
            },
            &candidate_universe,
        )?
    };
    let mut lexical_document_count = distinct_match_document_count(&lexical_output.matches);
    let should_widen_seeded_lexical_universe = lexical_seeded_with_terms
        && !std::ptr::eq(lexical_candidate_universe, &candidate_universe)
        && (lexical_document_count < query.limit
            || (wants_path_witness_recall && lexical_document_count < path_witness_working_limit));
    if should_widen_seeded_lexical_universe {
        lexical_output = search_case_insensitive_recall_terms_with_universe(
            searcher,
            &lexical_seed_terms,
            lexical_working_limit,
            &candidate_universe,
            false,
        )?;
        lexical_document_count = distinct_match_document_count(&lexical_output.matches);
    }

    let semantic_started_at = Instant::now();
    let strict_semantic = searcher.config.semantic_runtime.strict_mode;
    let unsupported_semantic_language =
        normalized_filters
            .language
            .as_ref()
            .copied()
            .filter(|language| {
                language
                    .capability_tier(LanguageSupportCapability::SemanticChunking)
                    .as_str()
                    == "unsupported"
            });
    let semantic_channel_result = if matches!(query.semantic, Some(false)) {
        empty_channel_result(
            EvidenceChannel::Semantic,
            ChannelHealthStatus::Disabled,
            Some("semantic channel disabled by request toggle".to_owned()),
        )
    } else if let Some(language) = unsupported_semantic_language {
        empty_channel_result(
            EvidenceChannel::Semantic,
            ChannelHealthStatus::Unavailable,
            Some(format!(
                "requested language filter '{}' does not support semantic_chunking",
                language.as_str()
            )),
        )
    } else if !searcher.config.semantic_runtime.enabled {
        empty_channel_result(
            EvidenceChannel::Semantic,
            ChannelHealthStatus::Disabled,
            Some("semantic runtime disabled in active configuration".to_owned()),
        )
    } else {
        match search_semantic_channel_hits(
            searcher,
            &query_text,
            &filters,
            semantic_limit,
            credentials,
            semantic_executor,
        ) {
            Ok(outcome) => {
                let (semantic_hits, semantic_hit_count) =
                    retain_semantic_hits_for_query(outcome.hits, &query_text, query.limit);
                ChannelResult::new(
                    EvidenceChannel::Semantic,
                    semantic_hits,
                    outcome.health,
                    outcome.diagnostics,
                    ChannelStats {
                        candidate_count: outcome.candidate_count,
                        hit_count: semantic_hit_count,
                        match_count: 0,
                    },
                )
            }
            Err(err) => {
                if strict_semantic {
                    return Err(FriggError::Internal(format!(
                        "semantic_status=strict_failure: {err}"
                    )));
                }
                empty_channel_result(
                    EvidenceChannel::Semantic,
                    ChannelHealthStatus::Degraded,
                    Some(err.to_string()),
                )
            }
        }
    };
    let semantic_retrieval_elapsed_us = semantic_started_at
        .elapsed()
        .as_micros()
        .try_into()
        .unwrap_or(u64::MAX);

    let should_expand_lexical = (lexical_document_count < query.limit || wants_path_witness_recall)
        && (semantic_channel_result.health.status != ChannelHealthStatus::Ok
            || semantic_channel_result.hits.is_empty()
            || wants_path_witness_recall
            || ranking_intent.wants_docs
            || ranking_intent.wants_contracts
            || ranking_intent.wants_error_taxonomy
            || ranking_intent.wants_tool_contracts
            || ranking_intent.wants_benchmarks);
    if should_expand_lexical {
        if prefer_graph_over_path_witness {
            if !lexical_seeded_with_terms {
                if let Some(token_regex) = build_hybrid_lexical_recall_regex(&query_text) {
                    let expanded_query = SearchTextQuery {
                        query: token_regex,
                        path_regex: None,
                        limit: lexical_working_limit,
                    };
                    let expanded =
                        search_regex_with_universe(searcher, &expanded_query, &candidate_universe)?;
                    merge_hybrid_lexical_search_output(
                        &mut lexical_output,
                        expanded,
                        lexical_working_limit,
                    );
                }
            }
        } else {
            if !lexical_seeded_with_terms {
                let recall_tokens = hybrid_lexical_recall_tokens(&query_text);

                for token in recall_tokens
                    .iter()
                    .take(HYBRID_LEXICAL_RECALL_MAX_TOKENS)
                    .cloned()
                {
                    let expanded = searcher.search_literal_with_candidate_universe(
                        &SearchTextQuery {
                            query: token,
                            path_regex: None,
                            limit: lexical_working_limit,
                        },
                        &candidate_universe,
                    )?;
                    merge_hybrid_lexical_search_output(
                        &mut lexical_output,
                        expanded,
                        lexical_working_limit,
                    );
                    if lexical_output.matches.len() >= lexical_working_limit {
                        break;
                    }
                }
            }

            let should_run_literal_phrase_expansion = lexical_output.matches.len() < query.limit
                && (!wants_path_witness_recall || lexical_output.matches.is_empty());
            if should_run_literal_phrase_expansion {
                let expanded = searcher.search_literal_with_candidate_universe(
                    &SearchTextQuery {
                        query: query_text.clone(),
                        path_regex: None,
                        limit: lexical_working_limit,
                    },
                    &candidate_universe,
                )?;
                merge_hybrid_lexical_search_output(
                    &mut lexical_output,
                    expanded,
                    lexical_working_limit,
                );
            }

            let should_run_regex_expansion = lexical_output.matches.len() < lexical_working_limit
                && (!wants_path_witness_recall || lexical_output.matches.is_empty());
            if should_run_regex_expansion {
                if let Some(token_regex) = build_hybrid_lexical_recall_regex(&query_text) {
                    let expanded_query = SearchTextQuery {
                        query: token_regex,
                        path_regex: None,
                        limit: lexical_working_limit,
                    };
                    let expanded =
                        search_regex_with_universe(searcher, &expanded_query, &candidate_universe)?;
                    merge_hybrid_lexical_search_output(
                        &mut lexical_output,
                        expanded,
                        lexical_working_limit,
                    );
                }
            }
        }

        if wants_path_witness_recall {
            let witness_started_at = Instant::now();
            witness_output = searcher.search_path_witness_recall_in_universe(
                &query_text,
                &candidate_universe,
                &normalized_filters,
                path_witness_working_limit,
                &ranking_intent,
            )?;
            witness_scoring_elapsed_us = witness_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX);
        }
    }
    let scan_elapsed_us = scan_started_at
        .elapsed()
        .as_micros()
        .try_into()
        .unwrap_or(u64::MAX);
    let lexical_hits = build_hybrid_lexical_hits_with_intent(
        &lexical_output.matches,
        &ranking_intent,
        &query_text,
    );
    let witness_hits = build_hybrid_path_witness_hits_with_intent(
        &witness_output.matches,
        &ranking_intent,
        &query_text,
    );
    let merged_ranking_matches = merged_ranking_matches_with_witness(
        &lexical_output.matches,
        &witness_output.matches,
        path_witness_working_limit,
    );
    let ranking_lexical_hits = if witness_output.matches.is_empty() {
        lexical_hits.clone()
    } else {
        build_hybrid_lexical_hits_with_intent(&merged_ranking_matches, &ranking_intent, &query_text)
    };
    let graph_seed_matches = if prefer_graph_over_path_witness {
        merged_ranking_matches
            .iter()
            .filter(|matched| {
                matches!(
                    SymbolLanguage::from_path(Path::new(&matched.path)),
                    Some(SymbolLanguage::Php | SymbolLanguage::Blade)
                )
            })
            .cloned()
            .collect::<Vec<TextMatch>>()
    } else {
        merged_ranking_matches.clone()
    };
    let has_exact_anchor_graph_seed = graph_seed_matches.iter().any(|matched| {
        matches!(
            SymbolLanguage::from_path(Path::new(&matched.path)),
            Some(SymbolLanguage::Php | SymbolLanguage::Blade)
        ) && hybrid_path_has_exact_stem_match(&matched.path, &exact_terms)
    });
    // Path-witness queries are dominated by file-surface evidence; graph expansion for common
    // stems like main/config mostly adds symbol extraction cost without improving the top-k set.
    let skip_graph_for_path_witness_intent = wants_path_witness_recall
        && !(ranking_intent.wants_jobs_listeners_witnesses
            || ranking_intent.wants_commands_middleware_witnesses
            || has_exact_anchor_graph_seed);
    let skip_graph_for_simple_literal_query = !prefer_graph_over_path_witness
        && !wants_path_witness_recall
        && exact_terms.len() == 1
        && !has_exact_anchor_graph_seed;
    let graph_started_at = Instant::now();
    let graph_hits = if skip_graph_for_path_witness_intent || skip_graph_for_simple_literal_query {
        Vec::new()
    } else {
        search_graph_channel_hits(
            searcher,
            &query_text,
            &candidate_universe,
            &graph_seed_matches,
            query.limit,
        )?
    };
    let graph_expansion_elapsed_us = graph_started_at
        .elapsed()
        .as_micros()
        .try_into()
        .unwrap_or(u64::MAX);
    let lexical_only_fast_path =
        witness_hits.is_empty() && graph_hits.is_empty() && semantic_channel_result.hits.is_empty();
    let total_rank_input_count = ranking_lexical_hits.len()
        + witness_hits.len()
        + graph_hits.len()
        + semantic_channel_result.hits.len();
    let (
        ranked_anchors,
        grouped_matches,
        matches,
        anchor_blending_sample,
        document_aggregation_sample,
        final_diversification_sample,
    ) = if lexical_only_fast_path {
        let blend_started_at = Instant::now();
        let ranked_anchors = super::rank_lexical_hybrid_hits(&ranking_lexical_hits, query.weights)?;
        let anchor_blending_sample = SearchStageSample::new(
            blend_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            ranking_lexical_hits.len(),
            ranked_anchors.len(),
        );
        let aggregation_started_at = Instant::now();
        let grouped_matches =
            super::group_hybrid_ranked_evidence(ranked_anchors.clone(), query.weights, query.limit);
        let document_aggregation_sample = SearchStageSample::new(
            aggregation_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            ranked_anchors.len(),
            grouped_matches.len(),
        );
        let diversification_started_at = Instant::now();
        let matches = super::diversify_hybrid_ranked_evidence(
            grouped_matches.clone(),
            query.limit,
            &query_text,
        );
        let final_diversification_sample = SearchStageSample::new(
            diversification_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            document_aggregation_sample.output_count,
            matches.len(),
        );
        (
            ranked_anchors,
            grouped_matches,
            matches,
            anchor_blending_sample,
            document_aggregation_sample,
            final_diversification_sample,
        )
    } else {
        let blend_started_at = Instant::now();
        let ranked_anchors = rank_hybrid_anchor_evidence_for_query_with_witness(
            &ranking_lexical_hits,
            &witness_hits,
            &graph_hits,
            &semantic_channel_result.hits,
            query.weights,
            query.limit.saturating_mul(4).max(32),
            &query_text,
        )?;
        let anchor_blending_sample = SearchStageSample::new(
            blend_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            total_rank_input_count,
            ranked_anchors.len(),
        );
        let aggregation_started_at = Instant::now();
        let grouped_matches = super::group_hybrid_ranked_evidence(
            ranked_anchors.clone(),
            query.weights,
            query.limit.saturating_mul(4).max(32),
        );
        let document_aggregation_sample = SearchStageSample::new(
            aggregation_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            ranked_anchors.len(),
            grouped_matches.len(),
        );
        let diversification_started_at = Instant::now();
        let matches = super::diversify_hybrid_ranked_evidence(
            grouped_matches.clone(),
            query.limit,
            &query_text,
        );
        let final_diversification_sample = SearchStageSample::new(
            diversification_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            document_aggregation_sample.output_count,
            matches.len(),
        );
        (
            ranked_anchors,
            grouped_matches,
            matches,
            anchor_blending_sample,
            document_aggregation_sample,
            final_diversification_sample,
        )
    };
    let matches = super::policy::apply_post_selection_guardrails(
        matches,
        &grouped_matches,
        &witness_hits,
        &ranking_intent,
        &query_text,
        query.limit,
    );
    let mut diagnostics = lexical_output.diagnostics.clone();
    merge_execution_diagnostics(&mut diagnostics, witness_output.diagnostics.clone());
    let graph_hit_count = graph_hits.len();
    let semantic_hit_count = semantic_channel_result.stats.hit_count;
    let mut channel_results = vec![
        ChannelResult::new(
            EvidenceChannel::LexicalManifest,
            lexical_hits,
            ChannelHealth::ok(),
            search_diagnostics_to_channel_diagnostics(&lexical_output.diagnostics),
            ChannelStats {
                candidate_count: lexical_output.matches.len(),
                hit_count: lexical_output.matches.len(),
                match_count: 0,
            },
        ),
        if wants_path_witness_recall {
            ChannelResult::new(
                EvidenceChannel::PathSurfaceWitness,
                witness_hits,
                ChannelHealth::ok(),
                search_diagnostics_to_channel_diagnostics(&witness_output.diagnostics),
                ChannelStats {
                    candidate_count: witness_output.matches.len(),
                    hit_count: witness_output.matches.len(),
                    match_count: 0,
                },
            )
        } else {
            empty_channel_result(
                EvidenceChannel::PathSurfaceWitness,
                ChannelHealthStatus::Filtered,
                Some("path/surface witness recall not requested for query intent".to_owned()),
            )
        },
        if skip_graph_for_path_witness_intent {
            empty_channel_result(
                EvidenceChannel::GraphPrecise,
                ChannelHealthStatus::Filtered,
                Some("graph channel skipped for path-witness oriented query intent".to_owned()),
            )
        } else if skip_graph_for_simple_literal_query {
            empty_channel_result(
                EvidenceChannel::GraphPrecise,
                ChannelHealthStatus::Filtered,
                Some(
                    "graph channel skipped for simple literal query without php/blade seeds"
                        .to_owned(),
                ),
            )
        } else {
            ChannelResult::new(
                EvidenceChannel::GraphPrecise,
                graph_hits,
                ChannelHealth::ok(),
                Vec::new(),
                ChannelStats {
                    candidate_count: merged_ranking_matches.len(),
                    hit_count: merged_ranking_matches.len().min(query.limit),
                    match_count: 0,
                },
            )
        },
        semantic_channel_result,
    ];
    for result in &mut channel_results {
        result.stats.match_count = match_count_for_hits(&matches, &result.hits);
        if result.channel == EvidenceChannel::Semantic {
            result.stats.hit_count = result.stats.match_count;
        }
    }
    let note = hybrid_execution_note_from_channel_results(
        query.semantic,
        searcher.config.semantic_runtime.enabled,
        &channel_results,
    );

    Ok(SearchHybridExecutionOutput {
        matches,
        ranked_anchors,
        diagnostics,
        channel_results,
        note,
        stage_attribution: Some(SearchStageAttribution {
            candidate_intake: SearchStageSample::new(
                candidate_intake_elapsed_us,
                candidate_repository_count,
                candidate_file_count,
            ),
            freshness_validation: SearchStageSample::new(
                freshness_validation_elapsed_us,
                candidate_repository_count,
                manifest_backed_repository_count,
            ),
            scan: SearchStageSample::new(
                scan_elapsed_us,
                candidate_file_count,
                lexical_output.matches.len(),
            ),
            witness_scoring: SearchStageSample::new(
                witness_scoring_elapsed_us,
                candidate_file_count,
                witness_output.matches.len(),
            ),
            graph_expansion: SearchStageSample::new(
                graph_expansion_elapsed_us,
                graph_seed_matches.len(),
                graph_hit_count,
            ),
            semantic_retrieval: SearchStageSample::new(
                semantic_retrieval_elapsed_us,
                semantic_limit,
                semantic_hit_count,
            ),
            anchor_blending: anchor_blending_sample,
            document_aggregation: document_aggregation_sample,
            final_diversification: final_diversification_sample,
        }),
    })
}

fn merge_execution_diagnostics(
    base: &mut SearchExecutionDiagnostics,
    supplement: SearchExecutionDiagnostics,
) {
    base.entries.extend(supplement.entries);
    sort_search_diagnostics_deterministically(&mut base.entries);
    base.entries.dedup();
}

fn merged_ranking_matches_with_witness(
    lexical_matches: &[TextMatch],
    witness_matches: &[TextMatch],
    limit: usize,
) -> Vec<TextMatch> {
    let mut combined = Vec::new();
    let mut seen = BTreeSet::new();
    for found in witness_matches.iter().chain(lexical_matches.iter()) {
        if seen.insert((
            found.repository_id.clone(),
            found.path.clone(),
            found.line,
            found.column,
            found.excerpt.clone(),
        )) {
            combined.push(found.clone());
        }
    }
    combined.truncate(limit);
    combined
}

fn prefers_graph_over_path_witness(intent: &HybridRankingIntent) -> bool {
    (intent.wants_jobs_listeners_witnesses || intent.wants_commands_middleware_witnesses)
        && !intent.wants_entrypoint_build_flow
        && !intent.wants_ci_workflow_witnesses
        && !intent.wants_scripts_ops_witnesses
        && !intent.wants_runtime_config_artifacts
        && !intent.wants_examples
        && !intent.wants_benchmarks
        && !intent.wants_test_witness_recall
        && !intent.wants_laravel_ui_witnesses
}

fn prefers_compact_lexical_seed_set(intent: &HybridRankingIntent, exact_terms: &[String]) -> bool {
    exact_terms.len() == 1
        && !intent.wants_path_witness_recall()
        && !intent.wants_docs
        && !intent.wants_contracts
        && !intent.wants_error_taxonomy
        && !intent.wants_tool_contracts
        && !intent.wants_benchmarks
        && !intent.wants_examples
        && !intent.wants_jobs_listeners_witnesses
        && !intent.wants_commands_middleware_witnesses
}

fn search_regex_with_universe(
    searcher: &TextSearcher,
    query: &SearchTextQuery,
    candidate_universe: &super::SearchCandidateUniverse,
) -> FriggResult<SearchExecutionOutput> {
    let matcher =
        super::compile_safe_regex(&query.query).map_err(super::regex_error_to_frigg_error)?;
    let prefilter_plan = super::build_regex_prefilter_plan(&query.query);
    searcher.search_regex_with_candidate_universe(
        query,
        candidate_universe,
        matcher,
        prefilter_plan,
    )
}

fn search_case_insensitive_recall_terms_with_universe(
    searcher: &TextSearcher,
    terms: &[String],
    limit: usize,
    candidate_universe: &super::SearchCandidateUniverse,
    path_prefilter: bool,
) -> FriggResult<SearchExecutionOutput> {
    if limit == 0 || terms.is_empty() {
        return Ok(SearchExecutionOutput::default());
    }

    let matcher = AhoCorasickBuilder::new()
        .ascii_case_insensitive(true)
        .build(terms.iter().map(String::as_str))
        .map_err(|err| FriggError::InvalidInput(format!("invalid recall terms: {err}")))?;
    let query_text = terms.join(" ");
    let scoped_query = SearchTextQuery {
        query: query_text.clone(),
        path_regex: if path_prefilter {
            build_recall_path_regex(terms)
        } else {
            None
        },
        limit,
    };
    let search_lines = |query: &SearchTextQuery| {
        searcher.search_with_streaming_lines_in_universe(
            query,
            candidate_universe,
            |line, columns| {
                columns.clear();
                columns.extend(
                    matcher
                        .find_iter(line)
                        .filter(|mat| is_ascii_word_boundary_match(line, mat.start(), mat.end()))
                        .map(|mat| mat.start() + 1),
                );
            },
        )
    };
    let scoped_output = search_lines(&scoped_query)?;
    if !scoped_output.matches.is_empty() || scoped_query.path_regex.is_none() {
        return Ok(scoped_output);
    }

    search_lines(&SearchTextQuery {
        query: query_text,
        path_regex: None,
        limit,
    })
}

fn distinct_match_document_count(matches: &[TextMatch]) -> usize {
    matches
        .iter()
        .map(|matched| (&matched.repository_id, &matched.path))
        .collect::<BTreeSet<_>>()
        .len()
}

fn is_ascii_word_boundary_match(line: &str, start: usize, end: usize) -> bool {
    let bytes = line.as_bytes();
    (start == 0 || !is_ascii_word_byte(bytes[start - 1]))
        && (end == bytes.len() || !is_ascii_word_byte(bytes[end]))
}

fn is_ascii_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn build_recall_path_regex(terms: &[String]) -> Option<Regex> {
    let pattern = terms
        .iter()
        .map(|term| escape(term))
        .collect::<Vec<_>>()
        .join("|");
    if pattern.is_empty() {
        return None;
    }

    Regex::new(&format!("(?i)(?:{pattern})")).ok()
}
