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

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::{
        build_root_ignore_matcher, hard_excluded_runtime_path, should_ignore_runtime_path,
    };

    fn unique_root(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("frigg-{prefix}-{nanos}"))
    }

    #[test]
    fn hard_excluded_runtime_path_detects_forbidden_first_component() {
        let root = Path::new("/");

        assert!(hard_excluded_runtime_path(root, Path::new(".git/config")));
        assert!(hard_excluded_runtime_path(
            root,
            Path::new("target/obj/main.o")
        ));
        assert!(!hard_excluded_runtime_path(root, Path::new("src/main.rs")));
        assert!(!hard_excluded_runtime_path(
            root,
            Path::new(".github/workflows/ci.yml")
        ));
    }

    #[test]
    fn should_ignore_runtime_path_applies_gitignore_rules() {
        let root = unique_root("ignore-workspace");
        fs::create_dir_all(&root).expect("temporary root should be created");

        fs::write(root.join(".gitignore"), "ignored/\n").expect(".gitignore should be writable");

        let matcher = build_root_ignore_matcher(&root);
        fs::create_dir_all(root.join("ignored")).expect("ignored dir should be created");

        assert!(should_ignore_runtime_path(
            &root,
            &root.join("ignored/secret.txt"),
            Some(&matcher)
        ));
        assert!(!should_ignore_runtime_path(
            &root,
            &root.join("src/main.rs"),
            Some(&matcher)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn should_ignore_runtime_path_defaults_to_true_for_outside_root_without_matcher() {
        let root = unique_root("ignore-workspace-empty");
        let absolute_path = root.join("src/main.rs");
        let _ = fs::create_dir_all(&root);

        assert!(!should_ignore_runtime_path(&root, &absolute_path, None,));

        let _ = fs::remove_dir_all(root);
    }
}
