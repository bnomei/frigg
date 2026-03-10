use super::*;

pub fn resolve_provenance_db_path(workspace_root: &Path) -> FriggResult<PathBuf> {
    let (_root_canonical, db_path) = resolve_provenance_db_path_with_root(workspace_root)?;
    Ok(db_path)
}

pub fn ensure_provenance_db_parent_dir(workspace_root: &Path) -> FriggResult<PathBuf> {
    let (root_canonical, db_path) = resolve_provenance_db_path_with_root(workspace_root)?;
    let parent = db_path.parent().ok_or_else(|| {
        FriggError::Internal(format!(
            "failed to determine provenance storage parent directory for {}",
            db_path.display()
        ))
    })?;

    fs::create_dir_all(parent).map_err(FriggError::Io)?;
    ensure_canonical_root_boundary(&db_path, &root_canonical)?;

    Ok(db_path)
}

fn resolve_provenance_db_path_with_root(workspace_root: &Path) -> FriggResult<(PathBuf, PathBuf)> {
    let root_canonical = workspace_root.canonicalize().map_err(|err| {
        FriggError::Internal(format!(
            "failed to canonicalize workspace root {}: {err}",
            workspace_root.display()
        ))
    })?;
    let db_path = root_canonical
        .join(PROVENANCE_STORAGE_DIR)
        .join(PROVENANCE_STORAGE_DB_FILE);
    ensure_canonical_root_boundary(&db_path, &root_canonical)?;
    Ok((root_canonical, db_path))
}

fn ensure_canonical_root_boundary(candidate: &Path, root_canonical: &Path) -> FriggResult<()> {
    let Some(existing_ancestor) = canonicalize_existing_ancestor(candidate)? else {
        return Err(FriggError::AccessDenied(format!(
            "provenance storage path has no canonical ancestor: {}",
            candidate.display()
        )));
    };

    if !existing_ancestor.starts_with(root_canonical) {
        return Err(FriggError::AccessDenied(format!(
            "provenance storage path escapes canonical workspace root boundary: {}",
            candidate.display()
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
