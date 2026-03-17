use std::cmp::Ordering;

use super::super::super::HybridChannelHit;
use super::super::super::HybridRankedEvidence;
use super::super::SelectionCandidate;
use super::super::SelectionFacts;
use super::super::SelectionState;
use super::super::hybrid_selection_score_from_context;
use super::PostSelectionContext;
use super::PostSelectionRepairAction;
use super::PostSelectionRuleMeta;
use super::is_test_support_guardrail_replacement;
use super::test_support_guardrail_replacement_priority;

pub(super) fn choose_best_candidate(
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

pub(super) fn insert_guardrail_candidate(
    mut matches: Vec<HybridRankedEvidence>,
    candidate: Option<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
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
        let replaced_path = matches[index].document.path.clone();
        matches[index] = candidate;
        ctx.record_repair(
            meta,
            PostSelectionRepairAction::Replaced,
            &matches[index].document.path,
            Some(replaced_path),
        );
    } else if matches.len() < ctx.limit {
        matches.push(candidate);
        let inserted = matches
            .last()
            .expect("guardrail insertion appended a candidate");
        ctx.record_repair(
            meta,
            PostSelectionRepairAction::Inserted,
            &inserted.document.path,
            None,
        );
    }

    matches
}

pub(super) fn insert_test_support_guardrail_candidate(
    mut matches: Vec<HybridRankedEvidence>,
    candidate: Option<HybridRankedEvidence>,
    ctx: &PostSelectionContext<'_>,
    meta: PostSelectionRuleMeta,
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
        let replaced_path = matches[index].document.path.clone();
        matches[index] = candidate;
        ctx.record_repair(
            meta,
            PostSelectionRepairAction::Replaced,
            &matches[index].document.path,
            Some(replaced_path),
        );
    } else if matches.len() < ctx.limit {
        matches.push(candidate);
        let inserted = matches
            .last()
            .expect("guardrail insertion appended a test-support candidate");
        ctx.record_repair(
            meta,
            PostSelectionRepairAction::Inserted,
            &inserted.document.path,
            None,
        );
    }

    matches
}

pub(super) fn selection_guardrail_state(
    matches: &[HybridRankedEvidence],
    ctx: &PostSelectionContext<'_>,
) -> SelectionState {
    SelectionState::from_selected(matches, ctx.intent, &ctx.selection_query_context)
}

