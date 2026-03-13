use std::path::{Path, PathBuf};

use ignore::gitignore::Gitignore;
use notify::EventKind;

use crate::domain::{FriggError, FriggResult};
use crate::settings::{FriggConfig, SemanticRuntimeConfig, SemanticRuntimeCredentials};
use crate::storage::{Storage, resolve_provenance_db_path};

use super::scheduler::WatchRefreshClass;
use crate::workspace_ignores::build_root_ignore_matcher;

#[derive(Debug, Clone)]
pub(super) struct WatchedRepository {
    pub repository_id: String,
    pub root: PathBuf,
    pub canonical_root: Option<PathBuf>,
    pub root_ignore_matcher: Gitignore,
    pub db_path: PathBuf,
}

pub(super) fn build_watched_repositories(
    config: &FriggConfig,
) -> FriggResult<Vec<WatchedRepository>> {
    config
        .repositories()
        .into_iter()
        .map(|repository| {
            let root = PathBuf::from(&repository.root_path);
            let db_path = resolve_provenance_db_path(&root)?;
            Ok(WatchedRepository {
                repository_id: repository.repository_id.0,
                canonical_root: root.canonicalize().ok(),
                root_ignore_matcher: build_root_ignore_matcher(&root),
                root,
                db_path,
            })
        })
        .collect()
}

#[cfg(test)]
pub(super) fn latest_manifest_is_valid(repository: &WatchedRepository) -> FriggResult<bool> {
    let storage = Storage::new(&repository.db_path);
    let latest = storage.load_latest_manifest_for_repository(&repository.repository_id)?;
    let Some(snapshot) = latest else {
        return Ok(false);
    };
    Ok(
        crate::manifest_validation::validate_manifest_snapshot_for_root(
            &repository.root,
            &snapshot,
        )
        .is_some(),
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StartupRefreshStatus {
    pub should_refresh: bool,
    pub reason: &'static str,
    pub snapshot_id: Option<String>,
    pub refresh_class: Option<WatchRefreshClass>,
}

pub(super) fn startup_refresh_status(
    repository: &WatchedRepository,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
) -> FriggResult<StartupRefreshStatus> {
    semantic_runtime
        .validate_startup(credentials)
        .map_err(|err| FriggError::InvalidInput(format!("{err}")))?;
    let storage = Storage::new(&repository.db_path);
    let freshness = crate::manifest_validation::repository_freshness_status(
        &storage,
        &repository.repository_id,
        &repository.root,
        semantic_runtime,
        |path| should_ignore_watch_path(repository, path),
    )?;
    Ok(StartupRefreshStatus {
        should_refresh: freshness.should_refresh_watch(),
        reason: freshness.watch_reason(),
        snapshot_id: freshness.snapshot_id,
        refresh_class: match freshness.manifest {
            crate::manifest_validation::RepositoryManifestFreshness::MissingSnapshot
            | crate::manifest_validation::RepositoryManifestFreshness::StaleSnapshot => {
                Some(WatchRefreshClass::ManifestFast)
            }
            crate::manifest_validation::RepositoryManifestFreshness::Ready => matches!(
                freshness.semantic,
                crate::manifest_validation::RepositorySemanticFreshness::MissingForActiveModel
            )
            .then_some(WatchRefreshClass::SemanticFollowup),
        },
    })
}

pub(super) fn event_kind_is_relevant(kind: &EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

pub(super) fn repository_index_for_path(
    repositories: &[WatchedRepository],
    path: &Path,
) -> Option<usize> {
    repositories
        .iter()
        .enumerate()
        .filter(|(_, repository)| repository_relative_watch_path(repository, path).is_some())
        .max_by_key(|(_, repository)| repository.root.components().count())
        .map(|(idx, _)| idx)
}

pub(super) fn repository_relative_watch_path<'a>(
    repository: &'a WatchedRepository,
    path: &'a Path,
) -> Option<PathBuf> {
    if !path.is_absolute() {
        return Some(path.to_path_buf());
    }

    if let Ok(relative) = path.strip_prefix(&repository.root) {
        return Some(relative.to_path_buf());
    }

    if let Some(canonical_root) = repository.canonical_root.as_deref() {
        if let Ok(relative) = path.strip_prefix(canonical_root) {
            return Some(relative.to_path_buf());
        }
        if let Ok(canonical_path) = path.canonicalize() {
            if let Ok(relative) = canonical_path.strip_prefix(canonical_root) {
                return Some(relative.to_path_buf());
            }
        }
    }

    None
}

pub(super) fn should_ignore_watch_path(repository: &WatchedRepository, path: &Path) -> bool {
    let Some(relative) = repository_relative_watch_path(repository, path) else {
        return true;
    };
    let Some(component) = relative.components().next() else {
        return false;
    };
    let component = component.as_os_str().to_string_lossy();
    if matches!(component.as_ref(), ".frigg" | ".git" | "target") {
        return true;
    }

    repository
        .root_ignore_matcher
        .matched_path_or_any_parents(&relative, path.is_dir())
        .is_ignore()
}
