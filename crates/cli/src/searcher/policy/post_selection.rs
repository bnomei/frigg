use std::cmp::Ordering;
use std::path::Path;

use super::super::HybridChannelHit;
use super::super::HybridRankedEvidence;
use super::super::intent::HybridRankingIntent;
use super::super::laravel::{
    is_laravel_blade_component_path, is_laravel_bootstrap_entrypoint_path,
    is_laravel_command_or_middleware_path, is_laravel_core_provider_path,
    is_laravel_layout_blade_view_path, is_laravel_livewire_component_path,
    is_laravel_livewire_view_path, is_laravel_non_livewire_blade_view_path,
    is_laravel_provider_path, is_laravel_route_path, is_laravel_view_component_class_path,
};
use super::super::query_terms::{
    hybrid_path_has_exact_stem_match, hybrid_path_overlap_count, hybrid_query_exact_terms,
};
use super::super::surfaces::{self, HybridSourceClass};

type TransformFn =
    for<'a> fn(Vec<HybridRankedEvidence>, &PostSelectionContext<'a>) -> Vec<HybridRankedEvidence>;

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct PostSelectionRule {
    id: &'static str,
    apply: TransformFn,
}

impl PostSelectionRule {
    const fn new(id: &'static str, apply: TransformFn) -> Self {
        Self { id, apply }
    }
}

const RULES: &[PostSelectionRule] = &[
    PostSelectionRule::new(
        "post_selection.runtime_config",
        apply_runtime_config_surface_selection,
    ),
    PostSelectionRule::new(
        "post_selection.entrypoint_build_workflow",
        apply_entrypoint_build_workflow_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.ci_scripts_ops",
        apply_ci_scripts_ops_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.mixed_support",
        apply_mixed_support_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.runtime_companion_tests",
        apply_runtime_companion_test_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_entrypoint",
        apply_laravel_entrypoint_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_blade_surface",
        apply_laravel_blade_surface_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_ui_test_harness",
        apply_laravel_ui_test_harness_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_layout_companion",
        apply_laravel_layout_companion_visibility,
    ),
];

pub(crate) struct PostSelectionContext<'a> {
    intent: &'a HybridRankingIntent,
    query_text: &'a str,
    exact_terms: Vec<String>,
    limit: usize,
    candidate_pool: &'a [HybridRankedEvidence],
    witness_hits: &'a [HybridChannelHit],
}

impl<'a> PostSelectionContext<'a> {
    pub(crate) fn new(
        intent: &'a HybridRankingIntent,
        query_text: &'a str,
        limit: usize,
        candidate_pool: &'a [HybridRankedEvidence],
        witness_hits: &'a [HybridChannelHit],
    ) -> Self {
        Self {
            intent,
            query_text,
            exact_terms: hybrid_query_exact_terms(query_text),
            limit,
            candidate_pool,
            witness_hits,
        }
    }
}

pub(crate) fn apply(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if matches.is_empty() {
        return matches;
    }

    for rule in RULES {
        matches = (rule.apply)(matches, ctx);
    }

    matches
}

fn apply_runtime_config_surface_selection(
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
            ctx.limit,
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
            ctx.limit,
            is_runtime_config_guardrail_replacement,
        );
    }

    matches
}

fn apply_entrypoint_build_workflow_visibility(
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
        ctx.limit,
        is_entrypoint_build_workflow_guardrail_replacement,
    )
}

fn apply_ci_scripts_ops_visibility(
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
                ctx.limit,
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
            ctx.limit,
            is_ci_workflow_guardrail_replacement,
        );
    }

    matches
}

