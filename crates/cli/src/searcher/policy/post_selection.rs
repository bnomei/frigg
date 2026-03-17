use std::cmp::Ordering;
use std::path::Path;

mod context;
mod guardrails;
mod laravel;
mod pipeline;
mod runtime;

use super::super::HybridChannelHit;
use super::super::HybridRankedEvidence;
use super::super::laravel::{
    is_laravel_blade_component_path, is_laravel_bootstrap_entrypoint_path,
    is_laravel_command_or_middleware_path, is_laravel_core_provider_path,
    is_laravel_layout_blade_view_path, is_laravel_livewire_component_path,
    is_laravel_livewire_view_path, is_laravel_non_livewire_blade_view_path,
    is_laravel_provider_path, is_laravel_route_path, is_laravel_view_component_class_path,
};
use super::super::query_terms::{hybrid_path_has_exact_stem_match, hybrid_path_overlap_count};
use super::super::surfaces::{self, HybridSourceClass};
#[cfg(test)]
use super::PolicyQueryContext;
use super::dsl::{Predicate, predicate_matches};
use super::trace::PolicyStage;
pub(crate) use context::PostSelectionContext;
use context::PostSelectionRepairAction;
use context::PostSelectionRuleMeta;
pub(crate) use context::PostSelectionTrace;
#[cfg(test)]
use context::PostSelectionTraceEvent;
use guardrails::{
    choose_best_candidate, hybrid_ranked_evidence_from_witness_hit, insert_guardrail_candidate,
    insert_test_support_guardrail_candidate, selection_guardrail_cmp,
    selection_guardrail_cmp_from_hit, selection_guardrail_facts, selection_guardrail_score,
    selection_guardrail_score_for_path, selection_guardrail_state,
};
use laravel::{
    apply_laravel_blade_surface_visibility, apply_laravel_entrypoint_visibility,
    apply_laravel_layout_companion_visibility, apply_laravel_ui_test_harness_visibility,
};
use pipeline::{
    HAS_SPECIFIC_WITNESS_TERMS, PostSelectionPipelineFacts, QUERY_MENTIONS_CLI, TransformFn,
    WANTS_BENCHMARKS, WANTS_CI_WORKFLOW_WITNESSES, WANTS_ENTRYPOINT_BUILD_FLOW, WANTS_EXAMPLES,
    WANTS_LARAVEL_UI_WITNESSES, WANTS_RUNTIME_CONFIG_ARTIFACTS, WANTS_RUNTIME_WITNESSES,
    WANTS_SCRIPTS_OPS_WITNESSES, WANTS_TEST_WITNESS_RECALL,
};
use runtime::{
    apply_ci_scripts_ops_visibility, apply_cli_entrypoint_visibility,
    apply_cli_specific_test_visibility, apply_entrypoint_build_workflow_visibility,
    apply_mixed_support_visibility, apply_runtime_companion_surface_visibility,
    apply_runtime_companion_test_visibility, apply_runtime_config_surface_selection,
    apply_runtime_entrypoint_visibility, apply_runtime_witness_rescue_visibility,
};

#[allow(dead_code)]
#[derive(Clone, Copy)]
struct PostSelectionRule {
    id: &'static str,
    stage: PolicyStage,
    when: Predicate<PostSelectionPipelineFacts>,
    apply: TransformFn,
}

impl PostSelectionRule {
    const fn new(
        id: &'static str,
        stage: PolicyStage,
        when: Predicate<PostSelectionPipelineFacts>,
        apply: TransformFn,
    ) -> Self {
        Self {
            id,
            stage,
            when,
            apply,
        }
    }

    const fn meta(self) -> PostSelectionRuleMeta {
        PostSelectionRuleMeta {
            id: self.id,
            stage: self.stage,
        }
    }
}

