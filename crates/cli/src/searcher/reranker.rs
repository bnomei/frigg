use std::collections::BTreeMap;
use std::path::Path;

use super::HybridRankedEvidence;
use super::hybrid_canonical_match_multiplier;
use super::hybrid_path_quality_multiplier_with_intent;
use super::intent::HybridRankingIntent;
use super::laravel::{
    LaravelUiSurfaceClass, is_laravel_blade_component_path, is_laravel_bootstrap_entrypoint_path,
    is_laravel_command_or_middleware_path, is_laravel_core_provider_path,
    is_laravel_form_action_blade_path, is_laravel_job_or_listener_path,
    is_laravel_layout_blade_view_path, is_laravel_livewire_component_path,
    is_laravel_livewire_view_path, is_laravel_nested_blade_component_path,
    is_laravel_non_livewire_blade_view_path, is_laravel_provider_path, is_laravel_route_path,
    is_laravel_view_component_class_path, laravel_ui_surface_class,
    laravel_ui_surface_novelty_bonus, laravel_ui_surface_repeat_penalty,
};
use super::query_terms::{
    hybrid_excerpt_has_build_flow_anchor, hybrid_excerpt_has_exact_identifier_anchor,
    hybrid_excerpt_has_test_double_anchor, hybrid_identifier_tokens, hybrid_overlap_count,
    hybrid_path_overlap_count_with_terms, hybrid_query_exact_terms, hybrid_query_overlap_terms,
    path_has_exact_query_term_match,
};
use super::surfaces::{
    HybridSourceClass, has_generic_runtime_anchor_stem, hybrid_source_class, is_bench_support_path,
    is_ci_workflow_path, is_cli_test_support_path, is_entrypoint_build_workflow_path,
    is_entrypoint_reference_doc_path, is_entrypoint_runtime_path, is_example_support_path,
    is_frontend_runtime_noise_path, is_generic_runtime_witness_doc_path,
    is_loose_python_test_module_path, is_navigation_reference_doc_path, is_navigation_runtime_path,
    is_non_code_test_doc_path, is_python_entrypoint_runtime_path, is_python_runtime_config_path,
    is_python_test_witness_path, is_repo_metadata_path, is_runtime_config_artifact_path,
    is_scripts_ops_path, is_test_harness_path, is_test_support_path,
};

pub(super) fn diversify_hybrid_ranked_evidence(
    ranked: Vec<HybridRankedEvidence>,
    limit: usize,
    query_text: &str,
) -> Vec<HybridRankedEvidence> {
    let intent = HybridRankingIntent::from_query(query_text);
    let query_context = HybridSelectionQueryContext::new(&intent, query_text);
    let mut seen_classes = BTreeMap::<HybridSourceClass, usize>::new();
    let mut seen_laravel_ui_surfaces = BTreeMap::<LaravelUiSurfaceClass, usize>::new();
    let mut remaining = ranked
        .into_iter()
        .map(|evidence| HybridSelectionCandidate::new(evidence, &intent, &query_context))
        .collect::<Vec<_>>();
    let mut selected = Vec::with_capacity(limit.min(remaining.len()));

    while selected.len() < limit && !remaining.is_empty() {
        let mut best_index = 0usize;
        let mut best_score = hybrid_selection_score(
            &remaining[0],
            &intent,
            &query_context,
            &seen_classes,
            &seen_laravel_ui_surfaces,
        );

        for (index, candidate) in remaining.iter().enumerate().skip(1) {
            let score = hybrid_selection_score(
                candidate,
                &intent,
                &query_context,
                &seen_classes,
                &seen_laravel_ui_surfaces,
            );
            if score.total_cmp(&best_score).is_gt()
                || (score.total_cmp(&best_score).is_eq()
                    && hybrid_ranked_evidence_order(
                        &candidate.evidence,
                        &remaining[best_index].evidence,
                    )
                    .is_lt())
            {
                best_index = index;
                best_score = score;
            }
        }

        let chosen = remaining.swap_remove(best_index);
        *seen_classes
            .entry(chosen.static_features.class)
            .or_insert(0) += 1;
        if let Some(surface) = laravel_ui_surface_class(&chosen.evidence.document.path) {
            *seen_laravel_ui_surfaces.entry(surface).or_insert(0) += 1;
        }
        selected.push(chosen.evidence);
    }

    selected
}

