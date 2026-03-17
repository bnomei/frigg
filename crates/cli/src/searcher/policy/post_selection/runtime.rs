use std::collections::BTreeSet;

use super::super::super::query_terms::hybrid_query_has_kotlin_android_ui_terms;
use super::super::super::query_terms::hybrid_query_mentions_cli_command;
use super::super::SelectionState;
use super::*;
use crate::searcher::policy::facts::SharedPathFacts;

fn selected_match_for_path<'a>(
    matches: &'a [HybridRankedEvidence],
    path: &str,
) -> &'a HybridRankedEvidence {
    matches
        .iter()
        .find(|entry| entry.document.path == path)
        .expect("selected evidence path should exist in matches")
}

fn runtime_config_artifact_guardrail_cmp(
    left: &str,
    right: &str,
    prefer_repo_root: bool,
) -> Ordering {
    let left_is_root_scoped = is_root_scoped_runtime_config_path(left);
    let right_is_root_scoped = is_root_scoped_runtime_config_path(right);
    let left_depth = left.trim_start_matches("./").split('/').count();
    let right_depth = right.trim_start_matches("./").split('/').count();

    prefer_repo_root
        .then(|| left_is_root_scoped.cmp(&right_is_root_scoped))
        .unwrap_or(Ordering::Equal)
        .then_with(|| right_depth.cmp(&left_depth))
}

fn query_mentions_cli_command(query_text: &str) -> bool {
    hybrid_query_mentions_cli_command(query_text)
}

