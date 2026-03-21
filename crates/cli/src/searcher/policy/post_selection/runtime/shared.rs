use super::*;

pub(super) fn selected_match_for_path<'a>(
    matches: &'a [HybridRankedEvidence],
    path: &str,
) -> &'a HybridRankedEvidence {
    matches
        .iter()
        .find(|entry| entry.document.path == path)
        .expect("selected evidence path should exist in matches")
}

pub(super) fn preserve_selected_build_workflow(
    matches: &[HybridRankedEvidence],
    ctx: &PostSelectionContext<'_>,
) -> bool {
    ctx.intent.wants_entrypoint_build_flow
        && matches.iter().any(|entry| {
            surfaces::is_entrypoint_build_workflow_path(&entry.document.path)
                || surfaces::is_ci_workflow_path(&entry.document.path)
        })
}

pub(super) fn runtime_config_artifact_guardrail_cmp(
    left: &str,
    right: &str,
    prefer_repo_root: bool,
) -> Ordering {
    let left_is_root_scoped = is_root_scoped_runtime_config_path(left);
    let right_is_root_scoped = is_root_scoped_runtime_config_path(right);
    let left_depth = left.trim_start_matches("./").split('/').count();
    let right_depth = right.trim_start_matches("./").split('/').count();

    (if prefer_repo_root {
        left_is_root_scoped.cmp(&right_is_root_scoped)
    } else {
        Ordering::Equal
    })
    .then_with(|| right_depth.cmp(&left_depth))
}

pub(super) fn query_mentions_cli_command(query_text: &str) -> bool {
    hybrid_query_mentions_cli_command(query_text)
}

pub(super) fn cli_specific_test_guardrail_cmp(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> Ordering {
    let left_facts = selection_guardrail_facts(left, state, ctx);
    let right_facts = selection_guardrail_facts(right, state, ctx);

    left_facts
        .specific_witness_path_overlap
        .cmp(&right_facts.specific_witness_path_overlap)
        .then_with(|| {
            left_facts
                .has_exact_query_term_match
                .cmp(&right_facts.has_exact_query_term_match)
        })
        .then_with(|| selection_guardrail_cmp(left, right, state, ctx))
}

pub(super) fn runtime_companion_surface_supports_query(
    entry: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
    query_wants_android_ui_surface: bool,
) -> bool {
    let facts = selection_guardrail_facts(entry, state, ctx);
    query_wants_android_ui_surface
        || facts.specific_witness_path_overlap > 0
        || (facts.runtime_subtree_affinity > 0
            && matches!(
                facts.class,
                HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
            ))
        || (facts.has_path_witness_source && facts.path_overlap > 0)
}

fn runtime_companion_surface_cluster_support(
    entry: &HybridRankedEvidence,
    matches: &[HybridRankedEvidence],
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> usize {
    let mut support_paths = BTreeSet::new();
    let entry_path = entry.document.path.as_str();
    let mut consider = |other: &HybridRankedEvidence| {
        if other.document.path == entry.document.path {
            return;
        }

        let other_facts = selection_guardrail_facts(other, state, ctx);
        let support_surface = matches!(
            other_facts.class,
            HybridSourceClass::Runtime | HybridSourceClass::Support | HybridSourceClass::Tests
        ) || other_facts.is_runtime_config_artifact
            || other_facts.is_entrypoint_runtime
            || other_facts.is_test_support;
        if !support_surface
            || other_facts.is_ci_workflow
            || other_facts.is_repo_metadata
            || other_facts.is_generic_runtime_witness_doc
            || other_facts.is_frontend_runtime_noise
        {
            return;
        }

        if SharedPathFacts::workspace_subtree_affinity(entry_path, &other.document.path) > 0 {
            support_paths.insert(other.document.path.clone());
        }
    };

    for other in matches {
        consider(other);
    }
    for other in ctx.candidate_pool {
        consider(other);
    }
    for hit in ctx.witness_hits {
        let evidence = hybrid_ranked_evidence_from_witness_hit(hit);
        consider(&evidence);
    }

    support_paths.len()
}

pub(super) fn runtime_companion_surface_guardrail_cmp(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
    matches: &[HybridRankedEvidence],
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> Ordering {
    let left_facts = selection_guardrail_facts(left, state, ctx);
    let right_facts = selection_guardrail_facts(right, state, ctx);
    let left_cluster_support = runtime_companion_surface_cluster_support(left, matches, state, ctx);
    let right_cluster_support =
        runtime_companion_surface_cluster_support(right, matches, state, ctx);

    left_cluster_support
        .cmp(&right_cluster_support)
        .then_with(|| {
            left_facts
                .specific_witness_path_overlap
                .cmp(&right_facts.specific_witness_path_overlap)
        })
        .then_with(|| {
            left_facts
                .runtime_subtree_affinity
                .cmp(&right_facts.runtime_subtree_affinity)
        })
        .then_with(|| selection_guardrail_cmp(left, right, state, ctx))
}

pub(super) fn is_runtime_config_ordering_candidate_path(path: &str) -> bool {
    surfaces::is_package_surface_path(path)
        || surfaces::is_workspace_config_surface_path(path)
        || surfaces::is_build_config_surface_path(path)
        || is_specific_runtime_config_surface_path(path)
}

pub(super) fn runtime_config_ordering_cmp(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> Ordering {
    let left_facts = selection_guardrail_facts(left, state, ctx);
    let right_facts = selection_guardrail_facts(right, state, ctx);
    let prefer_python_workspace_config =
        left_facts.wants_python_workspace_config && !left_facts.wants_rust_workspace_config;
    let prefer_rust_workspace_config =
        left_facts.wants_rust_workspace_config && !left_facts.wants_python_workspace_config;

    (if prefer_rust_workspace_config {
        left_facts
            .is_rust_workspace_config
            .cmp(&right_facts.is_rust_workspace_config)
    } else {
        Ordering::Equal
    })
    .then_with(|| {
        if prefer_python_workspace_config {
            left_facts
                .is_python_runtime_config
                .cmp(&right_facts.is_python_runtime_config)
        } else {
            Ordering::Equal
        }
    })
    .then_with(|| {
        right_facts
            .is_frontend_runtime_noise
            .cmp(&left_facts.is_frontend_runtime_noise)
    })
    .then_with(|| {
        left_facts
            .specific_witness_path_overlap
            .cmp(&right_facts.specific_witness_path_overlap)
    })
    .then_with(|| {
        left_facts
            .runtime_subtree_affinity
            .cmp(&right_facts.runtime_subtree_affinity)
    })
    .then_with(|| {
        runtime_config_surface_guardrail_priority_for_path(&left.document.path).cmp(
            &runtime_config_surface_guardrail_priority_for_path(&right.document.path),
        )
    })
    .then_with(|| {
        left_facts
            .is_repo_root_runtime_config_artifact
            .cmp(&right_facts.is_repo_root_runtime_config_artifact)
    })
    .then_with(|| selection_guardrail_cmp(left, right, state, ctx))
    .then_with(|| left_facts.path_overlap.cmp(&right_facts.path_overlap))
    .then_with(|| {
        left_facts
            .has_exact_query_term_match
            .cmp(&right_facts.has_exact_query_term_match)
    })
}
