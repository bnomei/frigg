use std::path::Path;

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use tracing::warn;

pub(crate) fn build_root_ignore_matcher(root: &Path) -> Gitignore {
    let mut builder = GitignoreBuilder::new(root);
    for ignore_path in [root.join(".gitignore"), root.join(".ignore")] {
        if !ignore_path.is_file() {
            continue;
        }
        if let Some(error) = builder.add(&ignore_path) {
            warn!(
                path = %ignore_path.display(),
                error = %error,
                "could not load workspace ignore rules"
            );
        }
    }

    builder.build().unwrap_or_else(|error| {
        warn!(
            root = %root.display(),
            error = %error,
            "could not compile workspace ignore matcher"
        );
        Gitignore::empty()
    })
}

pub(crate) fn hard_excluded_runtime_path(root: &Path, path: &Path) -> bool {
    let Some(relative) = repository_relative_runtime_path(root, path) else {
        return true;
    };
    let Some(component) = relative.components().next() else {
        return false;
    };
    matches!(
        component.as_os_str().to_string_lossy().as_ref(),
        ".frigg" | ".git" | "target"
    )
}

pub(crate) fn should_ignore_runtime_path(
    root: &Path,
    path: &Path,
    root_ignore_matcher: Option<&Gitignore>,
) -> bool {
    if hard_excluded_runtime_path(root, path) {
        return true;
    }
    let Some(root_ignore_matcher) = root_ignore_matcher else {
        return false;
    };
    let Some(relative) = repository_relative_runtime_path(root, path) else {
        return true;
    };
    root_ignore_matcher
        .matched_path_or_any_parents(relative, false)
        .is_ignore()
}

fn repository_relative_runtime_path<'a>(root: &'a Path, path: &'a Path) -> Option<&'a Path> {
    if path.is_absolute() {
        path.strip_prefix(root).ok()
    } else {
        Some(path)
    }
}
