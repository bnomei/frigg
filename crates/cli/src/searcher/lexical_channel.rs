use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use crate::domain::model::TextMatch;
use crate::domain::{EvidenceAnchor, EvidenceAnchorKind, EvidenceChannel, EvidenceHit};

use super::{
    HybridChannelHit, HybridDocumentRef, HybridRankingIntent, HybridSourceClass,
    SearchExecutionOutput, StoredPathWitnessProjection, hybrid_excerpt_has_build_flow_anchor,
    hybrid_excerpt_has_exact_identifier_anchor, hybrid_excerpt_has_test_double_anchor,
    hybrid_identifier_tokens, hybrid_overlap_count, hybrid_path_overlap_tokens,
    hybrid_query_exact_terms, hybrid_query_overlap_terms, hybrid_source_class,
    is_bench_support_path, is_ci_workflow_path, is_cli_test_support_path,
    is_entrypoint_build_workflow_path, is_entrypoint_reference_doc_path,
    is_entrypoint_runtime_path, is_example_support_path, is_frontend_runtime_noise_path,
    is_generic_runtime_witness_doc_path, is_laravel_blade_component_path,
    is_laravel_bootstrap_entrypoint_path, is_laravel_core_provider_path,
    is_laravel_form_action_blade_path, is_laravel_layout_blade_view_path,
    is_laravel_livewire_component_path, is_laravel_livewire_view_path,
    is_laravel_nested_blade_component_path, is_laravel_non_livewire_blade_view_path,
    is_laravel_provider_path, is_laravel_route_path, is_laravel_view_component_class_path,
    is_loose_python_test_module_path, is_navigation_reference_doc_path, is_navigation_runtime_path,
    is_non_code_test_doc_path, is_python_entrypoint_runtime_path, is_python_runtime_config_path,
    is_python_test_witness_path, is_repo_metadata_path, is_runtime_config_artifact_path,
    is_scripts_ops_path, is_test_harness_path, is_test_support_path,
    path_has_exact_query_term_match, sort_matches_deterministically,
    sort_search_diagnostics_deterministically,
};

#[cfg(test)]
pub(super) fn build_hybrid_lexical_hits(matches: &[TextMatch]) -> Vec<HybridChannelHit> {
    build_hybrid_lexical_hits_with_intent(matches, &HybridRankingIntent::default(), "")
}

#[cfg(test)]
pub(super) fn build_hybrid_lexical_hits_for_query(
    matches: &[TextMatch],
    query_text: &str,
) -> Vec<HybridChannelHit> {
    let intent = HybridRankingIntent::from_query(query_text);
    build_hybrid_lexical_hits_with_intent(matches, &intent, query_text)
}

pub(super) fn build_hybrid_lexical_hits_with_intent(
    matches: &[TextMatch],
    intent: &HybridRankingIntent,
    query_text: &str,
) -> Vec<HybridChannelHit> {
    build_hybrid_hits_from_matches_with_intent(
        matches,
        intent,
        query_text,
        EvidenceChannel::LexicalManifest,
        EvidenceAnchorKind::TextSpan,
    )
}

pub(super) fn build_hybrid_path_witness_hits_with_intent(
    matches: &[TextMatch],
    intent: &HybridRankingIntent,
    query_text: &str,
) -> Vec<HybridChannelHit> {
    let mut hits = build_hybrid_hits_from_matches_with_intent(
        matches,
        intent,
        query_text,
        EvidenceChannel::PathSurfaceWitness,
        EvidenceAnchorKind::PathWitness,
    );
    for hit in &mut hits {
        hit.provenance_ids = vec![format!(
            "path_witness:{}:{}:{}",
            hit.document.path, hit.document.line, hit.document.column
        )];
    }
    hits
}

