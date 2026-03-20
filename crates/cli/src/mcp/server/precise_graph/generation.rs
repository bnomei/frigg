use super::*;

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
                quiet_arg: None,
            },
            PreciseGeneratorSpec {
                language: SymbolLanguage::Go,
                generator_id: "go",
                tool_name: "scip-go",
                tool_candidates: &["$GOPATH/bin/scip-go", "scip-go"],
                version_arg_sets: &[&["version"], &["--version"]],
                generate_args: &[],
                infer_tsconfig: false,
                trigger_markers: &["go.mod", "go.sum"],
                output_artifact_name: "go.scip",
                stdout_artifact_fallback: false,
                quiet_arg: Some("-q"),
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
                quiet_arg: None,
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
                quiet_arg: None,
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
                generate_args: &["index", "."],
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
                quiet_arg: Some("--quiet"),
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
                quiet_arg: None,
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
        let mut args = spec
            .generate_args
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>();

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
        let generated_at_ms = Self::scip_now_unix_ms();
        let precise_config = Self::load_workspace_precise_config(&workspace.root);
        if Self::workspace_precise_generator_disabled(&precise_config, spec.generator_id) {
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::NotConfigured,
                generated_at_ms,
                artifact_path: None,
                failure_class: None,
                recommended_action: None,
                detail: Some("disabled_by_workspace_precise_config".to_owned()),
            };
        }
        if !Self::workspace_has_precise_generator_markers(&workspace.root, spec) {
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::NotConfigured,
                generated_at_ms,
                artifact_path: None,
                failure_class: None,
                recommended_action: None,
                detail: Some("missing_language_markers".to_owned()),
            };
        }

        let (tool, version) = match Self::probe_precise_generator_tool(&workspace.root, spec) {
            Ok((tool, version)) => (tool, version),
            Err(PreciseToolProbeError::MissingTool) => {
                let tool_candidates =
                    Self::precise_generator_tool_candidates(&workspace.root, spec);
                return WorkspacePreciseGenerationSummary {
                    status: WorkspacePreciseGenerationStatus::MissingTool,
                    generated_at_ms,
                    artifact_path: None,
                    failure_class: Some(WorkspacePreciseFailureClass::MissingTool),
                    recommended_action: Some(WorkspaceRecommendedAction::InstallTool),
                    detail: Some(format!(
                        "precise generator tool '{}' is not installed",
                        tool_candidates.join(" or ")
                    )),
                };
            }
            Err(PreciseToolProbeError::Failed(error)) => {
                return Self::precise_failed_summary(generated_at_ms, None, error);
            }
        };

        let output_dir = workspace.root.join(".frigg").join("scip");
        if let Err(err) = fs::create_dir_all(&output_dir) {
            return Self::precise_failed_summary(
                generated_at_ms,
                None,
                format!(
                    "failed to prepare SCIP artifact directory {}: {err}",
                    output_dir.display()
                ),
            );
        }

        if let Err(detail) =
            Self::maybe_patch_repo_local_scip_php_vendor_dir(&workspace.root, spec, &tool)
        {
            return Self::precise_failed_summary(generated_at_ms, None, detail);
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
                Err(detail) => return Self::precise_failed_summary(generated_at_ms, None, detail),
            },
            None => None,
        };
        let generator_workspace_root = filtered_generation_root
            .as_deref()
            .unwrap_or(&workspace.root)
            .to_path_buf();
        let generator_extra_args =
            Self::workspace_precise_generator_extra_args(&precise_config, spec.generator_id);

        let generation_result = (|| {
            let mut command = Command::new(&tool.command);
            command
                .current_dir(&generator_workspace_root)
                .stdin(Stdio::null())
                .stderr(Stdio::piped())
                .stdout(Stdio::piped());
            if let Some(quiet_arg) = spec.quiet_arg {
                command.arg(quiet_arg);
            }
            let output_path = output_dir.join(spec.output_artifact_name);
            let _ = fs::remove_file(&output_path);
            if spec.language == SymbolLanguage::Go {
                let output_path_arg = output_path.to_string_lossy().to_string();
                command.args(["-o", output_path_arg.as_str()]);
            }
            command.args(Self::precise_generation_command_args(
                workspace,
                &generator_workspace_root,
                spec,
            ));
            if !generator_extra_args.is_empty() {
                command.args(generator_extra_args.iter());
            }
            Self::apply_generator_environment(&mut command, workspace, spec);

            let output = match command.output() {
                Ok(output) => output,
                Err(err) => {
                    if err.kind() == std::io::ErrorKind::NotFound {
                        return WorkspacePreciseGenerationSummary {
                            status: WorkspacePreciseGenerationStatus::MissingTool,
                            generated_at_ms,
                            artifact_path: None,
                            failure_class: Some(WorkspacePreciseFailureClass::MissingTool),
                            recommended_action: Some(WorkspaceRecommendedAction::InstallTool),
                            detail: Some(err.to_string()),
                        };
                    }
                    return Self::precise_failed_summary(generated_at_ms, None, err.to_string());
                }
            };

            if !output.status.success() {
                let stderr_detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                return Self::precise_failed_summary(
                    generated_at_ms,
                    None,
                    if stderr_detail.is_empty() {
                        format!(
                            "generator '{}' exited unsuccessfully (version={version})",
                            tool.display
                        )
                    } else {
                        stderr_detail
                    },
                );
            }

            let artifact_path = match Self::write_precise_artifact(
                &generator_workspace_root,
                &output_dir,
                spec.output_artifact_name,
                &output.stdout,
                spec.stdout_artifact_fallback,
            ) {
                Ok(Some(path)) => path,
                Ok(None) => {
                    return Self::precise_failed_summary(
                        generated_at_ms,
                        None,
                        format!(
                            "generator '{}' succeeded but no SCIP artifact was produced",
                            tool.display
                        ),
                    );
                }
                Err(detail) => {
                    return Self::precise_failed_summary(generated_at_ms, None, detail);
                }
            };

            WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::Succeeded,
                generated_at_ms,
                artifact_path: Some(artifact_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: Some(format!(
                    "generator={} tool={} version={version}",
                    spec.generator_id, tool.display
                )),
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

        generation_result
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
            return WorkspacePreciseGenerationAction::SkippedActiveTask;
        }

        let server = self.clone();
        let workspace = workspace.clone();
        let selected_generators = selected.to_vec();
        let changed_paths = changed_paths.to_vec();
        let deleted_paths = deleted_paths.to_vec();
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
                server.maybe_spawn_workspace_runtime_prewarm(&workspace);
                let detail = Some(format!(
                    "generators={} succeeded={} failed={}",
                    succeeded + failed,
                    succeeded,
                    failed
                ));
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