pub(super) fn selection_guardrail_score(
    entry: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> f32 {
    hybrid_selection_score_from_context(&selection_guardrail_facts(entry, state, ctx))
}

pub(super) fn selection_guardrail_facts(
    entry: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> SelectionFacts {
    let candidate =
        SelectionCandidate::new(entry.clone(), ctx.intent, &ctx.selection_query_context);
    SelectionFacts::from_candidate(&candidate, ctx.intent, &ctx.selection_query_context, state)
}

pub(super) fn selection_guardrail_score_for_path(
    path: &str,
    matches: &[HybridRankedEvidence],
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> f32 {
    matches
        .iter()
        .find(|entry| entry.document.path == path)
        .map(|entry| selection_guardrail_score(entry, state, ctx))
        .unwrap_or(f32::NEG_INFINITY)
}

pub(super) fn selection_guardrail_cmp(
    left: &HybridRankedEvidence,
    right: &HybridRankedEvidence,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> Ordering {
    let left_facts = selection_guardrail_facts(left, state, ctx);
    let right_facts = selection_guardrail_facts(right, state, ctx);

    let score_cmp = hybrid_selection_score_from_context(&left_facts)
        .total_cmp(&hybrid_selection_score_from_context(&right_facts));
    let companion_cmp = if left_facts.wants_runtime_companion_tests
        && right_facts.wants_runtime_companion_tests
        && left_facts.is_test_support
        && right_facts.is_test_support
    {
        let guardrail_cmp = left_facts
            .path_overlap
            .cmp(&right_facts.path_overlap)
            .then_with(|| {
                left_facts
                    .has_exact_query_term_match
                    .cmp(&right_facts.has_exact_query_term_match)
            })
            .then_with(|| {
                companion_test_guardrail_priority(&left_facts)
                    .cmp(&companion_test_guardrail_priority(&right_facts))
            })
            .then_with(|| left_facts.path_depth.cmp(&right_facts.path_depth));
        let prefer_family_affinity_first = !left_facts.prefer_runtime_anchor_tests
            && (!left_facts.wants_example_or_bench_witnesses
                || (left_facts.wants_entrypoint_build_flow
                    && left_facts.wants_test_witness_recall));
        if !prefer_family_affinity_first {
            guardrail_cmp
                .then_with(|| {
                    left_facts
                        .is_runtime_adjacent_python_test
                        .cmp(&right_facts.is_runtime_adjacent_python_test)
                })
                .then_with(|| {
                    left_facts
                        .runtime_family_prefix_overlap
                        .cmp(&right_facts.runtime_family_prefix_overlap)
                })
        } else {
            left_facts
                .is_runtime_adjacent_python_test
                .cmp(&right_facts.is_runtime_adjacent_python_test)
                .then_with(|| {
                    left_facts
                        .runtime_family_prefix_overlap
                        .cmp(&right_facts.runtime_family_prefix_overlap)
                })
                .then_with(|| guardrail_cmp)
        }
    } else {
        Ordering::Equal
    };

    companion_cmp
        .then(score_cmp)
        .then_with(|| {
            left_facts
                .is_runtime_anchor_test_support
                .cmp(&right_facts.is_runtime_anchor_test_support)
        })
        .then_with(|| {
            left_facts
                .is_runtime_adjacent_python_test
                .cmp(&right_facts.is_runtime_adjacent_python_test)
        })
        .then_with(|| {
            left_facts
                .runtime_subtree_affinity
                .cmp(&right_facts.runtime_subtree_affinity)
        })
        .then_with(|| {
            left_facts
                .runtime_family_prefix_overlap
                .cmp(&right_facts.runtime_family_prefix_overlap)
        })
        .then_with(|| {
            left_facts
                .has_exact_query_term_match
                .cmp(&right_facts.has_exact_query_term_match)
        })
        .then_with(|| {
            left_facts
                .specific_witness_path_overlap
                .cmp(&right_facts.specific_witness_path_overlap)
        })
        .then_with(|| left_facts.path_overlap.cmp(&right_facts.path_overlap))
        .then_with(|| left_facts.path_depth.cmp(&right_facts.path_depth))
        .then_with(|| left.blended_score.total_cmp(&right.blended_score))
        .then_with(|| left.document.cmp(&right.document).reverse())
}

fn companion_test_guardrail_priority(facts: &SelectionFacts) -> usize {
    if facts.prefer_runtime_anchor_tests {
        if facts.is_runtime_anchor_test_support {
            if facts.is_runtime_adjacent_python_test {
                if facts.is_non_prefix_python_test_module {
                    4
                } else {
                    5
                }
            } else if facts.is_non_prefix_python_test_module {
                3
            } else {
                4
            }
        } else if facts.is_cli_test_support {
            3
        } else {
            0
        }
    } else if facts.is_cli_test_support || facts.is_test_harness {
        2
    } else if facts.is_runtime_adjacent_python_test {
        2
    } else if facts.is_runtime_anchor_test_support {
        if facts.is_non_prefix_python_test_module {
            1
        } else {
            2
        }
    } else {
        1
    }
}

pub(super) fn selection_guardrail_cmp_from_hit(
    left: &HybridChannelHit,
    right: &HybridChannelHit,
    state: &SelectionState,
    ctx: &PostSelectionContext<'_>,
) -> Ordering {
    selection_guardrail_cmp(
        &hybrid_ranked_evidence_from_witness_hit(left),
        &hybrid_ranked_evidence_from_witness_hit(right),
        state,
        ctx,
    )
}

pub(super) fn hybrid_ranked_evidence_from_witness_hit(
    hit: &HybridChannelHit,
) -> HybridRankedEvidence {
    HybridRankedEvidence {
        document: hit.document.clone(),
        anchor: hit.anchor.clone(),
        excerpt: hit.excerpt.clone(),
        blended_score: hit.raw_score.max(0.0),
        lexical_score: 0.0,
        witness_score: hit.raw_score.max(0.0),
        graph_score: 0.0,
        semantic_score: 0.0,
        lexical_sources: Vec::new(),
        witness_sources: hit.provenance_ids.clone(),
        graph_sources: Vec::new(),
        semantic_sources: Vec::new(),
    }
}
