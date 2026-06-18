use super::*;
use crate::storage::ManifestEntry;
use crate::workspace_ignores::{
    build_root_ignore_matcher, hard_excluded_runtime_path, should_ignore_runtime_path,
};
use ignore::WalkState;
use std::sync::{Arc, Mutex};

impl ManifestBuilder {
    pub fn build(&self, root: &Path) -> FriggResult<Vec<FileDigest>> {
        if !root.exists() {
            return Err(FriggError::InvalidInput(format!(
                "index root does not exist: {}",
                root.display()
            )));
        }

        let (paths, _diagnostics) = collect_manifest_walk_paths(root, self.follow_symlinks);
        let mut out = Vec::new();

        for path in paths {
            let mtime_ns = path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_to_unix_nanos);
            let (size_bytes, digest) = stream_file_blake3_digest(&path).map_err(FriggError::Io)?;

            out.push(FileDigest {
                path,
                size_bytes,
                mtime_ns,
                hash_blake3_hex: digest,
            });
        }
        out.sort_by(file_digest_order);
        out.dedup_by(|left, right| left.path == right.path);

        Ok(out)
    }

    pub fn build_with_diagnostics(&self, root: &Path) -> FriggResult<ManifestBuildOutput> {
        if !root.exists() {
            return Err(FriggError::InvalidInput(format!(
                "index root does not exist: {}",
                root.display()
            )));
        }

        let (paths, mut diagnostics) = collect_manifest_walk_paths(root, self.follow_symlinks);
        let mut entries = Vec::new();

        for path in paths {
            let mtime_ns = path
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(system_time_to_unix_nanos);
            let (size_bytes, digest) = match stream_file_blake3_digest(&path) {
                Ok(result) => result,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: Some(path),
                        kind: ManifestDiagnosticKind::Read,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            entries.push(FileDigest {
                path,
                size_bytes,
                mtime_ns,
                hash_blake3_hex: digest,
            });
        }
        entries.sort_by(file_digest_order);
        entries.dedup_by(|left, right| left.path == right.path);
        diagnostics.sort_by(manifest_build_diagnostic_order);

        Ok(ManifestBuildOutput {
            entries,
            diagnostics,
        })
    }

    pub fn build_metadata_with_diagnostics(
        &self,
        root: &Path,
    ) -> FriggResult<ManifestMetadataBuildOutput> {
        if !root.exists() {
            return Err(FriggError::InvalidInput(format!(
                "index root does not exist: {}",
                root.display()
            )));
        }

        let (paths, mut diagnostics) = collect_manifest_walk_paths(root, self.follow_symlinks);
        let mut entries = Vec::new();

        for path in paths {
            let metadata = match path.metadata() {
                Ok(metadata) => metadata,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: Some(path),
                        kind: ManifestDiagnosticKind::Read,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            let mtime_ns = metadata.modified().ok().and_then(system_time_to_unix_nanos);
            entries.push(FileMetadataDigest {
                path,
                size_bytes: metadata.len(),
                mtime_ns,
            });
        }
        entries.sort_by(file_metadata_digest_order);
        entries.dedup_by(|left, right| left.path == right.path);
        diagnostics.sort_by(manifest_build_diagnostic_order);

        Ok(ManifestMetadataBuildOutput {
            entries,
            diagnostics,
        })
    }

    pub fn build_changed_only_with_diagnostics(
        &self,
        root: &Path,
        previous_entries: &[FileDigest],
    ) -> FriggResult<ManifestBuildOutput> {
        self.build_changed_only_with_hints_and_diagnostics(root, previous_entries, &[])
    }

    pub fn build_changed_only_with_hints_and_diagnostics(
        &self,
        root: &Path,
        previous_entries: &[FileDigest],
        dirty_path_hints: &[PathBuf],
    ) -> FriggResult<ManifestBuildOutput> {
        let metadata_output = self.build_metadata_with_diagnostics(root)?;
        let previous_by_path = manifest_by_path(previous_entries);
        let hinted_paths = dirty_path_hints
            .iter()
            .filter_map(|path| normalize_dirty_hint_path(root, path))
            .collect::<BTreeSet<_>>();
        let mut entries = Vec::with_capacity(metadata_output.entries.len());
        let mut diagnostics = metadata_output.diagnostics;

        for metadata in metadata_output.entries {
            let is_hinted = hinted_paths.contains(&metadata.path);
            if let Some(previous) = previous_by_path.get(&metadata.path) {
                if !is_hinted && metadata_matches_previous_digest(&metadata, previous) {
                    entries.push(previous.clone());
                    continue;
                }
            }

            let (size_bytes, digest) = match stream_file_blake3_digest(&metadata.path) {
                Ok(result) => result,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: Some(metadata.path),
                        kind: ManifestDiagnosticKind::Read,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            entries.push(FileDigest {
                path: metadata.path,
                size_bytes,
                mtime_ns: metadata.mtime_ns,
                hash_blake3_hex: digest,
            });
        }

        entries.sort_by(file_digest_order);
        entries.dedup_by(|left, right| left.path == right.path);
        diagnostics.sort_by(manifest_build_diagnostic_order);

        Ok(ManifestBuildOutput {
            entries,
            diagnostics,
        })
    }
}

fn collect_manifest_walk_paths(
    root: &Path,
    follow_symlinks: bool,
) -> (Vec<PathBuf>, Vec<ManifestBuildDiagnostic>) {
    let root = Arc::new(root.to_path_buf());
    let root_ignore_matcher = Arc::new(build_root_ignore_matcher(root.as_ref()));
    let paths = Arc::new(Mutex::new(Vec::new()));
    let diagnostics = Arc::new(Mutex::new(Vec::new()));

    frigg_walk_builder(root.as_ref(), follow_symlinks)
        .build_parallel()
        .run(|| {
            let root = Arc::clone(&root);
            let root_ignore_matcher = Arc::clone(&root_ignore_matcher);
            let paths = Arc::clone(&paths);
            let diagnostics = Arc::clone(&diagnostics);
            Box::new(move |dent| {
                match dent {
                    Ok(entry) => {
                        if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                            return WalkState::Continue;
                        }

                        let path = entry.path().to_path_buf();
                        if should_ignore_runtime_path(
                            root.as_ref(),
                            &path,
                            Some(root_ignore_matcher.as_ref()),
                        ) {
                            return WalkState::Continue;
                        }
                        paths
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push(path);
                    }
                    Err(err) => {
                        diagnostics
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner())
                            .push(ManifestBuildDiagnostic {
                                path: None,
                                kind: ManifestDiagnosticKind::Walk,
                                message: err.to_string(),
                            });
                    }
                }
                WalkState::Continue
            })
        });

    let mut paths = paths
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    let mut diagnostics = diagnostics
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .clone();
    paths.sort();
    paths.dedup();
    diagnostics.sort_by(manifest_build_diagnostic_order);
    (paths, diagnostics)
}

