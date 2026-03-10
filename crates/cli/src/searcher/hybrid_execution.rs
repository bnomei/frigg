use crate::domain::{FriggError, FriggResult};
use crate::settings::SemanticRuntimeCredentials;

use super::{
    HYBRID_LEXICAL_RECALL_MAX_TOKENS, HybridExecutionNote, HybridRankingIntent,
    HybridSemanticStatus, SearchFilters, SearchHybridExecutionOutput, SearchHybridQuery,
    SearchTextQuery, TextSearcher, build_hybrid_lexical_hits_with_intent,
    build_hybrid_lexical_recall_regex, hybrid_lexical_recall_tokens,
    merge_hybrid_lexical_search_output, merge_hybrid_path_witness_recall_output,
    rank_hybrid_evidence_for_query, retain_semantic_hits_for_query, search_graph_channel_hits,
    search_semantic_channel_hits,
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
    if query_text.is_empty() {
        return Err(FriggError::InvalidInput(
            "hybrid search query must not be empty".to_owned(),
        ));
    }
    if query.limit == 0 {
        return Ok(SearchHybridExecutionOutput::default());
    }

    let lexical_limit = if ranking_intent.wants_path_witness_recall() {
        query
            .limit
            .saturating_mul(4)
            .max(searcher.config.max_search_results)
            .max(24)
    } else {
        query.limit.max(searcher.config.max_search_results)
    };
    let semantic_limit = query.limit.max(searcher.config.max_search_results);
    let mut lexical_output = searcher.search_literal_with_filters_diagnostics(
        SearchTextQuery {
            query: query_text.clone(),
            path_regex: None,
            limit: lexical_limit,
        },
        filters.clone(),
    )?;

    let semantic_requested = query
        .semantic
        .unwrap_or(searcher.config.semantic_runtime.enabled);
    let strict_semantic = searcher.config.semantic_runtime.strict_mode;
    let (semantic_hits, mut note) = if matches!(query.semantic, Some(false)) {
        (
            Vec::new(),
            HybridExecutionNote {
                semantic_requested,
                semantic_enabled: false,
                semantic_status: HybridSemanticStatus::Disabled,
                semantic_reason: Some("semantic channel disabled by request toggle".to_owned()),
                semantic_candidate_count: 0,
                semantic_hit_count: 0,
                semantic_match_count: 0,
            },
        )
    } else if !searcher.config.semantic_runtime.enabled {
        (
            Vec::new(),
            HybridExecutionNote {
                semantic_requested,
                semantic_enabled: false,
                semantic_status: HybridSemanticStatus::Disabled,
                semantic_reason: Some(
                    "semantic runtime disabled in active configuration".to_owned(),
                ),
                semantic_candidate_count: 0,
                semantic_hit_count: 0,
                semantic_match_count: 0,
            },
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
                (
                    semantic_hits,
                    HybridExecutionNote {
                        semantic_requested,
                        semantic_enabled: false,
                        semantic_status: outcome.status,
                        semantic_reason: outcome.reason,
                        semantic_candidate_count: outcome.candidate_count,
                        semantic_hit_count,
                        semantic_match_count: 0,
                    },
                )
            }
            Err(err) => {
                if strict_semantic {
                    return Err(FriggError::Internal(format!(
                        "semantic_status=strict_failure: {err}"
                    )));
                }
                (
                    Vec::new(),
                    HybridExecutionNote {
                        semantic_requested,
                        semantic_enabled: false,
                        semantic_status: HybridSemanticStatus::Degraded,
                        semantic_reason: Some(err.to_string()),
                        semantic_candidate_count: 0,
                        semantic_hit_count: 0,
                        semantic_match_count: 0,
                    },
                )
            }
        }
    };

    let should_expand_lexical = (lexical_output.matches.len() < query.limit
        || ranking_intent.wants_path_witness_recall())
        && (note.semantic_status != HybridSemanticStatus::Ok
            || semantic_hits.is_empty()
            || ranking_intent.wants_path_witness_recall()
            || ranking_intent.wants_docs
            || ranking_intent.wants_contracts
            || ranking_intent.wants_error_taxonomy
            || ranking_intent.wants_tool_contracts
            || ranking_intent.wants_benchmarks);
    if should_expand_lexical {
        let recall_tokens = hybrid_lexical_recall_tokens(&query_text);

        for token in recall_tokens
            .iter()
            .take(HYBRID_LEXICAL_RECALL_MAX_TOKENS)
            .cloned()
        {
            let expanded = searcher.search_literal_with_filters_diagnostics(
                SearchTextQuery {
                    query: token,
                    path_regex: None,
                    limit: lexical_limit,
                },
                filters.clone(),
            )?;
            merge_hybrid_lexical_search_output(&mut lexical_output, expanded, lexical_limit);
            if lexical_output.matches.len() >= lexical_limit {
                break;
            }
        }

        if lexical_output.matches.len() < lexical_limit {
            if let Some(token_regex) = build_hybrid_lexical_recall_regex(&query_text) {
                let expanded = searcher.search_regex_with_filters_diagnostics(
                    SearchTextQuery {
                        query: token_regex,
                        path_regex: None,
                        limit: lexical_limit,
                    },
                    filters.clone(),
                )?;
                merge_hybrid_lexical_search_output(&mut lexical_output, expanded, lexical_limit);
            }
        }

        if ranking_intent.wants_path_witness_recall() {
            let path_recall = searcher.search_path_witness_recall_with_filters(
                &query_text,
                &filters,
                lexical_limit,
                &ranking_intent,
            )?;
            merge_hybrid_path_witness_recall_output(
                &mut lexical_output,
                path_recall,
                lexical_limit,
            );
        }
    }
    let lexical_hits = build_hybrid_lexical_hits_with_intent(
        &lexical_output.matches,
        &ranking_intent,
        &query_text,
    );
    let graph_hits = search_graph_channel_hits(
        searcher,
        &query_text,
        &filters,
        &lexical_output.matches,
        query.limit,
    )?;

    let matches = rank_hybrid_evidence_for_query(
        &lexical_hits,
        &graph_hits,
        &semantic_hits,
        query.weights,
        query.limit,
        &query_text,
    )?;
    note.semantic_match_count = matches
        .iter()
        .filter(|evidence| evidence.semantic_score > 0.0)
        .count();
    note.semantic_enabled = note.semantic_match_count > 0;

    Ok(SearchHybridExecutionOutput {
        matches,
        diagnostics: lexical_output.diagnostics,
        note,
    })
}
