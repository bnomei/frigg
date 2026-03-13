use std::collections::BTreeMap;
use std::path::Path;

use crate::domain::FrameworkHint;

use super::super::super::HybridRankedEvidence;
use super::super::super::intent::HybridRankingIntent;
use super::super::super::laravel::{LaravelUiSurfaceClass, laravel_ui_surface_class};
use super::super::super::query_terms::{
    hybrid_canonical_match_multiplier, hybrid_excerpt_has_build_flow_anchor,
    hybrid_excerpt_has_exact_identifier_anchor, hybrid_excerpt_has_test_double_anchor,
    hybrid_identifier_tokens, hybrid_overlap_count, hybrid_path_overlap_count_with_terms,
    hybrid_query_exact_terms, hybrid_query_mentions_cli_command, hybrid_query_overlap_terms,
    hybrid_specific_witness_query_terms, path_has_exact_query_term_match,
};
use super::super::super::surfaces::{
    HybridSourceClass, has_generic_runtime_anchor_stem, hybrid_source_class, is_bench_support_path,
    is_ci_workflow_path, is_cli_test_support_path, is_entrypoint_build_workflow_path,
    is_entrypoint_reference_doc_path, is_entrypoint_runtime_path, is_example_support_path,
    is_frontend_runtime_noise_path, is_generic_runtime_witness_doc_path,
    is_loose_python_test_module_path, is_navigation_reference_doc_path, is_navigation_runtime_path,
    is_non_code_test_doc_path, is_python_entrypoint_runtime_path, is_python_runtime_config_path,
    is_python_test_witness_path, is_repo_metadata_path, is_root_scoped_runtime_config_path,
    is_runtime_adjacent_python_test_path, is_runtime_anchor_test_support_path,
    is_runtime_config_artifact_path, is_rust_workspace_config_path, is_scripts_ops_path,
    is_test_harness_path, is_test_support_path, is_typescript_runtime_module_index_path,
};
use super::super::hybrid_path_quality_multiplier_with_intent;

pub(crate) struct SelectionQueryContext {
    pub(crate) exact_terms: Vec<String>,
    pub(crate) query_overlap_terms: Vec<String>,
    pub(crate) blade_component_specific_terms: Vec<String>,
    pub(crate) specific_witness_terms: Vec<String>,
    pub(crate) query_mentions_cli: bool,
    pub(crate) query_has_identifier_anchor: bool,
    pub(crate) query_has_specific_blade_anchors: bool,
    pub(crate) wants_example_or_bench_witnesses: bool,
    pub(crate) penalize_generic_runtime_docs: bool,
    pub(crate) wants_python_witnesses: bool,
    pub(crate) wants_rust_workspace_config: bool,
    pub(crate) wants_python_workspace_config: bool,
}

impl SelectionQueryContext {
    pub(crate) fn new(intent: &HybridRankingIntent, query_text: &str) -> Self {
        let exact_terms = hybrid_query_exact_terms(query_text);
        let query_overlap_terms = hybrid_query_overlap_terms(query_text);
        let blade_component_specific_terms = blade_component_specific_query_terms(query_text);
        let specific_witness_terms = hybrid_specific_witness_query_terms(query_text);
        let query_mentions_cli = hybrid_query_mentions_cli_command(query_text);
        let query_has_identifier_anchor = query_overlap_terms.len() > exact_terms.len();
        let query_has_specific_blade_anchors =
            intent.wants_laravel_ui_witnesses && !specific_witness_terms.is_empty();

        Self {
            exact_terms,
            query_overlap_terms,
            blade_component_specific_terms,
            specific_witness_terms,
            query_mentions_cli,
            query_has_identifier_anchor,
            query_has_specific_blade_anchors,
            wants_example_or_bench_witnesses: intent.wants_examples || intent.wants_benchmarks,
            penalize_generic_runtime_docs: !intent.wants_docs
                && !intent.wants_onboarding
                && !intent.wants_readme,
            wants_python_witnesses: intent.has_framework_hint(FrameworkHint::Python),
            wants_rust_workspace_config: intent.has_framework_hint(FrameworkHint::Rust),
            wants_python_workspace_config: intent.has_framework_hint(FrameworkHint::Python),
        }
    }
}

pub(crate) struct SelectionStaticFeatures {
    pub(crate) class: HybridSourceClass,
    pub(crate) path_depth: usize,
    pub(crate) path_overlap: usize,
    pub(crate) blade_specific_path_overlap: usize,
    pub(crate) specific_witness_path_overlap: usize,
    pub(crate) canonical_match_multiplier: f32,
    pub(crate) runtime_witness_path_overlap_multiplier: f32,
    pub(crate) has_exact_query_term_match: bool,
    pub(crate) excerpt_overlap: usize,
    pub(crate) excerpt_has_exact_identifier_anchor: bool,
    pub(crate) excerpt_has_build_flow_anchor: bool,
    pub(crate) excerpt_has_test_double_anchor: bool,
    pub(crate) is_ci_workflow: bool,
    pub(crate) is_example_support: bool,
    pub(crate) has_path_witness_source: bool,
    pub(crate) is_repo_root_runtime_config_artifact: bool,
    pub(crate) is_typescript_runtime_module_index: bool,
}

pub(crate) struct SelectionCandidate {
    pub(crate) evidence: HybridRankedEvidence,
    pub(crate) static_features: SelectionStaticFeatures,
}