struct HybridSelectionQueryContext {
    exact_terms: Vec<String>,
    query_overlap_terms: Vec<String>,
    blade_component_specific_terms: Vec<String>,
    query_mentions_cli: bool,
    query_has_identifier_anchor: bool,
    query_has_specific_blade_anchors: bool,
    wants_example_or_bench_witnesses: bool,
    penalize_generic_runtime_docs: bool,
}

impl HybridSelectionQueryContext {
    fn new(intent: &HybridRankingIntent, query_text: &str) -> Self {
        let exact_terms = hybrid_query_exact_terms(query_text);
        let query_overlap_terms = hybrid_query_overlap_terms(query_text);
        let blade_component_specific_terms = blade_component_specific_query_terms(query_text);
        let query_mentions_cli =
            query_overlap_terms.iter().any(|token| token == "cli") || query_text.contains("cli");
        let query_has_identifier_anchor = query_overlap_terms.len() > exact_terms.len();
        let query_has_specific_blade_anchors =
            intent.wants_blade_component_witnesses && !blade_component_specific_terms.is_empty();
        let wants_example_or_bench_witnesses = intent.wants_examples || intent.wants_benchmarks;
        let penalize_generic_runtime_docs =
            !intent.wants_docs && !intent.wants_onboarding && !intent.wants_readme;

        Self {
            exact_terms,
            query_overlap_terms,
            blade_component_specific_terms,
            query_mentions_cli,
            query_has_identifier_anchor,
            query_has_specific_blade_anchors,
            wants_example_or_bench_witnesses,
            penalize_generic_runtime_docs,
        }
    }
}

struct HybridSelectionStaticFeatures {
    class: HybridSourceClass,
    path_overlap: usize,
    blade_specific_path_overlap: usize,
    canonical_match_multiplier: f32,
    runtime_witness_path_overlap_multiplier: f32,
    has_exact_query_term_match: bool,
    excerpt_overlap: usize,
    excerpt_has_exact_identifier_anchor: bool,
    excerpt_has_build_flow_anchor: bool,
    excerpt_has_test_double_anchor: bool,
}

struct HybridSelectionCandidate {
    evidence: HybridRankedEvidence,
    static_features: HybridSelectionStaticFeatures,
}

impl HybridSelectionCandidate {
    fn new(
        evidence: HybridRankedEvidence,
        intent: &HybridRankingIntent,
        query_context: &HybridSelectionQueryContext,
    ) -> Self {
        let class = hybrid_source_class(&evidence.document.path);
        let path_overlap = hybrid_path_overlap_count_with_terms(
            &evidence.document.path,
            &query_context.query_overlap_terms,
        );
        let blade_specific_path_overlap = blade_component_specific_path_overlap_count(
            &evidence.document.path,
            &query_context.blade_component_specific_terms,
        );
        let canonical_match_multiplier =
            hybrid_canonical_match_multiplier(&evidence.document.path, &query_context.exact_terms);
        let runtime_witness_path_overlap_multiplier = if intent.wants_runtime_witnesses {
            hybrid_runtime_witness_path_overlap_multiplier(path_overlap, class)
        } else {
            1.0
        };
        let has_exact_query_term_match =
            path_has_exact_query_term_match(&evidence.document.path, &query_context.exact_terms);
        let excerpt_overlap = if query_context.query_has_identifier_anchor
            && (intent.wants_runtime_witnesses || intent.wants_entrypoint_build_flow)
        {
            hybrid_overlap_count(
                &hybrid_identifier_tokens(&evidence.excerpt),
                &query_context.query_overlap_terms,
            )
        } else {
            0
        };
        let excerpt_has_exact_identifier_anchor = !query_context.exact_terms.is_empty()
            && hybrid_excerpt_has_exact_identifier_anchor(
                &evidence.excerpt,
                &query_context.exact_terms.join(" "),
            );
        let excerpt_has_build_flow_anchor = if intent.wants_entrypoint_build_flow {
            hybrid_excerpt_has_build_flow_anchor(
                &evidence.excerpt,
                &query_context.query_overlap_terms,
            )
        } else {
            false
        };
        let excerpt_has_test_double_anchor = if intent.wants_entrypoint_build_flow {
            hybrid_excerpt_has_test_double_anchor(&evidence.excerpt)
        } else {
            false
        };

        Self {
            evidence,
            static_features: HybridSelectionStaticFeatures {
                class,
                path_overlap,
                blade_specific_path_overlap,
                canonical_match_multiplier,
                runtime_witness_path_overlap_multiplier,
                has_exact_query_term_match,
                excerpt_overlap,
                excerpt_has_exact_identifier_anchor,
                excerpt_has_build_flow_anchor,
                excerpt_has_test_double_anchor,
            },
        }
    }
}