fn cli_specific_test_guardrail_cmp(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> Ordering {
    let left_facts = selection_guardrail_facts(left, state, ctx);
    let right_facts = selection_guardrail_facts(right, state, ctx);

    left_facts
        .specific_witness_path_overlap
        .cmp(&right_facts.specific_witness_path_overlap)
        .then_with(|| {
            left_facts
                .has_exact_query_term_match
                .cmp(&right_facts.has_exact_query_term_match)
        })
        .then_with(|| selection_guardrail_cmp(left, right, state, ctx))
}

fn runtime_companion_surface_supports_query(
    entry: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
    query_wants_android_ui_surface: bool,
) -> bool {
    let facts = selection_guardrail_facts(entry, state, ctx);
    query_wants_android_ui_surface
        || facts.specific_witness_path_overlap > 0
        || (facts.runtime_subtree_affinity > 0
            && matches!(
                facts.class,
                HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
            ))
        || (facts.has_path_witness_source && facts.path_overlap > 0)
}

fn runtime_companion_surface_cluster_support(
    entry: &HybridRankedEvidence,
    matches: &[HybridRankedEvidence],
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> usize {
    let mut support_paths = BTreeSet::new();
    let entry_path = entry.document.path.as_str();
    let mut consider = |other: &HybridRankedEvidence| {
        if other.document.path == entry.document.path {
            return;
        }

        let other_facts = selection_guardrail_facts(other, state, ctx);
        let support_surface = matches!(
            other_facts.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        ) || other_facts.is_runtime_config_artifact
            || other_facts.is_entrypoint_runtime
            || other_facts.is_test_support;
        if !support_surface
            || other_facts.is_ci_workflow
            || other_facts.is_repo_metadata
            || other_facts.is_generic_runtime_witness_doc
            || other_facts.is_frontend_runtime_noise
        {
            return;
        }

        if SharedPathFacts::workspace_subtree_affinity(entry_path, &other.document.path) > 0 {
            support_paths.insert(other.document.path.clone());
        }
    };

    for other in matches {
        consider(other);
    }
    for other in ctx.candidate_pool {
        consider(other);
    }
    for hit in ctx.witness_hits {
        let evidence = hybrid_ranked_evidence_from_witness_hit(hit);
        consider(&evidence);
    }

    support_paths.len()
}

fn runtime_companion_surface_guardrail_cmp(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
    matches: &[HybridRankedEvidence],
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> Ordering {
    let left_facts = selection_guardrail_facts(left, state, ctx);
    let right_facts = selection_guardrail_facts(right, state, ctx);
    let left_cluster_support = runtime_companion_surface_cluster_support(left, matches, state, ctx);
    let right_cluster_support =
        runtime_companion_surface_cluster_support(right, matches, state, ctx);

    left_cluster_support
        .cmp(&right_cluster_support)
        .then_with(|| {
            left_facts
                .specific_witness_path_overlap
                .cmp(&right_facts.specific_witness_path_overlap)
        })
        .then_with(|| {
            left_facts
                .runtime_subtree_affinity
                .cmp(&right_facts.runtime_subtree_affinity)
        })
        .then_with(|| selection_guardrail_cmp(left, right, state, ctx))
}

pub(super) fn apply_cli_specific_test_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    let wants_cli_specific_test = query_mentions_cli_command(ctx.query_text)
        && !ctx
            .selection_query_context
            .specific_witness_terms
            .is_empty()
        && (ctx.intent.wants_entrypoint_build_flow || ctx.intent.wants_test_witness_recall);
    if !wants_cli_specific_test {
        return matches;
    }

    let state = selection_guardrail_state(&matches, ctx);
    let has_specific_overlap = |entry: &HybridRankedEvidence| {
        selection_guardrail_facts(entry, &state, ctx).specific_witness_path_overlap > 0
    };
    let has_specific_overlap_hit = |hit: &HybridChannelHit| {
        let evidence = hybrid_ranked_evidence_from_witness_hit(hit);
        selection_guardrail_facts(&evidence, &state, ctx).specific_witness_path_overlap > 0
    };

    let selected_best = matches
        .iter()
        .filter(|entry| surfaces::is_cli_test_support_path(&entry.document.path))
        .filter(|entry| has_specific_overlap(entry))
        .max_by(|left, right| cli_specific_test_guardrail_cmp(left, right, &state, ctx))
        .map(|entry| entry.document.path.clone());
    let grouped_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .filter(|entry| surfaces::is_cli_test_support_path(&entry.document.path))
        .filter(|entry| has_specific_overlap(entry))
        .max_by(|left, right| cli_specific_test_guardrail_cmp(left, right, &state, ctx))
        .cloned();
    let witness_candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .filter(|hit| surfaces::is_cli_test_support_path(&hit.document.path))
        .filter(|hit| has_specific_overlap_hit(hit))
        .max_by(|left, right| {
            let left = hybrid_ranked_evidence_from_witness_hit(left);
            let right = hybrid_ranked_evidence_from_witness_hit(right);
            cli_specific_test_guardrail_cmp(&left, &right, &state, ctx)
        })
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        cli_specific_test_guardrail_cmp(left, right, &state, ctx)
    });

    let should_promote = match (candidate.as_ref(), selected_best.as_ref()) {
        (Some(candidate), Some(selected_path)) => {
            let candidate_facts = selection_guardrail_facts(candidate, &state, ctx);
            let selected_facts = matches
                .iter()
                .find(|entry| entry.document.path == *selected_path)
                .map(|entry| selection_guardrail_facts(entry, &state, ctx))
                .expect("selected CLI test path should exist in matches");

            candidate_facts
                .specific_witness_path_overlap
                .cmp(&selected_facts.specific_witness_path_overlap)
                .then_with(|| {
                    selection_guardrail_cmp(
                        candidate,
                        selected_match_for_path(&matches, selected_path),
                        &state,
                        ctx,
                    )
                })
                .then_with(|| {
                    selection_guardrail_score(candidate, &state, ctx).total_cmp(
                        &selection_guardrail_score_for_path(selected_path, &matches, &state, ctx),
                    )
                })
                .is_gt()
        }
        (Some(_), None) => true,
        _ => false,
    };

    if should_promote {
        insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            meta,
            is_test_support_guardrail_replacement,
        )
    } else {
        matches
    }
}

