use crate::domain::model::TextMatch;
use std::cmp::Ordering;
use std::collections::BinaryHeap;

use super::SearchDiagnostic;

pub(super) fn sort_matches_deterministically(matches: &mut [TextMatch]) {
    matches.sort_by(text_match_order);
}

pub(super) fn sort_search_diagnostics_deterministically(diagnostics: &mut [SearchDiagnostic]) {
    diagnostics.sort_by(search_diagnostic_order);
}

fn search_diagnostic_order(left: &SearchDiagnostic, right: &SearchDiagnostic) -> Ordering {
    left.repository_id
        .cmp(&right.repository_id)
        .then(left.path.cmp(&right.path))
        .then(left.kind.cmp(&right.kind))
        .then(left.message.cmp(&right.message))
}

pub(super) fn text_match_order(left: &TextMatch, right: &TextMatch) -> Ordering {
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
) -> Ordering {
    repository_id
        .cmp(&existing.repository_id)
        .then(path.cmp(&existing.path))
        .then(line.cmp(&existing.line))
        .then(column.cmp(&existing.column))
        .then(excerpt.cmp(&existing.excerpt))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct OrderedTextMatch(TextMatch);

impl Ord for OrderedTextMatch {
    fn cmp(&self, other: &Self) -> Ordering {
        text_match_order(&self.0, &other.0)
    }
}

impl PartialOrd for OrderedTextMatch {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug)]
pub(super) enum BoundedTextMatches {
    Unbounded(Vec<TextMatch>),
    Bounded {
        limit: usize,
        matches: BinaryHeap<OrderedTextMatch>,
    },
}

impl Default for BoundedTextMatches {
    fn default() -> Self {
        Self::Unbounded(Vec::new())
    }
}

impl BoundedTextMatches {
    pub(super) fn with_limit(limit: usize, bounded: bool) -> Self {
        if bounded {
            Self::Bounded {
                limit,
                matches: BinaryHeap::with_capacity(limit),
            }
        } else {
            Self::Unbounded(Vec::new())
        }
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        match self {
            Self::Unbounded(matches) => matches.len(),
            Self::Bounded { matches, .. } => matches.len(),
        }
    }

    pub(super) fn is_full(&self) -> bool {
        match self {
            Self::Unbounded(_) => false,
            Self::Bounded { limit, matches } => *limit > 0 && matches.len() == *limit,
        }
    }

    pub(super) fn worst(&self) -> Option<&TextMatch> {
        match self {
            Self::Unbounded(matches) => matches.last(),
            Self::Bounded { matches, .. } => matches.peek().map(|entry| &entry.0),
        }
    }

    pub(super) fn push(&mut self, candidate: TextMatch) {
        match self {
            Self::Unbounded(matches) => matches.push(candidate),
            Self::Bounded { limit, matches } => {
                if *limit == 0 {
                    return;
                }

                if matches.len() < *limit {
                    matches.push(OrderedTextMatch(candidate));
                    return;
                }

                if matches
                    .peek()
                    .is_some_and(|worst| text_match_order(&candidate, &worst.0).is_lt())
                {
                    matches.pop();
                    matches.push(OrderedTextMatch(candidate));
                }
            }
        }
    }

    pub(super) fn into_final_matches(self, limit: usize) -> Vec<TextMatch> {
        let mut matches = match self {
            Self::Unbounded(matches) => matches,
            Self::Bounded { matches, .. } => matches.into_iter().map(|entry| entry.0).collect(),
        };
        sort_matches_deterministically(&mut matches);
        matches.truncate(limit);
        matches
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_match(repository_id: &str, path: &str, line: usize, column: usize) -> TextMatch {
        TextMatch {
            match_id: None,
            repository_id: repository_id.to_owned(),
            path: path.to_owned(),
            line,
            column,
            excerpt: format!("{path}:{line}:{column}"),
            witness_score_hint_millis: None,
            witness_provenance_ids: None,
        }
    }

    #[test]
    fn bounded_text_matches_keep_the_best_deterministic_top_k() {
        let mut matches = BoundedTextMatches::with_limit(3, true);
        matches.push(text_match("repo-002", "zeta.rs", 9, 1));
        matches.push(text_match("repo-001", "beta.rs", 3, 2));
        matches.push(text_match("repo-001", "alpha.rs", 1, 1));
        matches.push(text_match("repo-001", "alpha.rs", 1, 2));
        matches.push(text_match("repo-003", "omega.rs", 7, 4));

        assert!(matches.is_full());
        assert_eq!(matches.len(), 3);
        assert!(matches.worst().is_some());

        let retained = matches.into_final_matches(3);
        assert_eq!(
            retained,
            vec![
                text_match("repo-001", "alpha.rs", 1, 1),
                text_match("repo-001", "alpha.rs", 1, 2),
                text_match("repo-001", "beta.rs", 3, 2),
            ]
        );
    }

    #[test]
    fn unbounded_matches_preserve_all_candidates_before_final_truncation() {
        let mut matches = BoundedTextMatches::with_limit(2, false);
        matches.push(text_match("repo-002", "zeta.rs", 9, 1));
        matches.push(text_match("repo-001", "alpha.rs", 1, 1));
        matches.push(text_match("repo-001", "beta.rs", 3, 2));

        assert!(!matches.is_full());
        assert_eq!(matches.len(), 3);

        let retained = matches.into_final_matches(2);
        assert_eq!(
            retained,
            vec![
                text_match("repo-001", "alpha.rs", 1, 1),
                text_match("repo-001", "beta.rs", 3, 2),
            ]
        );
    }
}