pub(super) fn normalize_repository_relative_path(
    workspace_root: &Path,
    path: &Path,
) -> FriggResult<String> {
    if let Some(relative) = repository_relative_path_string(workspace_root, path) {
        return Ok(relative);
    }

    let root_canonical = workspace_root.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize semantic workspace root '{}': {err}",
            workspace_root.display()
        ))
    })?;
    if let Some(relative) = repository_relative_path_string(&root_canonical, path) {
        return Ok(relative);
    }

    if path.is_relative()
        && let Some(relative) = repository_relative_path_string_from_relative(path)
    {
        return Ok(relative);
    }

    let path_canonical = path.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize semantic source path '{}': {err}",
            path.display()
        ))
    })?;
    repository_relative_path_string(&root_canonical, &path_canonical).ok_or_else(|| {
        FriggError::Internal(format!(
            "semantic chunk path '{}' escapes workspace root '{}'",
            path.display(),
            workspace_root.display()
        ))
    })
}

pub(super) fn normalize_deleted_repository_relative_path(
    workspace_root: &Path,
    path: &Path,
) -> FriggResult<Option<String>> {
    if let Some(relative) = repository_relative_path_string(workspace_root, path) {
        return Ok(Some(relative));
    }

    let root_canonical = workspace_root.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize semantic workspace root '{}': {err}",
            workspace_root.display()
        ))
    })?;
    if let Some(relative) = repository_relative_path_string(&root_canonical, path) {
        return Ok(Some(relative));
    }

    if path.is_relative()
        && let Some(relative) = repository_relative_path_string_from_relative(path)
    {
        return Ok(Some(relative));
    }

    let path_canonical = match path.canonicalize() {
        Ok(path_canonical) => path_canonical,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(FriggError::Internal(format!(
                "failed to canonicalize semantic source path '{}': {err}",
                path.display()
            )));
        }
    };

    Ok(repository_relative_path_string(
        &root_canonical,
        &path_canonical,
    ))
}

