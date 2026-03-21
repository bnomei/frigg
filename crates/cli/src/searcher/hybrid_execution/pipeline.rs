use std::collections::BTreeSet;
use std::path::Path;
use std::time::Instant;

use regex::{Regex, escape};

use crate::domain::{
    ChannelHealthStatus, ChannelResult, ChannelStats, EvidenceChannel, FriggError, FriggResult,
    model::TextMatch,
};
use crate::languages::{LanguageSupportCapability, SymbolLanguage};
use crate::searcher::lexical_channel::{
    HybridLexicalQueryFeatures, HybridPathWitnessQueryContext,
    build_hybrid_lexical_hits_with_features, candidate_universe_delta,
};
use crate::searcher::lexical_recall::build_hybrid_lexical_recall_regex_from_terms;
use crate::searcher::policy;
use crate::searcher::{
    HYBRID_LEXICAL_RECALL_MAX_TOKENS, HybridRankingIntent, SearchCandidateUniverse,
    SearchExecutionOutput, SearchFilters, SearchHybridExecutionOutput, SearchHybridQuery,
    SearchStageAttribution, SearchStageSample, SearchTextQuery, TextSearcher,
    apply_post_selection_guardrails_with_trace, build_hybrid_lexical_recall_regex,
    build_hybrid_path_witness_hits_with_intent, build_regex_prefilter_plan, compile_safe_regex,
    empty_channel_result, hybrid_execution_note_from_channel_results, hybrid_lexical_recall_tokens,
    hybrid_path_has_exact_stem_match, merge_hybrid_lexical_search_output, normalize_search_filters,
    regex_error_to_frigg_error, search_graph_channel_hits, search_semantic_channel_hits,
};
use crate::settings::SemanticRuntimeCredentials;

use super::fusion::{
    build_hybrid_channel_results, merge_execution_diagnostics, merged_ranking_matches_with_witness,
    prefers_compact_lexical_seed_set, prefers_graph_over_path_witness, run_hybrid_fusion,
};

