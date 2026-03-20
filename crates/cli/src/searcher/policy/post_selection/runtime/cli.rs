use super::*;

pub(in crate::searcher::policy::post_selection) fn apply_cli_specific_test_visibility(
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

pub(in crate::searcher::policy::post_selection) fn apply_cli_entrypoint_visibility(
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