fn repository_relative_path_string(base: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(base).ok()?;
    repository_relative_path_string_from_relative(relative)
}

fn repository_relative_path_string_from_relative(relative: &Path) -> Option<String> {
    let mut normalized = PathBuf::new();
    for component in relative.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::Normal(part) => normalized.push(part),
            std::path::Component::ParentDir
            | std::path::Component::RootDir
            | std::path::Component::Prefix(_) => return None,
        }
    }
    Some(normalized.to_string_lossy().replace('\\', "/"))
}

fn frigg_walk_builder(root: &Path, follow_symlinks: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .standard_filters(true)
        .require_git(false)
        .follow_links(follow_symlinks);
    builder
}

fn stream_file_blake3_digest(path: &Path) -> std::io::Result<(u64, String)> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Hasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut total_bytes = 0_u64;

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
        total_bytes = total_bytes.saturating_add(bytes_read as u64);
    }

    Ok((total_bytes, hasher.finalize().to_hex().to_string()))
}

pub fn diff(old: &[FileDigest], new: &[FileDigest]) -> ManifestDiff {
    let old_by_path = manifest_by_path(old);
    let new_by_path = manifest_by_path(new);

    let mut added = Vec::new();
    let mut modified = Vec::new();
    let mut deleted = Vec::new();

    for (path, new_entry) in &new_by_path {
        match old_by_path.get(path) {
            None => added.push(new_entry.clone()),
            Some(old_entry) if !same_manifest_record(old_entry, new_entry) => {
                modified.push(new_entry.clone())
            }
            Some(_) => {}
        }
    }

    for (path, old_entry) in &old_by_path {
        if !new_by_path.contains_key(path) {
            deleted.push(old_entry.clone());
        }
    }

    ManifestDiff {
        added,
        modified,
        deleted,
    }
}

pub(super) fn file_digest_to_manifest_entry(entry: &FileDigest) -> ManifestEntry {
    ManifestEntry {
        path: entry.path.to_string_lossy().to_string(),
        sha256: entry.hash_blake3_hex.clone(),
        size_bytes: entry.size_bytes,
        mtime_ns: entry.mtime_ns,
    }
}

pub(super) fn manifest_entry_to_file_digest(entry: ManifestEntry) -> FileDigest {
    FileDigest {
        path: PathBuf::from(entry.path),
        size_bytes: entry.size_bytes,
        mtime_ns: entry.mtime_ns,
        hash_blake3_hex: entry.sha256,
    }
}

