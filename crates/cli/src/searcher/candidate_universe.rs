use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::manifest_validation::latest_validated_manifest_snapshot;
use crate::storage::{Storage, resolve_provenance_db_path};
use crate::workspace_ignores::{build_root_ignore_matcher, should_ignore_runtime_path};

use super::attribution::elapsed_us;
use super::candidates::{
    hidden_workflow_candidates_for_repository, merge_candidate_files,
    normalize_repository_relative_path, root_scoped_runtime_config_candidates_for_repository,
    search_root_scoped_runtime_config_candidates_for_repository,
    walk_candidate_files_for_repository,
};
use super::ordering::sort_search_diagnostics_deterministically;
use super::{
    HybridRankingIntent, ManifestCandidateFilesBuild, NormalizedSearchFilters,
    RepositoryCandidateUniverse, SearchCandidateFile, SearchCandidateUniverse,
    SearchCandidateUniverseBuild, SearchExecutionDiagnostics, SearchTextQuery, TextSearcher,
};

impl TextSearcher {
    pub(super) fn build_candidate_universe(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
    ) -> SearchCandidateUniverse {
        self.build_candidate_universe_with_attribution(query, filters)
            .universe
    }

    pub(super) fn build_candidate_universe_with_attribution(
        &self,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
    ) -> SearchCandidateUniverseBuild {
        let mut diagnostics = SearchExecutionDiagnostics::default();
        let mut repositories = self.config.repositories();
        let mut candidate_intake_elapsed_us = 0_u64;
        let mut freshness_validation_elapsed_us = 0_u64;
        let mut manifest_backed_repository_count = 0_usize;
        repositories.sort_by(|left, right| {
            left.repository_id
                .cmp(&right.repository_id)
                .then(left.root_path.cmp(&right.root_path))
        });

        let repositories = repositories
            .into_iter()
            .filter(|repository| {
                filters
                    .repository_id
                    .as_ref()
                    .is_none_or(|repository_id| repository_id == &repository.repository_id.0)
            })
            .map(|repository| {
                let repository_id = repository.repository_id.0;
                let root = PathBuf::from(repository.root_path);
                let (snapshot_id, mut candidates) = self
                    .manifest_candidate_files_for_repository_with_attribution(
                        &repository_id,
                        &root,
                        query,
                        filters,
                    )
                    .map(|manifest| {
                        candidate_intake_elapsed_us = candidate_intake_elapsed_us
                            .saturating_add(manifest.candidate_intake_elapsed_us);
                        freshness_validation_elapsed_us = freshness_validation_elapsed_us
                            .saturating_add(manifest.freshness_validation_elapsed_us);
                        manifest_backed_repository_count =
                            manifest_backed_repository_count.saturating_add(1);
                        (Some(manifest.snapshot_id), manifest.candidates)
                    })
                    .unwrap_or_else(|| {
                        let walk_started_at = Instant::now();
                        let walked = walk_candidate_files_for_repository(
                            &repository_id,
                            &root,
                            query,
                            filters,
                            &mut diagnostics,
                        );
                        candidate_intake_elapsed_us =
                            candidate_intake_elapsed_us.saturating_add(elapsed_us(walk_started_at));
                        (None, walked)
                    });
                let root_config_started_at = Instant::now();
                merge_candidate_files(
                    &mut candidates,
                    search_root_scoped_runtime_config_candidates_for_repository(
                        &repository_id,
                        &root,
                        query,
                        filters,
                        &mut diagnostics,
                    ),
                );
                candidate_intake_elapsed_us =
                    candidate_intake_elapsed_us.saturating_add(elapsed_us(root_config_started_at));
                let candidates = candidates
                    .into_iter()
                    .map(|(relative_path, absolute_path)| SearchCandidateFile {
                        relative_path,
                        absolute_path,
                    })
                    .collect::<Vec<_>>();
                RepositoryCandidateUniverse {
                    repository_id,
                    root,
                    snapshot_id,
                    candidates,
                }
            })
            .collect::<Vec<_>>();
        let repository_count = repositories.len();
        let candidate_count = repositories
            .iter()
            .map(|repository| repository.candidates.len())
            .sum();

        sort_search_diagnostics_deterministically(&mut diagnostics.entries);

        SearchCandidateUniverseBuild {
            universe: SearchCandidateUniverse {
                repositories,
                diagnostics,
            },
            repository_count,
            candidate_count,
            manifest_backed_repository_count,
            candidate_intake_elapsed_us,
            freshness_validation_elapsed_us,
        }
    }

