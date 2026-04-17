use super::*;

pub(in crate::searcher::policy::post_selection) fn apply_runtime_config_surface_selection(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !(ctx.intent.wants_runtime_config_artifacts || ctx.intent.wants_entrypoint_build_flow) {
        return matches;
    }

    let root_config_filter: fn(&str) -> bool = is_root_scoped_runtime_config_path;
    let specific_surface_filter: fn(&str) -> bool = is_specific_runtime_config_surface_path;
    let local_config_filter: fn(&str) -> bool = is_local_runtime_config_surface_path;
    let preserve_selected_build_workflow = preserve_selected_build_workflow(&matches, ctx);

    if !matches
        .iter()
        .any(|entry| local_config_filter(&entry.document.path))
    {
        let state = selection_guardrail_state(&matches, ctx);
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| local_config_filter(&entry.document.path))
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
            .filter(|hit| local_config_filter(&hit.document.path))
            .max_by(|left, right| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                selection_guardrail_cmp(left, right, &state, ctx)
            });

        matches = insert_guardrail_candidate(matches, candidate, ctx, meta, |entry| {
            is_runtime_config_guardrail_replacement(entry)
                && (!preserve_selected_build_workflow
                    || (!surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
                        && !surfaces::is_ci_workflow_path(&entry.document.path)))
        });
    }

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

        matches = insert_guardrail_candidate(matches, candidate, ctx, meta, |entry| {
            is_runtime_config_guardrail_replacement(entry)
                && (!preserve_selected_build_workflow
                    || (!surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
                        && !surfaces::is_ci_workflow_path(&entry.document.path)))
        });
    }

    if !matches
        .iter()
        .any(|entry| root_config_filter(&entry.document.path))
    {
        let state = selection_guardrail_state(&matches, ctx);
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
                .then_with(|| selection_guardrail_cmp(left, right, &state, ctx))
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
                .then_with(|| selection_guardrail_cmp_from_hit(left, right, &state, ctx))
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
                .then_with(|| selection_guardrail_cmp(left, right, &state, ctx))
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
            });

        matches = insert_guardrail_candidate(matches, candidate, ctx, meta, |entry| {
            is_runtime_config_guardrail_replacement(entry)
                && (!preserve_selected_build_workflow
                    || (!surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
                        && !surfaces::is_ci_workflow_path(&entry.document.path)))
        });
    }

    matches
}

pub(in crate::searcher::policy::post_selection) fn apply_runtime_config_surface_ordering(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    _meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !(ctx.intent.wants_runtime_config_artifacts || ctx.intent.wants_entrypoint_build_flow) {
        return matches;
    }

    let surface_indices = matches
        .iter()
        .enumerate()
        .filter(|(_, entry)| is_runtime_config_ordering_candidate_path(&entry.document.path))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if surface_indices.len() < 2 {
        return matches;
    }

    let state = selection_guardrail_state(&matches, ctx);
    let mut ordered_surfaces = surface_indices
        .iter()
        .map(|index| matches[*index].clone())
        .collect::<Vec<_>>();
    ordered_surfaces.sort_by(|left, right| {
        runtime_config_ordering_cmp(right, left, &state, ctx)
            .then_with(|| left.document.path.cmp(&right.document.path))
    });

    for (slot_index, ordered_entry) in surface_indices.into_iter().zip(ordered_surfaces) {
        matches[slot_index] = ordered_entry;
    }

    matches
}
