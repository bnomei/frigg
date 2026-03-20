use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::domain::{FriggError, FriggResult};
use crate::searcher::{
    HybridRankedEvidence, HybridRankingIntent, SearchHybridExecutionOutput,
    path_quality_rule_trace, path_witness_rule_trace, selection_rule_trace,
};

use super::{
    HybridPlaybookCandidateTraceSnapshot, HybridPlaybookChannelHitSnapshot,
    HybridPlaybookChannelTrace, HybridPlaybookProbeOutcome, HybridPlaybookRankedHitSnapshot,
    HybridPlaybookTracePacket, LoadedHybridPlaybookRegression,
};

fn sanitize_trace_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    let mut last_was_dash = false;
    for ch in value.chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            sanitized.push(lowered);
            last_was_dash = false;
        } else if !last_was_dash {
            sanitized.push('-');
            last_was_dash = true;
        }
    }
    sanitized.trim_matches('-').to_owned()
}

fn collect_channel_traces(
    output: &SearchHybridExecutionOutput,
    trace_limit: usize,
) -> Vec<HybridPlaybookChannelTrace> {
    output
        .channel_results
        .iter()
        .map(|result| HybridPlaybookChannelTrace {
            channel: result.channel.as_str().to_owned(),
            health_status: result.health.status.as_str().to_owned(),
            health_reason: result.health.reason.clone(),
            candidate_count: result.stats.candidate_count,
            hit_count: result.stats.hit_count,
            match_count: result.stats.match_count,
            hits: result
                .hits
                .iter()
                .take(trace_limit)
                .enumerate()
                .map(|(index, hit)| HybridPlaybookChannelHitSnapshot {
                    rank: index + 1,
                    repository_id: hit.document.repository_id.clone(),
                    path: hit.document.path.clone(),
                    line: hit.document.line,
                    column: hit.document.column,
                    score: hit.raw_score,
                    excerpt: hit.excerpt.clone(),
                    provenance_ids: hit.provenance_ids.clone(),
                })
                .collect(),
        })
        .collect()
}

fn collect_ranked_hit_snapshots(
    hits: &[HybridRankedEvidence],
    trace_limit: usize,
) -> Vec<HybridPlaybookRankedHitSnapshot> {
    hits.iter()
        .take(trace_limit)
        .enumerate()
        .map(|(index, hit)| HybridPlaybookRankedHitSnapshot {
            rank: index + 1,
            repository_id: hit.document.repository_id.clone(),
            path: hit.document.path.clone(),
            line: hit.document.line,
            column: hit.document.column,
            blended_score: hit.blended_score,
            lexical_score: hit.lexical_score,
            witness_score: hit.witness_score,
            graph_score: hit.graph_score,
            semantic_score: hit.semantic_score,
            excerpt: hit.excerpt.clone(),
            lexical_sources: hit.lexical_sources.clone(),
            witness_sources: hit.witness_sources.clone(),
            graph_sources: hit.graph_sources.clone(),
            semantic_sources: hit.semantic_sources.clone(),
        })
        .collect()
}

fn collect_candidate_traces(
    candidates: &[HybridRankedEvidence],
    intent: &HybridRankingIntent,
    query_text: &str,
    trace_limit: usize,
) -> Vec<HybridPlaybookCandidateTraceSnapshot> {
    let mut selected: Vec<HybridRankedEvidence> = Vec::new();
    let mut traces = Vec::new();
    for (index, candidate) in candidates.iter().take(trace_limit).enumerate() {
        traces.push(HybridPlaybookCandidateTraceSnapshot {
            rank: index + 1,
            path: candidate.document.path.clone(),
            selection_rules: selection_rule_trace(candidate.clone(), &selected, intent, query_text),
            path_witness_rules: path_witness_rule_trace(
                &candidate.document.path,
                intent,
                query_text,
            ),
            path_quality_rules: path_quality_rule_trace(&candidate.document.path, intent),
        });
        selected.push(candidate.clone());
    }
    traces
}

fn collect_post_selection_repairs(
    output: &SearchHybridExecutionOutput,
) -> Vec<BTreeMap<String, String>> {
    output
        .post_selection_trace
        .as_ref()
        .map(|trace| {
            trace
                .events
                .iter()
                .map(|event| {
                    let mut entry = BTreeMap::new();
                    entry.insert("rule_id".to_owned(), event.rule_id.to_owned());
                    entry.insert(
                        "rule_stage".to_owned(),
                        format!("{:?}", event.rule_stage).to_ascii_lowercase(),
                    );
                    entry.insert(
                        "action".to_owned(),
                        format!("{:?}", event.action).to_ascii_lowercase(),
                    );
                    entry.insert("candidate_path".to_owned(), event.candidate_path.clone());
                    entry.insert(
                        "replaced_path".to_owned(),
                        event.replaced_path.clone().unwrap_or_default(),
                    );
                    entry
                })
                .collect()
        })
        .unwrap_or_default()
}

pub(super) fn write_trace_packet(
    trace_root: &Path,
    regression: &LoadedHybridPlaybookRegression,
    output: &SearchHybridExecutionOutput,
    outcome: &HybridPlaybookProbeOutcome,
) -> FriggResult<String> {
    let trace_limit = regression.spec.top_k.max(10);
    let intent = HybridRankingIntent::from_query(&regression.spec.query);
    let file_name = format!(
        "{}.json",
        sanitize_trace_component(&regression.metadata.playbook_id)
    );
    let trace_path = trace_root.join(file_name);
    let packet = HybridPlaybookTracePacket {
        playbook_id: regression.metadata.playbook_id.clone(),
        file_name: regression
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        query: regression.spec.query.clone(),
        top_k: regression.spec.top_k,
        semantic_status: outcome.semantic_status.clone(),
        semantic_reason: outcome.semantic_reason.clone(),
        status_allowed: outcome.status_allowed,
        duration_ms: outcome.duration_ms,
        matched_paths: outcome.matched_paths.clone(),
        required_witness_groups: outcome.required_witness_groups.clone(),
        target_witness_groups: outcome.target_witness_groups.clone(),
        stage_attribution: output.stage_attribution.clone(),
        channels: collect_channel_traces(output, trace_limit),
        ranked_anchors: collect_ranked_hit_snapshots(&output.ranked_anchors, trace_limit),
        coverage_grouped_pool: collect_ranked_hit_snapshots(
            &output.coverage_grouped_pool,
            trace_limit,
        ),
        final_matches: collect_ranked_hit_snapshots(&output.matches, trace_limit),
        candidate_traces: collect_candidate_traces(
            &output.coverage_grouped_pool,
            &intent,
            &regression.spec.query,
            trace_limit,
        ),
        post_selection_repairs: collect_post_selection_repairs(output),
    };
    let payload = serde_json::to_string_pretty(&packet)
        .map_err(|error| FriggError::Internal(error.to_string()))?;
    if let Some(parent) = trace_path.parent() {
        fs::create_dir_all(parent).map_err(FriggError::Io)?;
    }
    fs::write(&trace_path, payload).map_err(FriggError::Io)?;
    Ok(trace_path.display().to_string())
}
