use std::io;
use std::path::{Path, PathBuf};

use frigg::storage::{ensure_provenance_db_parent_dir, resolve_provenance_db_path};

#[cfg(test)]
pub(crate) fn find_enclosing_git_root(start: &Path) -> Option<PathBuf> {
    start.ancestors().find_map(|ancestor| {
        ancestor
            .join(".git")
            .exists()
            .then(|| ancestor.to_path_buf())
    })
}

pub(crate) fn resolve_storage_db_path(
    workspace_root: &Path,
    command_name: &str,
) -> io::Result<PathBuf> {
    resolve_provenance_db_path(workspace_root).map_err(|err| {
        io::Error::other(format!(
            "{command_name} summary status=failed root={} error={err}",
            workspace_root.display()
        ))
    })
}

pub(crate) fn ensure_storage_db_path_for_write(
    workspace_root: &Path,
    command_name: &str,
) -> io::Result<PathBuf> {
    ensure_provenance_db_parent_dir(workspace_root).map_err(|err| {
        io::Error::other(format!(
            "{command_name} summary status=failed root={} error={err}",
            workspace_root.display()
        ))
    })
}