pub(super) fn apply_runtime_entrypoint_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_entrypoint_build_flow
        || matches
            .iter()
            .any(|entry| surfaces::is_entrypoint_runtime_path(&entry.document.path))
    {
        return matches;
    }

    let state = selection_guardrail_state(&matches, ctx);
    let grouped_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .filter(|entry| surfaces::is_entrypoint_runtime_path(&entry.document.path))
        .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
        .cloned();
    let witness_candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .filter(|hit| surfaces::is_entrypoint_runtime_path(&hit.document.path))
        .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        selection_guardrail_cmp(left, right, &state, ctx)
    });

    insert_guardrail_candidate(
        matches,
        candidate,
        ctx,
        meta,
        is_runtime_entrypoint_guardrail_replacement,
    )
}

pub(super) fn apply_runtime_config_surface_selection(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !(ctx.intent.wants_runtime_config_artifacts || ctx.intent.wants_entrypoint_build_flow) {
        return matches;
    }

    let root_config_filter: fn(&str) -> bool = if ctx.intent.wants_runtime_config_artifacts {
        is_root_scoped_runtime_config_path
    } else {
        surfaces::is_runtime_config_artifact_path
    };
    let specific_surface_filter: fn(&str) -> bool = is_specific_runtime_config_surface_path;

    if !matches
        .iter()
        .any(|entry| specific_surface_filter(&entry.document.path))
    {
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| specific_surface_filter(&entry.document.path))
            .max_by(|left, right| {
                runtime_config_surface_guardrail_priority_for_path(&left.document.path)
                    .cmp(&runtime_config_surface_guardrail_priority_for_path(
                        &right.document.path,
                    ))
                    .then_with(|| left.blended_score.total_cmp(&right.blended_score))
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| specific_surface_filter(&hit.document.path))
            .max_by(|left, right| {
                runtime_config_surface_guardrail_priority_for_path(&left.document.path)
                    .cmp(&runtime_config_surface_guardrail_priority_for_path(
                        &right.document.path,
                    ))
                    .then_with(|| left.raw_score.total_cmp(&right.raw_score))
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                runtime_config_surface_guardrail_priority_for_path(&left.document.path)
                    .cmp(&runtime_config_surface_guardrail_priority_for_path(
                        &right.document.path,
                    ))
                    .then_with(|| left.blended_score.total_cmp(&right.blended_score))
            });

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            meta,
            is_runtime_config_guardrail_replacement,
        );
    }

    if !matches
        .iter()
        .any(|entry| root_config_filter(&entry.document.path))
    {
        let prefer_repo_root = ctx.intent.wants_entrypoint_build_flow;
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| root_config_filter(&entry.document.path))
            .max_by(|left, right| {
                runtime_config_artifact_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    prefer_repo_root,
                )
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| root_config_filter(&hit.document.path))
            .max_by(|left, right| {
                runtime_config_artifact_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    prefer_repo_root,
                )
                .then_with(|| left.raw_score.total_cmp(&right.raw_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                runtime_config_artifact_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    prefer_repo_root,
                )
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
            });

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            meta,
            is_runtime_config_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_cli_entrypoint_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_entrypoint_build_flow || !query_mentions_cli_command(ctx.query_text) {
        return matches;
    }

    let state = selection_guardrail_state(&matches, ctx);
    let selected_best = matches
        .iter()
        .filter(|entry| surfaces::is_cli_command_entrypoint_path(&entry.document.path))
        .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
        .map(|entry| entry.document.path.clone());
    let grouped_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .filter(|entry| surfaces::is_cli_command_entrypoint_path(&entry.document.path))
        .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
        .cloned();
    let witness_candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .filter(|hit| surfaces::is_cli_command_entrypoint_path(&hit.document.path))
        .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        selection_guardrail_cmp(left, right, &state, ctx)
    });

    let should_promote = match (candidate.as_ref(), selected_best.as_ref()) {
        (Some(candidate), Some(selected_path)) => selection_guardrail_score(candidate, &state, ctx)
            .total_cmp(&selection_guardrail_score_for_path(
                selected_path,
                &matches,
                &state,
                ctx,
            ))
            .then_with(|| {
                selection_guardrail_cmp(
                    candidate,
                    matches
                        .iter()
                        .find(|entry| entry.document.path == *selected_path)
                        .expect("selected companion test path should exist in matches"),
                    &state,
                    ctx,
                )
            })
            .is_gt(),
        (Some(_), None) => true,
        _ => false,
    };

    if should_promote {
        insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            meta,
            is_cli_entrypoint_guardrail_replacement,
        )
    } else {
        matches
    }
}