pub(super) fn deterministic_snapshot_id(repository_id: &str, entries: &[FileDigest]) -> String {
    let mut ordered = entries.to_vec();
    ordered.sort_by(file_digest_order);

    let mut hasher = Hasher::new();
    hasher.update(repository_id.as_bytes());
    hasher.update(&[0]);

    for entry in ordered {
        hasher.update(entry.path.to_string_lossy().as_bytes());
        hasher.update(&[0]);
        hasher.update(entry.size_bytes.to_string().as_bytes());
        hasher.update(&[0]);
        match entry.mtime_ns {
            Some(mtime_ns) => {
                hasher.update(b"1");
                hasher.update(mtime_ns.to_string().as_bytes());
            }
            None => {
                hasher.update(b"0");
            }
        }
        hasher.update(&[0]);
        hasher.update(entry.hash_blake3_hex.as_bytes());
        hasher.update(&[0]);
    }

    format!("snapshot-{}", hasher.finalize().to_hex())
}

fn same_manifest_record(left: &FileDigest, right: &FileDigest) -> bool {
    left.size_bytes == right.size_bytes
        && left.mtime_ns == right.mtime_ns
        && left.hash_blake3_hex == right.hash_blake3_hex
}

fn metadata_matches_previous_digest(left: &FileMetadataDigest, right: &FileDigest) -> bool {
    left.path == right.path
        && left.size_bytes == right.size_bytes
        && left.mtime_ns == right.mtime_ns
}

fn normalize_dirty_hint_path(root: &Path, path: &Path) -> Option<PathBuf> {
    let normalized = if path.is_absolute() {
        path.strip_prefix(root)
            .ok()
            .map(|relative| root.join(relative))?
    } else {
        root.join(path)
    };
    (!hard_excluded_runtime_path(root, &normalized)).then_some(normalized)
}

fn manifest_by_path(entries: &[FileDigest]) -> BTreeMap<PathBuf, FileDigest> {
    let mut by_path = BTreeMap::new();
    for entry in entries {
        by_path.insert(entry.path.clone(), entry.clone());
    }

    by_path
}

pub(crate) fn file_digest_order(left: &FileDigest, right: &FileDigest) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.size_bytes.cmp(&right.size_bytes))
        .then(left.mtime_ns.cmp(&right.mtime_ns))
        .then(left.hash_blake3_hex.cmp(&right.hash_blake3_hex))
}

fn file_metadata_digest_order(
    left: &FileMetadataDigest,
    right: &FileMetadataDigest,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.size_bytes.cmp(&right.size_bytes))
        .then(left.mtime_ns.cmp(&right.mtime_ns))
}

fn manifest_build_diagnostic_order(
    left: &ManifestBuildDiagnostic,
    right: &ManifestBuildDiagnostic,
) -> std::cmp::Ordering {
    left.path
        .cmp(&right.path)
        .then(left.kind.cmp(&right.kind))
        .then(left.message.cmp(&right.message))
}

fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
    system_time
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_repository_relative_path_handles_deleted_absolute_path_under_relative_root()
    -> FriggResult<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let missing_name = format!(
            ".frigg-missing-semantic-source-{nonce}-{}",
            std::process::id()
        );
        let absolute_missing = std::env::current_dir()
            .map_err(FriggError::Io)?
            .join(&missing_name);
        assert!(
            !absolute_missing.exists(),
            "test fixture path must not exist: {}",
            absolute_missing.display()
        );

        let normalized = normalize_repository_relative_path(Path::new("."), &absolute_missing)?;

        assert_eq!(normalized, missing_name);
        Ok(())
    }

    #[test]
    fn normalize_deleted_repository_relative_path_skips_missing_path_outside_root()
    -> FriggResult<()> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::current_dir().map_err(FriggError::Io)?;
        let missing_outside_root =
            std::env::temp_dir().join(format!("frigg-stale-deleted-source-{nonce}"));
        assert!(
            !missing_outside_root.exists(),
            "test fixture path must not exist: {}",
            missing_outside_root.display()
        );

        let normalized = normalize_deleted_repository_relative_path(&root, &missing_outside_root)?;

        assert_eq!(normalized, None);
        Ok(())
    }
}