fn hybrid_runtime_witness_path_overlap_multiplier(overlap: usize, class: HybridSourceClass) -> f32 {
    match class {
        HybridSourceClass::Runtime => match overlap {
            0 => 1.0,
            1 => 1.32,
            2 => 1.58,
            _ => 1.84,
        },
        HybridSourceClass::Support | HybridSourceClass::Tests => match overlap {
            0 => 1.0,
            1 => 1.18,
            2 => 1.30,
            _ => 1.42,
        },
        _ => 1.0,
    }
}

fn blade_component_specific_query_terms(query_text: &str) -> Vec<String> {
    const GENERIC_TERMS: &[&str] = &[
        "app",
        "application",
        "blade",
        "component",
        "components",
        "company",
        "create",
        "creates",
        "footer",
        "header",
        "layout",
        "layouts",
        "page",
        "pages",
        "pest",
        "render",
        "report",
        "reports",
        "resources",
        "section",
        "slot",
        "test",
        "tests",
        "view",
        "views",
    ];

    hybrid_query_exact_terms(query_text)
        .into_iter()
        .filter(|term| {
            !GENERIC_TERMS
                .iter()
                .any(|generic| generic == &term.as_str())
        })
        .collect()
}

fn blade_component_specific_path_overlap_count(path: &str, specific_terms: &[String]) -> usize {
    if specific_terms.is_empty() {
        return 0;
    }

    let path_terms = hybrid_identifier_tokens(path);
    specific_terms
        .iter()
        .filter(|term| path_terms.iter().any(|path_term| path_term == *term))
        .count()
}