fn apply_mixed_support_visibility(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if !ctx.intent.wants_test_witness_recall
        || !(ctx.intent.wants_examples || ctx.intent.wants_benchmarks)
    {
        return matches;
    }

    if ctx.intent.wants_examples && !matches.iter().any(is_example_support_document) {
        let grouped_candidate = ctx
            .candidate_pool
            .iter()
            .filter(|entry| {
                !matches
                    .iter()
                    .any(|selected| selected.document == entry.document)
            })
            .filter(|entry| surfaces::is_example_support_path(&entry.document.path))
            .max_by(|left, right| {
                example_support_guardrail_cmp(
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
            .filter(|hit| surfaces::is_example_support_path(&hit.document.path))
            .max_by(|left, right| {
                example_support_guardrail_cmp(
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
                example_support_guardrail_cmp(
                    &left.document.path,
                    &right.document.path,
                    ctx.query_text,
                    &ctx.exact_terms,
                )
                .then_with(|| left.blended_score.total_cmp(&right.blended_score))
            });

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx.limit,
            is_example_support_guardrail_replacement,
        );
    }

    if ctx.intent.wants_benchmarks && !matches.iter().any(is_bench_support_document) {
        let candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| surfaces::is_bench_support_path(&hit.document.path))
            .max_by(|left, right| {
                left.raw_score
                    .total_cmp(&right.raw_score)
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx.limit,
            is_plain_test_support_document,
        );
    }

    if !matches.iter().any(is_plain_test_support_document) {
        let candidate = ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .filter(|hit| is_plain_test_support_path(&hit.document.path))
            .max_by(|left, right| {
                left.raw_score
                    .total_cmp(&right.raw_score)
                    .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .map(hybrid_ranked_evidence_from_witness_hit);

        matches = insert_guardrail_candidate(
            matches,
            candidate,
            ctx.limit,
            is_bench_or_benchmark_support_document,
        );
    }

    matches
}

fn apply_runtime_companion_test_visibility(
    matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    let wants_runtime_companion_tests = ctx.intent.wants_test_witness_recall
        || ctx.intent.wants_entrypoint_build_flow
        || ctx.intent.wants_runtime_config_artifacts;
    if !wants_runtime_companion_tests {
        return matches;
    }

    let prefer_runtime_anchor_tests = !ctx.intent.wants_test_witness_recall;
    let has_runtime_adjacent_python_test_candidate = ctx
        .candidate_pool
        .iter()
        .filter(|entry| {
            !matches
                .iter()
                .any(|selected| selected.document == entry.document)
        })
        .any(|entry| surfaces::is_runtime_adjacent_python_test_path(&entry.document.path))
        || ctx
            .witness_hits
            .iter()
            .filter(|hit| {
                !matches
                    .iter()
                    .any(|selected| selected.document == hit.document)
            })
            .any(|hit| surfaces::is_runtime_adjacent_python_test_path(&hit.document.path));
    let selected_test_filter = |path: &str| {
        if !is_plain_test_support_path(path) {
            return false;
        }
        if !prefer_runtime_anchor_tests {
            return true;
        }

        hybrid_path_overlap_count(path, ctx.query_text) > 0
            || hybrid_path_has_exact_stem_match(path, &ctx.exact_terms)
            || surfaces::is_runtime_anchor_test_support_path(path)
            || surfaces::is_cli_test_support_path(path)
    };
    let candidate_filter = |path: &str| {
        if !selected_test_filter(path) {
            return false;
        }
        if has_runtime_adjacent_python_test_candidate
            && !surfaces::is_runtime_adjacent_python_test_path(path)
            && !surfaces::is_cli_test_support_path(path)
            && !surfaces::is_test_harness_path(path)
            && (!prefer_runtime_anchor_tests
                || !surfaces::is_runtime_anchor_test_support_path(path))
        {
            return false;
        }
        true
    };
    let selected_best = matches
        .iter()
        .filter(|entry| selected_test_filter(&entry.document.path))
        .max_by(|left, right| {
            test_support_candidate_cmp(
                &left.document.path,
                &right.document.path,
                &matches,
                ctx.candidate_pool,
                ctx.query_text,
                &ctx.exact_terms,
                prefer_runtime_anchor_tests,
            )
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
        .filter(|entry| candidate_filter(&entry.document.path))
        .max_by(|left, right| {
            test_support_candidate_cmp(
                &left.document.path,
                &right.document.path,
                &matches,
                ctx.candidate_pool,
                ctx.query_text,
                &ctx.exact_terms,
                prefer_runtime_anchor_tests,
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
        .filter(|hit| candidate_filter(&hit.document.path))
        .max_by(|left, right| {
            test_support_candidate_cmp(
                &left.document.path,
                &right.document.path,
                &matches,
                ctx.candidate_pool,
                ctx.query_text,
                &ctx.exact_terms,
                prefer_runtime_anchor_tests,
            )
                .then_with(|| left.raw_score.total_cmp(&right.raw_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(hybrid_ranked_evidence_from_witness_hit);
    let candidate = choose_best_candidate(grouped_candidate, witness_candidate, |left, right| {
        test_support_candidate_cmp(
            &left.document.path,
            &right.document.path,
            &matches,
            ctx.candidate_pool,
            ctx.query_text,
            &ctx.exact_terms,
            prefer_runtime_anchor_tests,
        )
            .then_with(|| left.blended_score.total_cmp(&right.blended_score))
    });

    let should_promote = match (candidate.as_ref(), selected_best.as_ref()) {
        (Some(candidate), Some(selected_path)) => test_support_candidate_cmp(
            &candidate.document.path,
            selected_path,
            &matches,
            ctx.candidate_pool,
            ctx.query_text,
            &ctx.exact_terms,
            prefer_runtime_anchor_tests,
        )
        .is_gt(),
        (Some(_), None) => true,
        _ => false,
    };

    if should_promote {
        insert_test_support_guardrail_candidate(matches, candidate, ctx.limit, selected_best)
    } else {
        matches
    }
}

fn apply_laravel_entrypoint_visibility(
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
            ctx.limit,
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
            ctx.limit,
            is_laravel_entrypoint_guardrail_replacement,
        );
    }

    matches
}

fn apply_laravel_blade_surface_visibility(
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
            ctx.limit,
            is_laravel_ui_guardrail_replacement,
        );
    }

    matches
}

fn apply_laravel_ui_test_harness_visibility(
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
        ctx.limit,
        is_laravel_ui_test_guardrail_replacement,
    )
}

fn apply_laravel_layout_companion_visibility(
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
            ctx.limit,
            is_laravel_ui_guardrail_replacement,
        );
    }

    matches
}

fn choose_best_candidate(
    grouped_candidate: Option<HybridRankedEvidence>,
    witness_candidate: Option<HybridRankedEvidence>,
    cmp: impl Fn(&HybridRankedEvidence, &HybridRankedEvidence) -> Ordering,
) -> Option<HybridRankedEvidence> {
    match (grouped_candidate, witness_candidate) {
        (Some(left), Some(right)) => {
            if cmp(&left, &right).is_ge() {
                Some(left)
            } else {
                Some(right)
            }
        }
        (Some(candidate), None) | (None, Some(candidate)) => Some(candidate),
        (None, None) => None,
    }
}

fn insert_guardrail_candidate(
    mut matches: Vec<HybridRankedEvidence>,
    candidate: Option<HybridRankedEvidence>,
    limit: usize,
    replacement_predicate: fn(&HybridRankedEvidence) -> bool,
) -> Vec<HybridRankedEvidence> {
    let Some(candidate) = candidate else {
        return matches;
    };

    let replacement_index = matches
        .iter()
        .enumerate()
        .rev()
        .find(|(_, entry)| replacement_predicate(entry))
        .map(|(index, _)| index);

    if let Some(index) = replacement_index {
        matches[index] = candidate;
    } else if matches.len() < limit {
        matches.push(candidate);
    }

    matches
}

fn insert_test_support_guardrail_candidate(
    mut matches: Vec<HybridRankedEvidence>,
    candidate: Option<HybridRankedEvidence>,
    limit: usize,
    selected_best_path: Option<String>,
) -> Vec<HybridRankedEvidence> {
    let Some(candidate) = candidate else {
        return matches;
    };

    let replacement_index = selected_best_path
        .as_deref()
        .and_then(|selected_path| {
            matches
                .iter()
                .position(|entry| entry.document.path == selected_path)
        })
        .or_else(|| {
            matches
                .iter()
                .enumerate()
                .filter(|(_, entry)| is_test_support_guardrail_replacement(entry))
                .max_by_key(|(_, entry)| test_support_guardrail_replacement_priority(entry))
                .map(|(index, _)| index)
        });

    if let Some(index) = replacement_index {
        matches[index] = candidate;
    } else if matches.len() < limit {
        matches.push(candidate);
    }

    matches
}

fn is_repo_root_runtime_config_document(entry: &HybridRankedEvidence) -> bool {
    is_repo_root_runtime_config_path(&entry.document.path)
}

fn is_ci_workflow_document(entry: &HybridRankedEvidence) -> bool {
    surfaces::is_ci_workflow_path(&entry.document.path)
}

fn is_repo_root_runtime_config_path(path: &str) -> bool {
    surfaces::is_runtime_config_artifact_path(path) && !path.trim_start_matches("./").contains('/')
}

fn is_specific_runtime_config_surface_path(path: &str) -> bool {
    if surfaces::is_typescript_runtime_module_index_path(path) {
        return true;
    }
    if !surfaces::is_entrypoint_runtime_path(path) {
        return false;
    }
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| !stem.eq_ignore_ascii_case("main"))
        .unwrap_or(false)
}

fn is_plain_test_support_path(path: &str) -> bool {
    surfaces::is_test_support_path(path)
        && !surfaces::is_example_support_path(path)
        && !surfaces::is_bench_support_path(path)
}

fn is_plain_test_support_document(entry: &HybridRankedEvidence) -> bool {
    is_plain_test_support_path(&entry.document.path)
}

fn is_example_support_document(entry: &HybridRankedEvidence) -> bool {
    surfaces::is_example_support_path(&entry.document.path)
}

fn example_support_guardrail_cmp(
    left: &str,
    right: &str,
    query_text: &str,
    exact_terms: &[String],
) -> Ordering {
    hybrid_path_overlap_count(left, query_text)
        .cmp(&hybrid_path_overlap_count(right, query_text))
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

fn test_support_guardrail_cmp(
    left: &str,
    right: &str,
    query_text: &str,
    exact_terms: &[String],
    prefer_runtime_anchor_tests: bool,
) -> Ordering {
    let left_overlap = hybrid_path_overlap_count(left, query_text);
    let right_overlap = hybrid_path_overlap_count(right, query_text);
    let left_priority = test_support_guardrail_priority(left, prefer_runtime_anchor_tests);
    let right_priority = test_support_guardrail_priority(right, prefer_runtime_anchor_tests);
    left_overlap
        .cmp(&right_overlap)
        .then_with(|| {
            hybrid_path_has_exact_stem_match(left, exact_terms)
                .cmp(&hybrid_path_has_exact_stem_match(right, exact_terms))
        })
        .then_with(|| left_priority.cmp(&right_priority))
        .then_with(|| {
            right
                .trim_start_matches("./")
                .split('/')
                .count()
                .cmp(&left.trim_start_matches("./").split('/').count())
        })
}

fn test_support_candidate_cmp(
    left: &str,
    right: &str,
    matches: &[HybridRankedEvidence],
    candidate_pool: &[HybridRankedEvidence],
    query_text: &str,
    exact_terms: &[String],
    prefer_runtime_anchor_tests: bool,
) -> Ordering {
    let runtime_adjacent_cmp = surfaces::is_runtime_adjacent_python_test_path(left)
        .cmp(&surfaces::is_runtime_adjacent_python_test_path(right));
    let family_cmp = test_support_family_affinity(left, matches, candidate_pool)
        .cmp(&test_support_family_affinity(right, matches, candidate_pool));
    let guardrail_cmp = test_support_guardrail_cmp(
        left,
        right,
        query_text,
        exact_terms,
        prefer_runtime_anchor_tests,
    );

    if prefer_runtime_anchor_tests {
        guardrail_cmp
            .then_with(|| runtime_adjacent_cmp)
            .then_with(|| family_cmp)
    } else {
        runtime_adjacent_cmp
            .then_with(|| family_cmp)
            .then_with(|| guardrail_cmp)
    }
}

fn test_support_guardrail_priority(path: &str, prefer_runtime_anchor_tests: bool) -> usize {
    if prefer_runtime_anchor_tests {
        if surfaces::is_runtime_anchor_test_support_path(path) {
            if surfaces::is_runtime_adjacent_python_test_path(path) {
                if is_non_prefix_python_test_module_path(path) {
                    4
                } else {
                    5
                }
            } else if is_non_prefix_python_test_module_path(path) {
                3
            } else {
                4
            }
        } else if surfaces::is_cli_test_support_path(path) {
            3
        } else {
            0
        }
    } else if surfaces::is_cli_test_support_path(path) || surfaces::is_test_harness_path(path) {
        2
    } else if surfaces::is_runtime_adjacent_python_test_path(path) {
        2
    } else if surfaces::is_runtime_anchor_test_support_path(path) {
        if is_non_prefix_python_test_module_path(path) {
            1
        } else {
            2
        }
    } else {
        1
    }
}

fn is_non_prefix_python_test_module_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.ends_with(".py")
        && Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.starts_with("test_"))
}

fn test_support_family_affinity(
    path: &str,
    matches: &[HybridRankedEvidence],
    candidate_pool: &[HybridRankedEvidence],
) -> usize {
    matches
        .iter()
        .chain(candidate_pool.iter())
        .filter(|entry| {
            surfaces::is_entrypoint_runtime_path(&entry.document.path)
                || surfaces::is_runtime_config_artifact_path(&entry.document.path)
        })
        .map(|entry| shared_path_prefix_segments(path, &entry.document.path))
        .max()
        .unwrap_or(0)
}

fn shared_path_prefix_segments(left: &str, right: &str) -> usize {
    left.trim_start_matches("./")
        .split('/')
        .zip(right.trim_start_matches("./").split('/'))
        .take_while(|(left, right)| left == right)
        .count()
}

fn is_promotable_laravel_blade_surface_path(path: &str) -> bool {
    is_laravel_blade_component_path(path) || is_laravel_non_livewire_blade_view_path(path)
}

fn is_layout_companion_blade_surface_path(path: &str) -> bool {
    is_promotable_laravel_blade_surface_path(path) && !is_laravel_layout_blade_view_path(path)
}

fn is_bench_support_document(entry: &HybridRankedEvidence) -> bool {
    surfaces::is_bench_support_path(&entry.document.path)
}

fn is_example_support_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_example_support_document(entry) {
        return false;
    }
    if is_repo_root_runtime_config_document(entry) {
        return false;
    }
    if surfaces::is_entrypoint_runtime_path(&entry.document.path) {
        return false;
    }
    if surfaces::is_bench_support_path(&entry.document.path) {
        return true;
    }
    if surfaces::is_ci_workflow_path(&entry.document.path) {
        return true;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Runtime
            | HybridSourceClass::Project
            | HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_support_path(&entry.document.path)
        || surfaces::is_test_harness_path(&entry.document.path)
}

fn is_bench_or_benchmark_support_document(entry: &HybridRankedEvidence) -> bool {
    is_bench_support_document(entry)
        || matches!(
            surfaces::hybrid_source_class(&entry.document.path),
            HybridSourceClass::BenchmarkDocs
        )
}

fn ci_workflow_guardrail_cmp(left: &str, right: &str, query_text: &str) -> Ordering {
    let left_overlap = hybrid_path_overlap_count(left, query_text);
    let right_overlap = hybrid_path_overlap_count(right, query_text);
    left_overlap
        .cmp(&right_overlap)
        .then_with(|| {
            ci_workflow_guardrail_priority_for_path(left)
                .cmp(&ci_workflow_guardrail_priority_for_path(right))
        })
        .then_with(|| {
            right
                .trim_start_matches("./")
                .split('/')
                .count()
                .cmp(&left.trim_start_matches("./").split('/').count())
        })
}

fn runtime_config_surface_guardrail_priority_for_path(path: &str) -> usize {
    let path_stem = Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_ascii_lowercase())
        .unwrap_or_default();
    if matches!(path_stem.as_str(), "server" | "cli") {
        3
    } else if surfaces::is_typescript_runtime_module_index_path(path) {
        2
    } else {
        1
    }
}

fn ci_workflow_guardrail_priority_for_path(path: &str) -> usize {
    if surfaces::is_entrypoint_build_workflow_path(path) {
        3
    } else {
        1
    }
}

fn scripts_ops_guardrail_cmp(
    left: &str,
    right: &str,
    query_text: &str,
    exact_terms: &[String],
) -> Ordering {
    let left_priority = scripts_ops_guardrail_priority_for_path(left);
    let right_priority = scripts_ops_guardrail_priority_for_path(right);
    left_priority
        .cmp(&right_priority)
        .then_with(|| {
            hybrid_path_overlap_count(left, query_text)
                .cmp(&hybrid_path_overlap_count(right, query_text))
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

fn scripts_ops_guardrail_priority_for_path(path: &str) -> usize {
    let normalized = path.trim_start_matches("./");
    if matches!(normalized, "justfile" | "makefile") {
        return 5;
    }
    if normalized.starts_with("scripts/") || normalized.starts_with("xtask/") {
        let segments = normalized.split('/').count();
        if segments == 2 {
            return 4;
        }
        return 2;
    }
    if normalized.contains("/scripts/") {
        return 1;
    }
    0
}

fn laravel_blade_surface_guardrail_cmp(
    left: &str,
    right: &str,
    query_text: &str,
    exact_terms: &[String],
) -> Ordering {
    let left_overlap = hybrid_path_overlap_count(left, query_text);
    let right_overlap = hybrid_path_overlap_count(right, query_text);
    left_overlap
        .cmp(&right_overlap)
        .then_with(|| {
            laravel_blade_surface_guardrail_priority(left)
                .cmp(&laravel_blade_surface_guardrail_priority(right))
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

fn laravel_blade_surface_guardrail_priority(path: &str) -> usize {
    if is_laravel_blade_component_path(path) {
        let normalized = path.trim_start_matches("./");
        if normalized.starts_with("resources/views/components/") {
            3
        } else {
            2
        }
    } else if is_laravel_non_livewire_blade_view_path(path) {
        1
    } else {
        0
    }
}

fn hybrid_ranked_evidence_from_witness_hit(hit: &HybridChannelHit) -> HybridRankedEvidence {
    HybridRankedEvidence {
        document: hit.document.clone(),
        anchor: hit.anchor.clone(),
        excerpt: hit.excerpt.clone(),
        blended_score: hit.raw_score.max(0.0),
        lexical_score: hit.raw_score.max(0.0),
        graph_score: 0.0,
        semantic_score: 0.0,
        lexical_sources: hit.provenance_ids.clone(),
        graph_sources: Vec::new(),
        semantic_sources: Vec::new(),
    }
}

fn is_runtime_config_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_repo_root_runtime_config_document(entry) {
        return false;
    }
    if is_specific_runtime_config_surface_path(&entry.document.path) {
        return false;
    }
    if surfaces::is_ci_workflow_path(&entry.document.path) {
        return true;
    }
    if surfaces::is_entrypoint_runtime_path(&entry.document.path) {
        return Path::new(entry.document.path.trim_start_matches("./"))
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| matches!(stem, "__main__" | "main" | "manage" | "run"));
    }
    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Runtime
            | HybridSourceClass::Project
            | HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_support_path(&entry.document.path)
        || surfaces::is_test_harness_path(&entry.document.path)
}

fn is_ci_workflow_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_ci_workflow_document(entry) {
        return false;
    }
    if surfaces::is_scripts_ops_path(&entry.document.path) {
        return scripts_ops_guardrail_priority_for_path(&entry.document.path) < 4;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Runtime
            | HybridSourceClass::Project
            | HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_support_path(&entry.document.path)
        || surfaces::is_test_harness_path(&entry.document.path)
}

fn is_entrypoint_build_workflow_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if surfaces::is_entrypoint_build_workflow_path(&entry.document.path) {
        return false;
    }
    if surfaces::is_entrypoint_runtime_path(&entry.document.path) {
        let normalized = entry.document.path.trim_start_matches("./");
        if matches!(normalized, "src/main.rs" | "src/lib.rs")
            || normalized.ends_with("/src/main.rs")
            || normalized.ends_with("/src/lib.rs")
        {
            return false;
        }
    }

    if surfaces::is_runtime_config_artifact_path(&entry.document.path) {
        return true;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Runtime
            | HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_support_path(&entry.document.path)
        || surfaces::is_test_harness_path(&entry.document.path)
}

fn is_scripts_ops_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if surfaces::is_scripts_ops_path(&entry.document.path) {
        return scripts_ops_guardrail_priority_for_path(&entry.document.path) < 4;
    }
    if surfaces::is_ci_workflow_path(&entry.document.path) {
        return false;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Runtime
            | HybridSourceClass::Project
            | HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_support_path(&entry.document.path)
        || surfaces::is_test_harness_path(&entry.document.path)
}

fn is_laravel_entrypoint_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_laravel_route_path(&entry.document.path)
        || is_laravel_bootstrap_entrypoint_path(&entry.document.path)
    {
        return false;
    }

    if is_laravel_provider_path(&entry.document.path)
        || is_laravel_core_provider_path(&entry.document.path)
        || is_laravel_command_or_middleware_path(&entry.document.path)
    {
        return true;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_support_path(&entry.document.path)
        || surfaces::is_test_harness_path(&entry.document.path)
}

fn is_laravel_ui_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_promotable_laravel_blade_surface_path(&entry.document.path) {
        return false;
    }

    if is_laravel_livewire_component_path(&entry.document.path)
        || is_laravel_livewire_view_path(&entry.document.path)
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

fn is_laravel_ui_test_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if surfaces::is_test_harness_path(&entry.document.path) {
        return false;
    }

    if is_laravel_livewire_component_path(&entry.document.path)
        || is_laravel_livewire_view_path(&entry.document.path)
        || is_laravel_view_component_class_path(&entry.document.path)
    {
        return true;
    }

    surfaces::is_test_support_path(&entry.document.path)
        || matches!(
            surfaces::hybrid_source_class(&entry.document.path),
            HybridSourceClass::Runtime
                | HybridSourceClass::Project
                | HybridSourceClass::Documentation
                | HybridSourceClass::Readme
        )
}

fn is_test_support_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_plain_test_support_document(entry) {
        return false;
    }
    if is_repo_root_runtime_config_document(entry) {
        return false;
    }
    if surfaces::is_runtime_config_artifact_path(&entry.document.path) {
        return true;
    }
    if surfaces::is_entrypoint_runtime_path(&entry.document.path) {
        return Path::new(entry.document.path.trim_start_matches("./"))
            .file_stem()
            .and_then(|stem| stem.to_str())
            .is_some_and(|stem| matches!(stem, "__main__" | "main" | "manage" | "run"));
    }
    if surfaces::is_ci_workflow_path(&entry.document.path) {
        return true;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Runtime
            | HybridSourceClass::Project
            | HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_support_path(&entry.document.path)
        || surfaces::is_test_harness_path(&entry.document.path)
}

fn test_support_guardrail_replacement_priority(entry: &HybridRankedEvidence) -> usize {
    if surfaces::is_frontend_runtime_noise_path(&entry.document.path) {
        5
    } else if surfaces::is_entrypoint_build_workflow_path(&entry.document.path) {
        4
    } else if surfaces::is_ci_workflow_path(&entry.document.path) {
        3
    } else if surfaces::is_runtime_config_artifact_path(&entry.document.path) {
        0
    } else if matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Project
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
            | HybridSourceClass::Specs
    ) {
        2
    } else if surfaces::is_entrypoint_runtime_path(&entry.document.path) {
        1
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{EvidenceAnchor, EvidenceAnchorKind, EvidenceChannel, EvidenceDocumentRef};

    fn make_ranked(path: &str, score: f32) -> HybridRankedEvidence {
        HybridRankedEvidence {
            document: EvidenceDocumentRef {
                repository_id: "repo".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            anchor: EvidenceAnchor::new(EvidenceAnchorKind::PathWitness, 1, 1, 1, 1),
            excerpt: path.to_owned(),
            blended_score: score,
            lexical_score: score,
            graph_score: 0.0,
            semantic_score: 0.0,
            lexical_sources: Vec::new(),
            graph_sources: Vec::new(),
            semantic_sources: Vec::new(),
        }
    }

    fn make_witness(path: &str, score: f32) -> HybridChannelHit {
        HybridChannelHit {
            channel: EvidenceChannel::PathSurfaceWitness,
            document: EvidenceDocumentRef {
                repository_id: "repo".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: 1,
            },
            anchor: EvidenceAnchor::new(EvidenceAnchorKind::PathWitness, 1, 1, 1, 1),
            raw_score: score,
            excerpt: path.to_owned(),
            provenance_ids: vec!["path_witness:test".to_owned()],
        }
    }

    fn apply_context(
        matches: Vec<HybridRankedEvidence>,
        candidate_pool: &[HybridRankedEvidence],
        witness_hits: &[HybridChannelHit],
        intent: &HybridRankingIntent,
        query_text: &str,
        limit: usize,
    ) -> Vec<HybridRankedEvidence> {
        let ctx =
            PostSelectionContext::new(intent, query_text, limit, candidate_pool, witness_hits);
        apply(matches, &ctx)
    }

    #[test]
    fn post_selection_policy_runtime_config_recovers_specific_surface_and_root_manifest_without_exceeding_limit()
     {
        let matches = vec![
            make_ranked(".github/workflows/ci.yml", 0.90),
            make_ranked("tests/runtime_config_test.rs", 0.84),
        ];
        let witness_hits = vec![
            make_witness("src/server.ts", 0.88),
            make_witness("src/index.ts", 0.87),
            make_witness("package.json", 0.86),
        ];
        let intent = HybridRankingIntent::from_query("runtime config package.json server build");
        assert!(intent.wants_runtime_config_artifacts);

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "package json server tsconfig",
            2,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert_eq!(final_matches.len(), 2);
        assert!(paths.contains(&"package.json"));
        assert!(paths.contains(&"src/server.ts"));
        assert!(!paths.contains(&"src/index.ts"));
    }

    #[test]
    fn post_selection_policy_runtime_config_uses_candidate_pool_when_witness_hits_are_missing() {
        let matches = vec![
            make_ranked("src/main.rs", 0.96),
            make_ranked("tests/runtime_config_test.rs", 0.84),
        ];
        let candidate_pool = vec![
            make_ranked("src/lib.rs", 0.95),
            make_ranked("Cargo.toml", 0.94),
        ];
        let intent = HybridRankingIntent::from_query("entry point build flow config cargo");
        assert!(intent.wants_runtime_config_artifacts);

        let final_matches = apply_context(
            matches,
            &candidate_pool,
            &[],
            &intent,
            "config cargo server",
            2,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&"Cargo.toml"));
        assert!(!paths.contains(&"tests/runtime_config_test.rs"));
    }

    #[test]
    fn post_selection_policy_entrypoint_build_flow_inserts_workflow_without_replacing_canonical_main_or_lib()
     {
        let matches = vec![
            make_ranked("src/main.rs", 0.96),
            make_ranked("src/runner.rs", 0.90),
            make_ranked("README.md", 0.70),
        ];
        let witness_hits = vec![make_witness(".github/workflows/release.yml", 0.92)];
        let intent = HybridRankingIntent::from_query("entrypoint build workflow release runner");
        assert!(intent.wants_entrypoint_build_flow);

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "build workflow release main",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert_eq!(final_matches.len(), 3);
        assert!(paths.contains(&"src/main.rs"));
        assert!(paths.contains(&".github/workflows/release.yml"));
        assert!(!paths.contains(&"README.md"));
    }

    #[test]
    fn post_selection_policy_entrypoint_build_flow_uses_candidate_pool_when_witness_hits_are_missing()
     {
        let matches = vec![
            make_ranked("src/main.rs", 0.96),
            make_ranked("src/lib.rs", 0.92),
            make_ranked("README.md", 0.70),
        ];
        let candidate_pool = vec![make_ranked(".github/workflows/build-docker.yml", 0.91)];
        let intent = HybridRankingIntent::from_query("entrypoint build workflow release runner");

        let final_matches = apply_context(
            matches,
            &candidate_pool,
            &[],
            &intent,
            "build workflow release main",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(paths.contains(&"src/main.rs"));
        assert!(paths.contains(&"src/lib.rs"));
        assert!(paths.contains(&".github/workflows/build-docker.yml"));
        assert!(!paths.contains(&"README.md"));
    }

    #[test]
    fn post_selection_policy_entrypoint_queries_recover_root_runtime_manifest() {
        let matches = vec![
            make_ranked("backend/app.py", 0.96),
            make_ranked("backend/cli.py", 0.92),
            make_ranked("README.md", 0.78),
        ];
        let witness_hits = vec![make_witness("backend/pyproject.toml", 0.86)];
        let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
        assert!(intent.wants_entrypoint_build_flow);
        assert!(surfaces::is_runtime_config_artifact_path("backend/pyproject.toml"));
        assert!(is_runtime_config_guardrail_replacement(&make_ranked(
            "README.md",
            0.78,
        )));

        let ctx = PostSelectionContext::new(
            &intent,
            "entry point bootstrap app startup cli main",
            3,
            &[],
            &witness_hits,
        );
        let final_matches = apply_runtime_config_surface_selection(matches, &ctx);
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(
            paths.contains(&"backend/pyproject.toml"),
            "final paths: {paths:?}"
        );
        assert!(!paths.contains(&"README.md"), "final paths: {paths:?}");
    }

    #[test]
    fn post_selection_policy_ci_scripts_prefers_top_level_ops_and_ci_surfaces() {
        let matches = vec![
            make_ranked("scripts/ty_benchmark/src/benchmark/run.py", 0.96),
            make_ranked("scripts/ty_benchmark/pyproject.toml", 0.94),
            make_ranked("crates/ruff/src/lib.rs", 0.90),
        ];
        let candidate_pool = vec![
            make_ranked("scripts/Dockerfile.ecosystem", 0.89),
            make_ranked(".github/workflows/build-docker.yml", 0.88),
        ];
        let intent = HybridRankingIntent::from_query(
            "ci release workflow github action publish package deploy cross compile script scripts dockerfile utils build binaries build docker",
        );
        assert!(intent.wants_ci_workflow_witnesses);
        assert!(intent.wants_scripts_ops_witnesses);

        let final_matches = apply_context(
            matches,
            &candidate_pool,
            &[],
            &intent,
            "scripts dockerfile build workflow",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(paths.contains(&"scripts/Dockerfile.ecosystem"));
        assert!(paths.contains(&".github/workflows/build-docker.yml"));
        assert!(!paths.contains(&"scripts/ty_benchmark/pyproject.toml"));
    }

    #[test]
    fn post_selection_policy_mixed_support_recovers_missing_bench_and_plain_test_at_limit() {
        let matches = vec![
            make_ranked("tests/support/render_helpers.rs", 0.93),
            make_ranked("benchmarks/rendering.md", 0.78),
        ];
        let witness_hits = vec![
            make_witness("tests/support/bench_assertions.rs", 0.88),
            make_witness("benches/support/render_harness.rs", 0.87),
        ];
        let intent = HybridRankingIntent::from_query("tests benchmark render harness");
        assert!(intent.wants_test_witness_recall);
        assert!(intent.wants_benchmarks);

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "test bench support render",
            2,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert_eq!(final_matches.len(), 2);
        assert!(paths.iter().any(|path| is_plain_test_support_path(path)));
        assert!(
            paths
                .iter()
                .any(|path| surfaces::is_bench_support_path(path))
        );
    }

    #[test]
    fn post_selection_policy_mixed_support_recovers_missing_example_support_at_limit() {
        let matches = vec![
            make_ranked("platform/main.roc", 0.97),
            make_ranked("tests/cmd-test.roc", 0.92),
            make_ranked("crates/roc_host/src/lib.rs", 0.88),
        ];
        let witness_hits = vec![
            make_witness("examples/command.roc", 0.87),
            make_witness("examples/bytes-stdin-stdout.roc", 0.86),
        ];
        let intent = HybridRankingIntent::from_query(
            "entry point main app package platform runtime tests bytes stdin command line examples benches benchmark",
        );
        assert!(intent.wants_entrypoint_build_flow);
        assert!(intent.wants_examples);
        assert!(intent.wants_test_witness_recall);

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "entry point main app package platform runtime tests bytes stdin command line examples benches benchmark",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(paths.contains(&"platform/main.roc"), "final paths: {paths:?}");
        assert!(
            paths.iter().any(|path| surfaces::is_example_support_path(path)),
            "final paths: {paths:?}"
        );
    }

    #[test]
    fn post_selection_policy_recovers_runtime_anchor_test_for_entrypoint_queries() {
        let matches = vec![
            make_ranked("backend/app.py", 0.96),
            make_ranked("backend/cli.py", 0.92),
            make_ranked("backend/pyproject.toml", 0.89),
        ];
        let witness_hits = vec![make_witness("backend/tests/test_server.py", 0.84)];
        let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
        assert!(intent.wants_entrypoint_build_flow);

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "entry point bootstrap app startup cli main",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(
            paths.contains(&"backend/tests/test_server.py"),
            "final paths: {paths:?}"
        );
        assert!(paths.contains(&"backend/app.py"));
        assert!(paths.contains(&"backend/cli.py"));
        assert!(!paths.contains(&"backend/pyproject.toml"));
    }

    #[test]
    fn post_selection_policy_entrypoint_queries_keep_runtime_config_when_inserting_companion_test()
    {
        let matches = vec![
            make_ranked("classic/original_autogpt/autogpt/app/main.py", 0.97),
            make_ranked("autogpt_platform/backend/backend/app.py", 0.95),
            make_ranked("autogpt_platform/backend/backend/cli.py", 0.94),
            make_ranked("autogpt_platform/backend/backend/copilot/executor/__main__.py", 0.92),
            make_ranked("autogpt_platform/backend/pyproject.toml", 0.90),
        ];
        let witness_hits = vec![make_witness(
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
            0.88,
        )];
        let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
        assert!(intent.wants_entrypoint_build_flow);

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "entry point bootstrap app startup cli main",
            5,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert_eq!(final_matches.len(), 5);
        assert!(
            paths.contains(&"autogpt_platform/backend/pyproject.toml"),
            "final paths: {paths:?}"
        );
        assert!(
            paths.contains(&"autogpt_platform/backend/backend/blocks/mcp/test_server.py"),
            "final paths: {paths:?}"
        );
    }

    #[test]
    fn post_selection_policy_entrypoint_queries_prefer_prefix_python_tests_over_loose_suffix_tests()
    {
        let matches = vec![
            make_ranked("autogpt_platform/backend/backend/copilot/executor/__main__.py", 0.96),
            make_ranked("autogpt_platform/backend/backend/app.py", 0.92),
            make_ranked("autogpt_platform/backend/backend/cli.py", 0.89),
        ];
        let witness_hits = vec![
            make_witness("autogpt_platform/backend/backend/copilot/service_test.py", 0.90),
            make_witness("autogpt_platform/backend/backend/blocks/mcp/test_server.py", 0.88),
        ];
        let intent = HybridRankingIntent::from_query("entry point bootstrap app startup cli main");
        assert!(intent.wants_entrypoint_build_flow);
        assert!(!intent.wants_test_witness_recall);
        assert!(test_support_candidate_cmp(
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
            "autogpt_platform/backend/backend/copilot/service_test.py",
            &matches,
            &matches,
            "entry point bootstrap app startup cli main",
            &hybrid_query_exact_terms("entry point bootstrap app startup cli main"),
            !intent.wants_test_witness_recall,
        )
        .is_gt());
        let chosen_witness = witness_hits
            .iter()
            .max_by(|left, right| {
                test_support_candidate_cmp(
                    &left.document.path,
                    &right.document.path,
                    &matches,
                    &matches,
                    "entry point bootstrap app startup cli main",
                    &hybrid_query_exact_terms("entry point bootstrap app startup cli main"),
                    !intent.wants_test_witness_recall,
                )
                .then_with(|| left.raw_score.total_cmp(&right.raw_score))
                .then_with(|| left.document.cmp(&right.document).reverse())
            })
            .expect("witness candidate should exist");
        assert_eq!(
            chosen_witness.document.path,
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py"
        );
        let inserted = insert_test_support_guardrail_candidate(
            matches.clone(),
            Some(hybrid_ranked_evidence_from_witness_hit(chosen_witness)),
            3,
            None,
        );
        let inserted_paths: Vec<_> = inserted
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();
        assert!(
            inserted_paths.contains(&"autogpt_platform/backend/backend/blocks/mcp/test_server.py"),
            "inserted paths: {inserted_paths:?}"
        );

        let ctx = PostSelectionContext::new(
            &intent,
            "entry point bootstrap app startup cli main",
            3,
            &[],
            &witness_hits,
        );
        let final_matches = apply_runtime_companion_test_visibility(matches, &ctx);
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(
            paths.contains(&"autogpt_platform/backend/backend/blocks/mcp/test_server.py"),
            "final paths: {paths:?}"
        );
        assert!(
            !paths.contains(&"autogpt_platform/backend/backend/copilot/service_test.py"),
            "final paths: {paths:?}"
        );
    }

    #[test]
    fn post_selection_policy_recovers_runtime_anchor_test_for_runtime_config_queries() {
        let matches = vec![
            make_ranked("autogpt_platform/frontend/tutorial/helpers/index.ts", 0.97),
            make_ranked("backend/pyproject.toml", 0.95),
            make_ranked("backend/cli.py", 0.90),
        ];
        let witness_hits = vec![
            make_witness("backend/tests/test_helpers.py", 0.86),
            make_witness("backend/tests/test_server.py", 0.88),
        ];
        let intent = HybridRankingIntent::from_query("config setup pyproject tests helpers e2e");
        assert!(intent.wants_runtime_config_artifacts);

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "config setup pyproject tests helpers e2e",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(paths.iter().any(|path| is_plain_test_support_path(path)));
        assert!(paths.contains(&"backend/pyproject.toml"));
        assert!(paths.contains(&"backend/cli.py"));
        assert!(!paths.contains(&"autogpt_platform/frontend/tutorial/helpers/index.ts"));
    }

    #[test]
    fn post_selection_policy_recovers_plain_test_for_explicit_test_focus_queries() {
        let matches = vec![
            make_ranked("autogpt_platform/frontend/tutorial/helpers/index.ts", 0.97),
            make_ranked("backend/pyproject.toml", 0.95),
            make_ranked("backend/cli.py", 0.90),
        ];
        let witness_hits = vec![
            make_witness("backend/tests/test_helpers.py", 0.86),
            make_witness("backend/tests/test_server.py", 0.80),
        ];
        let intent = HybridRankingIntent::from_query(
            "tests fixtures integration helpers e2e config setup pyproject",
        );

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "tests fixtures integration helpers e2e config setup pyproject",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(paths.iter().any(|path| is_plain_test_support_path(path)));
        assert!(paths.contains(&"backend/pyproject.toml"));
        assert!(paths.contains(&"backend/cli.py"));
        assert!(!paths.contains(&"autogpt_platform/frontend/tutorial/helpers/index.ts"));
    }

    #[test]
    fn post_selection_policy_replaces_weaker_existing_plain_test_with_stronger_family_match() {
        let matches = vec![
            make_ranked("autogpt_platform/backend/pyproject.toml", 0.95),
            make_ranked("autogpt_platform/backend/backend/cli.py", 0.90),
            make_ranked("classic/original_autogpt/tests/unit/test_config.py", 0.88),
        ];
        let witness_hits = vec![
            make_witness("autogpt_platform/backend/backend/api/test_helpers.py", 0.84),
            make_witness("autogpt_platform/backend/backend/blocks/mcp/test_e2e.py", 0.82),
        ];
        let intent = HybridRankingIntent::from_query("config setup pyproject tests helpers e2e");

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "config setup pyproject tests helpers e2e",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(paths.contains(&"autogpt_platform/backend/backend/api/test_helpers.py"));
        assert!(!paths.contains(&"classic/original_autogpt/tests/unit/test_config.py"));
    }

    #[test]
    fn post_selection_policy_explicit_test_queries_prefer_runtime_adjacent_python_tests() {
        let matches = vec![
            make_ranked("autogpt_platform/backend/pyproject.toml", 0.95),
            make_ranked("autogpt_platform/backend/backend/cli.py", 0.90),
            make_ranked("classic/original_autogpt/setup.py", 0.88),
        ];
        let witness_hits = vec![
            make_witness("classic/original_autogpt/tests/integration/test_setup.py", 0.90),
            make_witness("autogpt_platform/backend/backend/api/test_helpers.py", 0.86),
        ];
        let intent = HybridRankingIntent::from_query(
            "tests fixtures integration helpers e2e config setup pyproject",
        );

        let final_matches = apply_context(
            matches,
            &[],
            &witness_hits,
            &intent,
            "tests fixtures integration helpers e2e config setup pyproject",
            3,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert!(
            paths.contains(&"autogpt_platform/backend/backend/api/test_helpers.py"),
            "final paths: {paths:?}"
        );
        assert!(
            !paths.contains(&"classic/original_autogpt/tests/integration/test_setup.py"),
            "final paths: {paths:?}"
        );
    }

    #[test]
    fn post_selection_policy_laravel_ui_recovers_test_harness_without_displacing_existing_blade_surface()
     {
        let matches = vec![
            make_ranked("resources/views/components/button.blade.php", 0.95),
            make_ranked("app/Livewire/ButtonPanel.php", 0.88),
        ];
        let candidate_pool = vec![make_ranked("tests/TestCase.php", 0.84)];
        let intent = HybridRankingIntent::from_query("blade component button view");
        assert!(intent.wants_laravel_ui_witnesses);

        let final_matches = apply_context(
            matches,
            &candidate_pool,
            &[],
            &intent,
            "blade component button harness",
            2,
        );
        let paths: Vec<_> = final_matches
            .iter()
            .map(|entry| entry.document.path.as_str())
            .collect();

        assert_eq!(final_matches.len(), 2);
        assert!(paths.contains(&"resources/views/components/button.blade.php"));
        assert!(paths.contains(&"tests/TestCase.php"));
        assert!(!paths.contains(&"app/Livewire/ButtonPanel.php"));
    }
}
