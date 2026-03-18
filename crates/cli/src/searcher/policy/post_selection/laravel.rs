use super::*;

fn is_laravel_blade_page_companion_path(path: &str) -> bool {
    is_laravel_non_livewire_blade_view_path(path) && !is_laravel_layout_blade_view_path(path)
}

fn laravel_layout_companion_guardrail_cmp(
    left: &str,
    right: &str,
    query_text: &str,
    exact_terms: &[String],
) -> Ordering {
    let left_is_page_view = is_laravel_blade_page_companion_path(left);
    let right_is_page_view = is_laravel_blade_page_companion_path(right);

    left_is_page_view
        .cmp(&right_is_page_view)
        .then_with(|| laravel_blade_surface_guardrail_cmp(left, right, query_text, exact_terms))
}

pub(super) fn apply_laravel_entrypoint_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
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
            meta,
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
            meta,
            is_laravel_entrypoint_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_laravel_blade_surface_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
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
            meta,
            is_laravel_ui_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_laravel_livewire_surface_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_laravel_ui_witnesses
        || matches
            .iter()
            .any(|entry| is_promotable_laravel_livewire_surface_path(&entry.document.path))
    {
        return matches;
    }

    let prefers_livewire_views = ctx.intent.wants_livewire_view_witnesses;
    let selected_best = matches
        .iter()
        .filter(|entry| is_promotable_laravel_livewire_surface_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_livewire_surface_guardrail_cmp(
                &left.document.path,
                &right.document.path,
                ctx.query_text,
                &ctx.exact_terms,
                prefers_livewire_views,
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
        .filter(|entry| is_promotable_laravel_livewire_surface_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_livewire_surface_guardrail_cmp(
                &left.document.path,
                &right.document.path,
                ctx.query_text,
                &ctx.exact_terms,
                prefers_livewire_views,
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
        .filter(|hit| is_promotable_laravel_livewire_surface_path(&hit.document.path))
        .max_by(|left, right| {
            laravel_livewire_surface_guardrail_cmp(
                &left.document.path,
                &right.document.path,
                ctx.query_text,
                &ctx.exact_terms,
                prefers_livewire_views,
            )
            .then_with(|| left.raw_score.total_cmp(&right.raw_score))
            .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        laravel_livewire_surface_guardrail_cmp(
            &left.document.path,
            &right.document.path,
            ctx.query_text,
            &ctx.exact_terms,
            prefers_livewire_views,
        )
        .then_with(|| left.blended_score.total_cmp(&right.blended_score))
    });

    let should_promote = match (candidate.as_ref(), selected_best) {
        (Some(candidate), Some(selected_path)) => laravel_livewire_surface_guardrail_cmp(
            &candidate.document.path,
            selected_path,
            ctx.query_text,
            &ctx.exact_terms,
            prefers_livewire_views,
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
            is_laravel_livewire_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_laravel_ui_test_harness_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
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
        meta,
        is_laravel_ui_test_guardrail_replacement,
    )
}

pub(super) fn apply_laravel_layout_companion_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_laravel_layout_witnesses {
        return matches;
    }

    let selected_best = matches
        .iter()
        .filter(|entry| is_layout_companion_blade_surface_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_layout_companion_guardrail_cmp(
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
            laravel_layout_companion_guardrail_cmp(
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
            laravel_layout_companion_guardrail_cmp(
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
        laravel_layout_companion_guardrail_cmp(
            &left.document.path,
            &right.document.path,
            ctx.query_text,
            &ctx.exact_terms,
        )
        .then_with(|| left.blended_score.total_cmp(&right.blended_score))
    });

    let should_promote = match (candidate.as_ref(), selected_best) {
        (Some(candidate), Some(selected_path)) => laravel_layout_companion_guardrail_cmp(
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
            is_laravel_ui_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_laravel_blade_page_companion_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_laravel_ui_witnesses
        || !ctx.intent.wants_blade_component_witnesses
        || matches
            .iter()
            .any(|entry| is_laravel_blade_page_companion_path(&entry.document.path))
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
        .filter(|entry| is_laravel_blade_page_companion_path(&entry.document.path))
        .max_by(|left, right| {
            laravel_layout_companion_guardrail_cmp(
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
        .filter(|hit| is_laravel_blade_page_companion_path(&hit.document.path))
        .max_by(|left, right| {
            laravel_layout_companion_guardrail_cmp(
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
        laravel_layout_companion_guardrail_cmp(
            &left.document.path,
            &right.document.path,
            ctx.query_text,
            &ctx.exact_terms,
        )
        .then_with(|| left.blended_score.total_cmp(&right.blended_score))
    });

    let blade_component_count = matches
        .iter()
        .filter(|entry| is_laravel_blade_component_path(&entry.document.path))
        .count();
    let livewire_surface_count = matches
        .iter()
        .filter(|entry| is_promotable_laravel_livewire_surface_path(&entry.document.path))
        .count();
    let layout_count = matches
        .iter()
        .filter(|entry| is_laravel_layout_blade_view_path(&entry.document.path))
        .count();

    insert_guardrail_candidate(matches, candidate, ctx, meta, |entry| {
        if is_laravel_blade_page_companion_path(&entry.document.path) {
            return false;
        }

        if is_laravel_layout_blade_view_path(&entry.document.path) {
            return !ctx.intent.wants_laravel_layout_witnesses || layout_count > 1;
        }

        if is_promotable_laravel_livewire_surface_path(&entry.document.path) {
            return !ctx.intent.wants_livewire_view_witnesses || livewire_surface_count > 1;
        }

        if is_laravel_blade_component_path(&entry.document.path) {
            return blade_component_count > 1;
        }

        if is_laravel_view_component_class_path(&entry.document.path) {
            return true;
        }

        matches!(
            surfaces::hybrid_source_class(&entry.document.path),
            HybridSourceClass::Runtime
                | HybridSourceClass::Project
                | HybridSourceClass::Documentation
                | HybridSourceClass::Readme
        )
    })
}

fn is_promotable_laravel_livewire_surface_path(path: &str) -> bool {
    is_laravel_livewire_component_path(path) || is_laravel_livewire_view_path(path)
}

fn laravel_livewire_surface_guardrail_cmp(
    left: &str,
    right: &str,
    query_text: &str,
    exact_terms: &[String],
    prefers_livewire_views: bool,
) -> Ordering {
    let left_overlap = hybrid_path_overlap_count(left, query_text);
    let right_overlap = hybrid_path_overlap_count(right, query_text);
    left_overlap
        .cmp(&right_overlap)
        .then_with(|| {
            if prefers_livewire_views {
                is_laravel_livewire_view_path(left).cmp(&is_laravel_livewire_view_path(right))
            } else {
                is_laravel_livewire_component_path(left)
                    .cmp(&is_laravel_livewire_component_path(right))
            }
        })
        .then_with(|| {
            hybrid_path_has_exact_stem_match(left, exact_terms)
                .cmp(&hybrid_path_has_exact_stem_match(right, exact_terms))
        })
        .then_with(|| {
            right
                .trim_start_matches("./")
                .split('/')
                .count()
                .cmp(&left.trim_start_matches("./").split('/').count())
        })
}

fn is_laravel_livewire_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_promotable_laravel_livewire_surface_path(&entry.document.path)
        || is_laravel_non_livewire_blade_view_path(&entry.document.path)
    {
        return false;
    }

    if is_laravel_blade_component_path(&entry.document.path)
        || is_laravel_view_component_class_path(&entry.document.path)
    {
        return true;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Runtime
            | HybridSourceClass::Project
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    )
}