fn build_hybrid_hits_from_matches_with_intent(
    matches: &[TextMatch],
    intent: &HybridRankingIntent,
    query_text: &str,
    channel: EvidenceChannel,
    anchor_kind: EvidenceAnchorKind,
) -> Vec<EvidenceHit> {
    let mut frequency_by_document: BTreeMap<(String, String), f32> = BTreeMap::new();
    for found in matches {
        let key = (found.repository_id.clone(), found.path.clone());
        *frequency_by_document.entry(key).or_insert(0.0) += 1.0;
    }

    matches
        .iter()
        .map(|found| {
            let key = (found.repository_id.clone(), found.path.clone());
            let frequency = *frequency_by_document.get(&key).unwrap_or(&1.0);
            let raw_score = frequency.sqrt()
                * hybrid_path_quality_multiplier_with_intent(&found.path, intent)
                * hybrid_excerpt_alignment_multiplier(&found.excerpt, intent, query_text);
            let anchor = EvidenceAnchor::new(
                anchor_kind,
                found.line,
                found.column,
                found.line,
                found.column,
            );
            let anchor = match anchor_kind {
                EvidenceAnchorKind::PathWitness => anchor.with_detail(found.path.clone()),
                _ => anchor,
            };
            HybridChannelHit {
                channel,
                document: HybridDocumentRef {
                    repository_id: found.repository_id.clone(),
                    path: found.path.clone(),
                    line: found.line,
                    column: found.column,
                },
                anchor,
                raw_score,
                excerpt: found.excerpt.clone(),
                provenance_ids: vec![format!(
                    "text:{}:{}:{}",
                    found.path, found.line, found.column
                )],
            }
        })
        .collect()
}

fn hybrid_excerpt_alignment_multiplier(
    excerpt: &str,
    intent: &HybridRankingIntent,
    query_text: &str,
) -> f32 {
    let query_terms = hybrid_query_overlap_terms(query_text);
    if query_terms.is_empty() {
        return 1.0;
    }

    let excerpt_terms = hybrid_identifier_tokens(excerpt);
    let overlap = hybrid_overlap_count(&excerpt_terms, &query_terms);
    let mut multiplier = match overlap {
        0 => 1.0,
        1 => 1.05,
        2 => 1.14,
        _ => 1.24,
    };
    if hybrid_excerpt_has_exact_identifier_anchor(excerpt, query_text) {
        multiplier *= 1.18;
    }

    if intent.wants_entrypoint_build_flow {
        if hybrid_excerpt_has_build_flow_anchor(excerpt, &query_terms) {
            multiplier *= 1.24;
        }
        if hybrid_excerpt_has_test_double_anchor(excerpt) {
            multiplier *= 0.72;
        }
    }

    multiplier
}

