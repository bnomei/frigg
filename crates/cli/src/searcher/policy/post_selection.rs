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
        "post_selection.mixed_support",
        apply_mixed_support_visibility,
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
    let allow_typescript_entrypoint_guardrail = ctx.intent.wants_entrypoint_build_flow
        && !ctx.intent.wants_runtime_config_artifacts
        && (matches
            .iter()
            .any(is_typescript_runtime_config_guardrail_document)
            || ctx
                .witness_hits
                .iter()
                .any(|hit| is_typescript_runtime_config_guardrail_path(&hit.document.path)));
    if !(ctx.intent.wants_runtime_config_artifacts || allow_typescript_entrypoint_guardrail) {
        return matches;
    }

    let root_config_filter: fn(&str) -> bool = if ctx.intent.wants_runtime_config_artifacts {
        is_repo_root_runtime_config_path
    } else {
        is_repo_root_typescript_runtime_config_path
    };
    let specific_surface_filter: fn(&str) -> bool = if ctx.intent.wants_runtime_config_artifacts {
        is_specific_runtime_config_surface_path
    } else {
        is_typescript_specific_runtime_config_surface_path
    };

    if !matches
        .iter()
        .any(|entry| specific_surface_filter(&entry.document.path))
    {
        let candidate = ctx
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
        let candidate = ctx
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

    let candidate = ctx
        .witness_hits
        .iter()
        .filter(|hit| {
            !matches
                .iter()
                .any(|selected| selected.document == hit.document)
        })
        .filter(|hit| surfaces::is_entrypoint_build_workflow_path(&hit.document.path))
        .max_by(|left, right| {
            left.raw_score
                .total_cmp(&right.raw_score)
                .then_with(|| left.document.cmp(&right.document).reverse())
        })
        .map(hybrid_ranked_evidence_from_witness_hit);

    insert_guardrail_candidate(
        matches,
        candidate,
        ctx.limit,
        is_entrypoint_build_workflow_guardrail_replacement,
    )
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

fn is_repo_root_runtime_config_document(entry: &HybridRankedEvidence) -> bool {
    is_repo_root_runtime_config_path(&entry.document.path)
}

fn is_repo_root_runtime_config_path(path: &str) -> bool {
    surfaces::is_runtime_config_artifact_path(path) && !path.trim_start_matches("./").contains('/')
}

fn is_repo_root_typescript_runtime_config_path(path: &str) -> bool {
    if !is_repo_root_runtime_config_path(path) {
        return false;
    }

    matches!(
        Path::new(path).file_name().and_then(|name| name.to_str()),
        Some(
            "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock" | "tsconfig.json"
        )
    )
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

fn is_typescript_specific_runtime_config_surface_path(path: &str) -> bool {
    surfaces::is_typescript_runtime_module_index_path(path)
        || (surfaces::is_typescript_entrypoint_runtime_path(path)
            && Path::new(path)
                .file_stem()
                .and_then(|stem| stem.to_str())
                .map(|stem| !stem.eq_ignore_ascii_case("main"))
                .unwrap_or(false))
}

fn is_typescript_runtime_config_guardrail_path(path: &str) -> bool {
    is_repo_root_typescript_runtime_config_path(path)
        || is_typescript_specific_runtime_config_surface_path(path)
}

fn is_typescript_runtime_config_guardrail_document(entry: &HybridRankedEvidence) -> bool {
    is_typescript_runtime_config_guardrail_path(&entry.document.path)
}

fn is_plain_test_support_path(path: &str) -> bool {
    surfaces::is_test_support_path(path)
        && !surfaces::is_example_support_path(path)
        && !surfaces::is_bench_support_path(path)
}

fn is_plain_test_support_document(entry: &HybridRankedEvidence) -> bool {
    is_plain_test_support_path(&entry.document.path)
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

fn is_bench_or_benchmark_support_document(entry: &HybridRankedEvidence) -> bool {
    is_bench_support_document(entry)
        || matches!(
            surfaces::hybrid_source_class(&entry.document.path),
            HybridSourceClass::BenchmarkDocs
        )
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
    if surfaces::is_ci_workflow_path(&entry.document.path) {
        return true;
    }
    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Tests | HybridSourceClass::Specs
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