impl SelectionCandidate {
    pub(crate) fn new(
        evidence: HybridRankedEvidence,
        intent: &HybridRankingIntent,
        query_context: &SelectionQueryContext,
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
        let specific_witness_path_overlap = hybrid_path_overlap_count_with_terms(
            &evidence.document.path,
            &query_context.specific_witness_terms,
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
        let is_ci_workflow = is_ci_workflow_path(&evidence.document.path);
        let is_example_support = is_example_support_path(&evidence.document.path);
        let has_path_witness_source = evidence
            .lexical_sources
            .iter()
            .any(|source| source.starts_with("path_witness:"));
        let is_repo_root_runtime_config_artifact =
            is_root_scoped_runtime_config_path(&evidence.document.path);
        let path_depth = path_depth(&evidence.document.path);
        let is_typescript_runtime_module_index =
            is_typescript_runtime_module_index_path(&evidence.document.path);
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
            static_features: SelectionStaticFeatures {
                class,
                path_depth,
                path_overlap,
                blade_specific_path_overlap,
                specific_witness_path_overlap,
                canonical_match_multiplier,
                runtime_witness_path_overlap_multiplier,
                has_exact_query_term_match,
                excerpt_overlap,
                excerpt_has_exact_identifier_anchor,
                excerpt_has_build_flow_anchor,
                excerpt_has_test_double_anchor,
                is_ci_workflow,
                is_example_support,
                has_path_witness_source,
                is_repo_root_runtime_config_artifact,
                is_typescript_runtime_module_index,
            },
        }
    }
}

#[derive(Debug, Default)]
pub(crate) struct SelectionState {
    seen_classes: BTreeMap<HybridSourceClass, usize>,
    seen_laravel_ui_surfaces: BTreeMap<LaravelUiSurfaceClass, usize>,
    runtime_anchor_paths: Vec<String>,
    seen_ci_workflows: usize,
    seen_example_support: usize,
    seen_bench_support: usize,
    seen_plain_test_support: usize,
    seen_repo_root_runtime_configs: usize,
    seen_typescript_runtime_module_indexes: usize,
}

impl SelectionState {
    pub(crate) fn from_selected(
        selected: &[HybridRankedEvidence],
        intent: &HybridRankingIntent,
        query_context: &SelectionQueryContext,
    ) -> Self {
        let mut state = Self::default();
        for evidence in selected {
            let candidate = SelectionCandidate::new(evidence.clone(), intent, query_context);
            state.observe(&candidate);
        }
        state
    }

    pub(crate) fn observe(&mut self, candidate: &SelectionCandidate) {
        *self
            .seen_classes
            .entry(candidate.static_features.class)
            .or_insert(0) += 1;
        if let Some(surface) = laravel_ui_surface_class(&candidate.evidence.document.path) {
            *self.seen_laravel_ui_surfaces.entry(surface).or_insert(0) += 1;
        }
        if candidate.static_features.is_ci_workflow {
            self.seen_ci_workflows += 1;
        }
        if is_entrypoint_runtime_path(&candidate.evidence.document.path)
            || is_runtime_config_artifact_path(&candidate.evidence.document.path)
        {
            self.runtime_anchor_paths
                .push(candidate.evidence.document.path.clone());
        }
        if candidate.static_features.is_example_support {
            self.seen_example_support += 1;
        }
        let is_bench_support = is_bench_support_path(&candidate.evidence.document.path);
        if is_bench_support {
            self.seen_bench_support += 1;
        }
        if is_test_support_path(&candidate.evidence.document.path)
            && !candidate.static_features.is_example_support
            && !is_bench_support
        {
            self.seen_plain_test_support += 1;
        }
        if candidate
            .static_features
            .is_repo_root_runtime_config_artifact
        {
            self.seen_repo_root_runtime_configs += 1;
        }
        if candidate.static_features.is_typescript_runtime_module_index {
            self.seen_typescript_runtime_module_indexes += 1;
        }
    }
}