pub(super) fn hybrid_path_quality_multiplier_with_intent(
    path: &str,
    intent: &HybridRankingIntent,
) -> f32 {
    let class = hybrid_source_class(path);
    let wants_example_or_bench_witnesses = intent.wants_examples || intent.wants_benchmarks;
    let penalize_generic_runtime_docs =
        !intent.wants_docs && !intent.wants_onboarding && !intent.wants_readme;
    let mut multiplier = match class {
        HybridSourceClass::ErrorContracts => 1.0,
        HybridSourceClass::ToolContracts => 1.0,
        HybridSourceClass::BenchmarkDocs => 0.98,
        HybridSourceClass::Playbooks => {
            if intent.penalize_playbook_self_reference {
                0.25
            } else {
                0.45
            }
        }
        HybridSourceClass::Documentation => 0.88,
        HybridSourceClass::Readme => 0.78,
        HybridSourceClass::Specs => 0.82,
        HybridSourceClass::Fixtures => 0.92,
        HybridSourceClass::Project => 0.94,
        HybridSourceClass::Support => 0.78,
        HybridSourceClass::Tests => 0.97,
        HybridSourceClass::Runtime => 1.0,
        HybridSourceClass::Other => {
            match Path::new(path).extension().and_then(|ext| ext.to_str()) {
                Some(
                    "rs" | "php" | "go" | "py" | "ts" | "tsx" | "js" | "jsx" | "java" | "kt"
                    | "kts",
                ) => 1.0,
                _ => 0.9,
            }
        }
    };
    let is_repo_metadata = is_repo_metadata_path(path);

    if intent.wants_docs
        && matches!(
            class,
            HybridSourceClass::Documentation
                | HybridSourceClass::ErrorContracts
                | HybridSourceClass::ToolContracts
                | HybridSourceClass::BenchmarkDocs
        )
    {
        multiplier *= 1.36;
    }
    if intent.wants_readme && class == HybridSourceClass::Readme {
        multiplier *= 1.15;
    }
    if intent.wants_readme && path == "README.md" {
        multiplier *= 1.45;
    }
    if intent.wants_onboarding && class == HybridSourceClass::Readme {
        multiplier *= 1.85;
    }
    if intent.wants_onboarding && path == "README.md" {
        multiplier *= 1.25;
    }
    if intent.wants_onboarding && class == HybridSourceClass::Documentation {
        multiplier *= 1.15;
    }
    if intent.wants_contracts
        && matches!(
            class,
            HybridSourceClass::ErrorContracts | HybridSourceClass::ToolContracts
        )
    {
        multiplier *= 1.55;
    }
    if intent.wants_error_taxonomy && class == HybridSourceClass::ErrorContracts {
        multiplier *= 1.95;
    }
    if intent.wants_error_taxonomy && class == HybridSourceClass::Runtime {
        multiplier *= 1.18;
    }
    if intent.wants_error_taxonomy && class == HybridSourceClass::Tests {
        multiplier *= 1.26;
    }
    if intent.wants_error_taxonomy
        && matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme | HybridSourceClass::Specs
        )
    {
        multiplier *= 0.78;
    }
    if intent.wants_tool_contracts && class == HybridSourceClass::ToolContracts {
        multiplier *= 2.10;
    }
    if intent.wants_mcp_runtime_surface && class == HybridSourceClass::Runtime {
        multiplier *= 1.22;
    }
    if intent.wants_mcp_runtime_surface && class == HybridSourceClass::Support {
        multiplier *= 1.10;
    }
    if intent.wants_mcp_runtime_surface && class == HybridSourceClass::Documentation {
        multiplier *= 1.12;
    }
    if intent.wants_mcp_runtime_surface && class == HybridSourceClass::Readme {
        multiplier *= 0.92;
    }
    if intent.wants_mcp_runtime_surface && class == HybridSourceClass::Tests {
        multiplier *= 0.82;
    }
    if intent.wants_mcp_runtime_surface
        && matches!(
            class,
            HybridSourceClass::BenchmarkDocs
                | HybridSourceClass::Fixtures
                | HybridSourceClass::Playbooks
        )
    {
        multiplier *= 0.72;
    }
    if intent.wants_benchmarks && class == HybridSourceClass::BenchmarkDocs {
        multiplier *= 2.00;
    }
    if intent.wants_contracts && class == HybridSourceClass::Readme {
        multiplier *= 0.65;
    }
    if intent.wants_tool_contracts && class == HybridSourceClass::Readme {
        multiplier *= 0.68;
    }
    if intent.wants_benchmarks && class == HybridSourceClass::Readme {
        multiplier *= 0.68;
    }
    if intent.wants_tests && class == HybridSourceClass::Tests {
        multiplier *= 1.12;
    }
    if intent.wants_test_witness_recall && class == HybridSourceClass::Tests {
        multiplier *= 1.18;
    }
    if intent.wants_examples && class == HybridSourceClass::Support {
        multiplier *= 1.18;
    }
    if intent.wants_examples && class == HybridSourceClass::Tests {
        multiplier *= 0.90;
    }
    if intent.wants_examples && is_example_support_path(path) {
        multiplier *= 1.48;
    }
    if intent.wants_benchmarks && class == HybridSourceClass::Support {
        multiplier *= 1.14;
    }
    if intent.wants_benchmarks && class == HybridSourceClass::Tests {
        multiplier *= 0.90;
    }
    if intent.wants_benchmarks && is_bench_support_path(path) {
        multiplier *= 1.54;
    }
    if intent.wants_ci_workflow_witnesses && is_ci_workflow_path(path) {
        multiplier *= 2.20;
    }
    if intent.wants_scripts_ops_witnesses && is_scripts_ops_path(path) {
        multiplier *= 1.92;
    }
    if wants_example_or_bench_witnesses
        && class == HybridSourceClass::Tests
        && !is_example_support_path(path)
        && !is_bench_support_path(path)
    {
        multiplier *= 0.68;
    }
    if wants_example_or_bench_witnesses
        && class == HybridSourceClass::Runtime
        && !is_example_support_path(path)
        && !is_bench_support_path(path)
    {
        multiplier *= 0.82;
    }
    if intent.wants_examples
        && matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        )
    {
        multiplier *= 0.74;
    }
    if intent.wants_benchmarks
        && matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        )
    {
        multiplier *= 0.72;
    }
    if intent.wants_onboarding && is_example_support_path(path) {
        multiplier *= 1.22;
    }
    if intent.wants_ci_workflow_witnesses
        && matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        )
    {
        multiplier *= 0.72;
    }
    if intent.wants_scripts_ops_witnesses
        && matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        )
    {
        multiplier *= 0.76;
    }
    if intent.wants_tests && is_test_support_path(path) {
        multiplier *= 1.10;
    }
    if intent.wants_tests && is_cli_test_support_path(path) {
        multiplier *= 1.16;
    }
    if intent.wants_laravel_ui_witnesses {
        if intent.wants_laravel_form_action_witnesses {
            if is_laravel_form_action_blade_path(path) {
                multiplier *= 2.48;
            } else if is_laravel_blade_component_path(path) {
                multiplier *= 0.74;
            }
        }
        if is_laravel_view_component_class_path(path) {
            multiplier *= if intent.wants_laravel_layout_witnesses {
                0.34
            } else {
                0.46
            };
        }
        if intent.wants_blade_component_witnesses {
            if is_laravel_livewire_component_path(path) {
                multiplier *= 0.94;
            }
            if is_laravel_non_livewire_blade_view_path(path) {
                multiplier *= 1.18;
            }
            if is_laravel_livewire_view_path(path) {
                multiplier *= 1.04;
            }
            if is_laravel_blade_component_path(path) {
                multiplier *= if is_laravel_nested_blade_component_path(path) {
                    0.88
                } else {
                    2.24
                };
            }
        } else {
            if is_laravel_livewire_component_path(path) {
                multiplier *= 1.34;
            }
            if is_laravel_non_livewire_blade_view_path(path) {
                multiplier *= 2.10;
            }
            if is_laravel_livewire_view_path(path) {
                multiplier *= 1.72;
            }
            if is_laravel_blade_component_path(path) {
                multiplier *= 0.98;
            }
        }
        if intent.wants_laravel_layout_witnesses && is_laravel_layout_blade_view_path(path) {
            multiplier *= if intent.wants_blade_component_witnesses {
                1.52
            } else {
                1.78
            };
        }
        if is_repo_metadata {
            multiplier *= 0.42;
        }
        if matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ) {
            multiplier *= 0.72;
        }
    }
    if intent.wants_test_witness_recall {
        if !wants_example_or_bench_witnesses && is_test_support_path(path) {
            multiplier *= 1.18;
        }
        if !wants_example_or_bench_witnesses && is_cli_test_support_path(path) {
            multiplier *= 1.32;
        }
        if is_test_harness_path(path) {
            multiplier *= 1.52;
        }
        if is_python_test_witness_path(path) {
            multiplier *= 1.34;
        }
        if intent.wants_benchmarks && is_python_test_witness_path(path) {
            multiplier *= 1.18;
        }
        if is_loose_python_test_module_path(path) {
            multiplier *= 1.08;
        }
        if is_non_code_test_doc_path(path) {
            multiplier *= 0.24;
        }
        if is_frontend_runtime_noise_path(path) {
            multiplier *= 0.56;
        }
    }
    if intent.wants_fixtures && class == HybridSourceClass::Fixtures {
        multiplier *= 1.14;
    }
    if intent.wants_runtime && class == HybridSourceClass::Runtime {
        multiplier *= 1.05;
    }
    if intent.wants_runtime_witnesses {
        if class == HybridSourceClass::Runtime {
            multiplier *= 1.52;
        }
        if matches!(class, HybridSourceClass::Support | HybridSourceClass::Tests) {
            multiplier *= 1.24;
        }
        if is_python_entrypoint_runtime_path(path) {
            multiplier *= 1.36;
        }
        if is_python_runtime_config_path(path) {
            multiplier *= 1.28;
        }
        if is_python_test_witness_path(path) {
            multiplier *= 1.26;
        }
        if is_loose_python_test_module_path(path) {
            multiplier *= 0.84;
        }
        if matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ) && penalize_generic_runtime_docs
        {
            multiplier *= 0.64;
        }
        if penalize_generic_runtime_docs && is_generic_runtime_witness_doc_path(path) {
            multiplier *= 0.58;
        }
        if is_frontend_runtime_noise_path(path) {
            multiplier *= 0.54;
        }
        if is_repo_metadata && !is_python_runtime_config_path(path) {
            multiplier *= 0.26;
        }
    }
    if intent.wants_runtime_config_artifacts {
        if is_runtime_config_artifact_path(path) {
            multiplier *= 1.40;
        }
        if is_python_runtime_config_path(path) {
            multiplier *= 1.18;
        }
        if matches!(
            class,
            HybridSourceClass::Documentation | HybridSourceClass::Readme
        ) {
            multiplier *= 0.52;
        }
        if is_repo_metadata && !is_runtime_config_artifact_path(path) {
            multiplier *= 0.34;
        }
        if is_frontend_runtime_noise_path(path) {
            multiplier *= 0.64;
        }
    }
    if intent.wants_navigation_fallbacks {
        if is_navigation_runtime_path(path) {
            multiplier *= 1.24;
        }
        if is_navigation_reference_doc_path(path) {
            multiplier *= 0.72;
        }
    }
    if intent.wants_entrypoint_build_flow {
        if is_entrypoint_runtime_path(path) {
            multiplier *= 1.48;
        }
        if is_entrypoint_build_workflow_path(path) {
            multiplier *= 3.20;
        }
        if is_laravel_core_provider_path(path) {
            multiplier *= 1.94;
        } else if is_laravel_provider_path(path) {
            multiplier *= 1.28;
        }
        if is_laravel_route_path(path) {
            multiplier *= 4.20;
        }
        if is_laravel_bootstrap_entrypoint_path(path) {
            multiplier *= 1.58;
        }
        if is_python_runtime_config_path(path) {
            multiplier *= 1.18;
        }
        if matches!(class, HybridSourceClass::Runtime) && !is_entrypoint_runtime_path(path) {
            multiplier *= 0.88;
        }
        if matches!(class, HybridSourceClass::Tests | HybridSourceClass::Specs) {
            multiplier *= 0.74;
        }
        if is_loose_python_test_module_path(path) {
            multiplier *= 0.82;
        }
        if is_frontend_runtime_noise_path(path) {
            multiplier *= 0.52;
        }
        if is_entrypoint_reference_doc_path(path) {
            multiplier *= 0.72;
        }
    }

    multiplier
}