fn hybrid_selection_score(
    candidate: &HybridSelectionCandidate,
    intent: &HybridRankingIntent,
    query_context: &HybridSelectionQueryContext,
    seen_classes: &BTreeMap<HybridSourceClass, usize>,
    seen_laravel_ui_surfaces: &BTreeMap<LaravelUiSurfaceClass, usize>,
) -> f32 {
    let evidence = &candidate.evidence;
    let class = candidate.static_features.class;
    let path_overlap = candidate.static_features.path_overlap;
    let blade_specific_path_overlap = candidate.static_features.blade_specific_path_overlap;
    let seen_count = seen_classes.get(&class).copied().unwrap_or(0);
    let runtime_seen = seen_classes
        .get(&HybridSourceClass::Runtime)
        .copied()
        .unwrap_or(0);
    let mut score = evidence.blended_score
        * hybrid_path_quality_multiplier_with_intent(&evidence.document.path, intent);
    score *= candidate.static_features.canonical_match_multiplier;
    if intent.wants_runtime_witnesses {
        score *= candidate
            .static_features
            .runtime_witness_path_overlap_multiplier;
    }
    if candidate.static_features.excerpt_has_build_flow_anchor {
        score *= 1.12;
    }
    if !query_context.exact_terms.is_empty()
        && (intent.wants_contracts || intent.wants_error_taxonomy || intent.wants_tool_contracts)
    {
        if candidate
            .static_features
            .excerpt_has_exact_identifier_anchor
        {
            score += if seen_count == 0 { 0.78 } else { 0.38 };
            if matches!(
                class,
                HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
            ) {
                score += if seen_count == 0 { 0.18 } else { 0.08 };
            }
        } else if matches!(
            class,
            HybridSourceClass::Runtime
                | HybridSourceClass::Support
                | HybridSourceClass::Tests
                | HybridSourceClass::Documentation
                | HybridSourceClass::Readme
                | HybridSourceClass::Fixtures
                | HybridSourceClass::Playbooks
        ) {
            score -= if seen_count == 0 { 0.46 } else { 0.24 };
        }
    }

    if intent.wants_class(class) && seen_count == 0 {
        score += hybrid_class_novelty_bonus(class);
    }
    if seen_count > 0 {
        score -= hybrid_class_repeat_penalty(class) * seen_count as f32;
    }
    if intent.wants_laravel_ui_witnesses {
        if let Some(surface) = laravel_ui_surface_class(&evidence.document.path) {
            let surface_seen = seen_laravel_ui_surfaces.get(&surface).copied().unwrap_or(0);
            if surface_seen == 0 {
                score += laravel_ui_surface_novelty_bonus(
                    surface,
                    intent.wants_blade_component_witnesses,
                );
            } else {
                score -= laravel_ui_surface_repeat_penalty(
                    surface,
                    intent.wants_blade_component_witnesses,
                ) * surface_seen as f32;
            }
        }
    }
    if intent.wants_runtime_witnesses {
        if class == HybridSourceClass::Runtime && seen_count == 0 {
            score += 0.24;
        }
        if matches!(class, HybridSourceClass::Support | HybridSourceClass::Tests) && seen_count == 0
        {
            score += 0.10;
        }
        if candidate
            .static_features
            .excerpt_has_exact_identifier_anchor
            && matches!(
                class,
                HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
            )
        {
            score += if seen_count == 0 { 0.30 } else { 0.16 };
        }
        if matches!(
            class,
            HybridSourceClass::Playbooks | HybridSourceClass::Fixtures
        ) {
            score -= if seen_count == 0 { 0.42 } else { 0.24 };
        }
        if is_python_entrypoint_runtime_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.26 } else { 0.14 };
        }
        if is_python_runtime_config_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.18 } else { 0.10 };
        }
        if is_python_test_witness_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.28 } else { 0.12 };
        }
        if is_loose_python_test_module_path(&evidence.document.path) {
            score -= if seen_count == 0 { 0.18 } else { 0.10 };
        }
        if path_overlap > 0 {
            match class {
                HybridSourceClass::Runtime => {
                    score += if path_overlap == 1 { 0.10 } else { 0.18 };
                }
                HybridSourceClass::Support | HybridSourceClass::Tests => {
                    score += if path_overlap == 1 { 0.08 } else { 0.14 };
                }
                HybridSourceClass::Documentation | HybridSourceClass::Readme => {
                    score += if path_overlap == 1 { 0.02 } else { 0.06 };
                }
                _ => {}
            }
        }
        if query_context.penalize_generic_runtime_docs
            && is_generic_runtime_witness_doc_path(&evidence.document.path)
            && seen_count > 0
        {
            score -= 0.16 * seen_count as f32;
        }
        if query_context.penalize_generic_runtime_docs
            && is_generic_runtime_witness_doc_path(&evidence.document.path)
            && runtime_seen == 0
        {
            score -= 0.18;
        }
        if query_context.penalize_generic_runtime_docs
            && matches!(
                class,
                HybridSourceClass::Documentation | HybridSourceClass::Readme
            )
        {
            score -= match path_overlap {
                0 => 0.18,
                1 => 0.06,
                _ => 0.0,
            };
        }
        if is_repo_metadata_path(&evidence.document.path) {
            score -= if runtime_seen == 0 { 0.26 } else { 0.18 };
        }
        if is_python_runtime_config_path(&evidence.document.path) {
            score += if runtime_seen == 0 { 0.16 } else { 0.08 };
        }
        if class == HybridSourceClass::Runtime
            && path_overlap == 0
            && has_generic_runtime_anchor_stem(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.12 } else { 0.18 };
        }
        if !candidate
            .static_features
            .excerpt_has_exact_identifier_anchor
            && !candidate.static_features.has_exact_query_term_match
            && matches!(
                class,
                HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
            )
        {
            score -= match path_overlap {
                0 => {
                    if seen_count == 0 {
                        0.24
                    } else {
                        0.14
                    }
                }
                1 => {
                    if class == HybridSourceClass::Runtime {
                        0.18
                    } else {
                        0.10
                    }
                }
                _ => 0.0,
            };
        }
        if is_frontend_runtime_noise_path(&evidence.document.path) {
            score -= if runtime_seen == 0 { 0.28 } else { 0.18 };
        }
        if intent.wants_examples && is_example_support_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.40 } else { 0.22 };
        }
        if intent.wants_benchmarks && is_bench_support_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.44 } else { 0.24 };
        }
        if query_context.wants_example_or_bench_witnesses
            && class == HybridSourceClass::Tests
            && !is_example_support_path(&evidence.document.path)
            && !is_bench_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.34 } else { 0.18 };
        }
        if query_context.wants_example_or_bench_witnesses
            && class == HybridSourceClass::Runtime
            && !is_example_support_path(&evidence.document.path)
            && !is_bench_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.30 } else { 0.16 };
        }
        if query_context.wants_example_or_bench_witnesses
            && is_test_support_path(&evidence.document.path)
            && Path::new(&evidence.document.path)
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case("examples.rs"))
        {
            score -= if seen_count == 0 { 0.28 } else { 0.14 };
        }
        if is_python_test_witness_path(&evidence.document.path)
            && runtime_seen > 0
            && seen_count == 0
        {
            score += 0.18;
        }
    }
    if intent.wants_runtime_config_artifacts {
        if is_runtime_config_artifact_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.30 } else { 0.16 };
        }
        if is_python_runtime_config_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.16 } else { 0.08 };
        }
        if matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ) {
            score -= if path_overlap == 0 { 0.28 } else { 0.12 };
        }
        if is_repo_metadata_path(&evidence.document.path)
            && !is_runtime_config_artifact_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.32 } else { 0.18 };
        }
    }
    if intent.wants_laravel_ui_witnesses {
        if intent.wants_blade_component_witnesses {
            if blade_specific_path_overlap > 0 {
                score += match blade_specific_path_overlap {
                    1 => 0.28,
                    2 => 0.74,
                    _ => 1.16,
                };
            } else if query_context.query_has_specific_blade_anchors
                && is_laravel_blade_component_path(&evidence.document.path)
                && !intent.wants_laravel_layout_witnesses
            {
                score -= if seen_count == 0 { 0.46 } else { 0.22 };
            }
        }
        if intent.wants_laravel_form_action_witnesses {
            if is_laravel_form_action_blade_path(&evidence.document.path) {
                score += if seen_count == 0 { 1.42 } else { 0.54 };
            } else if is_laravel_blade_component_path(&evidence.document.path) {
                score -= if seen_count == 0 { 0.24 } else { 0.12 };
            }
        }
        if intent.wants_livewire_view_witnesses
            && is_laravel_non_livewire_blade_view_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.34 } else { 0.18 };
        }
        if intent.wants_livewire_view_witnesses
            && is_laravel_livewire_view_path(&evidence.document.path)
        {
            score += if seen_count == 0 { 0.72 } else { 0.28 };
        }
        if is_laravel_view_component_class_path(&evidence.document.path) {
            score -= if intent.wants_laravel_layout_witnesses {
                if seen_count == 0 { 1.10 } else { 1.42 }
            } else if seen_count == 0 {
                0.58
            } else {
                0.82
            };
        }
        if intent.wants_blade_component_witnesses {
            if is_laravel_livewire_component_path(&evidence.document.path) {
                score += if seen_count == 0 { 0.06 } else { -0.12 };
            }
            if is_laravel_non_livewire_blade_view_path(&evidence.document.path) {
                score += if seen_count == 0 { 0.24 } else { 0.06 };
            }
            if is_laravel_livewire_view_path(&evidence.document.path) {
                score += if seen_count == 0 { 0.12 } else { 0.02 };
            }
            if is_laravel_blade_component_path(&evidence.document.path) {
                score += if is_laravel_nested_blade_component_path(&evidence.document.path) {
                    if seen_count == 0 { 0.12 } else { -0.10 }
                } else if seen_count == 0 {
                    1.12
                } else {
                    0.42
                };
            }
        } else {
            if is_laravel_livewire_component_path(&evidence.document.path) {
                score += if seen_count == 0 { 0.18 } else { -0.18 };
            }
            if is_laravel_non_livewire_blade_view_path(&evidence.document.path) {
                score += if seen_count == 0 { 1.05 } else { 0.54 };
            }
            if is_laravel_livewire_view_path(&evidence.document.path) {
                score += if seen_count == 0 { 0.92 } else { 0.44 };
            }
            if is_laravel_blade_component_path(&evidence.document.path) {
                score += if path_overlap >= 3 {
                    if seen_count == 0 { 0.72 } else { 0.26 }
                } else if seen_count == 0 {
                    0.10
                } else {
                    -0.12
                };
            }
        }
        if is_test_harness_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.42 } else { 0.18 };
        }
        if intent.wants_commands_middleware_witnesses
            && is_laravel_command_or_middleware_path(&evidence.document.path)
        {
            score += if seen_count == 0 { 1.18 } else { 0.48 };
        }
        if intent.wants_jobs_listeners_witnesses
            && is_laravel_job_or_listener_path(&evidence.document.path)
        {
            score += if seen_count == 0 { 0.96 } else { 0.36 };
        }
        if intent.wants_laravel_layout_witnesses
            && is_laravel_layout_blade_view_path(&evidence.document.path)
        {
            score += if seen_count == 0 { 1.26 } else { 0.52 };
        }
        if is_repo_metadata_path(&evidence.document.path) {
            score -= if seen_count == 0 { 0.34 } else { 0.20 };
        }
        if let Some(surface) = laravel_ui_surface_class(&evidence.document.path) {
            let surface_seen = seen_laravel_ui_surfaces.get(&surface).copied().unwrap_or(0);
            if intent.wants_blade_component_witnesses {
                match surface {
                    LaravelUiSurfaceClass::BladeView => {
                        score += if surface_seen == 0 {
                            0.14
                        } else {
                            -0.18 * surface_seen as f32
                        };
                    }
                    LaravelUiSurfaceClass::LivewireComponent => {
                        score += if surface_seen == 0 {
                            0.08
                        } else {
                            -0.12 * surface_seen as f32
                        };
                    }
                    LaravelUiSurfaceClass::LivewireView => {
                        score += if surface_seen == 0 {
                            0.10
                        } else {
                            -0.12 * surface_seen as f32
                        };
                    }
                    LaravelUiSurfaceClass::BladeComponent => {
                        score += if is_laravel_nested_blade_component_path(&evidence.document.path)
                        {
                            if surface_seen == 0 {
                                0.10
                            } else {
                                -0.12 * surface_seen as f32
                            }
                        } else if surface_seen == 0 {
                            0.96
                        } else {
                            0.34 - (0.08 * surface_seen as f32)
                        };
                    }
                }
                if surface == LaravelUiSurfaceClass::BladeComponent
                    && !is_laravel_nested_blade_component_path(&evidence.document.path)
                    && !seen_laravel_ui_surfaces
                        .contains_key(&LaravelUiSurfaceClass::BladeComponent)
                {
                    score += 0.28;
                }
            } else {
                match surface {
                    LaravelUiSurfaceClass::BladeView => {
                        score += if surface_seen == 0 { 0.72 } else { 0.14 };
                    }
                    LaravelUiSurfaceClass::LivewireComponent => {
                        score += if surface_seen == 0 {
                            0.18
                        } else {
                            -0.14 * surface_seen as f32
                        };
                    }
                    LaravelUiSurfaceClass::LivewireView => {
                        score += if surface_seen == 0 { 0.84 } else { 0.18 };
                    }
                    LaravelUiSurfaceClass::BladeComponent => {
                        score -= if surface_seen == 0 {
                            0.04
                        } else {
                            0.72 * surface_seen as f32
                        };
                    }
                }
                if surface == LaravelUiSurfaceClass::BladeView
                    && !seen_laravel_ui_surfaces.contains_key(&LaravelUiSurfaceClass::BladeView)
                {
                    score += 0.22;
                }
                if surface == LaravelUiSurfaceClass::LivewireView
                    && !seen_laravel_ui_surfaces.contains_key(&LaravelUiSurfaceClass::LivewireView)
                {
                    score += 0.28;
                }
            }
        }
    }
    if intent.wants_test_witness_recall {
        if candidate.static_features.has_exact_query_term_match {
            score += if seen_count == 0 { 2.2 } else { 1.2 };
        }
        if !query_context.wants_example_or_bench_witnesses
            && is_test_support_path(&evidence.document.path)
        {
            score += if seen_count == 0 { 0.18 } else { 0.10 };
        }
        if !query_context.wants_example_or_bench_witnesses
            && query_context.query_mentions_cli
            && is_cli_test_support_path(&evidence.document.path)
        {
            score += if seen_count == 0 { 0.84 } else { 0.46 };
        }
        if is_test_harness_path(&evidence.document.path) {
            score += if seen_count == 0 { 1.1 } else { 0.6 };
        }
        if is_python_test_witness_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.34 } else { 0.18 };
        }
        if is_loose_python_test_module_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.12 } else { 0.06 };
        }
        if is_non_code_test_doc_path(&evidence.document.path) {
            score -= if seen_count == 0 { 0.44 } else { 0.26 };
        }
        if is_frontend_runtime_noise_path(&evidence.document.path) {
            score -= if seen_count == 0 { 0.28 } else { 0.16 };
        }
        if query_context.query_mentions_cli
            && class == HybridSourceClass::Runtime
            && !is_cli_test_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.34 } else { 0.20 };
        }
        if query_context.query_mentions_cli
            && class == HybridSourceClass::Tests
            && !is_cli_test_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.24 } else { 0.12 };
        }
    }
    if intent.wants_navigation_fallbacks {
        if is_navigation_runtime_path(&evidence.document.path) && seen_count == 0 {
            score += 0.14;
        }
        if is_navigation_reference_doc_path(&evidence.document.path) && runtime_seen == 0 {
            score -= 0.18;
        }
        if is_navigation_reference_doc_path(&evidence.document.path) && seen_count > 0 {
            score -= 0.10 * seen_count as f32;
        }
    }
    if intent.wants_ci_workflow_witnesses {
        if is_ci_workflow_path(&evidence.document.path) {
            score += if seen_count == 0 { 1.44 } else { 0.78 };
        }
        if matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ) && path_overlap == 0
        {
            score -= if seen_count == 0 { 0.22 } else { 0.12 };
        }
    }
    if intent.wants_scripts_ops_witnesses {
        if is_scripts_ops_path(&evidence.document.path) {
            score += if seen_count == 0 { 1.24 } else { 0.68 };
        }
        if candidate.static_features.has_exact_query_term_match {
            score += if seen_count == 0 { 0.76 } else { 0.40 };
        }
        if matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ) && path_overlap == 0
        {
            score -= if seen_count == 0 { 0.18 } else { 0.10 };
        }
    }
    if intent.wants_entrypoint_build_flow {
        if is_entrypoint_runtime_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.34 } else { 0.18 };
        }
        if is_entrypoint_build_workflow_path(&evidence.document.path) {
            score += if seen_count == 0 { 2.20 } else { 1.20 };
        }
        if is_laravel_core_provider_path(&evidence.document.path) {
            score += if seen_count == 0 { 1.40 } else { 0.78 };
        } else if is_laravel_provider_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.42 } else { 0.22 };
        }
        if is_laravel_route_path(&evidence.document.path) {
            score += if seen_count == 0 { 1.20 } else { 0.70 };
        }
        if is_laravel_bootstrap_entrypoint_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.26 } else { 0.14 };
        }
        if is_runtime_config_artifact_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.18 } else { 0.10 };
        }
        if is_python_runtime_config_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.16 } else { 0.08 };
        }
        if query_context.query_mentions_cli && is_cli_test_support_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.78 } else { 0.42 };
        }
        if candidate.static_features.excerpt_has_build_flow_anchor {
            score += 0.16;
        }
        if candidate.static_features.excerpt_has_test_double_anchor {
            score -= 0.24;
        }
        if query_context.query_mentions_cli
            && class == HybridSourceClass::Runtime
            && !is_cli_test_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.28 } else { 0.16 };
        }
        if class == HybridSourceClass::Runtime
            && path_overlap == 0
            && !is_entrypoint_runtime_path(&evidence.document.path)
            && !is_runtime_config_artifact_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.38 } else { 0.22 };
        }
        if matches!(class, HybridSourceClass::Tests | HybridSourceClass::Specs)
            && runtime_seen == 0
            && !(query_context.query_mentions_cli
                && is_cli_test_support_path(&evidence.document.path))
        {
            score -= 0.18;
        }
        if is_entrypoint_reference_doc_path(&evidence.document.path) && runtime_seen == 0 {
            score -= 0.14;
        }
        if is_frontend_runtime_noise_path(&evidence.document.path) {
            score -= if runtime_seen == 0 { 0.22 } else { 0.14 };
        }
        if is_loose_python_test_module_path(&evidence.document.path) {
            score -= if runtime_seen == 0 { 0.18 } else { 0.10 };
        }
    }
    if query_context.query_has_identifier_anchor
        && (intent.wants_runtime_witnesses || intent.wants_entrypoint_build_flow)
    {
        let best_overlap = path_overlap.max(candidate.static_features.excerpt_overlap);
        if best_overlap >= 2 {
            score += 0.18;
        } else if best_overlap == 1 {
            score += 0.08;
        } else if matches!(
            class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        ) && !is_entrypoint_runtime_path(&evidence.document.path)
        {
            score -= 0.14;
        }
    }

    score
}