pub(crate) struct SelectionFacts {
    pub(crate) base_score: f32,
    pub(crate) class: HybridSourceClass,
    pub(crate) path_depth: usize,
    pub(crate) path_overlap: usize,
    pub(crate) excerpt_overlap: usize,
    pub(crate) blade_specific_path_overlap: usize,
    pub(crate) specific_witness_path_overlap: usize,
    pub(crate) runtime_family_prefix_overlap: usize,
    pub(crate) seen_count: usize,
    pub(crate) runtime_seen: usize,
    pub(crate) seen_ci_workflows: usize,
    pub(crate) seen_example_support: usize,
    pub(crate) seen_bench_support: usize,
    pub(crate) seen_plain_test_support: usize,
    pub(crate) seen_repo_root_runtime_configs: usize,
    pub(crate) seen_typescript_runtime_module_indexes: usize,
    pub(crate) canonical_match_multiplier: f32,
    pub(crate) runtime_witness_path_overlap_multiplier: f32,
    pub(crate) excerpt_has_exact_identifier_anchor: bool,
    pub(crate) excerpt_has_build_flow_anchor: bool,
    pub(crate) excerpt_has_test_double_anchor: bool,
    pub(crate) has_exact_query_term_match: bool,
    pub(crate) has_path_witness_source: bool,
    pub(crate) is_examples_rs: bool,
    pub(crate) query_has_exact_terms: bool,
    pub(crate) query_mentions_cli: bool,
    pub(crate) query_has_identifier_anchor: bool,
    pub(crate) query_has_specific_blade_anchors: bool,
    pub(crate) wants_runtime_companion_tests: bool,
    pub(crate) prefer_runtime_anchor_tests: bool,
    pub(crate) wants_example_or_bench_witnesses: bool,
    pub(crate) penalize_generic_runtime_docs: bool,
    pub(crate) wants_python_witnesses: bool,
    pub(crate) wants_rust_workspace_config: bool,
    pub(crate) wants_python_workspace_config: bool,
    pub(crate) wants_contracts: bool,
    pub(crate) wants_error_taxonomy: bool,
    pub(crate) wants_tool_contracts: bool,
    pub(crate) wants_mcp_runtime_surface: bool,
    pub(crate) wants_class: bool,
    pub(crate) wants_runtime_witnesses: bool,
    pub(crate) wants_runtime_config_artifacts: bool,
    pub(crate) wants_laravel_ui_witnesses: bool,
    pub(crate) wants_blade_component_witnesses: bool,
    pub(crate) wants_laravel_form_action_witnesses: bool,
    pub(crate) wants_livewire_view_witnesses: bool,
    pub(crate) wants_commands_middleware_witnesses: bool,
    pub(crate) wants_jobs_listeners_witnesses: bool,
    pub(crate) wants_laravel_layout_witnesses: bool,
    pub(crate) wants_test_witness_recall: bool,
    pub(crate) wants_navigation_fallbacks: bool,
    pub(crate) wants_ci_workflow_witnesses: bool,
    pub(crate) wants_scripts_ops_witnesses: bool,
    pub(crate) wants_entrypoint_build_flow: bool,
    pub(crate) wants_examples: bool,
    pub(crate) wants_benchmarks: bool,
    pub(crate) is_ci_workflow: bool,
    pub(crate) is_example_support: bool,
    pub(crate) is_runtime_config_artifact: bool,
    pub(crate) is_repo_root_runtime_config_artifact: bool,
    pub(crate) is_typescript_runtime_module_index: bool,
    pub(crate) is_entrypoint_runtime: bool,
    pub(crate) is_entrypoint_build_workflow: bool,
    pub(crate) is_entrypoint_reference_doc: bool,
    pub(crate) is_python_entrypoint_runtime: bool,
    pub(crate) is_python_runtime_config: bool,
    pub(crate) is_python_test_witness: bool,
    pub(crate) is_loose_python_test_module: bool,
    pub(crate) is_bench_support: bool,
    pub(crate) is_test_support: bool,
    pub(crate) is_cli_test_support: bool,
    pub(crate) is_runtime_anchor_test_support: bool,
    pub(crate) is_runtime_adjacent_python_test: bool,
    pub(crate) is_non_prefix_python_test_module: bool,
    pub(crate) is_test_harness: bool,
    pub(crate) is_non_code_test_doc: bool,
    pub(crate) is_generic_runtime_witness_doc: bool,
    pub(crate) is_repo_metadata: bool,
    pub(crate) has_generic_runtime_anchor_stem: bool,
    pub(crate) is_frontend_runtime_noise: bool,
    pub(crate) is_navigation_runtime: bool,
    pub(crate) is_navigation_reference_doc: bool,
    pub(crate) is_scripts_ops: bool,
    pub(crate) is_rust_workspace_config: bool,
    pub(crate) path_stem_is_server_or_cli: bool,
    pub(crate) path_stem_is_main: bool,
    pub(crate) is_laravel_non_livewire_blade_view: bool,
    pub(crate) is_laravel_livewire_view: bool,
    pub(crate) is_laravel_blade_component: bool,
    pub(crate) is_laravel_nested_blade_component: bool,
    pub(crate) is_laravel_form_action_blade: bool,
    pub(crate) is_laravel_livewire_component: bool,
    pub(crate) is_laravel_view_component_class: bool,
    pub(crate) is_laravel_command_or_middleware: bool,
    pub(crate) is_laravel_job_or_listener: bool,
    pub(crate) is_laravel_layout_blade_view: bool,
    pub(crate) is_laravel_core_provider: bool,
    pub(crate) is_laravel_provider: bool,
    pub(crate) is_laravel_route: bool,
    pub(crate) is_laravel_bootstrap_entrypoint: bool,
    pub(crate) laravel_surface: Option<LaravelUiSurfaceClass>,
    pub(crate) laravel_surface_seen: usize,
}

#[allow(dead_code)]
pub(crate) struct SelectionIntentView<'a> {
    facts: &'a SelectionFacts,
}