pub(super) fn hybrid_canonical_match_multiplier(path: &str, exact_terms: &[String]) -> f32 {
    const CANONICAL_SUFFIXES: &[&str] = &[
        "reference",
        "request",
        "response",
        "result",
        "results",
        "handler",
        "formatter",
    ];

    let Some(stem) = Path::new(path).file_stem().and_then(|stem| stem.to_str()) else {
        return 1.0;
    };
    let normalized_stem = stem.trim().to_ascii_lowercase();
    if normalized_stem.is_empty() {
        return 1.0;
    }

    if exact_terms.is_empty() {
        return 1.0;
    }
    if exact_terms
        .iter()
        .any(|term| term.eq_ignore_ascii_case(&normalized_stem))
    {
        return 1.65;
    }

    for term in exact_terms {
        if !normalized_stem.starts_with(term.as_str()) || normalized_stem == *term {
            continue;
        }
        let suffix = &normalized_stem[term.len()..];
        if CANONICAL_SUFFIXES
            .iter()
            .any(|candidate| candidate == &suffix)
        {
            return 0.78;
        }
    }

    1.0
}

pub(super) struct HybridPathWitnessQueryContext {
    exact_terms: Vec<String>,
    query_overlap_terms: Vec<String>,
    query_mentions_cli: bool,
}