pub(super) fn apply_entrypoint_build_workflow_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    fn is_build_flow_workflow_path(path: &str) -> bool {
        surfaces::is_entrypoint_build_workflow_path(path) || surfaces::is_ci_workflow_path(path)
    }

    if !ctx.intent.wants_entrypoint_build_flow {
        return matches;
    }

    let selected_best = matches
        .iter()
        .filter(|entry| is_build_flow_workflow_path(&entry.document.path))
        .max_by(|left, right| {
            ci_workflow_guardrail_cmp(&left.document.path, &right.document.path, ctx.query_text)
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(|entry| entry.document.path.clone());
    let grouped_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .filter(|entry| is_build_flow_workflow_path(&entry.document.path))
        .max_by(|left, right| {
            ci_workflow_guardrail_cmp(&left.document.path, &right.document.path, ctx.query_text)
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .cloned();
    let witness_candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .filter(|hit| is_build_flow_workflow_path(&hit.document.path))
        .max_by(|left, right| {
            ci_workflow_guardrail_cmp(&left.document.path, &right.document.path, ctx.query_text)
                .then_with(|| left.raw_score.total_cmp(&right.raw_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        ci_workflow_guardrail_cmp(&left.document.path, &right.document.path, ctx.query_text)
            .then_with(|| left.blended_score.total_cmp(&right.blended_score))
    });

    let should_promote = match (candidate.as_ref(), selected_best.as_ref()) {
        (Some(candidate), Some(selected_path)) => matches
            .iter()
            .find(|entry| entry.document.path == *selected_path)
            .is_some_and(|selected| {
                ci_workflow_guardrail_cmp(
                    &candidate.document.path,
                    &selected.document.path,
                    ctx.query_text,
                )
                .then_with(|| candidate.blended_score.total_cmp(&selected.blended_score))
                .is_gt()
            }),
        (Some(_), None) => true,
        _ => false,
    };

    if !should_promote {
        return matches;
    }

    let Some(candidate) = candidate else {
        return matches;
    };

    if let Some(selected_path) = selected_best {
        if let Some(index) = matches
            .iter()
            .position(|entry| entry.document.path == selected_path)
        {
            let replaced_path = matches[index].document.path.clone();
            matches[index] = candidate;
            ctx.record_repair(
                meta,
                PostSelectionRepairAction::Replaced,
                &matches[index].document.path,
                Some(replaced_path),
            );
            return matches;
        }
    }

    insert_guardrail_candidate(
        matches,
        Some(candidate),
        ctx,
        meta,
        is_entrypoint_build_workflow_guardrail_replacement,
    )
}

pub(super) fn apply_runtime_companion_surface_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    let wants_runtime_companion_surface = ctx.intent.wants_runtime_witnesses
        || ctx.intent.wants_test_witness_recall
        || ctx.intent.wants_entrypoint_build_flow
        || ctx.intent.wants_runtime_config_artifacts;
    if !wants_runtime_companion_surface
        || ctx
            .selection_query_context
            .specific_witness_terms
            .is_empty()
    {
        return matches;
    }

    let state = selection_guardrail_state(&matches, ctx);
    let query_wants_android_ui_surface = hybrid_query_has_kotlin_android_ui_terms(ctx.query_text);
    let surface_matches_query = |entry: &HybridRankedEvidence| {
        runtime_companion_surface_supports_query(entry, &state, ctx, query_wants_android_ui_surface)
    };
    let surface_has_rescue_signal = |entry: &HybridRankedEvidence| {
        let facts = selection_guardrail_facts(entry, &state, ctx);
        facts.specific_witness_path_overlap > 0
            || facts.has_path_witness_source
            || facts.runtime_subtree_affinity > 0
    };
    let surface_hit_matches_query = |hit: &HybridChannelHit| {
        let evidence = hybrid_ranked_evidence_from_witness_hit(hit);
        surface_matches_query(&evidence)
    };
    let selected_surface_indexes = matches
        .iter()
        .enumerate()
        .filter(|(_, entry)| is_runtime_companion_surface_candidate_path(&entry.document.path))
        .filter(|(_, entry)| surface_matches_query(entry))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    let selected_best_index = selected_surface_indexes
        .iter()
        .copied()
        .max_by(|left, right| {
            runtime_companion_surface_guardrail_cmp(
                &matches[*left],
                &matches[*right],
                &matches,
                &state,
                ctx,
            )
        });
    let selected_best = selected_best_index.map(|index| matches[index].document.path.clone());
    let grouped_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .filter(|entry| is_runtime_companion_surface_candidate_path(&entry.document.path))
        .filter(|entry| surface_matches_query(entry))
        .max_by(|left, right| {
            runtime_companion_surface_guardrail_cmp(left, right, &matches, &state, ctx)
        })
        .cloned();
    let witness_candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .filter(|hit| is_runtime_companion_surface_candidate_path(&hit.document.path))
        .filter(|hit| surface_hit_matches_query(hit))
        .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        runtime_companion_surface_guardrail_cmp(left, right, &matches, &state, ctx)
    });
    let should_promote = match (candidate.as_ref(), selected_best.as_ref()) {
        (Some(candidate), Some(selected_path)) => {
            let selected = selected_match_for_path(&matches, selected_path);
            let candidate_has_rescue_signal = surface_has_rescue_signal(candidate);
            let selected_has_rescue_signal = surface_has_rescue_signal(selected);

            (candidate_has_rescue_signal && !selected_has_rescue_signal)
                || runtime_companion_surface_guardrail_cmp(
                    candidate, selected, &matches, &state, ctx,
                )
                .is_gt()
                || selection_guardrail_score(candidate, &state, ctx)
                    .total_cmp(&selection_guardrail_score_for_path(
                        selected_path,
                        &matches,
                        &state,
                        ctx,
                    ))
                    .is_gt()
        }
        (Some(_), None) => true,
        _ => false,
    };
    let should_reorder_selected = match (
        selected_surface_indexes.first().copied(),
        selected_best_index,
    ) {
        (Some(lead_index), Some(best_index)) if lead_index != best_index => {
            runtime_companion_surface_guardrail_cmp(
                &matches[best_index],
                &matches[lead_index],
                &matches,
                &state,
                ctx,
            )
            .is_gt()
        }
        _ => false,
    };
    if !should_promote {
        if should_reorder_selected {
            let lead_index = selected_surface_indexes[0];
            let best_index = selected_best_index.expect("best selected index should exist");
            let promoted_path = matches[best_index].document.path.clone();
            let replaced_path = matches[lead_index].document.path.clone();
            matches.swap(lead_index, best_index);
            ctx.record_repair(
                meta,
                PostSelectionRepairAction::Replaced,
                &promoted_path,
                Some(replaced_path),
            );
        }
        return matches;
    }

    let Some(candidate) = candidate else {
        return matches;
    };
    if let Some(selected_path) = selected_best {
        if let Some(index) = matches
            .iter()
            .position(|entry| entry.document.path == selected_path)
        {
            let replaced_path = matches[index].document.path.clone();
            matches[index] = candidate;
            ctx.record_repair(
                meta,
                PostSelectionRepairAction::Replaced,
                &matches[index].document.path,
                Some(replaced_path),
            );
            return matches;
        }
    }

    insert_guardrail_candidate(
        matches,
        Some(candidate),
        ctx,
        meta,
        is_runtime_companion_surface_guardrail_replacement,
    )
}

pub(super) fn apply_runtime_witness_rescue_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    let wants_runtime_rescue = ctx.intent.wants_runtime_witnesses
        || ctx.intent.wants_test_witness_recall
        || ctx.intent.wants_entrypoint_build_flow
        || ctx.intent.wants_runtime_config_artifacts;
    if !wants_runtime_rescue {
        return matches;
    }

    let state = selection_guardrail_state(&matches, ctx);
    let is_rescue_candidate = |entry: &HybridRankedEvidence| {
        let facts = selection_guardrail_facts(entry, &state, ctx);
        let candidate_surface = matches!(
            facts.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        ) || facts.is_runtime_config_artifact
            || facts.is_entrypoint_runtime
            || facts.is_test_support;
        let witness_backed = facts.has_path_witness_source
            || facts.specific_witness_path_overlap > 0
            || facts.runtime_subtree_affinity > 0;
        candidate_surface
            && witness_backed
            && !facts.is_ci_workflow
            && !facts.is_repo_metadata
            && !facts.is_generic_runtime_witness_doc
            && !facts.is_frontend_runtime_noise
    };
    let has_noise_slot = matches.iter().any(|entry| {
        let facts = selection_guardrail_facts(entry, &state, ctx);
        facts.is_ci_workflow
            || facts.is_repo_metadata
            || facts.is_generic_runtime_witness_doc
            || facts.is_frontend_runtime_noise
    });
    if !has_noise_slot || matches.iter().any(is_rescue_candidate) {
        return matches;
    }

    let grouped_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .filter(|entry| is_rescue_candidate(entry))
        .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
        .cloned();
    let witness_candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .map(hybrid_ranked_evidence_from_witness_hit)
        .filter(|entry| is_rescue_candidate(entry))
        .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx));
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        selection_guardrail_cmp(left, right, &state, ctx)
    });

    insert_guardrail_candidate(
        matches,
        candidate,
        ctx,
        meta,
        is_runtime_companion_surface_guardrail_replacement,
    )
}

