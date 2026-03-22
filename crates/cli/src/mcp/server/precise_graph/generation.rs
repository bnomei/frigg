use super::*;

struct PreciseGeneratorCommandRequest<'a> {
    workspace: &'a AttachedWorkspace,
    spec: &'a PreciseGeneratorSpec,
    tool: &'a ResolvedPreciseGeneratorTool,
    version: &'a str,
    generator_workspace_root: &'a Path,
    output_path: &'a Path,
    generator_extra_args: &'a [String],
    extra_args: &'a [String],
    generated_at_ms: u64,
}

struct WorkspacePythonShardRequest<'a> {
    workspace: &'a AttachedWorkspace,
    spec: &'a PreciseGeneratorSpec,
    tool: &'a ResolvedPreciseGeneratorTool,
    version: &'a str,
    generator_workspace_root: &'a Path,
    staging_dir: &'a Path,
    source_paths: &'a [PathBuf],
    target: &'a Path,
    generator_extra_args: &'a [String],
    generated_at_ms: u64,
    budget_bytes: u64,
}

struct WorkspacePythonGenerationRequest<'a> {
    workspace: &'a AttachedWorkspace,
    spec: &'a PreciseGeneratorSpec,
    tool: &'a ResolvedPreciseGeneratorTool,
    version: &'a str,
    generator_workspace_root: &'a Path,
    output_dir: &'a Path,
    generator_extra_args: &'a [String],
    generated_at_ms: u64,
}

impl FriggMcpServer {
    pub(in crate::mcp::server) fn precise_generator_specs() -> [PreciseGeneratorSpec; 6] {
        [
            PreciseGeneratorSpec {
                language: SymbolLanguage::Rust,
                generator_id: "rust",
                tool_name: "rust-analyzer",
                tool_candidates: &["rust-analyzer"],
                version_arg_sets: &[&["--version"], &["version"]],
                generate_args: &["scip", "."],
                infer_tsconfig: false,
                trigger_markers: &["Cargo.toml", "Cargo.lock", "src/lib.rs", "src/main.rs"],
                output_artifact_name: "rust.scip",
                stdout_artifact_fallback: true,
                output_flag: None,
            },
            PreciseGeneratorSpec {
                language: SymbolLanguage::Go,
                generator_id: "go",
                tool_name: "scip-go",
                tool_candidates: &["$GOPATH/bin/scip-go", "scip-go"],
                version_arg_sets: &[&["version"], &["--version"]],
                generate_args: &["-q"],
                infer_tsconfig: false,
                trigger_markers: &["go.mod", "go.sum"],
                output_artifact_name: "go.scip",
                stdout_artifact_fallback: false,
                output_flag: Some("-o"),
            },
            PreciseGeneratorSpec {
                language: SymbolLanguage::TypeScript,
                generator_id: "typescript",
                tool_name: "scip-typescript",
                tool_candidates: &[
                    "node_modules/.bin/scip-typescript",
                    "$NPM_PREFIX/bin/scip-typescript",
                    "$PNPM_BIN/scip-typescript",
                    "$BUN_BIN/scip-typescript",
                    "scip-typescript",
                ],
                version_arg_sets: &[&["--version"], &["version"]],
                generate_args: &["index"],
                infer_tsconfig: true,
                trigger_markers: &[
                    "package.json",
                    "tsconfig.json",
                    "jsconfig.json",
                    "src/index.ts",
                    "src/index.tsx",
                ],
                output_artifact_name: "typescript.scip",
                stdout_artifact_fallback: false,
                output_flag: None,
            },
            PreciseGeneratorSpec {
                language: SymbolLanguage::Php,
                generator_id: "php",
                tool_name: "scip-php",
                tool_candidates: &["vendor/bin/scip-php", "scip-php"],
                version_arg_sets: &[&["--help"], &["--version"], &["version"]],
                generate_args: &[],
                infer_tsconfig: false,
                trigger_markers: &["composer.json", "composer.lock"],
                output_artifact_name: "php.scip",
                stdout_artifact_fallback: true,
                output_flag: None,
            },
            PreciseGeneratorSpec {
                language: SymbolLanguage::Python,
                generator_id: "python",
                tool_name: "scip-python",
                tool_candidates: &[
                    "node_modules/.bin/scip-python",
                    "$NPM_PREFIX/bin/scip-python",
                    "scip-python",
                ],
                version_arg_sets: &[&["--version"], &["version"], &["index", "--help"]],
                generate_args: &["index", "--quiet"],
                infer_tsconfig: false,
                trigger_markers: &[
                    "pyproject.toml",
                    "setup.py",
                    "setup.cfg",
                    "requirements.txt",
                    "Pipfile",
                    "poetry.lock",
                    "uv.lock",
                ],
                output_artifact_name: "python.scip",
                stdout_artifact_fallback: false,
                output_flag: Some("--output"),
            },
            PreciseGeneratorSpec {
                language: SymbolLanguage::Kotlin,
                generator_id: "kotlin",
                tool_name: "scip-java",
                tool_candidates: &["scip-java"],
                version_arg_sets: &[&["--version"], &["version"], &["--help"]],
                generate_args: &["index"],
                infer_tsconfig: false,
                trigger_markers: &[
                    "build.gradle",
                    "build.gradle.kts",
                    "settings.gradle",
                    "settings.gradle.kts",
                    "gradle.properties",
                    "gradlew",
                    "gradlew.bat",
                ],
                output_artifact_name: "kotlin.scip",
                stdout_artifact_fallback: false,
                output_flag: None,
            },
        ]
    }

    pub(in crate::mcp::server) fn precise_generator_expected_output_path(
        workspace_root: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> PathBuf {
        workspace_root
            .join(".frigg/scip")
            .join(spec.output_artifact_name)
    }

    pub(in crate::mcp::server) fn precise_generator_tool_candidates(
        workspace_root: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> Vec<&'static str> {
        match spec.language {
            SymbolLanguage::Php => php_precise_generator_tool_candidates(workspace_root),
            _ => spec.tool_candidates.to_vec(),
        }
    }

    pub(in crate::mcp::server) fn precise_generator_language_label(
        spec: &PreciseGeneratorSpec,
    ) -> &'static str {
        match spec.language {
            SymbolLanguage::TypeScript => "typescript",
            SymbolLanguage::Kotlin => "kotlin",
            SymbolLanguage::Python => "python",
            SymbolLanguage::Rust => "rust",
            SymbolLanguage::Go => "go",
            SymbolLanguage::Php => "php",
            _ => spec.generator_id,
        }
    }