impl HybridPathWitnessQueryContext {
    pub(super) fn new(query_text: &str) -> Self {
        Self {
            exact_terms: hybrid_query_exact_terms(query_text),
            query_overlap_terms: hybrid_query_overlap_terms(query_text),
            query_mentions_cli: query_text.to_ascii_lowercase().contains("cli"),
        }
    }
}

fn score_path_witness_anchor_line(
    line: &str,
    path_terms: &[String],
    query_context: &HybridPathWitnessQueryContext,
) -> usize {
    let normalized_line = line.to_ascii_lowercase();
    let line_terms = hybrid_identifier_tokens(&normalized_line);
    let mut score = hybrid_overlap_count(&line_terms, &query_context.query_overlap_terms) * 4;
    score += hybrid_overlap_count(&line_terms, path_terms) * 2;
    if query_context
        .exact_terms
        .iter()
        .any(|term| normalized_line.contains(term.as_str()))
    {
        score += 8;
    }

    score
}

fn max_path_witness_anchor_score(
    path_terms: &[String],
    query_context: &HybridPathWitnessQueryContext,
) -> usize {
    query_context.query_overlap_terms.len().saturating_mul(4)
        + path_terms.len().saturating_mul(2)
        + if query_context.exact_terms.is_empty() {
            0
        } else {
            8
        }
}