pub(super) fn apply_ci_scripts_ops_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_ci_workflow_witnesses && !ctx.intent.wants_scripts_ops_witnesses {
        return matches;
    }

    if ctx.intent.wants_scripts_ops_witnesses {
        let selected_best = matches
            .iter()
            .filter(|entry| surfaces::is_scripts_ops_path(&entry.document.path))
            .max_by(|left, right| {
                scripts_ops_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    ctx.query_text,
                    &ctx.exact_terms,
                )
            })
            .map(|entry| entry.document.path.as_str());
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| surfaces::is_scripts_ops_path(&entry.document.path))
            .max_by(|left, right| {
                scripts_ops_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    ctx.query_text,
                    &ctx.exact_terms,
                )
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| surfaces::is_scripts_ops_path(&hit.document.path))
            .max_by(|left, right| {
                scripts_ops_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    ctx.query_text,
                    &ctx.exact_terms,
                )
                .then_with(|| left.raw_score.total_cmp(&right.raw_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                scripts_ops_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    ctx.query_text,
                    &ctx.exact_terms,
                )
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
            });
        let should_promote = match (candidate.as_ref(), selected_best) {
            (Some(candidate), Some(selected_path)) => scripts_ops_guardrail_cmp(
                &candidate.document.path,
                selected_path,
                ctx.query_text,
                &ctx.exact_terms,
            )
            .is_gt(),
            (Some(_), None) => true,
            _ => false,
        };
        if should_promote {
            matches = insert_guardrail_candidate(
                matches,
                candidate,
                ctx,
                meta,
                is_scripts_ops_guardrail_replacement,
            );
        }
    }

    if ctx.intent.wants_ci_workflow_witnesses
        && !matches
            .iter()
            .any(|entry| surfaces::is_ci_workflow_path(&entry.document.path))
    {
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| surfaces::is_ci_workflow_path(&entry.document.path))
            .max_by(|left, right| {
                ci_workflow_guardrail_cmp(&left.document.path, &right.document.path, ctx.query_text)
                    .then_with(|| left.blended_score.total_cmp(&right.blended_score))
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| surfaces::is_ci_workflow_path(&hit.document.path))
            .max_by(|left, right| {
                ci_workflow_guardrail_cmp(&left.document.path, &right.document.path, ctx.query_text)
                    .then_with(|| left.raw_score.total_cmp(&right.raw_score))
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                ci_workflow_guardrail_cmp(&left.document.path, &right.document.path, ctx.query_text)
                    .then_with(|| left.blended_score.total_cmp(&right.blended_score))
            });

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            meta,
            is_ci_workflow_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_mixed_support_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_test_witness_recall
        || !(ctx.intent.wants_examples || ctx.intent.wants_benchmarks)
    {
        return matches;
    }

    if ctx.intent.wants_examples {
        let state = selection_guardrail_state(&matches, ctx);
        let selected_best = matches
            .iter()
            .filter(|entry| surfaces::is_example_support_path(&entry.document.path))
            .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
            .map(|entry| entry.document.path.clone());
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| surfaces::is_example_support_path(&entry.document.path))
            .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| surfaces::is_example_support_path(&hit.document.path))
            .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                selection_guardrail_cmp(left, right, &state, ctx)
            });

        let should_promote = match (candidate.as_ref(), selected_best.as_ref()) {
            (Some(candidate), Some(selected_path)) => {
                selection_guardrail_score(candidate, &state, ctx)
                    .total_cmp(&selection_guardrail_score_for_path(
                        selected_path,
                        &matches,
                        &state,
                        ctx,
                    ))
                    .is_gt()
            }
            (Some(_), None) => true,
            _ => false,
        };

        if should_promote {
            matches = if selected_best.is_some() {
                insert_test_support_guardrail_candidate(
                    matches,
                    candidate,
                    ctx,
                    meta,
                    selected_best,
                )
            } else {
                insert_guardrail_candidate(
                    matches,
                    candidate,
                    ctx,
                    meta,
                    is_example_support_guardrail_replacement,
                )
            };
        }
    }

    if ctx.intent.wants_benchmarks && !matches.iter().any(is_bench_support_document) {
        let state = selection_guardrail_state(&matches, ctx);
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| is_bench_support_candidate_path(&entry.document.path))
            .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| is_bench_support_candidate_path(&hit.document.path))
            .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                selection_guardrail_cmp(left, right, &state, ctx)
            });

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            meta,
            is_plain_test_support_document,
        );
    }

    if !matches.iter().any(is_plain_test_support_document) {
        let state = selection_guardrail_state(&matches, ctx);
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| is_plain_test_support_path(&entry.document.path))
            .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| is_plain_test_support_path(&hit.document.path))
            .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                selection_guardrail_cmp(left, right, &state, ctx)
            });

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            meta,
            is_bench_or_benchmark_support_document,
        );
    }

    matches
}

