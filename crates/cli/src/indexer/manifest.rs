use super::*;
use crate::workspace_ignores::{
    build_root_ignore_matcher, hard_excluded_runtime_path, should_ignore_runtime_path,
};

impl ManifestBuilder {
    pub fn build(&self, root: &Path) -> FriggResult<Vec<FileDigest>> {
        if !root.exists() {
            return Err(FriggError::InvalidInput(format!(
                "index root does not exist: {}",
                root.display()
            )));
        }

        let mut out = Vec::new();
        let root_ignore_matcher = build_root_ignore_matcher(root);
        let walker = frigg_walk_builder(root, self.follow_symlinks).build();

        for dent in walker {
            let dent = match dent {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            if !dent.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = dent.path().to_path_buf();
            if should_ignore_runtime_path(root, &path, Some(&root_ignore_matcher)) {
                continue;
            }
            let mtime_ns = dent
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

        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        let root_ignore_matcher = build_root_ignore_matcher(root);
        let walker = frigg_walk_builder(root, self.follow_symlinks).build();

        for dent in walker {
            let dent = match dent {
                Ok(entry) => entry,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: None,
                        kind: ManifestDiagnosticKind::Walk,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            if !dent.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = dent.path().to_path_buf();
            if should_ignore_runtime_path(root, &path, Some(&root_ignore_matcher)) {
                continue;
            }
            let mtime_ns = dent
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

        let mut entries = Vec::new();
        let mut diagnostics = Vec::new();
        let root_ignore_matcher = build_root_ignore_matcher(root);
        let walker = frigg_walk_builder(root, self.follow_symlinks).build();

        for dent in walker {
            let dent = match dent {
                Ok(entry) => entry,
                Err(err) => {
                    diagnostics.push(ManifestBuildDiagnostic {
                        path: None,
                        kind: ManifestDiagnosticKind::Walk,
                        message: err.to_string(),
                    });
                    continue;
                }
            };
            if !dent.file_type().is_some_and(|ft| ft.is_file()) {
                continue;
            }

            let path = dent.path().to_path_buf();
            if should_ignore_runtime_path(root, &path, Some(&root_ignore_matcher)) {
                continue;
            }
            let metadata = match dent.metadata() {
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

pub(super) fn normalize_repository_relative_path(
    workspace_root: &Path,
    path: &Path,
) -> FriggResult<String> {
    if let Ok(relative) = path.strip_prefix(workspace_root) {
        return Ok(relative.to_string_lossy().replace('\\', "/"));
    }

    let root_canonical = workspace_root.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize semantic workspace root '{}': {err}",
            workspace_root.display()
        ))
    })?;
    let path_canonical = path.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize semantic source path '{}': {err}",
            path.display()
        ))
    })?;
    let relative = path_canonical
        .strip_prefix(&root_canonical)
        .map_err(|err| {
            FriggError::Internal(format!(
                "semantic chunk path '{}' escapes workspace root '{}': {err}",
                path.display(),
                workspace_root.display()
            ))
        })?;
    Ok(relative.to_string_lossy().replace('\\', "/"))
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
