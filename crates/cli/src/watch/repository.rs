use super::*;

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

pub(super) fn build_root_ignore_matcher(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    for ignore_path in [root.join(".gitignore"), root.join(".ignore")] {
        if !ignore_path.is_file() {
            continue;
        }
        if let Some(error) = builder.add(&ignore_path) {
            warn!(
                path = %ignore_path.display(),
                error = %error,
                "built-in watch mode could not load ignore rules"
            );
        }
    }

    builder.build().unwrap_or_else(|error| {
        warn!(
            root = %root.display(),
            error = %error,
            "built-in watch mode could not compile ignore matcher"
        );
        Gitignore::empty()
    })
}

#[cfg(test)]
pub(super) fn latest_manifest_is_valid(repository: &WatchedRepository) -> FriggResult<bool> {
    let storage = Storage::new(&repository.db_path);
    let latest = storage.load_latest_manifest_for_repository(&repository.repository_id)?;
    let Some(snapshot) = latest else {
        return Ok(false);
    };
    let digests = snapshot
        .entries
        .iter()
        .map(|entry| FileMetadataDigest {
            path: PathBuf::from(&entry.path),
            size_bytes: entry.size_bytes,
            mtime_ns: entry.mtime_ns,
        })
        .collect::<Vec<_>>();
    Ok(validate_manifest_digests_for_root(&repository.root, &digests).is_some())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StartupRefreshStatus {
    pub should_refresh: bool,
    pub reason: &'static str,
    pub snapshot_id: Option<String>,
}

pub(super) fn startup_refresh_status(
    repository: &WatchedRepository,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
) -> FriggResult<StartupRefreshStatus> {
    let storage = Storage::new(&repository.db_path);
    let latest = storage.load_latest_manifest_for_repository(&repository.repository_id)?;
    let Some(snapshot) = latest else {
        return Ok(StartupRefreshStatus {
            should_refresh: true,
            reason: "missing_manifest_snapshot",
            snapshot_id: None,
        });
    };
    let snapshot_id = snapshot.snapshot_id.clone();

    let digests = snapshot
        .entries
        .iter()
        .map(|entry| FileMetadataDigest {
            path: PathBuf::from(&entry.path),
            size_bytes: entry.size_bytes,
            mtime_ns: entry.mtime_ns,
        })
        .collect::<Vec<_>>();
    if validate_manifest_digests_for_root(&repository.root, &digests).is_none() {
        return Ok(StartupRefreshStatus {
            should_refresh: true,
            reason: "stale_manifest_snapshot",
            snapshot_id: Some(snapshot_id),
        });
    }

    if !semantic_runtime.enabled {
        return Ok(StartupRefreshStatus {
            should_refresh: false,
            reason: "manifest_valid",
            snapshot_id: Some(snapshot_id),
        });
    }

    semantic_runtime
        .validate_startup(credentials)
        .map_err(|err| FriggError::InvalidInput(format!("{err}")))?;
    let provider = semantic_runtime.provider.ok_or_else(|| {
        FriggError::Internal("semantic runtime provider missing after validation".to_owned())
    })?;
    let model = semantic_runtime.normalized_model().ok_or_else(|| {
        FriggError::Internal("semantic runtime model missing after validation".to_owned())
    })?;

    let has_semantic_eligible_entries = snapshot.entries.iter().any(|entry| {
        let path = PathBuf::from(&entry.path);
        !should_ignore_watch_path(repository, &path)
            && semantic_chunk_language_for_path(Path::new(&entry.path)).is_some()
    });
    if !has_semantic_eligible_entries {
        return Ok(StartupRefreshStatus {
            should_refresh: false,
            reason: "manifest_valid_no_semantic_eligible_entries",
            snapshot_id: Some(snapshot_id),
        });
    }

    let has_rows = storage.has_semantic_embeddings_for_repository_snapshot_model(
        &repository.repository_id,
        &snapshot.snapshot_id,
        provider.as_str(),
        model,
    )?;
    Ok(StartupRefreshStatus {
        should_refresh: !has_rows,
        reason: if has_rows {
            "manifest_and_semantic_snapshot_valid"
        } else {
            "semantic_snapshot_missing_for_active_model"
        },
        snapshot_id: Some(snapshot.snapshot_id),
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
