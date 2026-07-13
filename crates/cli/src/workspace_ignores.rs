//! Workspace ignore matching shared by manifest walks and runtime path filtering.
//!
//! Evaluates workspace `.gitignore` and `.ignore` files at every directory level so runtime
//! candidates and filesystem watching honor the same nested project rules as manifest walks.

use std::path::{Path, PathBuf};

use ignore::Match;
use ignore::gitignore::GitignoreBuilder;
use tracing::warn;

/// A workspace-scoped matcher for nested project ignore rules.
#[derive(Debug, Clone)]
pub(crate) struct WorkspaceIgnoreMatcher {
    root: PathBuf,
}

/// Build a workspace ignore matcher. Rules are resolved when a path is checked so edits to nested
/// ignore files take effect immediately without keeping a stale root-only matcher.
pub(crate) fn build_root_ignore_matcher(root: &Path) -> WorkspaceIgnoreMatcher {
    WorkspaceIgnoreMatcher {
        root: root.to_path_buf(),
    }
}

impl WorkspaceIgnoreMatcher {
    /// Returns whether the path is excluded by the workspace ignore hierarchy.
    ///
    /// Paths are matched even after deletion so ignored generated files do not spuriously dirty a
    /// watched repository.
    fn is_ignored(&self, path: &Path) -> bool {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        let Ok(relative) = path.strip_prefix(&self.root) else {
            return false;
        };
        let Some(parent) = relative.parent() else {
            return false;
        };

        let mut decision = None;
        let mut directory = self.root.clone();
        for component in parent.components() {
            directory.push(component.as_os_str());
            decision = match_ignore_rules(&directory, &path, path.is_dir()).or(decision);
        }
        decision.or(match_ignore_rules(&self.root, &path, path.is_dir())) == Some(true)
    }
}

/// Returns the last rule result from one directory, where `.ignore` takes precedence over the
/// sibling `.gitignore` as it does in the `ignore` crate's standard walkers.
fn match_ignore_rules(directory: &Path, path: &Path, is_dir: bool) -> Option<bool> {
    let mut decision = None;
    for filename in [".gitignore", ".ignore"] {
        let ignore_path = directory.join(filename);
        if !ignore_path.is_file() {
            continue;
        }
        let mut builder = GitignoreBuilder::new(directory);
        if let Some(error) = builder.add(&ignore_path) {
            warn!(path = %ignore_path.display(), error = %error, "could not load workspace ignore rules");
        }
        let Ok(matcher) = builder.build() else {
            warn!(path = %ignore_path.display(), "could not compile workspace ignore rules");
            continue;
        };
        decision = match matcher.matched_path_or_any_parents(path, is_dir) {
            Match::Ignore(_) => Some(true),
            Match::Whitelist(_) => Some(false),
            Match::None => decision,
        };
    }
    decision
}

/// True for paths under hard-excluded roots (`.frigg`, `.git`, `target`) regardless of gitignore.
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

/// True when path is hard-excluded or matched by the workspace ignore hierarchy.
pub(crate) fn should_ignore_runtime_path(
    root: &Path,
    path: &Path,
    root_ignore_matcher: Option<&WorkspaceIgnoreMatcher>,
) -> bool {
    if hard_excluded_runtime_path(root, path) {
        return true;
    }
    let Some(root_ignore_matcher) = root_ignore_matcher else {
        return false;
    };
    if repository_relative_runtime_path(root, path).is_none() {
        return true;
    }
    root_ignore_matcher.is_ignored(path)
}

/// Ignore-rule files must always reach the watcher so a changed rule can reconcile the manifest.
pub(crate) fn is_ignore_rule_file(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".gitignore" | ".ignore")
    )
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
    fn should_ignore_runtime_path_applies_nested_gitignore_rules() {
        let root = unique_root("nested-ignore-workspace");
        let views = root.join("storage/framework/views");
        fs::create_dir_all(&views).expect("nested views directory should exist");
        fs::write(views.join(".gitignore"), "*\n!.gitignore\n")
            .expect("nested ignore file should be writable");
        fs::write(views.join("compiled.php"), "<?php").expect("compiled view should be writable");
        fs::create_dir_all(root.join("app")).expect("source directory should exist");
        fs::write(root.join("app/Controller.php"), "<?php")
            .expect("source file should be writable");

        let matcher = build_root_ignore_matcher(&root);
        assert!(should_ignore_runtime_path(
            &root,
            &views.join("compiled.php"),
            Some(&matcher)
        ));
        assert!(!should_ignore_runtime_path(
            &root,
            &root.join("app/Controller.php"),
            Some(&matcher)
        ));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn nested_gitignore_rule_overrides_a_matching_root_rule() {
        let root = unique_root("nested-ignore-precedence");
        let storage = root.join("storage");
        fs::create_dir_all(&storage).expect("nested storage directory should exist");
        fs::write(root.join(".gitignore"), "*.php\n").expect("root ignore should be writable");
        fs::write(storage.join(".gitignore"), "!keep.php\n")
            .expect("nested ignore should be writable");
        fs::write(storage.join("keep.php"), "<?php").expect("kept PHP file should be writable");
        fs::write(storage.join("discard.php"), "<?php")
            .expect("discarded PHP file should be writable");

        let matcher = build_root_ignore_matcher(&root);
        assert!(!should_ignore_runtime_path(
            &root,
            &storage.join("keep.php"),
            Some(&matcher)
        ));
        assert!(should_ignore_runtime_path(
            &root,
            &storage.join("discard.php"),
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