#[allow(dead_code)]
impl SelectionIntentView<'_> {
    pub(crate) fn wants_class(&self) -> bool {
        self.facts.wants_class
    }

    pub(crate) fn wants_runtime_witnesses(&self) -> bool {
        self.facts.wants_runtime_witnesses
    }

    pub(crate) fn wants_entrypoint_build_flow(&self) -> bool {
        self.facts.wants_entrypoint_build_flow
    }

    pub(crate) fn wants_runtime_config_artifacts(&self) -> bool {
        self.facts.wants_runtime_config_artifacts
    }

    pub(crate) fn wants_test_witness_recall(&self) -> bool {
        self.facts.wants_test_witness_recall
    }

    pub(crate) fn wants_example_or_bench_witnesses(&self) -> bool {
        self.facts.wants_example_or_bench_witnesses
    }

    pub(crate) fn wants_examples(&self) -> bool {
        self.facts.wants_examples
    }

    pub(crate) fn wants_benchmarks(&self) -> bool {
        self.facts.wants_benchmarks
    }

    pub(crate) fn wants_rust_workspace_config(&self) -> bool {
        self.facts.wants_rust_workspace_config
    }

    pub(crate) fn wants_python_workspace_config(&self) -> bool {
        self.facts.wants_python_workspace_config
    }

    pub(crate) fn wants_python_witnesses(&self) -> bool {
        self.facts.wants_python_witnesses
    }

    pub(crate) fn wants_mcp_runtime_surface(&self) -> bool {
        self.facts.wants_mcp_runtime_surface
    }

    pub(crate) fn wants_runtime_companion_tests(&self) -> bool {
        self.facts.wants_runtime_companion_tests
    }

    pub(crate) fn prefer_runtime_anchor_tests(&self) -> bool {
        self.facts.prefer_runtime_anchor_tests
    }

    pub(crate) fn penalize_generic_runtime_docs(&self) -> bool {
        self.facts.penalize_generic_runtime_docs
    }

    pub(crate) fn wants_laravel_ui_witnesses(&self) -> bool {
        self.facts.wants_laravel_ui_witnesses
    }

    pub(crate) fn wants_blade_component_witnesses(&self) -> bool {
        self.facts.wants_blade_component_witnesses
    }

    pub(crate) fn wants_laravel_form_action_witnesses(&self) -> bool {
        self.facts.wants_laravel_form_action_witnesses
    }

    pub(crate) fn wants_livewire_view_witnesses(&self) -> bool {
        self.facts.wants_livewire_view_witnesses
    }

    pub(crate) fn wants_commands_middleware_witnesses(&self) -> bool {
        self.facts.wants_commands_middleware_witnesses
    }

    pub(crate) fn wants_jobs_listeners_witnesses(&self) -> bool {
        self.facts.wants_jobs_listeners_witnesses
    }

    pub(crate) fn wants_laravel_layout_witnesses(&self) -> bool {
        self.facts.wants_laravel_layout_witnesses
    }

    pub(crate) fn wants_navigation_fallbacks(&self) -> bool {
        self.facts.wants_navigation_fallbacks
    }

    pub(crate) fn wants_ci_workflow_witnesses(&self) -> bool {
        self.facts.wants_ci_workflow_witnesses
    }

    pub(crate) fn wants_scripts_ops_witnesses(&self) -> bool {
        self.facts.wants_scripts_ops_witnesses
    }
}

#[allow(dead_code)]
pub(crate) struct SelectionQueryView<'a> {
    facts: &'a SelectionFacts,
}

#[allow(dead_code)]
impl SelectionQueryView<'_> {
    pub(crate) fn mentions_cli(&self) -> bool {
        self.facts.query_mentions_cli
    }

    pub(crate) fn has_exact_terms(&self) -> bool {
        self.facts.query_has_exact_terms
    }

    pub(crate) fn has_identifier_anchor(&self) -> bool {
        self.facts.query_has_identifier_anchor
    }

    pub(crate) fn has_specific_blade_anchors(&self) -> bool {
        self.facts.query_has_specific_blade_anchors
    }
}

#[allow(dead_code)]
pub(crate) struct SelectionCandidateView<'a> {
    facts: &'a SelectionFacts,
}

