use std::path::Path;
use std::time::Instant;

use crate::domain::FriggResult;
use crate::searcher::{SearchFilters, SearchHybridQuery, TextSearcher};

use super::trace::write_trace_packet;
use super::witness::{semantic_status_allowed, witness_outcomes};
use super::{
    HybridPlaybookProbeOutcome, HybridPlaybookRunSummary, HybridPlaybookWitnessOutcome,
    HybridWitnessMatchBy, LoadedHybridPlaybookRegression, load_hybrid_playbook_regressions,
};

fn failed_group_outcomes(
    groups: &[super::HybridWitnessGroup],
) -> Vec<HybridPlaybookWitnessOutcome> {
    groups
        .iter()
        .map(|group| HybridPlaybookWitnessOutcome {
            group_id: group.group_id.clone(),
            match_any: group.match_any.clone(),
            match_mode: group.match_mode,
            accepted_prefixes: group.accepted_prefixes.clone(),
            required_when: group.required_when,
            matched_by: HybridWitnessMatchBy::None,
            matched_path: None,
            passed: false,
        })
        .collect()
}

pub fn run_hybrid_playbook_regression(
    searcher: &TextSearcher,
    regression: &LoadedHybridPlaybookRegression,
    trace_root: Option<&Path>,
) -> HybridPlaybookProbeOutcome {
    let started = Instant::now();
    let query = SearchHybridQuery {
        query: regression.spec.query.clone(),
        limit: regression.spec.top_k,
        weights: Default::default(),
        semantic: Some(true),
    };
    let result = if trace_root.is_some() {
        searcher.search_hybrid_with_filters_with_trace(query, SearchFilters::default())
    } else {
        searcher.search_hybrid_with_filters(query, SearchFilters::default())
    };

    match result {
        Ok(output) => {
            let semantic_status = output.note.semantic_status.as_str().to_owned();
            let allowed_statuses = regression
                .spec
                .allowed_semantic_statuses
                .iter()
                .map(|status| status.trim().to_ascii_lowercase())
                .collect::<Vec<_>>();
            let status_allowed = semantic_status_allowed(&allowed_statuses, &semantic_status);
            let matched_paths = output
                .matches
                .iter()
                .map(|entry| entry.document.path.clone())
                .collect::<Vec<_>>();
            let semantic_status_ok = output.note.semantic_status.as_str() == "ok";
            let required_witness_groups = witness_outcomes(
                &regression.spec.witness_groups,
                &matched_paths,
                semantic_status_ok,
                false,
            );
            let target_witness_groups = witness_outcomes(
                &regression.spec.target_witness_groups,
                &matched_paths,
                semantic_status_ok,
                true,
            );
            let mut outcome = HybridPlaybookProbeOutcome {
                file_name: regression
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                playbook_id: regression.metadata.playbook_id.clone(),
                semantic_status,
                semantic_reason: output.note.semantic_reason.clone(),
                status_allowed,
                duration_ms: started.elapsed().as_millis(),
                execution_error: None,
                matched_paths,
                trace_path: None,
                required_witness_groups,
                target_witness_groups,
            };
            if let Some(trace_root) = trace_root {
                if let Ok(trace_path) =
                    write_trace_packet(trace_root, regression, &output, &outcome)
                {
                    outcome.trace_path = Some(trace_path);
                }
            }
            outcome
        }
        Err(err) => HybridPlaybookProbeOutcome {
            file_name: regression
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned(),
            playbook_id: regression.metadata.playbook_id.clone(),
            semantic_status: "error".to_owned(),
            semantic_reason: None,
            status_allowed: false,
            duration_ms: started.elapsed().as_millis(),
            execution_error: Some(err.to_string()),
            matched_paths: Vec::new(),
            trace_path: None,
            required_witness_groups: failed_group_outcomes(&regression.spec.witness_groups),
            target_witness_groups: failed_group_outcomes(&regression.spec.target_witness_groups),
        },
    }
}

pub fn run_hybrid_playbook_regressions(
    searcher: &TextSearcher,
    playbooks_root: &Path,
    enforce_targets: bool,
    trace_root: Option<&Path>,
) -> FriggResult<HybridPlaybookRunSummary> {
    let regressions = load_hybrid_playbook_regressions(playbooks_root)?;
    let outcomes = regressions
        .iter()
        .map(|regression| run_hybrid_playbook_regression(searcher, regression, trace_root))
        .collect::<Vec<_>>();
    let required_failures = outcomes
        .iter()
        .filter(|outcome| !outcome.passed_required())
        .count();
    let target_failures = if enforce_targets {
        outcomes
            .iter()
            .filter(|outcome| !outcome.passed_targets())
            .count()
    } else {
        0
    };
    Ok(HybridPlaybookRunSummary {
        playbooks_root: playbooks_root.display().to_string(),
        enforce_targets,
        playbook_count: outcomes.len(),
        required_failures,
        target_failures,
        outcomes,
    })
}
