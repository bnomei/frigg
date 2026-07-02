//! Workspace-scoped SQLite storage path resolution and boundary checks.

use super::*;
use std::path::Component;

/// Resolves a workspace-relative write path and creates any missing parent directories.
///
/// The returned path is rooted under the canonical workspace root. Existing ancestors are checked
/// before directory creation so symlinked parent escapes cannot redirect newly-created directories
/// outside the workspace.
pub fn resolve_workspace_relative_write_path(
    workspace_root: &Path,
    relative_path: &Path,
) -> FriggResult<PathBuf> {
    validate_workspace_relative_path(relative_path)?;

    let root_canonical = canonicalize_workspace_root(workspace_root)?;
    let target_path = root_canonical.join(relative_path);
    let parent = target_path.parent().ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to determine workspace write parent directory for {}",
            target_path.display()
        ))
    })?;

    ensure_canonical_root_boundary(parent, &root_canonical, "workspace write parent path")?;
    fs::create_dir_all(parent).map_err(FriggError::Io)?;
    ensure_existing_path_inside_root(parent, &root_canonical, "workspace write parent path")?;
    ensure_canonical_root_boundary(&target_path, &root_canonical, "workspace write path")?;

    Ok(target_path)
}

/// Resolves the canonical `.frigg/storage.sqlite3` path for a workspace root.
pub fn resolve_provenance_db_path(workspace_root: &Path) -> FriggResult<PathBuf> {
    let (_root_canonical, db_path) = resolve_provenance_db_path_with_root(workspace_root)?;
    Ok(db_path)
}

/// Creates the storage parent directory when missing and verifies root boundary.
pub fn ensure_provenance_db_parent_dir(workspace_root: &Path) -> FriggResult<PathBuf> {
    let relative_db_path = Path::new(PROVENANCE_STORAGE_DIR).join(PROVENANCE_STORAGE_DB_FILE);
    resolve_workspace_relative_write_path(workspace_root, &relative_db_path)
}

fn resolve_provenance_db_path_with_root(workspace_root: &Path) -> FriggResult<(PathBuf, PathBuf)> {
    let root_canonical = canonicalize_workspace_root(workspace_root)?;
    let db_path = root_canonical
        .join(PROVENANCE_STORAGE_DIR)
        .join(PROVENANCE_STORAGE_DB_FILE);
    ensure_canonical_root_boundary(&db_path, &root_canonical, "storage path")?;
    Ok((root_canonical, db_path))
}

fn canonicalize_workspace_root(workspace_root: &Path) -> FriggResult<PathBuf> {
    workspace_root.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize workspace root {}: {err}",
            workspace_root.display()
        ))
    })
}

fn validate_workspace_relative_path(relative_path: &Path) -> FriggResult<()> {
    if relative_path.as_os_str().is_empty() {
        return Err(FriggError::AccessDenied(
            "workspace write path must not be empty".to_string(),
        ));
    }

    for component in relative_path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir => {
                return Err(FriggError::AccessDenied(format!(
                    "workspace write path contains parent traversal: {}",
                    relative_path.display()
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                return Err(FriggError::AccessDenied(format!(
                    "workspace write path must be relative: {}",
                    relative_path.display()
                )));
            }
        }
    }

    Ok(())
}

fn ensure_canonical_root_boundary(
    candidate: &Path,
    root_canonical: &Path,
    label: &str,
) -> FriggResult<()> {
    let Some(existing_ancestor) = canonicalize_existing_ancestor(candidate)? else {
        return Err(FriggError::AccessDenied(format!(
            "{label} has no canonical ancestor: {}",
            candidate.display()
        )));
    };

    if !existing_ancestor.starts_with(root_canonical) {
        return Err(FriggError::AccessDenied(format!(
            "{label} escapes canonical workspace root boundary: {}",
            candidate.display()
        )));
    }

    Ok(())
}

fn ensure_existing_path_inside_root(
    path: &Path,
    root_canonical: &Path,
    label: &str,
) -> FriggResult<()> {
    let path_canonical = path.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize {label} {}: {err}",
            path.display()
        ))
    })?;

    if !path_canonical.starts_with(root_canonical) {
        return Err(FriggError::AccessDenied(format!(
            "{label} escapes canonical workspace root boundary: {}",
            path.display()
        )));
    }

    Ok(())
}

fn canonicalize_existing_ancestor(path: &Path) -> FriggResult<Option<PathBuf>> {
    for ancestor in path.ancestors() {
        match ancestor.canonicalize() {
            Ok(canonical) => return Ok(Some(canonical)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(err) => {
                return Err(FriggError::Internal(format!(
                    "failed to canonicalize ancestor {}: {err}",
                    ancestor.display()
                )));
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_workspace(test_name: &str) -> PathBuf {
        let mut path = std::env::temp_dir();
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        path.push(format!(
            "frigg-storage-{test_name}-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("create temp workspace");
        path
    }

    #[test]
    fn storage_workspace_write_resolver_creates_valid_nested_parent() {
        let workspace = temp_workspace("valid-nested");
        let target = resolve_workspace_relative_write_path(
            &workspace,
            Path::new(".github/workflows/frigg.yml"),
        )
        .expect("resolve workspace write path");

        assert_eq!(
            target,
            workspace
                .canonicalize()
                .expect("canonical temp workspace")
                .join(".github/workflows/frigg.yml")
        );
        assert!(target.parent().expect("target parent").is_dir());

        fs::remove_dir_all(workspace).expect("remove temp workspace");
    }

    #[test]
    fn storage_workspace_write_resolver_rejects_absolute_path() {
        let workspace = temp_workspace("absolute");
        let err = resolve_workspace_relative_write_path(&workspace, Path::new("/tmp/frigg.yml"))
            .expect_err("absolute write path should fail");

        assert!(matches!(err, FriggError::AccessDenied(_)));
        fs::remove_dir_all(workspace).expect("remove temp workspace");
    }

    #[test]
    fn storage_workspace_write_resolver_rejects_parent_traversal() {
        let workspace = temp_workspace("parent-traversal");
        let err = resolve_workspace_relative_write_path(&workspace, Path::new("../frigg.yml"))
            .expect_err("parent traversal write path should fail");

        assert!(matches!(err, FriggError::AccessDenied(_)));
        fs::remove_dir_all(workspace).expect("remove temp workspace");
    }

    #[cfg(unix)]
    #[test]
    fn storage_workspace_write_resolver_rejects_symlinked_parent_escape() {
        let workspace = temp_workspace("symlink-parent");
        let outside = temp_workspace("symlink-outside");
        let fixture_targets = [
            (".github", "workflows/frigg.yml"),
            (".claude", "settings.local.json"),
            (".cursor", "rules/frigg.mdc"),
        ];

        for (symlink_dir, child_path) in fixture_targets {
            std::os::unix::fs::symlink(&outside, workspace.join(symlink_dir))
                .expect("create symlink");

            let relative_path = Path::new(symlink_dir).join(child_path);
            let err = resolve_workspace_relative_write_path(&workspace, &relative_path)
                .expect_err("symlinked parent escape should fail");

            assert!(matches!(err, FriggError::AccessDenied(_)));
            assert!(
                !outside.join(child_path).exists(),
                "resolver must not create escaped parent for {symlink_dir}"
            );
        }

        fs::remove_dir_all(workspace).expect("remove temp workspace");
        fs::remove_dir_all(outside).expect("remove outside temp dir");
    }
}