#[allow(dead_code)]
impl SelectionCandidateView<'_> {
    pub(crate) fn class(&self) -> HybridSourceClass {
        self.facts.class
    }

    pub(crate) fn path_overlap(&self) -> usize {
        self.facts.path_overlap
    }

    pub(crate) fn excerpt_overlap(&self) -> usize {
        self.facts.excerpt_overlap
    }

    pub(crate) fn specific_witness_path_overlap(&self) -> usize {
        self.facts.specific_witness_path_overlap
    }

    pub(crate) fn blade_specific_path_overlap(&self) -> usize {
        self.facts.blade_specific_path_overlap
    }

    pub(crate) fn has_exact_query_term_match(&self) -> bool {
        self.facts.has_exact_query_term_match
    }

    pub(crate) fn excerpt_has_exact_identifier_anchor(&self) -> bool {
        self.facts.excerpt_has_exact_identifier_anchor
    }

    pub(crate) fn excerpt_has_build_flow_anchor(&self) -> bool {
        self.facts.excerpt_has_build_flow_anchor
    }

    pub(crate) fn excerpt_has_test_double_anchor(&self) -> bool {
        self.facts.excerpt_has_test_double_anchor
    }

    pub(crate) fn has_path_witness_source(&self) -> bool {
        self.facts.has_path_witness_source
    }

    pub(crate) fn is_runtime_config_artifact(&self) -> bool {
        self.facts.is_runtime_config_artifact
    }

    pub(crate) fn is_repo_root_runtime_config_artifact(&self) -> bool {
        self.facts.is_repo_root_runtime_config_artifact
    }

    pub(crate) fn is_typescript_runtime_module_index(&self) -> bool {
        self.facts.is_typescript_runtime_module_index
    }

    pub(crate) fn is_entrypoint_runtime(&self) -> bool {
        self.facts.is_entrypoint_runtime
    }

    pub(crate) fn is_entrypoint_build_workflow(&self) -> bool {
        self.facts.is_entrypoint_build_workflow
    }

    pub(crate) fn is_python_runtime_config(&self) -> bool {
        self.facts.is_python_runtime_config
    }

    pub(crate) fn is_python_entrypoint_runtime(&self) -> bool {
        self.facts.is_python_entrypoint_runtime
    }

    pub(crate) fn is_python_test_witness(&self) -> bool {
        self.facts.is_python_test_witness
    }

    pub(crate) fn is_loose_python_test_module(&self) -> bool {
        self.facts.is_loose_python_test_module
    }

    pub(crate) fn is_ci_workflow(&self) -> bool {
        self.facts.is_ci_workflow
    }

    pub(crate) fn is_navigation_runtime(&self) -> bool {
        self.facts.is_navigation_runtime
    }

    pub(crate) fn is_navigation_reference_doc(&self) -> bool {
        self.facts.is_navigation_reference_doc
    }

    pub(crate) fn is_example_support(&self) -> bool {
        self.facts.is_example_support
    }

    pub(crate) fn is_bench_support(&self) -> bool {
        self.facts.is_bench_support
    }

    pub(crate) fn is_test_support(&self) -> bool {
        self.facts.is_test_support
    }

    pub(crate) fn is_examples_rs(&self) -> bool {
        self.facts.is_examples_rs
    }

    pub(crate) fn path_stem_is_server_or_cli(&self) -> bool {
        self.facts.path_stem_is_server_or_cli
    }

    pub(crate) fn path_stem_is_main(&self) -> bool {
        self.facts.path_stem_is_main
    }

    pub(crate) fn is_cli_test_support(&self) -> bool {
        self.facts.is_cli_test_support
    }

    pub(crate) fn is_runtime_anchor_test_support(&self) -> bool {
        self.facts.is_runtime_anchor_test_support
    }

    pub(crate) fn is_test_harness(&self) -> bool {
        self.facts.is_test_harness
    }

    pub(crate) fn is_non_code_test_doc(&self) -> bool {
        self.facts.is_non_code_test_doc
    }

    pub(crate) fn is_generic_runtime_witness_doc(&self) -> bool {
        self.facts.is_generic_runtime_witness_doc
    }

    pub(crate) fn is_repo_metadata(&self) -> bool {
        self.facts.is_repo_metadata
    }

    pub(crate) fn has_generic_runtime_anchor_stem(&self) -> bool {
        self.facts.has_generic_runtime_anchor_stem
    }

    pub(crate) fn is_frontend_runtime_noise(&self) -> bool {
        self.facts.is_frontend_runtime_noise
    }

    pub(crate) fn is_rust_workspace_config(&self) -> bool {
        self.facts.is_rust_workspace_config
    }

    pub(crate) fn is_scripts_ops(&self) -> bool {
        self.facts.is_scripts_ops
    }

    pub(crate) fn is_runtime_adjacent_python_test(&self) -> bool {
        self.facts.is_runtime_adjacent_python_test
    }

    pub(crate) fn is_non_prefix_python_test_module(&self) -> bool {
        self.facts.is_non_prefix_python_test_module
    }

    pub(crate) fn runtime_family_prefix_overlap(&self) -> usize {
        self.facts.runtime_family_prefix_overlap
    }

    pub(crate) fn path_depth(&self) -> usize {
        self.facts.path_depth
    }

    pub(crate) fn is_laravel_non_livewire_blade_view(&self) -> bool {
        self.facts.is_laravel_non_livewire_blade_view
    }

    pub(crate) fn is_laravel_livewire_view(&self) -> bool {
        self.facts.is_laravel_livewire_view
    }

    pub(crate) fn is_laravel_blade_component(&self) -> bool {
        self.facts.is_laravel_blade_component
    }

    pub(crate) fn is_laravel_nested_blade_component(&self) -> bool {
        self.facts.is_laravel_nested_blade_component
    }

    pub(crate) fn is_laravel_form_action_blade(&self) -> bool {
        self.facts.is_laravel_form_action_blade
    }

    pub(crate) fn is_laravel_livewire_component(&self) -> bool {
        self.facts.is_laravel_livewire_component
    }

    pub(crate) fn is_laravel_view_component_class(&self) -> bool {
        self.facts.is_laravel_view_component_class
    }

    pub(crate) fn is_laravel_command_or_middleware(&self) -> bool {
        self.facts.is_laravel_command_or_middleware
    }

    pub(crate) fn is_laravel_job_or_listener(&self) -> bool {
        self.facts.is_laravel_job_or_listener
    }

    pub(crate) fn is_laravel_layout_blade_view(&self) -> bool {
        self.facts.is_laravel_layout_blade_view
    }

    pub(crate) fn is_laravel_core_provider(&self) -> bool {
        self.facts.is_laravel_core_provider
    }

    pub(crate) fn is_laravel_provider(&self) -> bool {
        self.facts.is_laravel_provider
    }

    pub(crate) fn is_laravel_route(&self) -> bool {
        self.facts.is_laravel_route
    }

    pub(crate) fn is_laravel_bootstrap_entrypoint(&self) -> bool {
        self.facts.is_laravel_bootstrap_entrypoint
    }

    pub(crate) fn laravel_surface(&self) -> Option<LaravelUiSurfaceClass> {
        self.facts.laravel_surface
    }
}

#[allow(dead_code)]
pub(crate) struct SelectionStateView<'a> {
    facts: &'a SelectionFacts,
}

