use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::workspace_ignores::hard_excluded_runtime_path;
use ignore::WalkBuilder;

use super::surfaces::{is_ci_workflow_path, is_entrypoint_build_workflow_path};
use super::{
    HybridRankingIntent, NormalizedSearchFilters, SearchDiagnostic, SearchDiagnosticKind,
    SearchExecutionDiagnostics, SearchTextQuery,
};

pub(super) fn merge_candidate_files(
    base: &mut Vec<(String, PathBuf)>,
    supplement: Vec<(String, PathBuf)>,
) {
    let mut seen = base
        .iter()
        .map(|(rel_path, path)| (rel_path.clone(), path.clone()))
        .collect::<BTreeSet<_>>();
    for candidate in supplement {
        if seen.insert((candidate.0.clone(), candidate.1.clone())) {
            base.push(candidate);
        }
    }
}

pub(super) fn walk_candidate_files_for_repository(
    repository_id: &str,
    root: &Path,
    query: &SearchTextQuery,
    filters: &NormalizedSearchFilters,
    diagnostics: &mut SearchExecutionDiagnostics,
) -> Vec<(String, PathBuf)> {
    let walker = search_walk_builder(root).build();
    let mut file_candidates = Vec::new();

    for dent in walker {
        let dent = match dent {
            Ok(entry) => entry,
            Err(err) => {
                diagnostics.entries.push(SearchDiagnostic {
                    repository_id: repository_id.to_owned(),
                    path: None,
                    kind: SearchDiagnosticKind::Walk,
                    message: err.to_string(),
                });
                continue;
            }
        };
        if !dent.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = dent.path();
        if hard_excluded_runtime_path(root, path) {
            continue;
        }
        let rel_path = normalize_repository_relative_path(root, path);

        if let Some(language) = filters.language {
            if !language.matches_path(path) {
                continue;
            }
        }

        if let Some(path_regex) = &query.path_regex {
            if !path_regex.is_match(&rel_path) {
                continue;
            }
        }

        file_candidates.push((rel_path, path.to_path_buf()));
    }
    file_candidates.sort_by(|left, right| left.0.cmp(&right.0));
    file_candidates.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    file_candidates
}

pub(super) fn hidden_workflow_candidates_for_repository(
    root: &Path,
    filters: &NormalizedSearchFilters,
    intent: &HybridRankingIntent,
    diagnostics: &mut SearchExecutionDiagnostics,
) -> Vec<(String, PathBuf)> {
    if !intent.wants_entrypoint_build_flow && !intent.wants_ci_workflow_witnesses {
        return Vec::new();
    }

    let workflows_root = root.join(".github/workflows");
    if !workflows_root.is_dir() {
        return Vec::new();
    }

    let mut builder = WalkBuilder::new(&workflows_root);
    builder
        .standard_filters(true)
        .hidden(false)
        .require_git(false);
    let walker = builder.build();
    let mut file_candidates = Vec::new();
    let repository_id = root.to_string_lossy().into_owned();

    for dent in walker {
        let dent = match dent {
            Ok(entry) => entry,
            Err(err) => {
                diagnostics.entries.push(SearchDiagnostic {
                    repository_id: repository_id.clone(),
                    path: Some(".github/workflows".to_owned()),
                    kind: SearchDiagnosticKind::Walk,
                    message: err.to_string(),
                });
                continue;
            }
        };
        if !dent.file_type().is_some_and(|ft| ft.is_file()) {
            continue;
        }

        let path = dent.path();
        if hard_excluded_runtime_path(root, path) {
            continue;
        }
        let rel_path = normalize_repository_relative_path(root, path);
        if intent.wants_entrypoint_build_flow {
            if !is_entrypoint_build_workflow_path(&rel_path) {
                continue;
            }
        } else if !is_ci_workflow_path(&rel_path) {
            continue;
        }

        if let Some(language) = filters.language {
            if !language.matches_path(path) {
                continue;
            }
        }

        file_candidates.push((rel_path, path.to_path_buf()));
    }

    file_candidates.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
    file_candidates.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
    file_candidates
}

pub(super) fn normalize_repository_relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .ok()
        .map(|path| path.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string())
        .replace('\\', "/")
        .trim_start_matches("./")
        .to_owned()
}

fn search_walk_builder(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder.standard_filters(true).require_git(false);
    builder
}