    pub(super) fn candidate_universe_with_hidden_workflows(
        &self,
        candidate_universe: &SearchCandidateUniverse,
        filters: &NormalizedSearchFilters,
        intent: &HybridRankingIntent,
    ) -> SearchCandidateUniverse {
        let mut candidate_universe = candidate_universe.clone();
        for repository in &mut candidate_universe.repositories {
            let mut candidates = repository
                .candidates
                .iter()
                .map(|candidate| {
                    (
                        candidate.relative_path.clone(),
                        candidate.absolute_path.clone(),
                    )
                })
                .collect::<Vec<_>>();
            merge_candidate_files(
                &mut candidates,
                hidden_workflow_candidates_for_repository(
                    &repository.repository_id,
                    &repository.root,
                    filters,
                    intent,
                    &mut candidate_universe.diagnostics,
                ),
            );
            merge_candidate_files(
                &mut candidates,
                root_scoped_runtime_config_candidates_for_repository(
                    &repository.repository_id,
                    &repository.root,
                    filters,
                    intent,
                    &mut candidate_universe.diagnostics,
                ),
            );
            repository.candidates = candidates
                .into_iter()
                .map(|(relative_path, absolute_path)| SearchCandidateFile {
                    relative_path,
                    absolute_path,
                })
                .collect::<Vec<_>>();
        }

        sort_search_diagnostics_deterministically(&mut candidate_universe.diagnostics.entries);
        candidate_universe
    }

    fn manifest_candidate_files_for_repository_with_attribution(
        &self,
        repository_id: &str,
        root: &Path,
        query: &SearchTextQuery,
        filters: &NormalizedSearchFilters,
    ) -> Option<ManifestCandidateFilesBuild> {
        let db_path = resolve_provenance_db_path(root).ok()?;
        if !db_path.exists() {
            return None;
        }

        let storage = Storage::new(db_path);
        let freshness_started_at = Instant::now();
        let validated_snapshot = latest_validated_manifest_snapshot(
            &storage,
            repository_id,
            root,
            Some(&self.validated_manifest_candidate_cache),
        )?;
        let freshness_validation_elapsed_us = elapsed_us(freshness_started_at);
        let candidate_intake_started_at = Instant::now();
        let root_ignore_matcher = build_root_ignore_matcher(root);
        let mut candidates = Vec::new();
        for digest in validated_snapshot.digests {
            let path = digest.path;
            if should_ignore_runtime_path(root, &path, Some(&root_ignore_matcher)) {
                continue;
            }
            let rel_path = normalize_repository_relative_path(root, &path);

            if let Some(language) = filters.language {
                if !language.matches_path(&path) {
                    continue;
                }
            }
            if let Some(path_regex) = &query.path_regex {
                if !path_regex.is_match(&rel_path) {
                    continue;
                }
            }

            candidates.push((rel_path, path));
        }
        candidates.sort_by(|left, right| left.0.cmp(&right.0).then(left.1.cmp(&right.1)));
        candidates.dedup_by(|left, right| left.0 == right.0 && left.1 == right.1);
        Some(ManifestCandidateFilesBuild {
            snapshot_id: validated_snapshot.snapshot_id,
            candidates,
            candidate_intake_elapsed_us: elapsed_us(candidate_intake_started_at),
            freshness_validation_elapsed_us,
        })
    }
}