#[allow(dead_code)]
impl SelectionStateView<'_> {
    pub(crate) fn seen_count(&self) -> usize {
        self.facts.seen_count
    }

    pub(crate) fn runtime_seen(&self) -> usize {
        self.facts.runtime_seen
    }

    pub(crate) fn seen_ci_workflows(&self) -> usize {
        self.facts.seen_ci_workflows
    }

    pub(crate) fn seen_example_support(&self) -> usize {
        self.facts.seen_example_support
    }

    pub(crate) fn seen_bench_support(&self) -> usize {
        self.facts.seen_bench_support
    }

    pub(crate) fn seen_plain_test_support(&self) -> usize {
        self.facts.seen_plain_test_support
    }

    pub(crate) fn seen_repo_root_runtime_configs(&self) -> usize {
        self.facts.seen_repo_root_runtime_configs
    }

    pub(crate) fn seen_typescript_runtime_module_indexes(&self) -> usize {
        self.facts.seen_typescript_runtime_module_indexes
    }

    pub(crate) fn laravel_surface_seen(&self) -> usize {
        self.facts.laravel_surface_seen
    }
}

impl SelectionFacts {
    pub(crate) fn intent(&self) -> SelectionIntentView<'_> {
        SelectionIntentView { facts: self }
    }

    pub(crate) fn query(&self) -> SelectionQueryView<'_> {
        SelectionQueryView { facts: self }
    }

    pub(crate) fn candidate(&self) -> SelectionCandidateView<'_> {
        SelectionCandidateView { facts: self }
    }

    pub(crate) fn state(&self) -> SelectionStateView<'_> {
        SelectionStateView { facts: self }
    }

    pub(crate) fn from_candidate(
        candidate: &SelectionCandidate,
        intent: &HybridRankingIntent,
        query_context: &SelectionQueryContext,
        state: &SelectionState,
    ) -> Self {
        let evidence = &candidate.evidence;
        let class = candidate.static_features.class;
        let seen_count = state.seen_classes.get(&class).copied().unwrap_or(0);
        let runtime_seen = state
            .seen_classes
            .get(&HybridSourceClass::Runtime)
            .copied()
            .unwrap_or(0);
        let path_stem = Path::new(&evidence.document.path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.trim().to_ascii_lowercase())
            .unwrap_or_default();
        let laravel_surface = laravel_ui_surface_class(&evidence.document.path);
        let laravel_surface_seen = laravel_surface
            .and_then(|surface| state.seen_laravel_ui_surfaces.get(&surface).copied())
            .unwrap_or(0);
        let is_example_support = is_example_support_path(&evidence.document.path);
        let is_bench_support = is_bench_support_path(&evidence.document.path);
        let is_test_support = is_test_support_path(&evidence.document.path);
        let is_repo_root_runtime_config_artifact = candidate
            .static_features
            .is_repo_root_runtime_config_artifact;
        let is_frontend_runtime_noise = is_frontend_runtime_noise_path(&evidence.document.path)
            && !(is_repo_root_runtime_config_artifact
                && (intent.wants_entrypoint_build_flow || intent.wants_runtime_config_artifacts));
        let wants_runtime_companion_tests = intent.wants_test_witness_recall
            || intent.wants_entrypoint_build_flow
            || intent.wants_runtime_config_artifacts;
        let prefer_runtime_anchor_tests =
            wants_runtime_companion_tests && !intent.wants_test_witness_recall;
        let runtime_family_prefix_overlap = state
            .runtime_anchor_paths
            .iter()
            .map(|path| shared_path_prefix_segments(&evidence.document.path, path))
            .max()
            .unwrap_or(0);

        Self {
            base_score: evidence.blended_score
                * hybrid_path_quality_multiplier_with_intent(&evidence.document.path, intent),
            class,
            path_depth: candidate.static_features.path_depth,
            path_overlap: candidate.static_features.path_overlap,
            excerpt_overlap: candidate.static_features.excerpt_overlap,
            blade_specific_path_overlap: candidate.static_features.blade_specific_path_overlap,
            specific_witness_path_overlap: candidate.static_features.specific_witness_path_overlap,
            runtime_family_prefix_overlap,
            seen_count,
            runtime_seen,
            seen_ci_workflows: state.seen_ci_workflows,
            seen_example_support: state.seen_example_support,
            seen_bench_support: state.seen_bench_support,
            seen_plain_test_support: state.seen_plain_test_support,
            seen_repo_root_runtime_configs: state.seen_repo_root_runtime_configs,
            seen_typescript_runtime_module_indexes: state.seen_typescript_runtime_module_indexes,
            canonical_match_multiplier: candidate.static_features.canonical_match_multiplier,
            runtime_witness_path_overlap_multiplier: candidate
                .static_features
                .runtime_witness_path_overlap_multiplier,
            excerpt_has_exact_identifier_anchor: candidate
                .static_features
                .excerpt_has_exact_identifier_anchor,
            excerpt_has_build_flow_anchor: candidate.static_features.excerpt_has_build_flow_anchor,
            excerpt_has_test_double_anchor: candidate
                .static_features
                .excerpt_has_test_double_anchor,
            has_exact_query_term_match: candidate.static_features.has_exact_query_term_match,
            has_path_witness_source: candidate.static_features.has_path_witness_source,
            is_examples_rs: is_test_support
                && Path::new(&evidence.document.path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.eq_ignore_ascii_case("examples.rs")),
            query_has_exact_terms: !query_context.exact_terms.is_empty(),
            query_mentions_cli: query_context.query_mentions_cli,
            query_has_identifier_anchor: query_context.query_has_identifier_anchor,
            query_has_specific_blade_anchors: query_context.query_has_specific_blade_anchors,
            wants_runtime_companion_tests,
            prefer_runtime_anchor_tests,
            wants_example_or_bench_witnesses: query_context.wants_example_or_bench_witnesses,
            penalize_generic_runtime_docs: query_context.penalize_generic_runtime_docs,
            wants_python_witnesses: query_context.wants_python_witnesses,
            wants_rust_workspace_config: query_context.wants_rust_workspace_config,
            wants_python_workspace_config: query_context.wants_python_workspace_config,
            wants_contracts: intent.wants_contracts,
            wants_error_taxonomy: intent.wants_error_taxonomy,
            wants_tool_contracts: intent.wants_tool_contracts,
            wants_mcp_runtime_surface: intent.wants_mcp_runtime_surface,
            wants_class: intent.wants_class(class),
            wants_runtime_witnesses: intent.wants_runtime_witnesses,
            wants_runtime_config_artifacts: intent.wants_runtime_config_artifacts,
            wants_laravel_ui_witnesses: intent.wants_laravel_ui_witnesses,
            wants_blade_component_witnesses: intent.wants_blade_component_witnesses,
            wants_laravel_form_action_witnesses: intent.wants_laravel_form_action_witnesses,
            wants_livewire_view_witnesses: intent.wants_livewire_view_witnesses,
            wants_commands_middleware_witnesses: intent.wants_commands_middleware_witnesses,
            wants_jobs_listeners_witnesses: intent.wants_jobs_listeners_witnesses,
            wants_laravel_layout_witnesses: intent.wants_laravel_layout_witnesses,
            wants_test_witness_recall: intent.wants_test_witness_recall,
            wants_navigation_fallbacks: intent.wants_navigation_fallbacks,
            wants_ci_workflow_witnesses: intent.wants_ci_workflow_witnesses,
            wants_scripts_ops_witnesses: intent.wants_scripts_ops_witnesses,
            wants_entrypoint_build_flow: intent.wants_entrypoint_build_flow,
            wants_examples: intent.wants_examples,
            wants_benchmarks: intent.wants_benchmarks,
            is_ci_workflow: candidate.static_features.is_ci_workflow,
            is_example_support,
            is_runtime_config_artifact: is_runtime_config_artifact_path(&evidence.document.path),
            is_repo_root_runtime_config_artifact,
            is_typescript_runtime_module_index: candidate
                .static_features
                .is_typescript_runtime_module_index,
            is_entrypoint_runtime: is_entrypoint_runtime_path(&evidence.document.path),
            is_entrypoint_build_workflow: is_entrypoint_build_workflow_path(
                &evidence.document.path,
            ),
            is_entrypoint_reference_doc: is_entrypoint_reference_doc_path(&evidence.document.path),
            is_python_entrypoint_runtime: is_python_entrypoint_runtime_path(
                &evidence.document.path,
            ),
            is_python_runtime_config: is_python_runtime_config_path(&evidence.document.path),
            is_python_test_witness: is_python_test_witness_path(&evidence.document.path),
            is_loose_python_test_module: is_loose_python_test_module_path(&evidence.document.path),
            is_bench_support,
            is_test_support,
            is_cli_test_support: is_cli_test_support_path(&evidence.document.path),
            is_runtime_anchor_test_support: is_runtime_anchor_test_support_path(
                &evidence.document.path,
            ),
            is_runtime_adjacent_python_test: is_runtime_adjacent_python_test_path(
                &evidence.document.path,
            ),
            is_non_prefix_python_test_module: is_non_prefix_python_test_module_path(
                &evidence.document.path,
            ),
            is_test_harness: is_test_harness_path(&evidence.document.path),
            is_non_code_test_doc: is_non_code_test_doc_path(&evidence.document.path),
            is_generic_runtime_witness_doc: is_generic_runtime_witness_doc_path(
                &evidence.document.path,
            ),
            is_repo_metadata: is_repo_metadata_path(&evidence.document.path),
            has_generic_runtime_anchor_stem: has_generic_runtime_anchor_stem(
                &evidence.document.path,
            ),
            is_frontend_runtime_noise,
            is_navigation_runtime: is_navigation_runtime_path(&evidence.document.path),
            is_navigation_reference_doc: is_navigation_reference_doc_path(&evidence.document.path),
            is_scripts_ops: is_scripts_ops_path(&evidence.document.path),
            is_rust_workspace_config: is_rust_workspace_config_path(&evidence.document.path),
            path_stem_is_server_or_cli: matches!(path_stem.as_str(), "server" | "cli"),
            path_stem_is_main: path_stem == "main",
            is_laravel_non_livewire_blade_view:
                super::super::super::is_laravel_non_livewire_blade_view_path(
                    &evidence.document.path,
                ),
            is_laravel_livewire_view: super::super::super::is_laravel_livewire_view_path(
                &evidence.document.path,
            ),
            is_laravel_blade_component: super::super::super::is_laravel_blade_component_path(
                &evidence.document.path,
            ),
            is_laravel_nested_blade_component:
                super::super::super::is_laravel_nested_blade_component_path(&evidence.document.path),
            is_laravel_form_action_blade: super::super::super::is_laravel_form_action_blade_path(
                &evidence.document.path,
            ),
            is_laravel_livewire_component: super::super::super::is_laravel_livewire_component_path(
                &evidence.document.path,
            ),
            is_laravel_view_component_class:
                super::super::super::is_laravel_view_component_class_path(&evidence.document.path),
            is_laravel_command_or_middleware:
                super::super::super::is_laravel_command_or_middleware_path(&evidence.document.path),
            is_laravel_job_or_listener: super::super::super::is_laravel_job_or_listener_path(
                &evidence.document.path,
            ),
            is_laravel_layout_blade_view: super::super::super::is_laravel_layout_blade_view_path(
                &evidence.document.path,
            ),
            is_laravel_core_provider: super::super::super::is_laravel_core_provider_path(
                &evidence.document.path,
            ),
            is_laravel_provider: super::super::super::is_laravel_provider_path(
                &evidence.document.path,
            ),
            is_laravel_route: super::super::super::is_laravel_route_path(&evidence.document.path),
            is_laravel_bootstrap_entrypoint:
                super::super::super::is_laravel_bootstrap_entrypoint_path(&evidence.document.path),
            laravel_surface,
            laravel_surface_seen,
        }
    }
}