    fn workspace_contains_source_extensions(
        workspace_root: &Path,
        extensions: &[&str],
        excluded_files: &[&str],
    ) -> bool {
        WalkDir::new(workspace_root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| {
                let path = entry.path();
                if path == workspace_root {
                    return true;
                }
                let Ok(relative) = path.strip_prefix(workspace_root) else {
                    return false;
                };
                let Some(first) = relative.components().next() else {
                    return true;
                };
                let segment = first.as_os_str().to_string_lossy();
                !matches!(
                    segment.as_ref(),
                    ".git"
                        | ".frigg"
                        | "node_modules"
                        | ".venv"
                        | "venv"
                        | ".mypy_cache"
                        | ".pytest_cache"
                        | ".ruff_cache"
                        | ".gradle"
                        | "build"
                        | "dist"
                        | "target"
                )
            })
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .any(|entry| {
                let path = entry.path();
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if excluded_files.contains(&file_name.as_str()) {
                    return false;
                }
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| {
                        extensions
                            .iter()
                            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
                    })
            })
    }

    pub(in crate::mcp::server) fn derived_python_precise_project_name(
        workspace: &AttachedWorkspace,
    ) -> String {
        let source = if workspace.display_name.trim().is_empty() {
            workspace
                .root
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("python-workspace")
        } else {
            workspace.display_name.as_str()
        };

        let mut sanitized = String::with_capacity(source.len());
        let mut last_was_separator = false;
        for ch in source.chars() {
            if ch.is_ascii_alphanumeric() {
                sanitized.push(ch.to_ascii_lowercase());
                last_was_separator = false;
            } else if matches!(ch, '.' | '_' | '-') {
                sanitized.push(ch);
                last_was_separator = false;
            } else if !last_was_separator {
                sanitized.push('-');
                last_was_separator = true;
            }
        }

        let trimmed = sanitized.trim_matches(|ch| ch == '-' || ch == '_' || ch == '.');
        if trimmed.is_empty() {
            "python-workspace".to_owned()
        } else {
            trimmed.to_owned()
        }
    }

    fn scip_precise_generation_cache_key(repository_id: &str, generator_id: &str) -> String {
        format!("{repository_id}:{generator_id}")
    }

    pub(in crate::mcp::server) fn scip_now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    pub(in crate::mcp::server) fn workspace_has_precise_generator_markers(
        workspace_root: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> bool {
        let has_markers = spec
            .trigger_markers
            .iter()
            .any(|marker| workspace_root.join(marker).exists());

        match spec.language {
            SymbolLanguage::Python => {
                has_markers
                    || Self::workspace_contains_source_extensions(workspace_root, &["py"], &[])
            }
            SymbolLanguage::Kotlin => {
                has_markers
                    && Self::workspace_contains_source_extensions(
                        workspace_root,
                        &["kt", "kts"],
                        &["build.gradle.kts", "settings.gradle.kts", "init.gradle.kts"],
                    )
            }
            _ => has_markers,
        }
    }

    pub(in crate::mcp::server) fn scip_cached_workspace_precise_generation(
        &self,
        repository_id: &str,
        generator_id: &str,
    ) -> Option<WorkspacePreciseGenerationSummary> {
        let cache_key = Self::scip_precise_generation_cache_key(repository_id, generator_id);
        self.runtime_state
            .precise_generation_status_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .map(|cached| cached.summary.clone())
    }

    fn scip_cache_workspace_precise_generation(
        &self,
        repository_id: &str,
        generator_id: &str,
        summary: WorkspacePreciseGenerationSummary,
    ) {
        let cache_key = Self::scip_precise_generation_cache_key(repository_id, generator_id);
        self.runtime_state
            .precise_generation_status_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                cache_key,
                CachedWorkspacePreciseGeneration {
                    summary,
                    generated_at: Instant::now(),
                },
            );
    }

    pub(in crate::mcp::server) fn scip_invalidate_repository_precise_generation_cache(
        &self,
        repository_id: &str,
    ) {
        let mut cache = self
            .runtime_state
            .precise_generation_status_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prefix = format!("{repository_id}:");
        cache.retain(|key, _| !key.starts_with(&prefix));
    }

    pub(in crate::mcp::server) fn invalidate_repository_precise_graph_caches(
        &self,
        repository_id: &str,
    ) {
        self.cache_state
            .latest_precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(repository_id);
        self.cache_state
            .precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| key.repository_id != repository_id);
    }

    fn resolve_generator_candidate(
        workspace_root: &Path,
        candidate: &str,
    ) -> Option<ResolvedPreciseGeneratorTool> {
        let expanded = if candidate.contains("$GOPATH") {
            let gopath = std::env::var("GOPATH").ok().or_else(|| {
                let output = Command::new("go")
                    .args(["env", "GOPATH"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                (!value.is_empty()).then_some(value)
            })?;
            candidate.replace("$GOPATH", &gopath)
        } else if candidate.contains("$NPM_PREFIX") {
            let npm_prefix = std::env::var("NPM_PREFIX").ok().or_else(|| {
                let output = Command::new("npm")
                    .args(["prefix", "-g"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                (!value.is_empty()).then_some(value)
            })?;
            candidate.replace("$NPM_PREFIX", &npm_prefix)
        } else if candidate.contains("$PNPM_BIN") {
            let pnpm_bin = std::env::var("PNPM_BIN").ok().or_else(|| {
                let output = Command::new("pnpm")
                    .args(["bin", "-g"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                (!value.is_empty()).then_some(value)
            })?;
            candidate.replace("$PNPM_BIN", &pnpm_bin)
        } else if candidate.contains("$BUN_BIN") {
            let bun_bin = std::env::var("BUN_BIN").ok().or_else(|| {
                let output = Command::new("bun")
                    .args(["pm", "bin", "-g"])
                    .stdout(Stdio::piped())
                    .stderr(Stdio::null())
                    .output()
                    .ok()?;
                if !output.status.success() {
                    return None;
                }
                let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                (!value.is_empty()).then_some(value)
            })?;
            candidate.replace("$BUN_BIN", &bun_bin)
        } else {
            candidate.to_owned()
        };

        if expanded.contains('/') || expanded.contains('\\') {
            let path = PathBuf::from(&expanded);
            let path = if path.is_absolute() {
                path
            } else {
                workspace_root.join(path)
            };
            if !path.is_file() {
                return None;
            }
            let display = path.display().to_string();
            return Some(ResolvedPreciseGeneratorTool {
                command: display.clone(),
                display,
            });
        }

        #[cfg(test)]
        if let Some(bin_dir) = Self::test_precise_generator_bin_override()
            && !expanded.contains('/')
            && !expanded.contains('\\')
        {
            let candidate_path = bin_dir.join(&expanded);
            if candidate_path.is_file() {
                let display = candidate_path.display().to_string();
                return Some(ResolvedPreciseGeneratorTool {
                    command: display.clone(),
                    display,
                });
            }
        }

        Some(ResolvedPreciseGeneratorTool {
            command: expanded.clone(),
            display: expanded,
        })
    }

    pub(in crate::mcp::server) fn resolve_precise_generator_tools(
        workspace_root: &Path,
        tool_candidates: &[&str],
    ) -> Vec<ResolvedPreciseGeneratorTool> {
        let mut resolved = Vec::new();
        let mut seen = BTreeSet::new();
        for candidate in tool_candidates {
            #[cfg(test)]
            if Self::test_precise_generator_bin_override().is_some() && candidate.contains('$') {
                continue;
            }
            #[cfg(test)]
            if !candidate.contains(std::path::MAIN_SEPARATOR)
                && !candidate.contains('/')
                && !candidate.contains('\\')
            {
                if let Some(bin_dir) = Self::test_precise_generator_bin_override() {
                    let tool_name = Path::new(candidate)
                        .file_name()
                        .and_then(|name| name.to_str())
                        .map(|name| name.trim_start_matches('$'))
                        .unwrap_or(candidate);
                    let override_path = bin_dir.join(tool_name);
                    if override_path.is_file() {
                        let display = override_path.display().to_string();
                        if seen.insert(display.clone()) {
                            resolved.push(ResolvedPreciseGeneratorTool {
                                command: display.clone(),
                                display,
                            });
                        }
                    }
                    continue;
                }
            }
            let Some(tool) = Self::resolve_generator_candidate(workspace_root, candidate) else {
                continue;
            };
            if seen.insert(tool.display.clone()) {
                resolved.push(tool);
            }
        }
        resolved
    }

    pub(in crate::mcp::server) fn probe_precise_generator_tool(
        workspace_root: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> Result<(ResolvedPreciseGeneratorTool, String), PreciseToolProbeError> {
        let tool_candidates = Self::precise_generator_tool_candidates(workspace_root, spec);
        for version_args in spec.version_arg_sets {
            for tool in Self::resolve_precise_generator_tools(workspace_root, &tool_candidates) {
                let output = Command::new(&tool.command)
                    .current_dir(workspace_root)
                    .args(*version_args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output();
                let output = match output {
                    Ok(output) => output,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                        continue;
                    }
                    Err(err) => return Err(PreciseToolProbeError::Failed(err.to_string())),
                };
                let mut version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                if version.is_empty() {
                    version = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                }
                if !output.status.success() && version.is_empty() {
                    continue;
                }
                if version.is_empty() {
                    version = "unknown".to_owned();
                }
                return Ok((tool, version));
            }
        }
        Err(PreciseToolProbeError::MissingTool)
    }

    pub(in crate::mcp::server) fn generator_dirty_path_matches(
        spec: &PreciseGeneratorSpec,
        path: &str,
    ) -> bool {
        let normalized = path.replace('\\', "/").to_ascii_lowercase();
        match spec.language {
            SymbolLanguage::Rust => {
                normalized.ends_with(".rs")
                    || normalized.ends_with("cargo.toml")
                    || normalized.ends_with("cargo.lock")
            }
            SymbolLanguage::Go => {
                normalized.ends_with(".go")
                    || normalized.ends_with("go.mod")
                    || normalized.ends_with("go.sum")
            }
            SymbolLanguage::TypeScript => {
                normalized.ends_with(".ts")
                    || normalized.ends_with(".tsx")
                    || normalized.ends_with(".js")
                    || normalized.ends_with(".jsx")
                    || normalized.ends_with(".mjs")
                    || normalized.ends_with(".cjs")
                    || normalized.ends_with("package.json")
                    || normalized.ends_with("tsconfig.json")
                    || normalized.ends_with("jsconfig.json")
            }
            SymbolLanguage::Php => {
                normalized.ends_with(".php")
                    || normalized.ends_with("composer.json")
                    || normalized.ends_with("composer.lock")
            }
            SymbolLanguage::Python => {
                normalized.ends_with(".py")
                    || normalized.ends_with("pyproject.toml")
                    || normalized.ends_with("setup.py")
                    || normalized.ends_with("setup.cfg")
                    || normalized.ends_with("requirements.txt")
                    || normalized.ends_with("requirements-dev.txt")
                    || normalized.ends_with("requirements-test.txt")
                    || normalized.ends_with("pipfile")
                    || normalized.ends_with("poetry.lock")
                    || normalized.ends_with("uv.lock")
            }
            SymbolLanguage::Kotlin => {
                normalized.ends_with(".kt")
                    || normalized.ends_with(".kts")
                    || normalized.ends_with("build.gradle")
                    || normalized.ends_with("build.gradle.kts")
                    || normalized.ends_with("settings.gradle")
                    || normalized.ends_with("settings.gradle.kts")
                    || normalized.ends_with("gradle.properties")
                    || normalized.ends_with("gradle/libs.versions.toml")
                    || normalized.ends_with("gradle-wrapper.properties")
                    || normalized.ends_with("gradlew")
                    || normalized.ends_with("gradlew.bat")
            }
            _ => false,
        }
    }

    fn precise_generation_command_args(
        workspace: &AttachedWorkspace,
        generator_workspace_root: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> Vec<String> {
        let mut args = Vec::new();

        if spec.language == SymbolLanguage::TypeScript {
            let has_tsconfig = generator_workspace_root.join("tsconfig.json").is_file()
                || generator_workspace_root.join("jsconfig.json").is_file();
            if spec.infer_tsconfig && !has_tsconfig {
                args.push("--infer-tsconfig".to_owned());
            }
        }

        if spec.language == SymbolLanguage::Python {
            args.push("--project-name".to_owned());
            args.push(Self::derived_python_precise_project_name(workspace));
        }

        args
    }

    fn append_precise_output_arg(
        command: &mut Command,
        spec: &PreciseGeneratorSpec,
        output_path: &Path,
    ) {
        if let Some(output_flag) = spec.output_flag {
            let output_path_arg = output_path.to_string_lossy().to_string();
            command.args([output_flag, output_path_arg.as_str()]);
        }
    }

    fn display_precise_artifact_path(path: &Path) -> String {
        fs::canonicalize(path)
            .unwrap_or_else(|_| path.to_path_buf())
            .display()
            .to_string()
    }

    fn precise_generation_succeeded_summary(
        generated_at_ms: u64,
        artifact_paths: &[PathBuf],
        detail: String,
    ) -> WorkspacePreciseGenerationSummary {
        let mut published_paths = artifact_paths
            .iter()
            .map(|path| Self::display_precise_artifact_path(path))
            .collect::<Vec<_>>();
        published_paths.sort();
        published_paths.dedup();
        let artifact_count = published_paths.len();
        let artifact_path = (artifact_count == 1).then(|| published_paths[0].clone());
        let artifact_sample_paths = if artifact_count > 1 {
            published_paths
                .iter()
                .take(Self::PRECISE_DISCOVERY_SAMPLE_LIMIT)
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        WorkspacePreciseGenerationSummary {
            status: WorkspacePreciseGenerationStatus::Succeeded,
            generated_at_ms,
            artifact_path,
            artifact_count: (artifact_count > 1).then_some(artifact_count),
            artifact_sample_paths,
            failure_class: None,
            recommended_action: None,
            detail: Some(detail),
        }
    }

    fn remove_file_if_exists(path: &Path) -> Result<(), String> {
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("failed to remove {}: {err}", path.display())),
        }
    }

    fn python_shard_artifact_name(target: &Path) -> String {
        let normalized = if target.as_os_str().is_empty() {
            "workspace".to_owned()
        } else {
            target.to_string_lossy().replace('\\', "/")
        };
        let mut label = String::new();
        for ch in normalized.chars() {
            if ch.is_ascii_alphanumeric() {
                label.push(ch.to_ascii_lowercase());
            } else if !label.ends_with('-') {
                label.push('-');
            }
        }
        let label = label.trim_matches('-');
        let mut hasher = DeterministicSignatureHasher::new();
        hasher.write_str(&normalized);
        let digest = hasher.finish_hex();
        let suffix = &digest[..8];
        let prefix = if label.is_empty() { "workspace" } else { label };
        format!("python--{prefix}-{suffix}.scip")
    }

    fn collect_python_source_relative_paths(root: &Path) -> Vec<PathBuf> {
        let mut sources = Vec::new();
        for entry in WalkDir::new(root)
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
                let Some(first) = relative.components().next() else {
                    return true;
                };
                let segment = first.as_os_str().to_string_lossy();
                !matches!(
                    segment.as_ref(),
                    ".git"
                        | ".frigg"
                        | "node_modules"
                        | ".venv"
                        | "venv"
                        | ".mypy_cache"
                        | ".pytest_cache"
                        | ".ruff_cache"
                        | "__pycache__"
                        | "dist"
                        | "build"
                        | "target"
                )
            })
            .filter_map(Result::ok)
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let is_python = path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("py"));
            if !is_python {
                continue;
            }
            if let Ok(relative) = path.strip_prefix(root) {
                sources.push(relative.to_path_buf());
            }
        }
        sources.sort();
        sources.dedup();
        sources
    }

    fn python_shard_targets(source_paths: &[PathBuf], prefix: Option<&Path>) -> Vec<PathBuf> {
        let mut targets = BTreeSet::new();
        for source in source_paths {
            let candidate = match prefix {
                Some(prefix) => {
                    if !source.starts_with(prefix) || source == prefix {
                        continue;
                    }
                    let Ok(remainder) = source.strip_prefix(prefix) else {
                        continue;
                    };
                    let Some(child) = remainder.components().next() else {
                        continue;
                    };
                    prefix.join(child.as_os_str())
                }
                None => {
                    let Some(first) = source.components().next() else {
                        continue;
                    };
                    if source.components().count() == 1 {
                        source.clone()
                    } else {
                        PathBuf::from(first.as_os_str())
                    }
                }
            };
            targets.insert(candidate);
        }
        targets.into_iter().collect()
    }

    fn python_source_inventory_bytes(root: &Path, source_paths: &[PathBuf]) -> u64 {
        source_paths.iter().fold(0_u64, |total, relative_path| {
            let source_bytes = fs::metadata(root.join(relative_path))
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            total.saturating_add(source_bytes)
        })
    }

    fn python_generation_previously_sharded(
        &self,
        workspace: &AttachedWorkspace,
        spec: &PreciseGeneratorSpec,
        output_dir: &Path,
    ) -> Result<bool, String> {
        let managed_artifacts = Self::python_managed_artifact_paths(output_dir, spec)?;
        if managed_artifacts.iter().any(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("python--") && name.ends_with(".scip"))
        }) {
            return Ok(true);
        }

        let cached_generation = self
            .scip_cached_workspace_precise_generation(&workspace.repository_id, spec.generator_id);
        Ok(cached_generation
            .as_ref()
            .and_then(|summary| summary.artifact_path.as_deref())
            .is_some_and(|artifact_path| {
                Path::new(artifact_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("python--") && name.ends_with(".scip"))
            })
            || cached_generation
                .as_ref()
                .and_then(|summary| summary.artifact_count)
                .is_some_and(|count| count > 1))
    }

    fn python_managed_artifact_paths(
        output_dir: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> Result<Vec<PathBuf>, String> {
        let mut paths = Vec::new();
        let entries = match fs::read_dir(output_dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(paths),
            Err(err) => {
                return Err(format!(
                    "failed to read precise artifact directory {}: {err}",
                    output_dir.display()
                ));
            }
        };
        for entry in entries {
            let entry = entry.map_err(|err| {
                format!(
                    "failed to inspect precise artifact entry in {}: {err}",
                    output_dir.display()
                )
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if file_name == spec.output_artifact_name
                || (file_name.starts_with("python--") && file_name.ends_with(".scip"))
            {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }

    fn clear_python_managed_artifacts(
        output_dir: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> Result<(), String> {
        for path in Self::python_managed_artifact_paths(output_dir, spec)? {
            Self::remove_file_if_exists(&path)?;
        }
        Ok(())
    }

    fn publish_precise_artifact(staged_path: &Path, final_path: &Path) -> Result<(), String> {
        if let Some(parent) = final_path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "failed to prepare precise artifact directory {}: {err}",
                    parent.display()
                )
            })?;
        }
        match fs::rename(staged_path, final_path) {
            Ok(()) => Ok(()),
            Err(rename_err) => {
                fs::copy(staged_path, final_path).map_err(|copy_err| {
                    format!(
                        "failed to publish precise artifact {} to {}: rename={rename_err}; copy={copy_err}",
                        staged_path.display(),
                        final_path.display()
                    )
                })?;
                Self::remove_file_if_exists(staged_path)?;
                Ok(())
            }
        }
    }

    fn publish_staged_python_artifacts(
        staging_dir: &Path,
        output_dir: &Path,
    ) -> Result<Vec<PathBuf>, String> {
        let entries = fs::read_dir(staging_dir).map_err(|err| {
            format!(
                "failed to read staged python artifact directory {}: {err}",
                staging_dir.display()
            )
        })?;
        let mut published = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|err| {
                format!(
                    "failed to inspect staged python artifact in {}: {err}",
                    staging_dir.display()
                )
            })?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let Some(file_name) = path.file_name() else {
                continue;
            };
            let final_path = output_dir.join(file_name);
            Self::publish_precise_artifact(&path, &final_path)?;
            published.push(final_path);
        }
        published.sort();
        Ok(published)
    }

    fn run_precise_generator_command(
        request: PreciseGeneratorCommandRequest<'_>,
    ) -> Result<PathBuf, WorkspacePreciseGenerationSummary> {
        let output_dir = request
            .output_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| request.generator_workspace_root.to_path_buf());
        if let Err(detail) = Self::remove_file_if_exists(request.output_path) {
            return Err(Self::precise_failed_summary(
                request.generated_at_ms,
                None,
                detail,
            ));
        }

        let mut command = Command::new(&request.tool.command);
        command
            .current_dir(request.generator_workspace_root)
            .stdin(Stdio::null())
            .stderr(Stdio::piped())
            .stdout(Stdio::piped());
        command.args(request.spec.generate_args);
        command.args(Self::precise_generation_command_args(
            request.workspace,
            request.generator_workspace_root,
            request.spec,
        ));
        Self::append_precise_output_arg(&mut command, request.spec, request.output_path);
        if !request.extra_args.is_empty() {
            command.args(request.extra_args.iter());
        }
        if !request.generator_extra_args.is_empty() {
            command.args(request.generator_extra_args.iter());
        }
        Self::apply_generator_environment(&mut command, request.workspace, request.spec);

        let output = match command.output() {
            Ok(output) => output,
            Err(err) => {
                if err.kind() == std::io::ErrorKind::NotFound {
                    return Err(WorkspacePreciseGenerationSummary {
                        status: WorkspacePreciseGenerationStatus::MissingTool,
                        generated_at_ms: request.generated_at_ms,
                        artifact_path: None,
                        artifact_count: None,
                        artifact_sample_paths: Vec::new(),
                        failure_class: Some(WorkspacePreciseFailureClass::MissingTool),
                        recommended_action: Some(WorkspaceRecommendedAction::InstallTool),
                        detail: Some(err.to_string()),
                    });
                }
                return Err(Self::precise_failed_summary(
                    request.generated_at_ms,
                    None,
                    err.to_string(),
                ));
            }
        };

        if !output.status.success() {
            let stderr_detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(Self::precise_failed_summary(
                request.generated_at_ms,
                None,
                if stderr_detail.is_empty() {
                    format!(
                        "generator '{}' exited unsuccessfully (version={})",
                        request.tool.display, request.version
                    )
                } else {
                    stderr_detail
                },
            ));
        }

        match Self::write_precise_artifact(
            request.generator_workspace_root,
            &output_dir,
            request
                .output_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(request.spec.output_artifact_name),
            &output.stdout,
            request.spec.stdout_artifact_fallback,
        ) {
            Ok(Some(path)) => Ok(path),
            Ok(None) => Err(Self::precise_failed_summary(
                request.generated_at_ms,
                None,
                format!(
                    "generator '{}' succeeded but no SCIP artifact was produced",
                    request.tool.display
                ),
            )),
            Err(detail) => Err(Self::precise_failed_summary(
                request.generated_at_ms,
                None,
                detail,
            )),
        }
    }

    fn run_workspace_python_shard(
        request: WorkspacePythonShardRequest<'_>,
    ) -> Result<Vec<PathBuf>, WorkspacePreciseGenerationSummary> {
        let artifact_name = Self::python_shard_artifact_name(request.target);
        let output_path = request.staging_dir.join(&artifact_name);
        let target_arg = request.target.to_string_lossy().to_string();
        let extra_args = vec!["--target-only".to_owned(), target_arg.clone()];
        let artifact_path = Self::run_precise_generator_command(PreciseGeneratorCommandRequest {
            workspace: request.workspace,
            spec: request.spec,
            tool: request.tool,
            version: request.version,
            generator_workspace_root: request.generator_workspace_root,
            output_path: &output_path,
            generator_extra_args: request.generator_extra_args,
            extra_args: &extra_args,
            generated_at_ms: request.generated_at_ms,
        })?;
        let artifact_bytes = fs::metadata(&artifact_path)
            .map_err(|err| {
                Self::precise_failed_summary(
                    request.generated_at_ms,
                    None,
                    format!(
                        "failed to inspect generated python shard {}: {err}",
                        artifact_path.display()
                    ),
                )
            })?
            .len();
        if artifact_bytes <= request.budget_bytes {
            return Ok(vec![artifact_path]);
        }

        let _ = Self::remove_file_if_exists(&artifact_path);
        let child_targets = Self::python_shard_targets(request.source_paths, Some(request.target));
        if child_targets.is_empty() {
            return Err(Self::precise_failed_summary(
                request.generated_at_ms,
                None,
                format!(
                    "python shard '{}' produced artifact bytes {} above configured per-file limit {} and could not be split further",
                    target_arg, artifact_bytes, request.budget_bytes
                ),
            ));
        }

        let mut published = Vec::new();
        for child_target in child_targets {
            published.extend(Self::run_workspace_python_shard(
                WorkspacePythonShardRequest {
                    workspace: request.workspace,
                    spec: request.spec,
                    tool: request.tool,
                    version: request.version,
                    generator_workspace_root: request.generator_workspace_root,
                    staging_dir: request.staging_dir,
                    source_paths: request.source_paths,
                    target: &child_target,
                    generator_extra_args: request.generator_extra_args,
                    generated_at_ms: request.generated_at_ms,
                    budget_bytes: request.budget_bytes,
                },
            )?);
        }
        Ok(published)
    }

    fn run_workspace_python_precise_generation(
        &self,
        request: WorkspacePythonGenerationRequest<'_>,
    ) -> WorkspacePreciseGenerationSummary {
        let budget_bytes = u64::try_from(
            self.find_references_resource_budgets()
                .scip_max_artifact_bytes,
        )
        .unwrap_or(u64::MAX);
        let staging_dir = request.output_dir.join(format!(
            ".python-stage-{}-{}",
            std::process::id(),
            request.generated_at_ms
        ));
        let result =
            (|| -> Result<WorkspacePreciseGenerationSummary, WorkspacePreciseGenerationSummary> {
                let source_paths =
                    Self::collect_python_source_relative_paths(request.generator_workspace_root);
                let source_inventory_bytes = Self::python_source_inventory_bytes(
                    request.generator_workspace_root,
                    &source_paths,
                );
                let prefer_shards =
                    self.python_generation_previously_sharded(
                        request.workspace,
                        request.spec,
                        request.output_dir,
                    )
                    .map_err(|detail| {
                        Self::precise_failed_summary(request.generated_at_ms, None, detail)
                    })? || (source_paths.len() > 1 && source_inventory_bytes > budget_bytes);
                fs::create_dir_all(&staging_dir).map_err(|err| {
                    Self::precise_failed_summary(
                        request.generated_at_ms,
                        None,
                        format!(
                            "failed to prepare staged python artifact directory {}: {err}",
                            staging_dir.display()
                        ),
                    )
                })?;
                Self::clear_python_managed_artifacts(request.output_dir, request.spec).map_err(
                    |detail| Self::precise_failed_summary(request.generated_at_ms, None, detail),
                )?;

                if !prefer_shards {
                    let monolith_output_path = staging_dir.join(request.spec.output_artifact_name);
                    let monolith_path =
                        Self::run_precise_generator_command(PreciseGeneratorCommandRequest {
                            workspace: request.workspace,
                            spec: request.spec,
                            tool: request.tool,
                            version: request.version,
                            generator_workspace_root: request.generator_workspace_root,
                            output_path: &monolith_output_path,
                            generator_extra_args: request.generator_extra_args,
                            extra_args: &[],
                            generated_at_ms: request.generated_at_ms,
                        })?;
                    let monolith_bytes = fs::metadata(&monolith_path)
                        .map_err(|err| {
                            Self::precise_failed_summary(
                                request.generated_at_ms,
                                None,
                                format!(
                                    "failed to inspect generated python artifact {}: {err}",
                                    monolith_path.display()
                                ),
                            )
                        })?
                        .len();
                    if monolith_bytes <= budget_bytes {
                        let final_path = request.output_dir.join(request.spec.output_artifact_name);
                        Self::publish_precise_artifact(&monolith_path, &final_path).map_err(
                            |detail| {
                                Self::precise_failed_summary(request.generated_at_ms, None, detail)
                            },
                        )?;
                        return Ok(Self::precise_generation_succeeded_summary(
                            request.generated_at_ms,
                            &[final_path],
                            format!(
                                "generator={} tool={} version={}",
                                request.spec.generator_id, request.tool.display, request.version
                            ),
                        ));
                    }

                    let _ = Self::remove_file_if_exists(&monolith_path);
                } else {
                    tracing::info!(
                        repository_id = %request.workspace.repository_id,
                        generator = request.spec.generator_id,
                        source_paths = source_paths.len(),
                        source_inventory_bytes,
                        budget_bytes,
                        "skipping monolithic python precise generation and starting with shard targets"
                    );
                }

                let shard_targets = Self::python_shard_targets(&source_paths, None);
                if shard_targets.is_empty() {
                    return Err(Self::precise_failed_summary(
                        request.generated_at_ms,
                        None,
                        format!(
                            "python precise generation requires shard targets but none were available (source_paths={} source_inventory_bytes={} budget_bytes={})",
                            source_paths.len(),
                            source_inventory_bytes,
                            budget_bytes
                        ),
                    ));
                }

                for target in shard_targets {
                    Self::run_workspace_python_shard(WorkspacePythonShardRequest {
                        workspace: request.workspace,
                        spec: request.spec,
                        tool: request.tool,
                        version: request.version,
                        generator_workspace_root: request.generator_workspace_root,
                        staging_dir: &staging_dir,
                        source_paths: &source_paths,
                        target: &target,
                        generator_extra_args: request.generator_extra_args,
                        generated_at_ms: request.generated_at_ms,
                        budget_bytes,
                    })?;
                }

                let published_paths =
                    Self::publish_staged_python_artifacts(&staging_dir, request.output_dir)
                        .map_err(|detail| {
                            Self::precise_failed_summary(request.generated_at_ms, None, detail)
                        })?;
                Ok(Self::precise_generation_succeeded_summary(
                    request.generated_at_ms,
                    &published_paths,
                    format!(
                        "generator={} tool={} version={} shards={}",
                        request.spec.generator_id,
                        request.tool.display,
                        request.version,
                        published_paths.len()
                    ),
                ))
            })();

        if let Err(error) = fs::remove_dir_all(&staging_dir)
            && error.kind() != std::io::ErrorKind::NotFound
        {
            warn!(
                path = %staging_dir.display(),
                error = %error,
                "failed to clean staged python artifact directory"
            );
        }

        match result {
            Ok(summary) => summary,
            Err(summary) => summary,
        }
    }

    fn workspace_precise_generation_needed(
        &self,
        workspace: &AttachedWorkspace,
        spec: &PreciseGeneratorSpec,
        changed_paths: &[String],
        deleted_paths: &[String],
    ) -> bool {
        let precise_config = Self::load_workspace_precise_config(&workspace.root);
        if Self::workspace_precise_generator_disabled(&precise_config, spec.generator_id) {
            return false;
        }
        if !Self::workspace_has_precise_generator_markers(&workspace.root, spec) {
            return false;
        }
        let generation_matcher = Self::compile_workspace_precise_exclude_matcher(
            &workspace.root,
            &precise_config.generation_excludes,
        );
        if changed_paths.is_empty() && deleted_paths.is_empty() {
            return self
                .scip_cached_workspace_precise_generation(
                    &workspace.repository_id,
                    spec.generator_id,
                )
                .is_none();
        }
        changed_paths
            .iter()
            .chain(deleted_paths.iter())
            .filter(|path| {
                !Self::workspace_precise_excludes_path(
                    &workspace.root,
                    Path::new(path.as_str()),
                    generation_matcher.as_ref(),
                    false,
                )
            })
            .any(|path| Self::generator_dirty_path_matches(spec, path))
    }

    fn write_precise_artifact(
        root: &Path,
        output_dir: &Path,
        artifact_name: &str,
        stdout: &[u8],
        stdout_artifact_fallback: bool,
    ) -> Result<Option<PathBuf>, String> {
        let expected_path = output_dir.join(artifact_name);
        if expected_path.is_file() {
            return Ok(Some(expected_path));
        }

        let candidate_names = [
            "index.scip",
            artifact_name,
            "output.scip",
            "scip.index.scip",
        ];
        for candidate_name in candidate_names {
            let candidate = [root.join(candidate_name), output_dir.join(candidate_name)]
                .into_iter()
                .find(|path| path.is_file())
                .unwrap_or_else(|| output_dir.join(candidate_name));
            if candidate == expected_path {
                continue;
            }
            if candidate.is_file() {
                match fs::rename(&candidate, &expected_path) {
                    Ok(()) => return Ok(Some(expected_path)),
                    Err(rename_err) => {
                        fs::copy(&candidate, &expected_path).map_err(|copy_err| {
                            format!(
                                "failed to move artifact {} to {}: rename={rename_err}; copy={copy_err}",
                                candidate.display(),
                                expected_path.display()
                            )
                        })?;
                        return Ok(Some(expected_path));
                    }
                }
            }
        }

        if stdout_artifact_fallback && !stdout.is_empty() {
            fs::write(&expected_path, stdout).map_err(|err| {
                format!(
                    "failed to write artifact {}: {err}",
                    expected_path.display()
                )
            })?;
            return Ok(Some(expected_path));
        }

        Ok(None)
    }

    fn precise_failure_class(detail: &str) -> WorkspacePreciseFailureClass {
        let normalized = detail.to_ascii_lowercase();
        if normalized.contains("not installed")
            || normalized.contains("not on path")
            || normalized.contains("no such file")
        {
            return WorkspacePreciseFailureClass::MissingTool;
        }
        if normalized.contains("timed out") {
            return WorkspacePreciseFailureClass::ToolTimeout;
        }
        if normalized.contains("operation not permitted")
            || normalized.contains("permission denied")
            || normalized.contains("failed to create")
            || normalized.contains("failed to prepare")
            || normalized.contains("failed to write")
            || normalized.contains("unable to locate a java runtime")
            || normalized.contains("java_home")
            || normalized.contains("jdk")
            || normalized.contains("jvm")
            || normalized.contains("gradle")
            || normalized.contains("python interpreter")
            || normalized.contains("virtual environment")
            || normalized.contains("virtualenv")
            || normalized.contains(".venv")
            || normalized.contains("venv")
            || normalized.contains("no module named")
            || normalized.contains("pip")
        {
            return WorkspacePreciseFailureClass::ToolEnvFailure;
        }
        if normalized.contains("panic")
            || normalized.contains("invariant violation")
            || normalized.contains("stack trace")
        {
            return WorkspacePreciseFailureClass::ToolPanic;
        }
        if normalized.contains("no scip artifact was produced")
            || normalized.contains("invalid input")
            || normalized.contains("protobuf")
        {
            return WorkspacePreciseFailureClass::ToolInvalidOutput;
        }
        WorkspacePreciseFailureClass::ToolFailed
    }

    fn precise_recommended_action(
        failure_class: WorkspacePreciseFailureClass,
    ) -> WorkspaceRecommendedAction {
        match failure_class {
            WorkspacePreciseFailureClass::MissingTool => WorkspaceRecommendedAction::InstallTool,
            WorkspacePreciseFailureClass::ToolEnvFailure => {
                WorkspaceRecommendedAction::CheckEnvironment
            }
            WorkspacePreciseFailureClass::ToolPanic => {
                WorkspaceRecommendedAction::UpstreamToolFailure
            }
            WorkspacePreciseFailureClass::ToolTimeout => WorkspaceRecommendedAction::RerunReindex,
            WorkspacePreciseFailureClass::ToolInvalidOutput
            | WorkspacePreciseFailureClass::ToolFailed => {
                WorkspaceRecommendedAction::UseHeuristicMode
            }
        }
    }

    fn precise_failed_summary(
        generated_at_ms: u64,
        artifact_path: Option<String>,
        detail: String,
    ) -> WorkspacePreciseGenerationSummary {
        let failure_class = Self::precise_failure_class(&detail);
        WorkspacePreciseGenerationSummary {
            status: WorkspacePreciseGenerationStatus::Failed,
            generated_at_ms,
            artifact_path,
            artifact_count: None,
            artifact_sample_paths: Vec::new(),
            failure_class: Some(failure_class),
            recommended_action: Some(Self::precise_recommended_action(failure_class)),
            detail: Some(detail),
        }
    }

    fn apply_generator_environment(
        command: &mut Command,
        workspace: &AttachedWorkspace,
        spec: &PreciseGeneratorSpec,
    ) {
        if spec.language != SymbolLanguage::Go {
            return;
        }

        let go_root = workspace.root.join(".frigg").join("go");
        let gocache = go_root.join("build-cache");
        let gomodcache = go_root.join("mod-cache");
        let _ = fs::create_dir_all(&gocache);
        let _ = fs::create_dir_all(&gomodcache);
        command.env("GOCACHE", &gocache);
        command.env("GOMODCACHE", &gomodcache);
        command.env("GOPATH", &go_root);
    }

    fn maybe_patch_repo_local_scip_php_vendor_dir(
        workspace_root: &Path,
        spec: &PreciseGeneratorSpec,
        tool: &ResolvedPreciseGeneratorTool,
    ) -> Result<(), String> {
        if spec.language != SymbolLanguage::Php || !tool.display.contains("vendor/bin/scip-php") {
            return Ok(());
        }

        let composer_path =
            workspace_root.join("vendor/davidrjenni/scip-php/src/Composer/Composer.php");
        let source = match fs::read_to_string(&composer_path) {
            Ok(source) => source,
            Err(_) => return Ok(()),
        };
        if source.contains("https://github.com/davidrjenni/scip-php/issues/235") {
            return Ok(());
        }

        let broken = r#"        $scipPhpVendorDir = self::join(__DIR__, '..', '..', 'vendor');
        if (realpath($scipPhpVendorDir) === false) {
            throw new RuntimeException("Invalid scip-php vendor directory: {$scipPhpVendorDir}.");
        }
        $this->scipPhpVendorDir = realpath($scipPhpVendorDir);"#;
        let fixed = r#"        $scipPhpVendorDir = self::join(__DIR__, '..', '..', 'vendor');
        if (realpath($scipPhpVendorDir) === false) {
            // FRIGG workaround for davidrjenni/scip-php v0.0.2 vendor resolution. See https://github.com/davidrjenni/scip-php/issues/235
            $scipPhpVendorDir = self::join($projectRoot, 'vendor');
        }
        if (realpath($scipPhpVendorDir) === false) {
            throw new RuntimeException("Invalid scip-php vendor directory: {$scipPhpVendorDir}.");
        }
        $this->scipPhpVendorDir = realpath($scipPhpVendorDir);"#;

        if !source.contains(broken) {
            return Ok(());
        }

        let patched = source.replacen(broken, fixed, 1);
        fs::write(&composer_path, patched).map_err(|err| {
            format!(
                "failed to patch repo-local scip-php vendor workaround at {}: {err}",
                composer_path.display()
            )
        })
    }

    fn run_workspace_precise_generation(
        &self,
        workspace: &AttachedWorkspace,
        spec: &PreciseGeneratorSpec,
    ) -> WorkspacePreciseGenerationSummary {
        let started_at = Instant::now();
        let generated_at_ms = Self::scip_now_unix_ms();
        let finish =
            |summary: WorkspacePreciseGenerationSummary| -> WorkspacePreciseGenerationSummary {
                let duration_ms = started_at.elapsed().as_millis() as u64;
                match summary.status {
                    WorkspacePreciseGenerationStatus::Succeeded => {
                        tracing::info!(
                            repository_id = %workspace.repository_id,
                            root = %workspace.root.display(),
                            generator = spec.generator_id,
                            status = ?summary.status,
                            artifact_path = summary.artifact_path.as_deref().unwrap_or(""),
                            artifact_count = summary.artifact_count.unwrap_or(1),
                            duration_ms,
                            detail = summary.detail.as_deref().unwrap_or(""),
                            "workspace precise generator completed"
                        );
                    }
                    WorkspacePreciseGenerationStatus::NotConfigured => {
                        tracing::info!(
                            repository_id = %workspace.repository_id,
                            root = %workspace.root.display(),
                            generator = spec.generator_id,
                            status = ?summary.status,
                            duration_ms,
                            detail = summary.detail.as_deref().unwrap_or(""),
                            "workspace precise generator skipped"
                        );
                    }
                    _ => {
                        warn!(
                            repository_id = %workspace.repository_id,
                            root = %workspace.root.display(),
                            generator = spec.generator_id,
                            status = ?summary.status,
                            artifact_path = summary.artifact_path.as_deref().unwrap_or(""),
                            failure_class = ?summary.failure_class,
                            recommended_action = ?summary.recommended_action,
                            duration_ms,
                            detail = summary.detail.as_deref().unwrap_or(""),
                            "workspace precise generator failed"
                        );
                    }
                }
                summary
            };
        let precise_config = Self::load_workspace_precise_config(&workspace.root);
        if Self::workspace_precise_generator_disabled(&precise_config, spec.generator_id) {
            return finish(WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::NotConfigured,
                generated_at_ms,
                artifact_path: None,
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
                failure_class: None,
                recommended_action: None,
                detail: Some("disabled_by_workspace_precise_config".to_owned()),
            });
        }
        if !Self::workspace_has_precise_generator_markers(&workspace.root, spec) {
            return finish(WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::NotConfigured,
                generated_at_ms,
                artifact_path: None,
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
                failure_class: None,
                recommended_action: None,
                detail: Some("missing_language_markers".to_owned()),
            });
        }

        let (tool, version) = match Self::probe_precise_generator_tool(&workspace.root, spec) {
            Ok((tool, version)) => (tool, version),
            Err(PreciseToolProbeError::MissingTool) => {
                let tool_candidates =
                    Self::precise_generator_tool_candidates(&workspace.root, spec);
                return finish(WorkspacePreciseGenerationSummary {
                    status: WorkspacePreciseGenerationStatus::MissingTool,
                    generated_at_ms,
                    artifact_path: None,
                    artifact_count: None,
                    artifact_sample_paths: Vec::new(),
                    failure_class: Some(WorkspacePreciseFailureClass::MissingTool),
                    recommended_action: Some(WorkspaceRecommendedAction::InstallTool),
                    detail: Some(format!(
                        "precise generator tool '{}' is not installed",
                        tool_candidates.join(" or ")
                    )),
                });
            }
            Err(PreciseToolProbeError::Failed(error)) => {
                return finish(Self::precise_failed_summary(generated_at_ms, None, error));
            }
        };

        let output_dir = workspace.root.join(".frigg").join("scip");
        if let Err(err) = fs::create_dir_all(&output_dir) {
            return finish(Self::precise_failed_summary(
                generated_at_ms,
                None,
                format!(
                    "failed to prepare SCIP artifact directory {}: {err}",
                    output_dir.display()
                ),
            ));
        }

        if let Err(detail) =
            Self::maybe_patch_repo_local_scip_php_vendor_dir(&workspace.root, spec, &tool)
        {
            return finish(Self::precise_failed_summary(generated_at_ms, None, detail));
        }

        let generation_matcher = Self::compile_workspace_precise_exclude_matcher(
            &workspace.root,
            &precise_config.generation_excludes,
        );
        let filtered_generation_root = match generation_matcher.as_ref() {
            Some(matcher) => match Self::create_precise_generation_workspace(
                &workspace.root,
                matcher,
                spec.generator_id,
            ) {
                Ok(path) => Some(path),
                Err(detail) => {
                    return finish(Self::precise_failed_summary(generated_at_ms, None, detail));
                }
            },
            None => None,
        };
        let generator_workspace_root = filtered_generation_root
            .as_deref()
            .unwrap_or(&workspace.root)
            .to_path_buf();
        let generator_extra_args =
            Self::workspace_precise_generator_extra_args(&precise_config, spec.generator_id);
        tracing::info!(
            repository_id = %workspace.repository_id,
            root = %workspace.root.display(),
            generator = spec.generator_id,
            tool = %tool.display,
            workspace_root = %generator_workspace_root.display(),
            filtered_generation_root = filtered_generation_root.is_some(),
            version = %version,
            "workspace precise generator started"
        );

        let generation_result = (|| {
            if spec.language == SymbolLanguage::Python {
                return self.run_workspace_python_precise_generation(
                    WorkspacePythonGenerationRequest {
                        workspace,
                        spec,
                        tool: &tool,
                        version: &version,
                        generator_workspace_root: &generator_workspace_root,
                        output_dir: &output_dir,
                        generator_extra_args: &generator_extra_args,
                        generated_at_ms,
                    },
                );
            }

            let output_path = output_dir.join(spec.output_artifact_name);
            match Self::run_precise_generator_command(PreciseGeneratorCommandRequest {
                workspace,
                spec,
                tool: &tool,
                version: &version,
                generator_workspace_root: &generator_workspace_root,
                output_path: &output_path,
                generator_extra_args: &generator_extra_args,
                extra_args: &[],
                generated_at_ms,
            }) {
                Ok(artifact_path) => Self::precise_generation_succeeded_summary(
                    generated_at_ms,
                    &[artifact_path],
                    format!(
                        "generator={} tool={} version={version}",
                        spec.generator_id, tool.display
                    ),
                ),
                Err(summary) => summary,
            }
        })();

        if let Some(filtered_generation_root) = filtered_generation_root {
            if let Err(error) = fs::remove_dir_all(&filtered_generation_root) {
                warn!(
                    path = %filtered_generation_root.display(),
                    error = %error,
                    "failed to clean filtered precise generation workspace"
                );
            }
        }

        finish(generation_result)
    }

    pub(in crate::mcp::server) fn maybe_spawn_workspace_precise_generation(
        &self,
        workspace: &AttachedWorkspace,
        changed_paths: &[String],
        deleted_paths: &[String],
    ) -> WorkspacePreciseGenerationAction {
        let specs = Self::precise_generator_specs();
        let mut selected = Vec::new();
        for spec in specs {
            if self.workspace_precise_generation_needed(
                workspace,
                &spec,
                changed_paths,
                deleted_paths,
            ) {
                selected.push(spec);
            }
        }
        if selected.is_empty() {
            tracing::info!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                changed_paths = changed_paths.len(),
                deleted_paths = deleted_paths.len(),
                "workspace precise generation skipped because no generators need refresh"
            );
            return WorkspacePreciseGenerationAction::SkippedNoWork;
        }

        let active_precise_generation = self
            .runtime_state
            .runtime_task_registry
            .read()
            .expect("runtime task registry poisoned")
            .active_tasks()
            .iter()
            .any(|task| {
                task.kind == crate::mcp::types::RuntimeTaskKind::PreciseGenerate
                    && task.repository_id == workspace.repository_id
            });
        if active_precise_generation {
            tracing::info!(
                repository_id = %workspace.repository_id,
                root = %workspace.root.display(),
                changed_paths = changed_paths.len(),
                deleted_paths = deleted_paths.len(),
                generators = %selected
                    .iter()
                    .map(|spec| spec.generator_id)
                    .collect::<Vec<_>>()
                    .join(","),
                "workspace precise generation skipped because a generation task is already active"
            );
            return WorkspacePreciseGenerationAction::SkippedActiveTask;
        }

        let server = self.clone();
        let workspace = workspace.clone();
        let selected_generators = selected.to_vec();
        let changed_paths = changed_paths.to_vec();
        let deleted_paths = deleted_paths.to_vec();
        let selected_generator_ids = selected_generators
            .iter()
            .map(|spec| spec.generator_id)
            .collect::<Vec<_>>()
            .join(",");
        tracing::info!(
            repository_id = %workspace.repository_id,
            root = %workspace.root.display(),
            changed_paths = changed_paths.len(),
            deleted_paths = deleted_paths.len(),
            generators = %selected_generator_ids,
            "workspace precise generation started"
        );
        let task_id = self
            .runtime_state
            .runtime_task_registry
            .write()
            .expect("runtime task registry poisoned")
            .start_task(
                crate::mcp::types::RuntimeTaskKind::PreciseGenerate,
                workspace.repository_id.clone(),
                "precise_generation",
                Some(format!(
                    "changed_paths={} deleted_paths={}",
                    changed_paths.len(),
                    deleted_paths.len()
                )),
            );
        let task_registry = Arc::clone(&self.runtime_state.runtime_task_registry);
        let task_id_for_thread = task_id.clone();
        let spawn_result = std::thread::Builder::new()
            .name(format!(
                "frigg-precise-generate-{}",
                workspace.repository_id
            ))
            .spawn(move || {
                let mut succeeded = 0usize;
                let mut failed = 0usize;
                for spec in selected_generators {
                    let summary = server.run_workspace_precise_generation(&workspace, &spec);
                    match summary.status {
                        WorkspacePreciseGenerationStatus::Succeeded => succeeded += 1,
                        _ => failed += 1,
                    }
                    server.scip_cache_workspace_precise_generation(
                        &workspace.repository_id,
                        spec.generator_id,
                        summary,
                    );
                }
                server.invalidate_repository_summary_cache(&workspace.repository_id);
                server.invalidate_repository_search_response_caches(&workspace.repository_id);
                server.invalidate_repository_navigation_response_caches(&workspace.repository_id);
                server.invalidate_repository_precise_graph_caches(&workspace.repository_id);
                server.maybe_spawn_workspace_runtime_prewarm(&workspace);
                let detail = Some(format!(
                    "generators={} succeeded={} failed={}",
                    succeeded + failed,
                    succeeded,
                    failed
                ));
                tracing::info!(
                    repository_id = %workspace.repository_id,
                    root = %workspace.root.display(),
                    generators = succeeded + failed,
                    succeeded,
                    failed,
                    "workspace precise generation finished"
                );
                task_registry
                    .write()
                    .expect("runtime task registry poisoned")
                    .finish_task(
                        &task_id_for_thread,
                        if failed == 0 {
                            crate::mcp::types::RuntimeTaskStatus::Succeeded
                        } else {
                            crate::mcp::types::RuntimeTaskStatus::Failed
                        },
                        detail,
                    );
            });
        if let Err(err) = spawn_result {
            self.runtime_state
                .runtime_task_registry
                .write()
                .expect("runtime task registry poisoned")
                .finish_task(
                    &task_id,
                    crate::mcp::types::RuntimeTaskStatus::Failed,
                    Some(format!("failed to spawn precise generation thread: {err}")),
                );
        }

        WorkspacePreciseGenerationAction::Triggered
    }
}