const RULES: &[PostSelectionRule] = &[
    PostSelectionRule::new(
        "post_selection.runtime_config",
        PolicyStage::PostSelectionRuntime,
        Predicate::any(&[WANTS_RUNTIME_CONFIG_ARTIFACTS, WANTS_ENTRYPOINT_BUILD_FLOW]),
        apply_runtime_config_surface_selection,
    ),
    PostSelectionRule::new(
        "post_selection.runtime_entrypoint",
        PolicyStage::PostSelectionRuntime,
        Predicate::all(&[WANTS_ENTRYPOINT_BUILD_FLOW]),
        apply_runtime_entrypoint_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.cli_entrypoint",
        PolicyStage::PostSelectionRuntime,
        Predicate::all(&[WANTS_ENTRYPOINT_BUILD_FLOW, QUERY_MENTIONS_CLI]),
        apply_cli_entrypoint_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.entrypoint_build_workflow",
        PolicyStage::PostSelectionRuntime,
        Predicate::all(&[WANTS_ENTRYPOINT_BUILD_FLOW]),
        apply_entrypoint_build_workflow_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.ci_scripts_ops",
        PolicyStage::PostSelectionRuntime,
        Predicate::any(&[WANTS_CI_WORKFLOW_WITNESSES, WANTS_SCRIPTS_OPS_WITNESSES]),
        apply_ci_scripts_ops_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.mixed_support",
        PolicyStage::PostSelectionMixedSupport,
        Predicate::new(
            &[WANTS_TEST_WITNESS_RECALL],
            &[WANTS_EXAMPLES, WANTS_BENCHMARKS],
            &[],
        ),
        apply_mixed_support_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.cli_specific_test",
        PolicyStage::PostSelectionRuntime,
        Predicate::new(
            &[QUERY_MENTIONS_CLI, HAS_SPECIFIC_WITNESS_TERMS],
            &[WANTS_ENTRYPOINT_BUILD_FLOW, WANTS_TEST_WITNESS_RECALL],
            &[],
        ),
        apply_cli_specific_test_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.runtime_companion_surface",
        PolicyStage::PostSelectionRuntime,
        Predicate::new(
            &[HAS_SPECIFIC_WITNESS_TERMS],
            &[
                WANTS_RUNTIME_WITNESSES,
                WANTS_TEST_WITNESS_RECALL,
                WANTS_ENTRYPOINT_BUILD_FLOW,
                WANTS_RUNTIME_CONFIG_ARTIFACTS,
            ],
            &[],
        ),
        apply_runtime_companion_surface_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.runtime_witness_rescue",
        PolicyStage::PostSelectionRuntime,
        Predicate::any(&[
            WANTS_RUNTIME_WITNESSES,
            WANTS_TEST_WITNESS_RECALL,
            WANTS_ENTRYPOINT_BUILD_FLOW,
            WANTS_RUNTIME_CONFIG_ARTIFACTS,
        ]),
        apply_runtime_witness_rescue_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.runtime_companion_tests",
        PolicyStage::PostSelectionRuntime,
        Predicate::any(&[
            WANTS_TEST_WITNESS_RECALL,
            WANTS_ENTRYPOINT_BUILD_FLOW,
            WANTS_RUNTIME_CONFIG_ARTIFACTS,
        ]),
        apply_runtime_companion_test_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_entrypoint",
        PolicyStage::PostSelectionLaravel,
        Predicate::all(&[WANTS_ENTRYPOINT_BUILD_FLOW]),
        apply_laravel_entrypoint_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_blade_surface",
        PolicyStage::PostSelectionLaravel,
        Predicate::all(&[WANTS_LARAVEL_UI_WITNESSES]),
        apply_laravel_blade_surface_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_ui_test_harness",
        PolicyStage::PostSelectionLaravel,
        Predicate::all(&[WANTS_LARAVEL_UI_WITNESSES]),
        apply_laravel_ui_test_harness_visibility,
    ),
    PostSelectionRule::new(
        "post_selection.laravel_layout_companion",
        PolicyStage::PostSelectionLaravel,
        Predicate::all(&[WANTS_LARAVEL_UI_WITNESSES]),
        apply_laravel_layout_companion_visibility,
    ),
];

pub(crate) fn apply(
    mut matches: Vec<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
) -> Vec<HybridRankedEvidence> {
    if matches.is_empty() {
        return matches;
    }

    let facts = PostSelectionPipelineFacts::from_context(ctx);
    for rule in RULES {
        if !predicate_matches(&facts, rule.when) {
            continue;
        }
        matches = (rule.apply)(matches, ctx, rule.meta());
    }

    matches
}
fn is_root_scoped_runtime_config_document(entry: &HybridRankedEvidence) -> bool {
    is_root_scoped_runtime_config_path(&entry.document.path)
}

