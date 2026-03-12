use super::*;

pub(super) fn apply_runtime_config_surface_selection(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !(ctx.intent.wants_runtime_config_artifacts || ctx.intent.wants_entrypoint_build_flow) {
        return matches;
    }

    let root_config_filter: fn(&str) -> bool = if ctx.intent.wants_runtime_config_artifacts {
        is_repo_root_runtime_config_path
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
            "post_selection.runtime_config",
            is_runtime_config_guardrail_replacement,
        );
    }

    if !matches
        .iter()
        .any(|entry| root_config_filter(&entry.document.path))
    {
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
            .filter(|hit| root_config_filter(&hit.document.path))
            .max_by(|left, right| {
                left.raw_score
                    .total_cmp(&right.raw_score)
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);
        let candidate =
            choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
                left.blended_score.total_cmp(&right.blended_score)
            });

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            "post_selection.runtime_config",
            is_runtime_config_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_entrypoint_build_workflow_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_entrypoint_build_flow
        || matches
            .iter()
            .any(|entry| surfaces::is_entrypoint_build_workflow_path(&entry.document.path))
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
        .filter(|entry| surfaces::is_entrypoint_build_workflow_path(&entry.document.path))
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
        .filter(|hit| surfaces::is_entrypoint_build_workflow_path(&hit.document.path))
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

    insert_guardrail_candidate(
        matches,
        candidate,
        ctx,
        "post_selection.entrypoint_build_workflow",
        is_entrypoint_build_workflow_guardrail_replacement,
    )
}

pub(super) fn apply_ci_scripts_ops_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
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
                "post_selection.ci_scripts_ops",
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
            "post_selection.ci_scripts_ops",
            is_ci_workflow_guardrail_replacement,
        );
    }

    matches
}

pub(super) fn apply_mixed_support_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_test_witness_recall
        || !(ctx.intent.wants_examples || ctx.intent.wants_benchmarks)
    {
        return matches;
    }

    if ctx.intent.wants_examples && !matches.iter().any(is_example_support_document) {
        let state = selection_guardrail_state(&matches, ctx);
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

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx,
            "post_selection.mixed_support",
            is_example_support_guardrail_replacement,
        );
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
            "post_selection.mixed_support",
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
            "post_selection.mixed_support",
            is_bench_or_benchmark_support_document,
        );
    }

    matches
}

pub(super) fn apply_runtime_companion_test_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    let wants_runtime_companion_tests = ctx.intent.wants_test_witness_recall
        || ctx.intent.wants_entrypoint_build_flow
        || ctx.intent.wants_runtime_config_artifacts;
    if !wants_runtime_companion_tests {
        return matches;
    }

    let prefer_runtime_adjacent_python =
        ctx.intent.wants_test_witness_recall && !ctx.intent.wants_entrypoint_build_flow;
    let state = selection_guardrail_state(&matches, ctx);
    let selected_best = matches
        .iter()
        .filter(|entry| {
            is_plain_test_support_path(&entry.document.path)
                && (!prefer_runtime_adjacent_python
                    || surfaces::is_runtime_adjacent_python_test_path(&entry.document.path))
        })
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
        .filter(|entry| {
            is_plain_test_support_path(&entry.document.path)
                && (!prefer_runtime_adjacent_python
                    || surfaces::is_runtime_adjacent_python_test_path(&entry.document.path))
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
        (Some(candidate), Some(selected_path)) => selection_guardrail_score(candidate, &state, ctx)
            .total_cmp(&selection_guardrail_score_for_path(
                selected_path,
                &matches,
                &state,
                ctx,
            ))
            .is_gt(),
        (Some(_), None) => true,
        _ => false,
    };

    if should_promote {
        insert_test_support_guardrail_candidate(
            matches,
            candidate,
            ctx,
            "post_selection.runtime_companion_tests",
            selected_best,
        )
    } else {
        matches
    }
}
