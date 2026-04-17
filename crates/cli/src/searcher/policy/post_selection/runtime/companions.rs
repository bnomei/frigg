use super::*;

pub(in crate::searcher::policy::post_selection) fn apply_runtime_companion_surface_visibility(
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

    if ctx.intent.wants_runtime_config_artifacts {
        let has_root_runtime_config = matches
            .iter()
            .any(|entry| is_root_scoped_runtime_config_path(&entry.document.path));
        let has_specific_runtime_surface = matches.iter().any(|entry| {
            is_specific_runtime_config_surface_path(&entry.document.path)
                || surfaces::is_entrypoint_runtime_path(&entry.document.path)
        });
        if has_root_runtime_config && has_specific_runtime_surface {
            return matches;
        }
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
    let preserve_selected_build_workflow = preserve_selected_build_workflow(&matches, ctx);
    if (ctx.intent.wants_jobs_listeners_witnesses || ctx.intent.wants_commands_middleware_witnesses)
        && !ctx.intent.wants_entrypoint_build_flow
        && !ctx.intent.wants_runtime_config_artifacts
        && selected_best.is_some()
        && matches
            .iter()
            .any(is_runtime_companion_surface_guardrail_replacement)
    {
        return insert_guardrail_candidate(matches, Some(candidate), ctx, meta, |entry| {
            is_runtime_companion_surface_guardrail_replacement(entry)
                && (!preserve_selected_build_workflow
                    || (!surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
                        && !surfaces::is_ci_workflow_path(&entry.document.path)))
        });
    }
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

    insert_guardrail_candidate(matches, Some(candidate), ctx, meta, |entry| {
        is_runtime_companion_surface_guardrail_replacement(entry)
            && (!preserve_selected_build_workflow
                || (!surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
                    && !surfaces::is_ci_workflow_path(&entry.document.path)))
    })
}

pub(in crate::searcher::policy::post_selection) fn apply_runtime_witness_rescue_visibility(
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
    let wants_specific_runtime_family =
        ctx.intent.wants_jobs_listeners_witnesses || ctx.intent.wants_commands_middleware_witnesses;
    let satisfies_requested_runtime_family = |entry: &HybridRankedEvidence| {
        let facts = selection_guardrail_facts(entry, &state, ctx);
        let witness_backed = facts.has_path_witness_source
            || facts.specific_witness_path_overlap > 0
            || facts.runtime_subtree_affinity > 0;
        let graph_backed = entry.graph_score > 0.0 || !entry.graph_sources.is_empty();
        let path_family_match = (ctx.intent.wants_jobs_listeners_witnesses
            && facts.is_laravel_job_or_listener)
            || (ctx.intent.wants_commands_middleware_witnesses
                && facts.is_laravel_command_or_middleware);
        path_family_match
            && (witness_backed || graph_backed)
            && !facts.is_ci_workflow
            && !facts.is_repo_metadata
            && !facts.is_generic_runtime_witness_doc
            && !facts.is_frontend_runtime_noise
    };
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
        let graph_backed_runtime = (ctx.intent.wants_jobs_listeners_witnesses
            || ctx.intent.wants_commands_middleware_witnesses)
            && (entry.graph_score > 0.0 || !entry.graph_sources.is_empty())
            && (facts.has_exact_query_term_match
                || facts.path_overlap > 0
                || facts.is_laravel_job_or_listener
                || facts.is_laravel_command_or_middleware);
        candidate_surface
            && (witness_backed || graph_backed_runtime)
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
    let has_satisfied_runtime_rescue = if wants_specific_runtime_family {
        matches.iter().any(satisfies_requested_runtime_family)
    } else {
        matches.iter().any(is_rescue_candidate)
    };
    if !has_noise_slot || has_satisfied_runtime_rescue {
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

    let preserve_selected_build_workflow = preserve_selected_build_workflow(&matches, ctx);
    insert_guardrail_candidate(matches, candidate, ctx, meta, |entry| {
        is_runtime_companion_surface_guardrail_replacement(entry)
            && (!preserve_selected_build_workflow
                || (!surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
                    && !surfaces::is_ci_workflow_path(&entry.document.path)))
    })
}

pub(in crate::searcher::policy::post_selection) fn apply_mixed_support_visibility(
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

pub(in crate::searcher::policy::post_selection) fn apply_runtime_companion_test_visibility(
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

pub(in crate::searcher::policy::post_selection) fn apply_runtime_companion_test_ordering(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    _meta: PostSelectionRuleMeta,
) -> Vec<HybridRankedEvidence> {
    let wants_runtime_companion_tests = ctx.intent.wants_test_witness_recall
        || ctx.intent.wants_entrypoint_build_flow
        || ctx.intent.wants_runtime_config_artifacts;
    if !wants_runtime_companion_tests {
        return matches;
    }

    let test_indices = matches
        .iter()
        .enumerate()
        .filter(|(_, entry)| is_plain_test_support_path(&entry.document.path))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if test_indices.len() < 2 {
        return matches;
    }

    let state = selection_guardrail_state(&matches, ctx);
    let mut ordered_tests = test_indices
        .iter()
        .map(|index| matches[*index].clone())
        .collect::<Vec<_>>();
    ordered_tests.sort_by(|left, right| {
        selection_guardrail_cmp(right, left, &state, ctx)
            .then_with(|| left.document.path.cmp(&right.document.path))
    });

    for (slot_index, ordered_entry) in test_indices.into_iter().zip(ordered_tests) {
        matches[slot_index] = ordered_entry;
    }

    matches
}
