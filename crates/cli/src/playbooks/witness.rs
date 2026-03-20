use super::{
    HybridPlaybookWitnessOutcome, HybridWitnessGroup, HybridWitnessMatchBy, HybridWitnessMatchMode,
    HybridWitnessRequirement,
};

fn path_matches_prefix(candidate: &str, prefix: &str) -> bool {
    let normalized_candidate = candidate.trim().trim_matches('/');
    let normalized_prefix = prefix.trim().trim_matches('/');
    if normalized_candidate.is_empty() || normalized_prefix.is_empty() {
        return false;
    }
    normalized_candidate == normalized_prefix
        || normalized_candidate
            .strip_prefix(normalized_prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn witness_group_match(
    group: &HybridWitnessGroup,
    matched_paths: &[String],
) -> (HybridWitnessMatchBy, Option<String>, bool) {
    if let Some(path) = group
        .match_any
        .iter()
        .find_map(|expected| {
            matched_paths
                .iter()
                .find(|candidate| *candidate == expected)
        })
        .cloned()
    {
        return (HybridWitnessMatchBy::Exact, Some(path), true);
    }

    if group.match_mode == HybridWitnessMatchMode::ExactOrPrefix {
        if let Some(path) = group
            .accepted_prefixes
            .iter()
            .find_map(|prefix| {
                matched_paths
                    .iter()
                    .find(|candidate| path_matches_prefix(candidate, prefix))
            })
            .cloned()
        {
            return (HybridWitnessMatchBy::Prefix, Some(path), false);
        }
    }

    (HybridWitnessMatchBy::None, None, false)
}

pub(super) fn witness_outcomes(
    groups: &[HybridWitnessGroup],
    matched_paths: &[String],
    semantic_status_ok: bool,
    target_only: bool,
) -> Vec<HybridPlaybookWitnessOutcome> {
    groups
        .iter()
        .filter(|group| {
            !target_only
                || matches!(group.required_when, HybridWitnessRequirement::Always)
                || semantic_status_ok
        })
        .map(|group| {
            let required = if target_only {
                true
            } else {
                match group.required_when {
                    HybridWitnessRequirement::Always => true,
                    HybridWitnessRequirement::SemanticOk => semantic_status_ok,
                }
            };
            let (matched_by, matched_path, exact_matched) =
                witness_group_match(group, matched_paths);
            let passed = !required || exact_matched;
            HybridPlaybookWitnessOutcome {
                group_id: group.group_id.clone(),
                match_any: group.match_any.clone(),
                match_mode: group.match_mode,
                accepted_prefixes: group.accepted_prefixes.clone(),
                required_when: group.required_when,
                matched_by,
                matched_path,
                passed,
            }
        })
        .collect()
}

pub(super) fn semantic_status_allowed(allowed_statuses: &[String], semantic_status: &str) -> bool {
    if allowed_statuses.is_empty() {
        return true;
    }

    let semantic_status = semantic_status.trim().to_ascii_lowercase();
    if allowed_statuses
        .iter()
        .any(|status| status.trim().eq_ignore_ascii_case(&semantic_status))
    {
        return true;
    }

    semantic_status == "unavailable"
        && allowed_statuses
            .iter()
            .any(|status| status.trim().eq_ignore_ascii_case("disabled"))
}
