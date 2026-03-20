use std::collections::BTreeSet;
use std::time::Instant;

use crate::domain::{
    ChannelHealth, ChannelHealthStatus, ChannelResult, ChannelStats, EvidenceChannel, FriggResult,
    model::TextMatch,
};
use crate::searcher::ranker::{group_all_hybrid_ranked_evidence, rank_lexical_hybrid_hits};
use crate::searcher::reranker::{
    CoverageProjectionHintMap, build_coverage_grouped_pool, diversify_hybrid_ranked_evidence,
};
use crate::searcher::{
    HybridChannelWeights, HybridRankedEvidence, HybridRankingIntent, SearchExecutionDiagnostics,
    SearchExecutionOutput, SearchStageSample, empty_channel_result, match_count_for_hits,
    rank_hybrid_anchor_evidence_for_query_with_witness, search_diagnostics_to_channel_diagnostics,
    sort_search_diagnostics_deterministically,
};

pub(super) struct HybridFusionOutput {
    pub(super) ranked_anchors: Vec<HybridRankedEvidence>,
    pub(super) coverage_grouped_pool: Vec<HybridRankedEvidence>,
    pub(super) matches: Vec<HybridRankedEvidence>,
    pub(super) anchor_blending_sample: SearchStageSample,
    pub(super) document_aggregation_sample: SearchStageSample,
    pub(super) final_diversification_sample: SearchStageSample,
}

pub(super) fn run_hybrid_fusion(
    ranking_lexical_hits: &[crate::domain::EvidenceHit],
    witness_hits: &[crate::domain::EvidenceHit],
    graph_hits: &[crate::domain::EvidenceHit],
    semantic_hits: &[crate::domain::EvidenceHit],
    weights: HybridChannelWeights,
    limit: usize,
    query_text: &str,
    total_rank_input_count: usize,
    coverage_hints: &CoverageProjectionHintMap,
) -> FriggResult<HybridFusionOutput> {
    let lexical_only_fast_path =
        witness_hits.is_empty() && graph_hits.is_empty() && semantic_hits.is_empty();
    if lexical_only_fast_path {
        let blend_started_at = Instant::now();
        let ranked_anchors = rank_lexical_hybrid_hits(ranking_lexical_hits, weights)?;
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
        let grouped_matches = group_all_hybrid_ranked_evidence(ranked_anchors.clone(), weights);
        let document_aggregation_sample = SearchStageSample::new(
            aggregation_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            ranked_anchors.len(),
            grouped_matches.len(),
        );
        let coverage_grouped_pool =
            build_coverage_grouped_pool(grouped_matches.clone(), limit, limit, coverage_hints);
        let diversification_started_at = Instant::now();
        let matches =
            diversify_hybrid_ranked_evidence(coverage_grouped_pool.clone(), limit, query_text);
        let final_diversification_sample = SearchStageSample::new(
            diversification_started_at
                .elapsed()
                .as_micros()
                .try_into()
                .unwrap_or(u64::MAX),
            coverage_grouped_pool.len(),
            matches.len(),
        );
        return Ok(HybridFusionOutput {
            ranked_anchors,
            coverage_grouped_pool,
            matches,
            anchor_blending_sample,
            document_aggregation_sample,
            final_diversification_sample,
        });
    }

    let rank_limit = limit.saturating_mul(4).max(32);
    let blend_started_at = Instant::now();
    let ranked_anchors = rank_hybrid_anchor_evidence_for_query_with_witness(
        ranking_lexical_hits,
        witness_hits,
        graph_hits,
        semantic_hits,
        weights,
        rank_limit,
        query_text,
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
    let grouped_matches = group_all_hybrid_ranked_evidence(ranked_anchors.clone(), weights);
    let document_aggregation_sample = SearchStageSample::new(
        aggregation_started_at
            .elapsed()
            .as_micros()
            .try_into()
            .unwrap_or(u64::MAX),
        ranked_anchors.len(),
        grouped_matches.len(),
    );
    let coverage_grouped_pool =
        build_coverage_grouped_pool(grouped_matches.clone(), limit, rank_limit, coverage_hints);
    let diversification_started_at = Instant::now();
    let matches =
        diversify_hybrid_ranked_evidence(coverage_grouped_pool.clone(), limit, query_text);
    let final_diversification_sample = SearchStageSample::new(
        diversification_started_at
            .elapsed()
            .as_micros()
            .try_into()
            .unwrap_or(u64::MAX),
        coverage_grouped_pool.len(),
        matches.len(),
    );
    Ok(HybridFusionOutput {
        ranked_anchors,
        coverage_grouped_pool,
        matches,
        anchor_blending_sample,
        document_aggregation_sample,
        final_diversification_sample,
    })
}

pub(super) fn build_hybrid_channel_results(
    lexical_output: SearchExecutionOutput,
    witness_output: SearchExecutionOutput,
    lexical_hits: Vec<crate::domain::EvidenceHit>,
    witness_hits: Vec<crate::domain::EvidenceHit>,
    graph_hits: Vec<crate::domain::EvidenceHit>,
    merged_ranking_matches: Vec<TextMatch>,
    semantic_channel_result: ChannelResult,
    matches: &[HybridRankedEvidence],
    wants_path_witness_recall: bool,
    skip_graph_for_path_witness_intent: bool,
    skip_graph_for_simple_literal_query: bool,
    query_limit: usize,
) -> Vec<ChannelResult> {
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
                    hit_count: merged_ranking_matches.len().min(query_limit),
                    match_count: 0,
                },
            )
        },
        semantic_channel_result,
    ];
    for result in &mut channel_results {
        result.stats.match_count = match_count_for_hits(matches, &result.hits);
        if result.channel == EvidenceChannel::Semantic {
            result.stats.hit_count = result.stats.match_count;
        }
    }
    channel_results
}

pub(super) fn merge_execution_diagnostics(
    base: &mut SearchExecutionDiagnostics,
    supplement: SearchExecutionDiagnostics,
) {
    base.entries.extend(supplement.entries);
    sort_search_diagnostics_deterministically(&mut base.entries);
    base.entries.dedup();
}

pub(super) fn merged_ranking_matches_with_witness(
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

pub(super) fn prefers_graph_over_path_witness(intent: &HybridRankingIntent) -> bool {
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

pub(super) fn prefers_compact_lexical_seed_set(
    intent: &HybridRankingIntent,
    exact_terms: &[String],
) -> bool {
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
