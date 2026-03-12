use super::*;

pub(super) fn apply_laravel_entrypoint_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_entrypoint_build_flow {
        return matches;
    }

    if !matches
        .iter()
        .any(|entry| is_laravel_route_path(&entry.document.path))
    {
        let candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| is_laravel_route_path(&hit.document.path))
            .max_by(|left, right| {
                left.raw_score
                    .total_cmp(&right.raw_score)
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            "post_selection.laravel_entrypoint",
            is_laravel_entrypoint_guardrail_replacement,
        );
    }

    if !matches
        .iter()
        .any(|entry| is_laravel_bootstrap_entrypoint_path(&entry.document.path))
    {
        let candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| is_laravel_bootstrap_entrypoint_path(&hit.document.path))
            .max_by(|left, right| {
                left.raw_score
                    .total_cmp(&right.raw_score)
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            "post_selection.laravel_entrypoint",
            is_laravel_entrypoint_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_laravel_blade_surface_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_laravel_ui_witnesses {
        return matches;
    }

    let selected_best = matches
        .iter()
        .filter(|entry| is_promotable_laravel_blade_surface_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_blade_surface_guardrail_cmp(
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
        .filter(|entry| is_promotable_laravel_blade_surface_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_blade_surface_guardrail_cmp(
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
        .filter(|hit| is_promotable_laravel_blade_surface_path(&hit.document.path))
        .max_by(|left, right| {
            laravel_blade_surface_guardrail_cmp(
                &left.document.path,
                &right.document.path,
                ctx.query_text,
                &ctx.exact_terms,
            )
            .then_with(|| left.raw_score.total_cmp(&right.raw_score))
            .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        laravel_blade_surface_guardrail_cmp(
            &left.document.path,
            &right.document.path,
            ctx.query_text,
            &ctx.exact_terms,
        )
        .then_with(|| left.blended_score.total_cmp(&right.blended_score))
    });

    let should_promote = match (candidate.as_ref(), selected_best) {
        (Some(candidate), Some(selected_path)) => laravel_blade_surface_guardrail_cmp(
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
            "post_selection.laravel_blade_surface",
            is_laravel_ui_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_laravel_ui_test_harness_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_laravel_ui_witnesses
        || matches
            .iter()
            .any(|entry| surfaces::is_test_harness_path(&entry.document.path))
    {
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
        .filter(|entry| surfaces::is_test_harness_path(&entry.document.path))
        .max_by(|left, right| {
            left.blended_score
                .total_cmp(&right.blended_score)
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
        .filter(|hit| surfaces::is_test_harness_path(&hit.document.path))
        .max_by(|left, right| {
            left.raw_score
                .total_cmp(&right.raw_score)
                .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        left.blended_score.total_cmp(&right.blended_score)
    });

    insert_guardrail_candidate(
        matches,
        candidate,
        ctx,
        "post_selection.laravel_ui_test_harness",
        is_laravel_ui_test_guardrail_replacement,
    )
}

pub(super) fn apply_laravel_layout_companion_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_laravel_layout_witnesses {
        return matches;
    }

    let selected_best = matches
        .iter()
        .filter(|entry| is_layout_companion_blade_surface_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_blade_surface_guardrail_cmp(
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
        .filter(|entry| is_layout_companion_blade_surface_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_blade_surface_guardrail_cmp(
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
        .filter(|hit| is_layout_companion_blade_surface_path(&hit.document.path))
        .max_by(|left, right| {
            laravel_blade_surface_guardrail_cmp(
                &left.document.path,
                &right.document.path,
                ctx.query_text,
                &ctx.exact_terms,
            )
            .then_with(|| left.raw_score.total_cmp(&right.raw_score))
            .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        laravel_blade_surface_guardrail_cmp(
            &left.document.path,
            &right.document.path,
            ctx.query_text,
            &ctx.exact_terms,
        )
        .then_with(|| left.blended_score.total_cmp(&right.blended_score))
    });

    let should_promote = match (candidate.as_ref(), selected_best) {
        (Some(candidate), Some(selected_path)) => laravel_blade_surface_guardrail_cmp(
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
            "post_selection.laravel_layout_companion",
            is_laravel_ui_guardrail_replacement,
        );
    }

    matches
}
