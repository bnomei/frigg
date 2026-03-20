use std::path::Path;

use crate::searcher::RepositoryCandidateUniverse;
use crate::searcher::candidates::normalize_repository_relative_path;
use crate::storage::{PathAnchorSketchProjection, Storage};

pub(super) fn projection_source_paths_for_repository(
    repository: &RepositoryCandidateUniverse,
    storage: &Storage,
    snapshot_id: &str,
) -> Vec<String> {
    let candidate_paths = repository_candidate_paths(repository);
    let mut manifest_paths = storage
        .load_manifest_for_snapshot(snapshot_id)
        .ok()
        .map(|entries| {
            entries
                .into_iter()
                .map(|entry| {
                    normalize_repository_relative_path(&repository.root, Path::new(&entry.path))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    manifest_paths.sort();
    manifest_paths.dedup();

    if manifest_paths.is_empty()
        || candidate_paths
            .iter()
            .any(|path| manifest_paths.binary_search(path).is_err())
    {
        return candidate_paths;
    }

    manifest_paths
}

pub(super) fn repository_candidate_paths(repository: &RepositoryCandidateUniverse) -> Vec<String> {
    let mut candidate_paths = repository
        .candidates
        .iter()
        .map(|candidate| candidate.relative_path.clone())
        .collect::<Vec<_>>();
    candidate_paths.sort();
    candidate_paths.dedup();
    candidate_paths
}

pub(super) fn group_anchor_sketches_by_path(
    rows: Vec<PathAnchorSketchProjection>,
) -> std::collections::BTreeMap<String, Vec<PathAnchorSketchProjection>> {
    let mut grouped = std::collections::BTreeMap::<String, Vec<PathAnchorSketchProjection>>::new();
    for row in rows {
        grouped.entry(row.path.clone()).or_default().push(row);
    }
    for anchors in grouped.values_mut() {
        anchors.sort_by(|left, right| {
            left.anchor_rank
                .cmp(&right.anchor_rank)
                .then(left.line.cmp(&right.line))
        });
    }
    grouped
}
