use super::*;

pub(in crate::searcher::policy::post_selection) fn apply_runtime_entrypoint_visibility(
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

pub(in crate::searcher::policy::post_selection) fn apply_entrypoint_build_workflow_visibility(
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

pub(in crate::searcher::policy::post_selection) fn apply_ci_scripts_ops_visibility(
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
