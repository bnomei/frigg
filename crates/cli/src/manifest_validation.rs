use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::indexer::FileMetadataDigest;

pub(crate) fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
    system_time
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
}

pub(crate) fn validate_manifest_digests_for_root(
    root: &Path,
    file_digests: &[FileMetadataDigest],
) -> Option<Vec<FileMetadataDigest>> {
    let mut validated = Vec::with_capacity(file_digests.len());
    for digest in file_digests {
        let path = if digest.path.is_absolute() {
            digest.path.clone()
        } else {
            root.join(&digest.path)
        };
        if !path.starts_with(root) {
            return None;
        }

        let metadata = fs::metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() != digest.size_bytes {
            return None;
        }

        let mtime_ns = metadata.modified().ok().and_then(system_time_to_unix_nanos);
        if mtime_ns != digest.mtime_ns {
            return None;
        }

        validated.push(FileMetadataDigest {
            path,
            size_bytes: metadata.len(),
            mtime_ns,
        });
    }

    Some(validated)
}