pub(super) fn apply_runtime_companion_test_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    let wants_runtime_companion_tests = ctx.intent.wants_test_witness_recall
        || ctx.intent.wants_entrypoint_build_flow
        || ctx.intent.wants_runtime_config_artifacts;
    if !wants_runtime_companion_tests {
        return matches;
    }

    let has_cli_specific_witness_candidate = {
        let state = selection_guardrail_state(&matches, ctx);
        let specific_witness_overlap = |entry: &HybridRankedEvidence| {
            selection_guardrail_facts(entry, &state, ctx).specific_witness_path_overlap > 0
        };
        let specific_witness_hit_overlap = |hit: &HybridChannelHit| {
            let evidence = hybrid_ranked_evidence_from_witness_hit(hit);
            selection_guardrail_facts(&evidence, &state, ctx).specific_witness_path_overlap > 0
        };
        let has_cli_query = query_mentions_cli_command(ctx.query_text)
            && !ctx
                .selection_query_context
                .specific_witness_terms
                .is_empty();

        has_cli_query
            && (matches.iter().any(|entry| {
                surfaces::is_cli_test_support_path(&entry.document.path)
                    && specific_witness_overlap(entry)
            }) || ctx.witness_hits.iter().any(|hit| {
                surfaces::is_cli_test_support_path(&hit.document.path)
                    && specific_witness_hit_overlap(hit)
            }))
    };

    if has_cli_specific_witness_candidate {
        return matches;
    }

    let prefer_runtime_adjacent_python =
        ctx.intent.wants_test_witness_recall && !ctx.intent.wants_entrypoint_build_flow;
    let prefer_non_scripts_plain_tests = !ctx.intent.wants_scripts_ops_witnesses;
    let state = selection_guardrail_state(&matches, ctx);
    let selected_best = matches
        .iter()
        .filter(|entry| {
            is_plain_test_support_path(&entry.document.path)
                && (!prefer_runtime_adjacent_python
                    || surfaces::is_runtime_adjacent_python_test_path(&entry.document.path))
                && (!prefer_non_scripts_plain_tests
                    || !surfaces::is_scripts_ops_path(&entry.document.path))
        })
        .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
        .map(|entry| entry.document.path.clone());
    let selected_best = selected_best.or_else(|| {
        matches
            .iter()
            .filter(|entry| {
                is_plain_test_support_path(&entry.document.path)
                    && (!prefer_non_scripts_plain_tests
                        || !surfaces::is_scripts_ops_path(&entry.document.path))
            })
            .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
            .map(|entry| entry.document.path.clone())
    });

    let grouped_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .filter(|entry| {
            is_plain_test_support_path(&entry.document.path)
                && (!prefer_runtime_adjacent_python
                    || surfaces::is_runtime_adjacent_python_test_path(&entry.document.path))
                && (!prefer_non_scripts_plain_tests
                    || !surfaces::is_scripts_ops_path(&entry.document.path))
        })
        .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
        .cloned();
    let witness_candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .filter(|hit| {
            is_plain_test_support_path(&hit.document.path)
                && (!prefer_runtime_adjacent_python
                    || surfaces::is_runtime_adjacent_python_test_path(&hit.document.path))
                && (!prefer_non_scripts_plain_tests
                    || !surfaces::is_scripts_ops_path(&hit.document.path))
        })
        .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        selection_guardrail_cmp(left, right, &state, ctx)
    })
    .or_else(|| {
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| is_plain_test_support_path(&entry.document.path))
            .max_by(|left, right| selection_guardrail_cmp(left, right, &state, ctx))
            .cloned();
        let witness_candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| is_plain_test_support_path(&hit.document.path))
            .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
            .map(hybrid_ranked_evidence_from_witness_hit);

        choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
            selection_guardrail_cmp(left, right, &state, ctx)
        })
    });
    let selected_best_is_benchmark_test = selected_best
        .as_deref()
        .is_some_and(is_benchmark_test_support_path);
    let candidate_is_benchmark_test = candidate
        .as_ref()
        .is_some_and(|entry| is_benchmark_test_support_path(&entry.document.path));

    let should_promote = match (candidate.as_ref(), selected_best.as_ref()) {
        (Some(_), Some(_))
            if ctx.intent.wants_benchmarks
                && selected_best_is_benchmark_test
                && !candidate_is_benchmark_test =>
        {
            false
        }
        (Some(candidate), Some(selected_path)) => selection_guardrail_cmp(
            candidate,
            selected_match_for_path(&matches, selected_path),
            &state,
            ctx,
        )
        .then_with(|| {
            selection_guardrail_score(candidate, &state, ctx).total_cmp(
                &selection_guardrail_score_for_path(selected_path, &matches, &state, ctx),
            )
        })
        .is_gt(),
        (Some(_), None) => true,
        _ => false,
    };

    if should_promote {
        insert_test_support_guardrail_candidate(matches, candidate, ctx, meta, selected_best)
    } else {
        matches
    }
}