#[cfg(test)]
impl Default for SelectionFacts {
    fn default() -> Self {
        Self {
            base_score: 1.0,
            class: HybridSourceClass::Other,
            path_depth: 0,
            path_overlap: 0,
            excerpt_overlap: 0,
            blade_specific_path_overlap: 0,
            specific_witness_path_overlap: 0,
            runtime_family_prefix_overlap: 0,
            seen_count: 0,
            runtime_seen: 0,
            seen_ci_workflows: 0,
            seen_example_support: 0,
            seen_bench_support: 0,
            seen_plain_test_support: 0,
            seen_repo_root_runtime_configs: 0,
            seen_typescript_runtime_module_indexes: 0,
            canonical_match_multiplier: 1.0,
            runtime_witness_path_overlap_multiplier: 1.0,
            excerpt_has_exact_identifier_anchor: false,
            excerpt_has_build_flow_anchor: false,
            excerpt_has_test_double_anchor: false,
            has_exact_query_term_match: false,
            has_path_witness_source: false,
            is_examples_rs: false,
            query_has_exact_terms: false,
            query_mentions_cli: false,
            query_has_identifier_anchor: false,
            query_has_specific_blade_anchors: false,
            wants_runtime_companion_tests: false,
            prefer_runtime_anchor_tests: false,
            wants_example_or_bench_witnesses: false,
            penalize_generic_runtime_docs: false,
            wants_python_witnesses: false,
            wants_rust_workspace_config: false,
            wants_python_workspace_config: false,
            wants_contracts: false,
            wants_error_taxonomy: false,
            wants_tool_contracts: false,
            wants_mcp_runtime_surface: false,
            wants_class: false,
            wants_runtime_witnesses: false,
            wants_runtime_config_artifacts: false,
            wants_laravel_ui_witnesses: false,
            wants_blade_component_witnesses: false,
            wants_laravel_form_action_witnesses: false,
            wants_livewire_view_witnesses: false,
            wants_commands_middleware_witnesses: false,
            wants_jobs_listeners_witnesses: false,
            wants_laravel_layout_witnesses: false,
            wants_test_witness_recall: false,
            wants_navigation_fallbacks: false,
            wants_ci_workflow_witnesses: false,
            wants_scripts_ops_witnesses: false,
            wants_entrypoint_build_flow: false,
            wants_examples: false,
            wants_benchmarks: false,
            is_ci_workflow: false,
            is_example_support: false,
            is_runtime_config_artifact: false,
            is_repo_root_runtime_config_artifact: false,
            is_typescript_runtime_module_index: false,
            is_entrypoint_runtime: false,
            is_entrypoint_build_workflow: false,
            is_entrypoint_reference_doc: false,
            is_python_entrypoint_runtime: false,
            is_python_runtime_config: false,
            is_python_test_witness: false,
            is_loose_python_test_module: false,
            is_bench_support: false,
            is_test_support: false,
            is_cli_test_support: false,
            is_runtime_anchor_test_support: false,
            is_runtime_adjacent_python_test: false,
            is_non_prefix_python_test_module: false,
            is_test_harness: false,
            is_non_code_test_doc: false,
            is_generic_runtime_witness_doc: false,
            is_repo_metadata: false,
            has_generic_runtime_anchor_stem: false,
            is_frontend_runtime_noise: false,
            is_navigation_runtime: false,
            is_navigation_reference_doc: false,
            is_scripts_ops: false,
            is_rust_workspace_config: false,
            path_stem_is_server_or_cli: false,
            path_stem_is_main: false,
            is_laravel_non_livewire_blade_view: false,
            is_laravel_livewire_view: false,
            is_laravel_blade_component: false,
            is_laravel_nested_blade_component: false,
            is_laravel_form_action_blade: false,
            is_laravel_livewire_component: false,
            is_laravel_view_component_class: false,
            is_laravel_command_or_middleware: false,
            is_laravel_job_or_listener: false,
            is_laravel_layout_blade_view: false,
            is_laravel_core_provider: false,
            is_laravel_provider: false,
            is_laravel_route: false,
            is_laravel_bootstrap_entrypoint: false,
            laravel_surface: None,
            laravel_surface_seen: 0,
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

fn path_depth(path: &str) -> usize {
    path.trim_start_matches("./")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .count()
}

fn shared_path_prefix_segments(left: &str, right: &str) -> usize {
    left.trim_start_matches("./")
        .split('/')
        .zip(right.trim_start_matches("./").split('/'))
        .take_while(|(left, right)| left == right)
        .count()
}

fn is_non_prefix_python_test_module_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.ends_with(".py")
        && Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| !name.starts_with("test_"))
}