pub(in crate::searcher) fn search_hybrid_with_filters_using_executor(
    searcher: &TextSearcher,
    query: SearchHybridQuery,
    filters: SearchFilters,
    credentials: &SemanticRuntimeCredentials,
    semantic_executor: &dyn crate::searcher::SemanticRuntimeQueryEmbeddingExecutor,
    capture_post_selection_trace: bool,
) -> FriggResult<SearchHybridExecutionOutput> {
    let query_text = query.query.trim().to_owned();
    let ranking_intent = HybridRankingIntent::from_query(&query_text);
    let lexical_query_features = HybridLexicalQueryFeatures::from_query_text(&query_text);
    let prefer_graph_over_path_witness = prefers_graph_over_path_witness(&ranking_intent);
    let wants_path_witness_recall = ranking_intent.wants_path_witness_recall();
    let exact_terms = lexical_query_features.exact_terms();
    let prefers_compact_seed_set = prefers_compact_lexical_seed_set(&ranking_intent, exact_terms);
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
            query.limit.saturating_add(2).max(8)
        } else {
            query.limit.saturating_mul(2).max(16)
        }
    } else if prefers_compact_seed_set {
        query.limit.saturating_add(7).max(12)
    } else {
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
    let path_witness_query_context = wants_path_witness_recall
        .then(|| HybridPathWitnessQueryContext::from_query_text(&query_text));
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
    let initial_lexical_seed_is_full_literal =
        !lexical_seeded_with_terms && !prefers_compact_seed_set;
    let scan_started_at = Instant::now();
    let mut lexical_output = if lexical_seeded_with_terms {
        search_case_insensitive_recall_terms_with_universe(
            searcher,
            &lexical_seed_terms,
            lexical_working_limit,
            lexical_candidate_universe,
            true,
        )?
    } else if prefers_compact_seed_set {
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
        if let Some(delta_candidate_universe) =
            candidate_universe_delta(&candidate_universe, lexical_candidate_universe)
        {
            let supplemental = search_case_insensitive_recall_terms_with_universe(
                searcher,
                &lexical_seed_terms,
                lexical_working_limit,
                &delta_candidate_universe,
                false,
            )?;
            merge_hybrid_lexical_search_output(
                &mut lexical_output,
                supplemental,
                lexical_working_limit,
            );
        }
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
            query.limit,
            credentials,
            semantic_executor,
        ) {
            Ok(outcome) => ChannelResult::new(
                EvidenceChannel::Semantic,
                outcome.hits,
                outcome.health,
                outcome.diagnostics,
                ChannelStats {
                    candidate_count: outcome.candidate_count,
                    hit_count: outcome.hit_count,
                    match_count: 0,
                },
            ),
            Err(err) => {
                if strict_semantic {
                    return Err(FriggError::StrictSemanticFailure {
                        reason: err.to_string(),
                    });
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
                && (!wants_path_witness_recall || lexical_output.matches.is_empty())
                && !initial_lexical_seed_is_full_literal;
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
    let lexical_hits = build_hybrid_lexical_hits_with_features(
        &lexical_output.matches,
        &ranking_intent,
        &lexical_query_features,
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
        build_hybrid_lexical_hits_with_features(
            &merged_ranking_matches,
            &ranking_intent,
            &lexical_query_features,
        )
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
        ) && hybrid_path_has_exact_stem_match(&matched.path, exact_terms)
    });
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
    let total_rank_input_count = ranking_lexical_hits.len()
        + witness_hits.len()
        + graph_hits.len()
        + semantic_channel_result.hits.len();
    let coverage_hints = searcher
        .projection_store_service
        .coverage_hint_keys_for_repositories(&candidate_universe.repositories);
    let fusion_result = run_hybrid_fusion(
        &ranking_lexical_hits,
        &witness_hits,
        &graph_hits,
        &semantic_channel_result.hits,
        query.weights,
        query.limit,
        &query_text,
        total_rank_input_count,
        &coverage_hints,
    )?;
    let lexical_only_mode = semantic_channel_result.health.status != ChannelHealthStatus::Ok
        || semantic_channel_result.hits.is_empty();
    let (matches, post_selection_trace) = if capture_post_selection_trace {
        apply_post_selection_guardrails_with_trace(
            fusion_result.matches,
            &fusion_result.coverage_grouped_pool,
            &witness_hits,
            &ranking_intent,
            &query_text,
            lexical_only_mode,
            query.limit,
        )
    } else {
        (
            policy::apply_post_selection_guardrails(
                fusion_result.matches,
                &fusion_result.coverage_grouped_pool,
                &witness_hits,
                &ranking_intent,
                &query_text,
                lexical_only_mode,
                query.limit,
            ),
            None,
        )
    };
    let mut diagnostics = lexical_output.diagnostics.clone();
    merge_execution_diagnostics(&mut diagnostics, witness_output.diagnostics.clone());
    let lexical_match_count = lexical_output.matches.len();
    let lexical_backend = lexical_output.lexical_backend;
    let lexical_backend_note = lexical_output.lexical_backend_note.clone();
    let witness_match_count = witness_output.matches.len();
    let graph_hit_count = graph_hits.len();
    let semantic_hit_count = semantic_channel_result.stats.hit_count;
    let channel_results = build_hybrid_channel_results(
        lexical_output,
        witness_output,
        lexical_hits,
        witness_hits,
        graph_hits,
        merged_ranking_matches,
        semantic_channel_result,
        &matches,
        wants_path_witness_recall,
        skip_graph_for_path_witness_intent,
        skip_graph_for_simple_literal_query,
        query.limit,
    );
    let mut note = hybrid_execution_note_from_channel_results(
        query.semantic,
        searcher.config.semantic_runtime.enabled,
        &channel_results,
    );
    note.lexical_backend = lexical_backend;
    note.lexical_backend_note = lexical_backend_note;

    Ok(SearchHybridExecutionOutput {
        matches,
        ranked_anchors: fusion_result.ranked_anchors,
        coverage_grouped_pool: fusion_result.coverage_grouped_pool,
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
                lexical_match_count,
            ),
            witness_scoring: SearchStageSample::new(
                witness_scoring_elapsed_us,
                candidate_file_count,
                witness_match_count,
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
            anchor_blending: fusion_result.anchor_blending_sample,
            document_aggregation: fusion_result.document_aggregation_sample,
            final_diversification: fusion_result.final_diversification_sample,
        }),
        post_selection_trace,
    })
}

fn search_regex_with_universe(
    searcher: &TextSearcher,
    query: &SearchTextQuery,
    candidate_universe: &SearchCandidateUniverse,
) -> FriggResult<SearchExecutionOutput> {
    let matcher = compile_safe_regex(&query.query).map_err(regex_error_to_frigg_error)?;
    let prefilter_plan = build_regex_prefilter_plan(&query.query);
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
    candidate_universe: &SearchCandidateUniverse,
    path_prefilter: bool,
) -> FriggResult<SearchExecutionOutput> {
    if limit == 0 || terms.is_empty() {
        return Ok(SearchExecutionOutput::default());
    }

    let regex_query = build_hybrid_lexical_recall_regex_from_terms(terms).ok_or_else(|| {
        FriggError::InvalidInput(
            "invalid recall terms: could not build lexical recall regex".to_owned(),
        )
    })?;
    let scoped_query = SearchTextQuery {
        query: regex_query.clone(),
        path_regex: if path_prefilter {
            build_recall_path_regex(terms)
        } else {
            None
        },
        limit,
    };
    let scoped_output = search_regex_with_universe(searcher, &scoped_query, candidate_universe)?;
    if !scoped_output.matches.is_empty() || scoped_query.path_regex.is_none() {
        return Ok(scoped_output);
    }

    search_regex_with_universe(
        searcher,
        &SearchTextQuery {
            query: regex_query,
            path_regex: None,
            limit,
        },
        candidate_universe,
    )
}

fn distinct_match_document_count(matches: &[TextMatch]) -> usize {
    matches
        .iter()
        .map(|matched| (&matched.repository_id, &matched.path))
        .collect::<BTreeSet<_>>()
        .len()
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