pub(super) fn best_path_witness_anchor_in_file(
    path: &str,
    file_path: &Path,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<(usize, String)> {
    let file = File::open(file_path).ok()?;
    best_path_witness_anchor_in_reader(path, BufReader::new(file), query_context)
}

fn best_path_witness_anchor_in_reader<R: BufRead>(
    path: &str,
    mut reader: R,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<(usize, String)> {
    let path_terms = hybrid_path_overlap_tokens(path);
    let max_score = max_path_witness_anchor_score(&path_terms, query_context);
    let mut buffer = String::new();
    let mut line_number = 0usize;
    let mut first_non_empty: Option<(usize, String)> = None;
    let mut best_excerpt: Option<(usize, String)> = None;
    let mut best_score = 0usize;

    loop {
        buffer.clear();
        let bytes_read = reader.read_line(&mut buffer).ok()?;
        if bytes_read == 0 {
            break;
        }

        line_number += 1;
        let line = buffer.trim();
        if line.is_empty() {
            continue;
        }
        if first_non_empty.is_none() {
            first_non_empty = Some((line_number, line.to_owned()));
        }

        let score = score_path_witness_anchor_line(line, &path_terms, query_context);
        if score > best_score {
            best_score = score;
            best_excerpt = Some((line_number, line.to_owned()));
            if best_score >= max_score {
                break;
            }
        }
    }

    best_excerpt.or(first_non_empty)
}

pub(super) fn hybrid_path_witness_recall_score(
    path: &str,
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<f32> {
    let projection = StoredPathWitnessProjection::from_path(path);
    hybrid_path_witness_recall_score_for_projection(path, &projection, intent, query_context)
}

pub(super) fn hybrid_path_witness_recall_score_for_projection(
    path: &str,
    projection: &StoredPathWitnessProjection,
    intent: &HybridRankingIntent,
    query_context: &HybridPathWitnessQueryContext,
) -> Option<f32> {
    if !intent.wants_path_witness_recall() {
        return None;
    }

    let path_overlap =
        hybrid_overlap_count(&projection.path_terms, &query_context.query_overlap_terms);
    let is_entrypoint = projection.flags.is_entrypoint_runtime;
    let is_entrypoint_build_workflow =
        intent.wants_entrypoint_build_flow && projection.flags.is_entrypoint_build_workflow;
    let is_ci_workflow = intent.wants_ci_workflow_witnesses && projection.flags.is_ci_workflow;
    let is_config_artifact = projection.flags.is_runtime_config_artifact;
    let is_python_config = projection.flags.is_python_runtime_config;
    let is_python_test = projection.flags.is_python_test_witness;
    let is_example_support = projection.flags.is_example_support;
    let is_bench_support = projection.flags.is_bench_support;
    let wants_example_or_bench_witnesses = intent.wants_examples || intent.wants_benchmarks;
    let is_cli_test = projection.flags.is_cli_test_support;
    let is_test_harness = projection.flags.is_test_harness;
    let is_scripts_ops = intent.wants_scripts_ops_witnesses && projection.flags.is_scripts_ops;
    if path_overlap == 0
        && !is_entrypoint
        && !is_entrypoint_build_workflow
        && !is_ci_workflow
        && !(intent.wants_runtime_config_artifacts && is_config_artifact)
        && !is_python_config
        && !is_python_test
        && !(intent.wants_examples && is_example_support)
        && !(intent.wants_benchmarks && is_bench_support)
        && !(query_context.query_mentions_cli && is_cli_test)
        && !(intent.wants_laravel_ui_witnesses && is_test_harness)
        && !is_scripts_ops
    {
        return None;
    }

    let mut score = path_overlap as f32;
    if is_entrypoint {
        score += 4.0;
    }
    if is_entrypoint_build_workflow {
        score += 7.2;
    }
    if is_ci_workflow {
        score += 6.2;
    }
    if intent.wants_laravel_ui_witnesses && projection.flags.is_laravel_non_livewire_blade_view {
        score += if intent.wants_blade_component_witnesses {
            2.4
        } else {
            6.0
        };
    }
    if intent.wants_laravel_ui_witnesses && projection.flags.is_laravel_livewire_view {
        score += if intent.wants_blade_component_witnesses {
            1.2
        } else {
            3.6
        };
    }
    if intent.wants_livewire_view_witnesses && projection.flags.is_laravel_livewire_view {
        score += 2.8;
    }
    if intent.wants_livewire_view_witnesses && projection.flags.is_laravel_non_livewire_blade_view {
        score -= 1.1;
    }
    if intent.wants_laravel_ui_witnesses && projection.flags.is_laravel_blade_component {
        if intent.wants_laravel_form_action_witnesses
            && projection.flags.is_laravel_form_action_blade
        {
            score += if intent.wants_blade_component_witnesses {
                5.2
            } else {
                3.8
            };
        }
        score += if intent.wants_blade_component_witnesses {
            if projection.flags.is_laravel_nested_blade_component {
                1.4
            } else {
                6.2
            }
        } else if path_overlap >= 3 {
            2.2
        } else {
            0.8
        };
    } else if intent.wants_laravel_form_action_witnesses
        && projection.flags.is_laravel_form_action_blade
    {
        score += 4.8;
    }
    if intent.wants_laravel_ui_witnesses && projection.flags.is_laravel_livewire_component {
        score += if intent.wants_blade_component_witnesses {
            0.8
        } else {
            1.8
        };
    }
    if intent.wants_laravel_ui_witnesses && projection.flags.is_laravel_view_component_class {
        score -= if intent.wants_laravel_layout_witnesses {
            4.4
        } else {
            2.8
        };
    }
    if intent.wants_commands_middleware_witnesses
        && projection.flags.is_laravel_command_or_middleware
    {
        score += 4.2;
    }
    if intent.wants_jobs_listeners_witnesses && projection.flags.is_laravel_job_or_listener {
        score += 3.4;
    }
    if intent.wants_laravel_layout_witnesses && projection.flags.is_laravel_layout_blade_view {
        score += if intent.wants_blade_component_witnesses {
            4.2
        } else {
            6.4
        };
    }
    if intent.wants_entrypoint_build_flow && projection.flags.is_laravel_route {
        score += 4.8;
    }
    if intent.wants_entrypoint_build_flow && projection.flags.is_laravel_bootstrap_entrypoint {
        score += 3.6;
    }
    if intent.wants_entrypoint_build_flow && projection.flags.is_laravel_core_provider {
        score += 4.4;
    } else if intent.wants_entrypoint_build_flow && projection.flags.is_laravel_provider {
        score += 2.4;
    }
    if intent.wants_runtime_config_artifacts && is_config_artifact {
        score += 3.2;
    }
    if is_python_config {
        score += 3.0;
    }
    if is_python_test {
        score += 3.4;
    }
    if intent.wants_examples && is_example_support {
        score += 4.2;
    }
    if intent.wants_benchmarks && is_bench_support {
        score += 4.4;
    }
    if intent.wants_laravel_ui_witnesses && is_test_harness {
        score += 2.2;
    }
    if is_scripts_ops {
        score += 4.2;
    }
    if intent.wants_test_witness_recall
        && path_has_exact_query_term_match(path, &query_context.exact_terms)
    {
        score += 4.0;
    }
    if intent.wants_scripts_ops_witnesses
        && path_has_exact_query_term_match(path, &query_context.exact_terms)
    {
        score += 2.8;
    }
    if intent.wants_test_witness_recall && projection.flags.is_test_support {
        score += 2.6;
    }
    if wants_example_or_bench_witnesses
        && projection.flags.is_test_support
        && !is_example_support
        && !is_bench_support
    {
        score -= 2.4;
    }
    if query_context.query_mentions_cli && is_cli_test {
        score += 3.8;
    }
    if matches!(
        projection.source_class,
        HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
    ) {
        score += 0.4;
    }
    if projection.flags.is_frontend_runtime_noise {
        score -= 4.0;
    }

    (score > 0.0).then_some(score)
}

pub(super) fn merge_hybrid_lexical_search_output(
    base: &mut SearchExecutionOutput,
    supplement: SearchExecutionOutput,
    limit: usize,
) {
    let mut merged_by_key: BTreeMap<(String, String, usize, usize, String), TextMatch> =
        BTreeMap::new();
    for found in &base.matches {
        merged_by_key.insert(
            (
                found.repository_id.clone(),
                found.path.clone(),
                found.line,
                found.column,
                found.excerpt.clone(),
            ),
            found.clone(),
        );
    }
    for found in supplement.matches {
        merged_by_key
            .entry((
                found.repository_id.clone(),
                found.path.clone(),
                found.line,
                found.column,
                found.excerpt.clone(),
            ))
            .or_insert(found);
    }

    base.matches = merged_by_key.into_values().collect::<Vec<_>>();
    sort_matches_deterministically(&mut base.matches);
    base.matches.truncate(limit);

    base.diagnostics
        .entries
        .extend(supplement.diagnostics.entries);
    sort_search_diagnostics_deterministically(&mut base.diagnostics.entries);
    base.diagnostics.entries.dedup();
}

pub(super) fn semantic_excerpt(content_text: &str, fallback_path: &str) -> String {
    content_text
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| fallback_path.to_owned())
}

pub(super) fn hybrid_path_has_exact_stem_match(path: &str, exact_terms: &[String]) -> bool {
    let Some(stem) = Path::new(path).file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let normalized_stem = stem.trim().to_ascii_lowercase();
    if normalized_stem.is_empty() {
        return false;
    }

    exact_terms
        .iter()
        .any(|term| term.eq_ignore_ascii_case(&normalized_stem))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn witness_anchor_reader_keeps_best_matching_line() {
        let query_context = HybridPathWitnessQueryContext::new("build flow entrypoint");
        let source = Cursor::new(
            "\nplain header\nsetup unrelated values\nbuild entrypoint wires workflow\n",
        );

        let anchor = best_path_witness_anchor_in_reader("scripts/build.rs", source, &query_context);

        assert_eq!(
            anchor,
            Some((4, "build entrypoint wires workflow".to_owned()))
        );
    }

    #[test]
    fn witness_anchor_reader_falls_back_to_first_non_empty_line() {
        let query_context = HybridPathWitnessQueryContext::new("jobs listeners queue");
        let source = Cursor::new("\nheader line\nanother unrelated value\n");

        let anchor = best_path_witness_anchor_in_reader("docs/overview.md", source, &query_context);

        assert_eq!(anchor, Some((2, "header line".to_owned())));
    }
}