fn hybrid_class_novelty_bonus(class: HybridSourceClass) -> f32 {
    match class {
        HybridSourceClass::ErrorContracts
        | HybridSourceClass::ToolContracts
        | HybridSourceClass::BenchmarkDocs => 0.08,
        HybridSourceClass::Documentation
        | HybridSourceClass::Runtime
        | HybridSourceClass::Project
        | HybridSourceClass::Tests => 0.04,
        HybridSourceClass::Support => 0.02,
        HybridSourceClass::Fixtures => 0.035,
        HybridSourceClass::Readme => 0.02,
        HybridSourceClass::Playbooks | HybridSourceClass::Specs | HybridSourceClass::Other => 0.0,
    }
}

fn hybrid_class_repeat_penalty(class: HybridSourceClass) -> f32 {
    match class {
        HybridSourceClass::ToolContracts => 0.09,
        HybridSourceClass::BenchmarkDocs => 0.07,
        HybridSourceClass::ErrorContracts | HybridSourceClass::Documentation => 0.05,
        HybridSourceClass::Readme => 0.03,
        HybridSourceClass::Runtime
        | HybridSourceClass::Project
        | HybridSourceClass::Tests
        | HybridSourceClass::Fixtures => 0.015,
        HybridSourceClass::Support => 0.02,
        HybridSourceClass::Playbooks | HybridSourceClass::Specs | HybridSourceClass::Other => 0.01,
    }
}

fn hybrid_ranked_evidence_order(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
) -> std::cmp::Ordering {
    right
        .blended_score
        .total_cmp(&left.blended_score)
        .then_with(|| right.lexical_score.total_cmp(&left.lexical_score))
        .then_with(|| right.graph_score.total_cmp(&left.graph_score))
        .then_with(|| right.semantic_score.total_cmp(&left.semantic_score))
        .then(left.document.cmp(&right.document))
        .then(left.excerpt.cmp(&right.excerpt))
}
