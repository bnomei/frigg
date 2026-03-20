use super::*;

#[cfg(test)]
static TEST_PRECISE_GENERATOR_BIN_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

impl FriggMcpServer {
    pub(in crate::mcp::server) fn load_workspace_precise_config(
        root: &Path,
    ) -> WorkspacePreciseConfig {
        let config_path = root.join(PRECISE_WORKSPACE_CONFIG_FILE);
        let Ok(raw) = fs::read_to_string(&config_path) else {
            return WorkspacePreciseConfig::default();
        };

        match serde_json::from_str::<WorkspacePreciseConfigFile>(&raw) {
            Ok(config) => config.precise,
            Err(error) => {
                warn!(
                    path = %config_path.display(),
                    error = %error,
                    "failed to parse workspace precise config; falling back to defaults"
                );
                WorkspacePreciseConfig::default()
            }
        }
    }

    pub(in crate::mcp::server) fn workspace_precise_generator_disabled(
        config: &WorkspacePreciseConfig,
        generator_id: &str,
    ) -> bool {
        config
            .disabled_generators
            .iter()
            .any(|value| value.eq_ignore_ascii_case(generator_id))
    }

    pub(in crate::mcp::server) fn workspace_precise_generator_extra_args(
        config: &WorkspacePreciseConfig,
        generator_id: &str,
    ) -> Vec<String> {
        config
            .generator_extra_args
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case(generator_id))
            .map(|(_, args)| args.clone())
            .unwrap_or_default()
    }

    pub(in crate::mcp::server) fn compile_workspace_precise_exclude_matcher(
        root: &Path,
        patterns: &[String],
    ) -> Option<Gitignore> {
        if patterns.is_empty() {
            return None;
        }

        let mut builder = GitignoreBuilder::new(root);
        for pattern in patterns {
            if let Err(error) = builder.add_line(None, pattern) {
                warn!(
                    root = %root.display(),
                    pattern,
                    error = %error,
                    "failed to parse workspace precise exclude pattern"
                );
            }
        }

        Some(builder.build().unwrap_or_else(|error| {
            warn!(
                root = %root.display(),
                error = %error,
                "failed to compile workspace precise exclude matcher"
            );
            Gitignore::empty()
        }))
    }

    pub(in crate::mcp::server) fn workspace_precise_excludes_path(
        root: &Path,
        path: &Path,
        matcher: Option<&Gitignore>,
        is_dir: bool,
    ) -> bool {
        let Some(matcher) = matcher else {
            return false;
        };
        let Ok(relative) = path.strip_prefix(root) else {
            return false;
        };
        if relative.as_os_str().is_empty() {
            return false;
        }
        matcher
            .matched_path_or_any_parents(relative, is_dir)
            .is_ignore()
    }

    pub(in crate::mcp::server) fn create_precise_generation_workspace(
        root: &Path,
        matcher: &Gitignore,
        generator_id: &str,
    ) -> Result<PathBuf, String> {
        let staging_root = root
            .join(".frigg")
            .join("tmp")
            .join("precise-generation")
            .join(format!("{generator_id}-{}", Self::scip_now_unix_ms()));
        fs::create_dir_all(&staging_root).map_err(|error| {
            format!(
                "failed to prepare filtered precise generation workspace {}: {error}",
                staging_root.display()
            )
        })?;

        let walker = WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                let path = entry.path();
                if path == root {
                    return true;
                }
                let Ok(relative) = path.strip_prefix(root) else {
                    return false;
                };
                if relative
                    .components()
                    .next()
                    .is_some_and(|component| component.as_os_str() == ".frigg")
                {
                    return false;
                }
                !matcher
                    .matched_path_or_any_parents(relative, entry.file_type().is_dir())
                    .is_ignore()
            });

        for entry in walker {
            let entry = entry.map_err(|error| {
                format!(
                    "failed to walk filtered precise generation workspace {}: {error}",
                    root.display()
                )
            })?;
            let path = entry.path();
            if path == root {
                continue;
            }
            let relative = path.strip_prefix(root).map_err(|error| {
                format!(
                    "failed to resolve filtered precise generation relative path for {}: {error}",
                    path.display()
                )
            })?;
            let target = staging_root.join(relative);
            if entry.file_type().is_dir() {
                fs::create_dir_all(&target).map_err(|error| {
                    format!(
                        "failed to create filtered precise generation directory {}: {error}",
                        target.display()
                    )
                })?;
                continue;
            }
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent).map_err(|error| {
                    format!(
                        "failed to prepare filtered precise generation parent {}: {error}",
                        parent.display()
                    )
                })?;
            }
            Self::link_precise_generation_entry(path, &target).map_err(|error| {
                format!(
                    "failed to link filtered precise generation entry {} -> {}: {error}",
                    path.display(),
                    target.display()
                )
            })?;
        }

        Ok(staging_root)
    }

    #[cfg(unix)]
    fn link_precise_generation_entry(source: &Path, target: &Path) -> std::io::Result<()> {
        std::os::unix::fs::symlink(source, target)
    }

    #[cfg(not(unix))]
    fn link_precise_generation_entry(source: &Path, target: &Path) -> std::io::Result<()> {
        fs::copy(source, target).map(|_| ())
    }

    #[cfg(test)]
    pub(in crate::mcp::server) fn set_test_precise_generator_bin_override(
        bin_dir: Option<PathBuf>,
    ) {
        *TEST_PRECISE_GENERATOR_BIN_OVERRIDE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("test precise generator override lock should not be poisoned") = bin_dir;
    }

    #[cfg(test)]
    pub(in crate::mcp::server) fn test_precise_generator_bin_override() -> Option<PathBuf> {
        TEST_PRECISE_GENERATOR_BIN_OVERRIDE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("test precise generator override lock should not be poisoned")
            .clone()
    }
}