fn is_ci_workflow_document(entry: &HybridRankedEvidence) -> bool {
    surfaces::is_ci_workflow_path(&entry.document.path)
}

fn is_root_scoped_runtime_config_path(path: &str) -> bool {
    surfaces::is_root_scoped_runtime_config_path(path)
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

fn is_runtime_companion_surface_candidate_path(path: &str) -> bool {
    if surfaces::is_runtime_config_artifact_path(path)
        || surfaces::is_entrypoint_runtime_path(path)
        || surfaces::is_ci_workflow_path(path)
        || surfaces::is_test_support_path(path)
        || surfaces::is_test_harness_path(path)
    {
        return false;
    }

    surfaces::is_kotlin_android_ui_runtime_surface_path(path)
        || (matches!(
            surfaces::hybrid_source_class(path),
            HybridSourceClass::Runtime
        ) && !surfaces::is_frontend_runtime_noise_path(path))
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

fn is_promotable_laravel_blade_surface_path(path: &str) -> bool {
    is_laravel_blade_component_path(path) || is_laravel_non_livewire_blade_view_path(path)
}

fn is_layout_companion_blade_surface_path(path: &str) -> bool {
    is_promotable_laravel_blade_surface_path(path) && !is_laravel_layout_blade_view_path(path)
}

fn is_benchmark_test_support_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    is_plain_test_support_path(&normalized)
        && (normalized.starts_with("benchmark/") || normalized.contains("/benchmark/"))
}

fn is_bench_support_candidate_path(path: &str) -> bool {
    surfaces::is_bench_support_path(path) || is_benchmark_test_support_path(path)
}

fn is_bench_support_document(entry: &HybridRankedEvidence) -> bool {
    is_bench_support_candidate_path(&entry.document.path)
}

fn is_example_support_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_example_support_document(entry) {
        return false;
    }
    if is_root_scoped_runtime_config_document(entry) {
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
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !surfaces::is_entrypoint_build_workflow_path(&normalized) {
        return 1;
    }

    if ["bundle", "deploy", "pages", "publish", "release"]
        .iter()
        .any(|term| normalized.contains(term))
    {
        4
    } else if normalized.contains("build") {
        3
    } else {
        2
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

fn is_runtime_config_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_root_scoped_runtime_config_document(entry) {
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

fn is_runtime_entrypoint_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if surfaces::is_entrypoint_runtime_path(&entry.document.path)
        || is_root_scoped_runtime_config_document(entry)
        || surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
        || is_ci_workflow_document(entry)
    {
        return false;
    }

    if surfaces::is_frontend_runtime_noise_path(&entry.document.path)
        || surfaces::is_typescript_runtime_module_index_path(&entry.document.path)
    {
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

fn is_runtime_companion_surface_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if is_runtime_companion_surface_candidate_path(&entry.document.path) {
        return false;
    }

    if surfaces::is_frontend_runtime_noise_path(&entry.document.path)
        || surfaces::is_runtime_config_artifact_path(&entry.document.path)
        || surfaces::is_entrypoint_runtime_path(&entry.document.path)
        || surfaces::is_ci_workflow_path(&entry.document.path)
    {
        return true;
    }

    matches!(
        surfaces::hybrid_source_class(&entry.document.path),
        HybridSourceClass::Project
            | HybridSourceClass::Support
            | HybridSourceClass::Tests
            | HybridSourceClass::Specs
            | HybridSourceClass::Documentation
            | HybridSourceClass::Readme
    ) || surfaces::is_test_harness_path(&entry.document.path)
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
    if surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
        || surfaces::is_ci_workflow_path(&entry.document.path)
    {
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

fn is_cli_entrypoint_guardrail_replacement(entry: &HybridRankedEvidence) -> bool {
    if surfaces::is_cli_command_entrypoint_path(&entry.document.path)
        || is_root_scoped_runtime_config_document(entry)
        || surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
        || is_ci_workflow_document(entry)
    {
        return false;
    }

    if surfaces::is_frontend_runtime_noise_path(&entry.document.path)
        || surfaces::is_typescript_runtime_module_index_path(&entry.document.path)
        || surfaces::is_entrypoint_runtime_path(&entry.document.path)
    {
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
    if is_root_scoped_runtime_config_document(entry) {
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
#[path = "post_selection/tests.rs"]
mod tests;
