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
    hybrid_excerpt_has_build_flow_anchor, hybrid_excerpt_has_test_double_anchor,
    hybrid_identifier_tokens, hybrid_overlap_count, hybrid_path_overlap_count,
    hybrid_query_exact_terms, hybrid_query_overlap_terms, path_has_exact_query_term_match,
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
    let mut seen_classes = BTreeMap::<HybridSourceClass, usize>::new();
    let mut seen_laravel_ui_surfaces = BTreeMap::<LaravelUiSurfaceClass, usize>::new();
    let mut remaining = ranked;
    let mut selected = Vec::with_capacity(limit.min(remaining.len()));

    while selected.len() < limit && !remaining.is_empty() {
        let best_index = remaining
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                hybrid_selection_score(
                    left,
                    &intent,
                    &seen_classes,
                    &seen_laravel_ui_surfaces,
                    query_text,
                )
                .total_cmp(&hybrid_selection_score(
                    right,
                    &intent,
                    &seen_classes,
                    &seen_laravel_ui_surfaces,
                    query_text,
                ))
                .then_with(|| hybrid_ranked_evidence_order(right, left))
            })
            .map(|(index, _)| index)
            .unwrap_or(0);
        let chosen = remaining.remove(best_index);
        let class = hybrid_source_class(&chosen.document.path);
        *seen_classes.entry(class).or_insert(0) += 1;
        if let Some(surface) = laravel_ui_surface_class(&chosen.document.path) {
            *seen_laravel_ui_surfaces.entry(surface).or_insert(0) += 1;
        }
        selected.push(chosen);
    }

    selected
}

fn hybrid_runtime_witness_path_overlap_multiplier(
    path: &str,
    class: HybridSourceClass,
    query_text: &str,
) -> f32 {
    let overlap = hybrid_path_overlap_count(path, query_text);

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

fn blade_component_specific_path_overlap_count(path: &str, query_text: &str) -> usize {
    let specific_terms = blade_component_specific_query_terms(query_text);
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
    evidence: &HybridRankedEvidence,
    intent: &HybridRankingIntent,
    seen_classes: &BTreeMap<HybridSourceClass, usize>,
    seen_laravel_ui_surfaces: &BTreeMap<LaravelUiSurfaceClass, usize>,
    query_text: &str,
) -> f32 {
    let class = hybrid_source_class(&evidence.document.path);
    let wants_example_or_bench_witnesses = intent.wants_examples || intent.wants_benchmarks;
    let penalize_generic_runtime_docs =
        !intent.wants_docs && !intent.wants_onboarding && !intent.wants_readme;
    let exact_terms = hybrid_query_exact_terms(query_text);
    let query_overlap_terms = hybrid_query_overlap_terms(query_text);
    let query_mentions_cli =
        query_overlap_terms.iter().any(|token| token == "cli") || query_text.contains("cli");
    let query_has_identifier_anchor = query_overlap_terms.len() > exact_terms.len();
    let path_overlap = hybrid_path_overlap_count(&evidence.document.path, query_text);
    let blade_specific_path_overlap =
        blade_component_specific_path_overlap_count(&evidence.document.path, query_text);
    let query_has_specific_blade_anchors = intent.wants_blade_component_witnesses
        && !blade_component_specific_query_terms(query_text).is_empty();
    let seen_count = seen_classes.get(&class).copied().unwrap_or(0);
    let runtime_seen = seen_classes
        .get(&HybridSourceClass::Runtime)
        .copied()
        .unwrap_or(0);
    let mut score = evidence.blended_score
        * hybrid_path_quality_multiplier_with_intent(&evidence.document.path, intent);
    score *= hybrid_canonical_match_multiplier(&evidence.document.path, query_text);
    if intent.wants_runtime_witnesses {
        score *= hybrid_runtime_witness_path_overlap_multiplier(
            &evidence.document.path,
            class,
            query_text,
        );
    }
    if intent.wants_entrypoint_build_flow
        && hybrid_excerpt_has_build_flow_anchor(&evidence.excerpt, &query_overlap_terms)
    {
        score *= 1.12;
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
        if penalize_generic_runtime_docs
            && is_generic_runtime_witness_doc_path(&evidence.document.path)
            && seen_count > 0
        {
            score -= 0.16 * seen_count as f32;
        }
        if penalize_generic_runtime_docs
            && is_generic_runtime_witness_doc_path(&evidence.document.path)
            && runtime_seen == 0
        {
            score -= 0.18;
        }
        if penalize_generic_runtime_docs
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
        if is_frontend_runtime_noise_path(&evidence.document.path) {
            score -= if runtime_seen == 0 { 0.28 } else { 0.18 };
        }
        if intent.wants_examples && is_example_support_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.40 } else { 0.22 };
        }
        if intent.wants_benchmarks && is_bench_support_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.44 } else { 0.24 };
        }
        if wants_example_or_bench_witnesses
            && class == HybridSourceClass::Tests
            && !is_example_support_path(&evidence.document.path)
            && !is_bench_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.34 } else { 0.18 };
        }
        if wants_example_or_bench_witnesses
            && class == HybridSourceClass::Runtime
            && !is_example_support_path(&evidence.document.path)
            && !is_bench_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.30 } else { 0.16 };
        }
        if wants_example_or_bench_witnesses
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
            } else if query_has_specific_blade_anchors
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
        if path_has_exact_query_term_match(&evidence.document.path, query_text) {
            score += if seen_count == 0 { 2.2 } else { 1.2 };
        }
        if !wants_example_or_bench_witnesses && is_test_support_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.18 } else { 0.10 };
        }
        if !wants_example_or_bench_witnesses
            && query_mentions_cli
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
        if query_mentions_cli
            && class == HybridSourceClass::Runtime
            && !is_cli_test_support_path(&evidence.document.path)
        {
            score -= if seen_count == 0 { 0.34 } else { 0.20 };
        }
        if query_mentions_cli
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
        if evidence.document.path == "crates/cli/src/mcp/server.rs" {
            score += 0.08;
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
        if path_has_exact_query_term_match(&evidence.document.path, query_text) {
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
        if query_mentions_cli && is_cli_test_support_path(&evidence.document.path) {
            score += if seen_count == 0 { 0.78 } else { 0.42 };
        }
        if hybrid_excerpt_has_build_flow_anchor(&evidence.excerpt, &query_overlap_terms) {
            score += 0.16;
        }
        if hybrid_excerpt_has_test_double_anchor(&evidence.excerpt) {
            score -= 0.24;
        }
        if query_mentions_cli
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
            && !(query_mentions_cli && is_cli_test_support_path(&evidence.document.path))
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
    if query_has_identifier_anchor
        && (intent.wants_runtime_witnesses || intent.wants_entrypoint_build_flow)
    {
        let excerpt_overlap = hybrid_overlap_count(
            &hybrid_identifier_tokens(&evidence.excerpt),
            &query_overlap_terms,
        );
        let best_overlap = path_overlap.max(excerpt_overlap);
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
