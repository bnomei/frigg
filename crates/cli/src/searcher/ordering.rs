use crate::domain::model::TextMatch;

use super::SearchDiagnostic;

pub(super) fn sort_matches_deterministically(matches: &mut [TextMatch]) {
    matches.sort_by(text_match_order);
}

pub(super) fn sort_search_diagnostics_deterministically(diagnostics: &mut [SearchDiagnostic]) {
    diagnostics.sort_by(search_diagnostic_order);
}

fn search_diagnostic_order(
    left: &SearchDiagnostic,
    right: &SearchDiagnostic,
) -> std::cmp::Ordering {
    left.repository_id
        .cmp(&right.repository_id)
        .then(left.path.cmp(&right.path))
        .then(left.kind.cmp(&right.kind))
        .then(left.message.cmp(&right.message))
}

pub(super) fn text_match_order(left: &TextMatch, right: &TextMatch) -> std::cmp::Ordering {
    left.repository_id
        .cmp(&right.repository_id)
        .then(left.path.cmp(&right.path))
        .then(left.line.cmp(&right.line))
        .then(left.column.cmp(&right.column))
        .then(left.excerpt.cmp(&right.excerpt))
}

pub(super) fn text_match_candidate_order(
    repository_id: &str,
    path: &str,
    line: usize,
    column: usize,
    excerpt: &str,
    existing: &TextMatch,
) -> std::cmp::Ordering {
    repository_id
        .cmp(&existing.repository_id)
        .then(path.cmp(&existing.path))
        .then(line.cmp(&existing.line))
        .then(column.cmp(&existing.column))
        .then(excerpt.cmp(&existing.excerpt))
}

pub(super) fn retain_bounded_match(
    matches: &mut Vec<TextMatch>,
    limit: usize,
    candidate: TextMatch,
) {
    if matches.len() < limit {
        insert_sorted_match(matches, candidate);
        return;
    }

    if matches
        .last()
        .is_some_and(|worst| text_match_order(&candidate, worst).is_lt())
    {
        insert_sorted_match(matches, candidate);
        matches.truncate(limit);
    }
}

fn insert_sorted_match(matches: &mut Vec<TextMatch>, candidate: TextMatch) {
    let insert_at = matches.partition_point(|existing| {
        matches!(
            text_match_order(existing, &candidate),
            std::cmp::Ordering::Less
        )
    });
    matches.insert(insert_at, candidate);
}
