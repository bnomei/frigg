use super::*;
use std::ffi::OsStr;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::mcp::types::{
    RepositorySessionSummary, RepositoryWatchSummary, WorkspacePreciseFailureClass,
    WorkspacePreciseGenerationAction, WorkspacePreciseState, WorkspacePreciseSummary,
    WorkspaceRecommendedAction,
};

#[allow(dead_code)]
const PRECISE_GENERATION_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreciseGeneratorKind {
    RustAnalyzer,
    ScipGo,
    ScipTypescript,
    ScipPhp,
}

impl PreciseGeneratorKind {
    const FIRST_WAVE: [Self; 4] = [
        Self::RustAnalyzer,
        Self::ScipGo,
        Self::ScipTypescript,
        Self::ScipPhp,
    ];

    fn language(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust",
            Self::ScipGo => "go",
            Self::ScipTypescript => "typescript",
            Self::ScipPhp => "php",
        }
    }

    fn cache_key_segment(self) -> &'static str {
        self.language()
    }

    fn tool_name(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust-analyzer",
            Self::ScipGo => "scip-go",
            Self::ScipTypescript => "scip-typescript",
            Self::ScipPhp => "scip-php",
        }
    }

    fn tool_candidates(self) -> &'static [&'static str] {
        match self {
            Self::RustAnalyzer => &["rust-analyzer"],
            Self::ScipGo => &["$GOPATH/bin/scip-go", "scip-go"],
            Self::ScipTypescript => &[
                "node_modules/.bin/scip-typescript",
                "$NPM_PREFIX/bin/scip-typescript",
                "$PNPM_BIN/scip-typescript",
                "$BUN_BIN/scip-typescript",
                "scip-typescript",
            ],
            Self::ScipPhp => &["vendor/bin/scip-php", "scip-php"],
        }
    }

    fn expected_output_filename(self) -> &'static str {
        match self {
            Self::RustAnalyzer => "rust.scip",
            Self::ScipGo => "go.scip",
            Self::ScipTypescript => "typescript.scip",
            Self::ScipPhp => "php.scip",
        }
    }

    fn expected_output_path(self, root: &Path) -> PathBuf {
        root.join(".frigg/scip")
            .join(self.expected_output_filename())
    }

    fn root_markers(self) -> &'static [&'static str] {
        match self {
            Self::RustAnalyzer => &["Cargo.toml"],
            Self::ScipGo => &["go.mod"],
            Self::ScipTypescript => &["package.json", "tsconfig.json", "jsconfig.json"],
            Self::ScipPhp => &["composer.json", "composer.lock"],
        }
    }

    #[allow(dead_code)]
    fn generation_args(self) -> &'static [&'static str] {
        match self {
            Self::RustAnalyzer => &["scip", "."],
            Self::ScipGo => &[],
            Self::ScipTypescript => &["index"],
            Self::ScipPhp => &[],
        }
    }

    fn version_arg_sets(self) -> &'static [&'static [&'static str]] {
        match self {
            Self::RustAnalyzer => &[&["--version"], &["version"]],
            Self::ScipGo => &[&["version"], &["--version"]],
            Self::ScipTypescript => &[&["--version"], &["version"]],
            Self::ScipPhp => &[&["--help"], &["--version"], &["version"]],
        }
    }

    fn applies_to_workspace(self, root: &Path) -> bool {
        self.root_markers()
            .iter()
            .any(|marker| root.join(marker).exists())
    }

    #[allow(dead_code)]
    fn dirty_paths_are_relevant(self, dirty_path_hints: &[PathBuf]) -> bool {
        if dirty_path_hints.is_empty() {
            return false;
        }
        dirty_path_hints.iter().any(|path| {
            let file_name = path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            let extension = path
                .extension()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_ascii_lowercase();
            match self {
                Self::RustAnalyzer => {
                    file_name == "cargo.toml" || file_name == "cargo.lock" || extension == "rs"
                }
                Self::ScipGo => file_name == "go.mod" || file_name == "go.sum" || extension == "go",
                Self::ScipTypescript => {
                    matches!(
                        file_name.as_str(),
                        "package.json"
                            | "package-lock.json"
                            | "pnpm-lock.yaml"
                            | "yarn.lock"
                            | "tsconfig.json"
                            | "jsconfig.json"
                    ) || matches!(
                        extension.as_str(),
                        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs"
                    )
                }
                Self::ScipPhp => {
                    file_name == "composer.json"
                        || file_name == "composer.lock"
                        || file_name == "scip-php"
                        || extension == "php"
                }
            }
        })
    }
}

impl FriggMcpServer {
    fn concise_precise_failure_summary(
        tool: Option<&str>,
        failure_class: Option<WorkspacePreciseFailureClass>,
        detail: Option<&str>,
    ) -> Option<String> {
        let tool_label = tool
            .and_then(|value| value.rsplit('/').next())
            .filter(|value| !value.is_empty())
            .unwrap_or("precise generator");

        let summary = match failure_class {
            Some(WorkspacePreciseFailureClass::MissingTool) => Some("missing tool".to_owned()),
            Some(WorkspacePreciseFailureClass::ToolPanic) => Some("panic".to_owned()),
            Some(WorkspacePreciseFailureClass::ToolTimeout) => Some("timed out".to_owned()),
            Some(WorkspacePreciseFailureClass::ToolEnvFailure) => {
                Some("environment failure".to_owned())
            }
            Some(WorkspacePreciseFailureClass::ToolInvalidOutput) => {
                Some("invalid output".to_owned())
            }
            Some(WorkspacePreciseFailureClass::ToolFailed) => Some("failed".to_owned()),
            None => detail.and_then(|value| {
                value
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .map(ToOwned::to_owned)
            }),
        }?;

        if summary.starts_with(tool_label) {
            Some(summary)
        } else {
            Some(format!("{tool_label} {summary}"))
        }
    }

    fn precise_generation_cache_key(
        repository_id: &str,
        generator: PreciseGeneratorKind,
    ) -> String {
        format!("{repository_id}:{}", generator.cache_key_segment())
    }

    #[allow(dead_code)]
    fn cache_workspace_precise_generation(
        &self,
        repository_id: &str,
        generator: PreciseGeneratorKind,
        summary: &WorkspacePreciseGenerationSummary,
    ) {
        self.runtime_state
            .precise_generation_status_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(
                Self::precise_generation_cache_key(repository_id, generator),
                CachedWorkspacePreciseGeneration {
                    summary: summary.clone(),
                    generated_at: Instant::now(),
                },
            );
    }

    #[allow(dead_code)]
    pub(super) fn invalidate_repository_precise_generation_cache(&self, repository_id: &str) {
        let mut cache = self
            .runtime_state
            .precise_generation_status_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prefix = format!("{repository_id}:");
        cache.retain(|key, _| !key.starts_with(&prefix));
    }

    fn cached_workspace_precise_generation(
        &self,
        repository_id: &str,
        generator: PreciseGeneratorKind,
    ) -> Option<WorkspacePreciseGenerationSummary> {
        self.runtime_state
            .precise_generation_status_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&Self::precise_generation_cache_key(
                repository_id,
                generator,
            ))
            .map(|cached| cached.summary.clone())
    }

    fn probe_precise_generator(
        &self,
        workspace_root: &Path,
        generator: PreciseGeneratorKind,
    ) -> (
        WorkspacePreciseGeneratorState,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        for args in generator.version_arg_sets() {
            for tool in
                Self::resolve_precise_generator_tools(workspace_root, generator.tool_candidates())
            {
                let output = Command::new(&tool.command)
                    .current_dir(workspace_root)
                    .args(*args)
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .output();
                match output {
                    Ok(output) if output.status.success() => {
                        let version = String::from_utf8_lossy(&output.stdout).trim().to_owned();
                        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
                        let version = (!version.is_empty())
                            .then_some(version)
                            .or_else(|| (!stderr.is_empty()).then_some(stderr));
                        return (
                            WorkspacePreciseGeneratorState::Available,
                            Some(tool.display.clone()),
                            version,
                            None,
                        );
                    }
                    Ok(_) => continue,
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(err) => {
                        return (
                            WorkspacePreciseGeneratorState::Error,
                            None,
                            None,
                            Some(err.to_string()),
                        );
                    }
                }
            }
        }

        (
            WorkspacePreciseGeneratorState::MissingTool,
            None,
            None,
            Some(format!(
                "{} is not installed or not on PATH",
                generator.tool_name()
            )),
        )
    }

    fn workspace_precise_generator_summaries(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Vec<WorkspacePreciseGeneratorSummary> {
        let precise_config = Self::load_workspace_precise_config(&workspace.root);
        PreciseGeneratorKind::FIRST_WAVE
            .into_iter()
            .filter(|generator| generator.applies_to_workspace(&workspace.root))
            .map(|generator| {
                if Self::workspace_precise_generator_disabled(
                    &precise_config,
                    generator.cache_key_segment(),
                ) {
                    return WorkspacePreciseGeneratorSummary {
                        state: WorkspacePreciseGeneratorState::NotConfigured,
                        language: Some(generator.language().to_owned()),
                        tool: Some(generator.tool_name().to_owned()),
                        version: None,
                        expected_output_path: Some(
                            generator
                                .expected_output_path(&workspace.root)
                                .display()
                                .to_string(),
                        ),
                        last_generation: self.cached_workspace_precise_generation(
                            &workspace.repository_id,
                            generator,
                        ),
                        reason: Some("disabled_by_workspace_precise_config".to_owned()),
                    };
                }
                let (state, resolved_tool, version, reason) =
                    self.probe_precise_generator(&workspace.root, generator);
                WorkspacePreciseGeneratorSummary {
                    state,
                    language: Some(generator.language().to_owned()),
                    tool: Some(resolved_tool.unwrap_or_else(|| generator.tool_name().to_owned())),
                    version,
                    expected_output_path: Some(
                        generator
                            .expected_output_path(&workspace.root)
                            .display()
                            .to_string(),
                    ),
                    last_generation: self
                        .cached_workspace_precise_generation(&workspace.repository_id, generator),
                    reason,
                }
            })
            .collect()
    }

    #[allow(dead_code)]
    fn workspace_precise_generation_targets(
        &self,
        workspace: &AttachedWorkspace,
        dirty_path_hints: &[PathBuf],
    ) -> Vec<PreciseGeneratorKind> {
        self.workspace_precise_generator_summaries(workspace)
            .into_iter()
            .filter_map(|summary| {
                let tool = summary.tool.as_deref()?;
                let generator = PreciseGeneratorKind::FIRST_WAVE
                    .into_iter()
                    .find(|candidate| candidate.tool_name() == tool)?;
                let artifact_missing = !generator.expected_output_path(&workspace.root).is_file();
                let dirty_relevant = generator.dirty_paths_are_relevant(dirty_path_hints);
                (summary.state == WorkspacePreciseGeneratorState::Available
                    && (artifact_missing || dirty_relevant))
                    .then_some(generator)
            })
            .collect()
    }

    #[allow(dead_code)]
    fn run_command_with_timeout(
        command: &mut Command,
        timeout: Duration,
    ) -> Result<std::process::Output, String> {
        let mut child = command.spawn().map_err(|err| {
            format!(
                "failed to spawn {}: {err}",
                command.get_program().to_string_lossy()
            )
        })?;
        let started_at = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(_status)) => {
                    return child
                        .wait_with_output()
                        .map_err(|err| format!("failed to collect process output: {err}"));
                }
                Ok(None) => {
                    if started_at.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(format!("generator timed out after {}s", timeout.as_secs()));
                    }
                    thread::sleep(Duration::from_millis(100));
                }
                Err(err) => return Err(format!("failed while waiting for process: {err}")),
            }
        }
    }

    #[allow(dead_code)]
    fn now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    #[allow(dead_code)]
    fn run_precise_generator_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
        generator: PreciseGeneratorKind,
    ) -> WorkspacePreciseGenerationSummary {
        let generated_at_ms = Self::now_unix_ms();
        let final_path = generator.expected_output_path(&workspace.root);
        let output_dir = final_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| workspace.root.join(".frigg/scip"));
        let generated_root_path = workspace.root.join("index.scip");
        let backup_root_path = workspace
            .root
            .join(format!(".frigg-index-backup-{}.scip", std::process::id()));

        let (state, _resolved_tool, _version, reason) =
            self.probe_precise_generator(&workspace.root, generator);
        if state != WorkspacePreciseGeneratorState::Available {
            return WorkspacePreciseGenerationSummary {
                status: match state {
                    WorkspacePreciseGeneratorState::MissingTool => {
                        WorkspacePreciseGenerationStatus::MissingTool
                    }
                    WorkspacePreciseGeneratorState::Unsupported => {
                        WorkspacePreciseGenerationStatus::Unsupported
                    }
                    WorkspacePreciseGeneratorState::NotConfigured => {
                        WorkspacePreciseGenerationStatus::NotConfigured
                    }
                    WorkspacePreciseGeneratorState::Available => {
                        WorkspacePreciseGenerationStatus::Skipped
                    }
                    WorkspacePreciseGeneratorState::Error => {
                        WorkspacePreciseGenerationStatus::Failed
                    }
                },
                generated_at_ms,
                artifact_path: Some(final_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: reason,
            };
        }

        if let Err(err) = fs::create_dir_all(&output_dir) {
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::Failed,
                generated_at_ms,
                artifact_path: Some(final_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: Some(format!(
                    "failed to create SCIP output directory {}: {err}",
                    output_dir.display()
                )),
            };
        }

        let had_existing_root_output = generated_root_path.is_file();
        if had_existing_root_output
            && let Err(err) = fs::rename(&generated_root_path, &backup_root_path)
        {
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::Failed,
                generated_at_ms,
                artifact_path: Some(final_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: Some(format!(
                    "failed to protect existing {} before generation: {err}",
                    generated_root_path.display()
                )),
            };
        }

        let restore_existing_root_output = |restore: bool| {
            if restore && backup_root_path.is_file() {
                let _ = fs::rename(&backup_root_path, &generated_root_path);
            }
        };

        let tool_candidates: &[&str] = match generator {
            PreciseGeneratorKind::ScipPhp => &["vendor/bin/scip-php", "scip-php"],
            _ => &[generator.tool_name()],
        };
        let mut output = None;
        let mut used_tool = None;
        for tool in tool_candidates {
            let mut command = Command::new(tool);
            command
                .args(generator.generation_args())
                .current_dir(&workspace.root)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            match Self::run_command_with_timeout(&mut command, PRECISE_GENERATION_TIMEOUT) {
                Ok(result) => {
                    output = Some(result);
                    used_tool = Some(*tool);
                    break;
                }
                Err(err) if err.contains("No such file") || err.contains("not found") => continue,
                Err(err) => {
                    let status = if err.contains("timed out") {
                        WorkspacePreciseGenerationStatus::Timeout
                    } else {
                        WorkspacePreciseGenerationStatus::Failed
                    };
                    let _ = fs::remove_file(&generated_root_path);
                    restore_existing_root_output(had_existing_root_output);
                    return WorkspacePreciseGenerationSummary {
                        status,
                        generated_at_ms,
                        artifact_path: Some(final_path.display().to_string()),
                        failure_class: None,
                        recommended_action: None,
                        detail: Some(err),
                    };
                }
            }
        }
        let Some(output) = output else {
            restore_existing_root_output(had_existing_root_output);
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::MissingTool,
                generated_at_ms,
                artifact_path: Some(final_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: Some(format!(
                    "{} is not installed or not on PATH",
                    generator.tool_name()
                )),
            };
        };
        let used_tool = used_tool.unwrap_or_else(|| generator.tool_name());

        if !output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            let mut detail = format!("{} exited with status {}", used_tool, output.status);
            if !stderr.is_empty() {
                detail.push_str(&format!(": {stderr}"));
            } else if !stdout.is_empty() {
                detail.push_str(&format!(": {stdout}"));
            }
            let _ = fs::remove_file(&generated_root_path);
            restore_existing_root_output(had_existing_root_output);
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::Failed,
                generated_at_ms,
                artifact_path: Some(final_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: Some(detail),
            };
        }

        if !generated_root_path.is_file() {
            restore_existing_root_output(had_existing_root_output);
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::Failed,
                generated_at_ms,
                artifact_path: Some(final_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: Some(format!(
                    "{} completed successfully but did not produce {}",
                    used_tool,
                    generated_root_path.display()
                )),
            };
        }

        let temp_output_path = output_dir.join(format!(
            ".{}.tmp-{}",
            generator.expected_output_filename(),
            std::process::id()
        ));
        let move_result = fs::rename(&generated_root_path, &temp_output_path)
            .and_then(|_| fs::rename(&temp_output_path, &final_path));
        restore_existing_root_output(had_existing_root_output);
        if let Err(err) = move_result {
            let _ = fs::remove_file(&temp_output_path);
            return WorkspacePreciseGenerationSummary {
                status: WorkspacePreciseGenerationStatus::Failed,
                generated_at_ms,
                artifact_path: Some(final_path.display().to_string()),
                failure_class: None,
                recommended_action: None,
                detail: Some(format!(
                    "failed to publish generated SCIP artifact to {}: {err}",
                    final_path.display()
                )),
            };
        }

        WorkspacePreciseGenerationSummary {
            status: WorkspacePreciseGenerationStatus::Succeeded,
            generated_at_ms,
            artifact_path: Some(final_path.display().to_string()),
            failure_class: None,
            recommended_action: None,
            detail: Some(format!(
                "generated with {} in {}",
                used_tool,
                workspace.root.display()
            )),
        }
    }

    #[allow(dead_code)]
    fn generate_precise_artifacts_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
        dirty_path_hints: &[PathBuf],
    ) -> Result<(), String> {
        let generators = self.workspace_precise_generation_targets(workspace, dirty_path_hints);
        if generators.is_empty() {
            return Ok(());
        }
        let mut any_success = false;
        let mut details = Vec::new();
        for generator in generators {
            let summary = self.run_precise_generator_for_workspace(workspace, generator);
            self.cache_workspace_precise_generation(&workspace.repository_id, generator, &summary);
            if summary.status == WorkspacePreciseGenerationStatus::Succeeded {
                any_success = true;
            }
            details.push(format!(
                "{}:{}",
                generator.tool_name(),
                summary
                    .detail
                    .clone()
                    .unwrap_or_else(|| format!("{:?}", summary.status))
            ));
        }

        if any_success {
            self.invalidate_repository_precise_generation_cache(&workspace.repository_id);
            self.invalidate_repository_summary_cache(&workspace.repository_id);
            self.invalidate_repository_search_response_caches(&workspace.repository_id);
            self.invalidate_repository_navigation_response_caches(&workspace.repository_id);
            self.prewarm_precise_graph_for_workspace(workspace)?;
            return Ok(());
        }

        Err(details.join("; "))
    }

    #[allow(dead_code)]
    pub(super) fn maybe_spawn_workspace_runtime_prewarm_with_dirty_hints(
        &self,
        workspace: &AttachedWorkspace,
        dirty_path_hints: &[PathBuf],
    ) {
        let precise_generation_targets =
            self.workspace_precise_generation_targets(workspace, dirty_path_hints);
        if !precise_generation_targets.is_empty()
            && !self
                .runtime_state
                .runtime_task_registry
                .read()
                .expect("runtime task registry poisoned")
                .has_active_task_for_repository(
                    crate::mcp::types::RuntimeTaskKind::PreciseGenerate,
                    &workspace.repository_id,
                )
        {
            let server = self.clone();
            let workspace = workspace.clone();
            let dirty_path_hints = dirty_path_hints.to_vec();
            let task_id = self
                .runtime_state
                .runtime_task_registry
                .write()
                .expect("runtime task registry poisoned")
                .start_task(
                    crate::mcp::types::RuntimeTaskKind::PreciseGenerate,
                    workspace.repository_id.clone(),
                    "precise_generate",
                    Some(format!(
                        "precise generators: {}",
                        precise_generation_targets
                            .iter()
                            .map(|generator| generator.tool_name())
                            .collect::<Vec<_>>()
                            .join(", ")
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
                    let result = server
                        .generate_precise_artifacts_for_workspace(&workspace, &dirty_path_hints);
                    let (status, detail) = match result {
                        Ok(()) => (crate::mcp::types::RuntimeTaskStatus::Succeeded, None),
                        Err(err) => (crate::mcp::types::RuntimeTaskStatus::Failed, Some(err)),
                    };
                    task_registry
                        .write()
                        .expect("runtime task registry poisoned")
                        .finish_task(&task_id_for_thread, status, detail);
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
        }
    }

    fn workspace_has_dirty_root(&self, workspace: &AttachedWorkspace) -> bool {
        self.runtime_state
            .validated_manifest_candidate_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_dirty_root(&workspace.root)
    }

    pub(super) fn workspace_storage_summary(
        workspace: &AttachedWorkspace,
    ) -> WorkspaceStorageSummary {
        if !workspace.db_path.is_file() {
            return WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: false,
                initialized: false,
                index_state: WorkspaceStorageIndexState::MissingDb,
                error: None,
            };
        }

        let storage = Storage::new(&workspace.db_path);
        match storage.schema_version() {
            Ok(0) => WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: true,
                initialized: false,
                index_state: WorkspaceStorageIndexState::Uninitialized,
                error: None,
            },
            Ok(_) => match storage.verify() {
                Ok(_) => {
                    match storage.load_latest_manifest_for_repository(&workspace.repository_id) {
                        Ok(Some(_)) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Ready,
                            error: None,
                        },
                        Ok(None) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Uninitialized,
                            error: None,
                        },
                        Err(err) => WorkspaceStorageSummary {
                            db_path: workspace.db_path.display().to_string(),
                            exists: true,
                            initialized: true,
                            index_state: WorkspaceStorageIndexState::Error,
                            error: Some(err.to_string()),
                        },
                    }
                }
                Err(err) => WorkspaceStorageSummary {
                    db_path: workspace.db_path.display().to_string(),
                    exists: true,
                    initialized: true,
                    index_state: WorkspaceStorageIndexState::Error,
                    error: Some(err.to_string()),
                },
            },
            Err(err) => WorkspaceStorageSummary {
                db_path: workspace.db_path.display().to_string(),
                exists: true,
                initialized: false,
                index_state: WorkspaceStorageIndexState::Error,
                error: Some(err.to_string()),
            },
        }
    }

    pub(super) fn cached_repository_summary(
        &self,
        repository_id: &str,
    ) -> Option<RepositorySummary> {
        let cache = self
            .cache_state
            .repository_summary_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.get(repository_id)?;
        (entry.generated_at.elapsed() <= Self::REPOSITORY_SUMMARY_CACHE_TTL)
            .then(|| entry.summary.clone())
    }

    pub(super) fn cache_repository_summary(
        &self,
        repository_id: &str,
        summary: &RepositorySummary,
    ) {
        let mut cache = self
            .cache_state
            .repository_summary_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(
            repository_id.to_owned(),
            CachedRepositorySummary {
                summary: summary.clone(),
                generated_at: Instant::now(),
            },
        );
    }

    pub(super) fn invalidate_repository_summary_cache(&self, repository_id: &str) {
        self.cache_state
            .repository_summary_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(repository_id);
    }

    pub(super) fn repository_summary(&self, workspace: &AttachedWorkspace) -> RepositorySummary {
        let dirty_root = self.workspace_has_dirty_root(workspace);
        if !dirty_root
            && let Some(summary) = self.cached_repository_summary(&workspace.repository_id)
        {
            return summary;
        }
        if dirty_root {
            self.invalidate_repository_summary_cache(&workspace.repository_id);
        }

        let storage = Self::workspace_storage_summary(workspace);
        let health = self.workspace_index_health_summary(workspace, &storage);
        let session_adopted = self
            .session_state
            .inner
            .adopted_repository_ids
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .contains(&workspace.repository_id);
        let active_session_count = self
            .runtime_state
            .workspace_registry
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .active_session_count(&workspace.repository_id);
        let watch = self
            .runtime_state
            .watch_runtime
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .as_ref()
            .map(|runtime| runtime.lease_status(&workspace.repository_id))
            .unwrap_or_default();
        let summary = RepositorySummary {
            repository_id: workspace.repository_id.clone(),
            display_name: workspace.display_name.clone(),
            root_path: workspace.root.display().to_string(),
            session: RepositorySessionSummary {
                adopted: session_adopted,
                active_session_count,
            },
            watch: RepositoryWatchSummary {
                active: watch.active,
                lease_count: watch.lease_count,
            },
            storage: Some(storage),
            health: Some(health),
        };
        if !dirty_root {
            self.cache_repository_summary(&workspace.repository_id, &summary);
        }
        summary
    }

    pub(super) fn workspace_index_health_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexHealthSummary {
        WorkspaceIndexHealthSummary {
            lexical: self.workspace_lexical_index_summary(workspace, storage),
            semantic: self.workspace_semantic_index_summary(workspace, storage),
            scip: self.workspace_scip_index_summary(workspace),
            precise_generators: self.workspace_precise_generator_summaries(workspace),
        }
    }

    pub(super) fn workspace_repository_freshness_status(
        &self,
        workspace: &AttachedWorkspace,
        semantic_runtime: &SemanticRuntimeConfig,
    ) -> Result<crate::manifest_validation::RepositoryFreshnessStatus, String> {
        if !workspace.db_path.is_file() {
            return Ok(crate::manifest_validation::RepositoryFreshnessStatus {
                snapshot_id: None,
                manifest_entry_count: None,
                manifest: RepositoryManifestFreshness::MissingSnapshot,
                semantic: if semantic_runtime.enabled {
                    RepositorySemanticFreshness::MissingManifestSnapshot
                } else {
                    RepositorySemanticFreshness::Disabled
                },
                validated_manifest_digests: None,
                semantic_target: None,
            });
        }

        let storage = Storage::new(&workspace.db_path);
        if matches!(storage.schema_version(), Ok(0)) {
            return Ok(crate::manifest_validation::RepositoryFreshnessStatus {
                snapshot_id: None,
                manifest_entry_count: None,
                manifest: RepositoryManifestFreshness::MissingSnapshot,
                semantic: if semantic_runtime.enabled {
                    RepositorySemanticFreshness::MissingManifestSnapshot
                } else {
                    RepositorySemanticFreshness::Disabled
                },
                validated_manifest_digests: None,
                semantic_target: None,
            });
        }

        repository_freshness_status(
            &storage,
            &workspace.repository_id,
            &workspace.root,
            semantic_runtime,
            |_| false,
        )
        .map_err(|err| err.to_string())
    }

    pub(super) fn repository_response_cache_freshness(
        &self,
        workspaces: &[AttachedWorkspace],
        mode: RepositoryResponseCacheFreshnessMode,
    ) -> Result<RepositoryResponseCacheFreshness, ErrorData> {
        let semantic_runtime = self.cache_freshness_runtime(mode);
        let mut cacheable = true;
        let mut scopes = Vec::with_capacity(workspaces.len());
        let mut repositories = Vec::with_capacity(workspaces.len());

        for workspace in workspaces {
            let status = self
                .workspace_repository_freshness_status(workspace, &semantic_runtime)
                .map_err(|err| {
                    Self::internal(
                        format!(
                            "failed to compute response cache freshness for repository '{}': {err}",
                            workspace.repository_id
                        ),
                        None,
                    )
                })?;
            let dirty_root = self
                .runtime_state
                .validated_manifest_candidate_cache
                .read()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .is_dirty_root(&workspace.root);

            let manifest = Self::repository_manifest_freshness_label(&status.manifest);
            let semantic = Self::repository_semantic_freshness_label(&status.semantic);
            let snapshot_id = status.snapshot_id.clone();
            let semantic_target = status.semantic_target.clone();

            repositories.push(json!({
                "repository_id": workspace.repository_id,
                "snapshot_id": snapshot_id,
                "manifest": manifest,
                "semantic": semantic,
                "dirty_root": dirty_root,
                "provider": semantic_target.as_ref().map(|target| target.provider.clone()),
                "model": semantic_target.as_ref().map(|target| target.model.clone()),
            }));

            if dirty_root || !matches!(status.manifest, RepositoryManifestFreshness::Ready) {
                cacheable = false;
                continue;
            }
            let Some(snapshot_id) = status.snapshot_id else {
                cacheable = false;
                continue;
            };

            scopes.push(RepositoryFreshnessCacheScope {
                repository_id: workspace.repository_id.clone(),
                snapshot_id,
                semantic_state: matches!(mode, RepositoryResponseCacheFreshnessMode::SemanticAware)
                    .then(|| semantic.to_owned()),
                semantic_provider: matches!(
                    mode,
                    RepositoryResponseCacheFreshnessMode::SemanticAware
                )
                .then(|| {
                    semantic_target
                        .as_ref()
                        .map(|target| target.provider.clone())
                })
                .flatten(),
                semantic_model: matches!(mode, RepositoryResponseCacheFreshnessMode::SemanticAware)
                    .then(|| semantic_target.as_ref().map(|target| target.model.clone()))
                    .flatten(),
            });
        }

        scopes.sort();

        Ok(RepositoryResponseCacheFreshness {
            scopes: cacheable.then_some(scopes),
            basis: json!({
                "mode": mode.as_str(),
                "cacheable": cacheable,
                "repositories": repositories,
                "runtime_cache_contract": self.runtime_cache_contract_summary(&[
                    crate::mcp::server_cache::RuntimeCacheFamily::ValidatedManifestCandidate,
                    crate::mcp::server_cache::RuntimeCacheFamily::SearchTextResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::SearchHybridResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::SearchSymbolResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::GoToDefinitionResponse,
                    crate::mcp::server_cache::RuntimeCacheFamily::FindDeclarationsResponse,
                ]),
            }),
        })
    }

    fn cache_freshness_runtime(
        &self,
        mode: RepositoryResponseCacheFreshnessMode,
    ) -> SemanticRuntimeConfig {
        let mut runtime = self.config.semantic_runtime.clone();
        if matches!(mode, RepositoryResponseCacheFreshnessMode::ManifestOnly) {
            runtime.enabled = false;
        }
        runtime
    }

    fn repository_manifest_freshness_label(
        freshness: &RepositoryManifestFreshness,
    ) -> &'static str {
        match freshness {
            RepositoryManifestFreshness::MissingSnapshot => "missing_snapshot",
            RepositoryManifestFreshness::StaleSnapshot => "stale_snapshot",
            RepositoryManifestFreshness::Ready => "ready",
        }
    }

    fn repository_semantic_freshness_label(
        freshness: &RepositorySemanticFreshness,
    ) -> &'static str {
        match freshness {
            RepositorySemanticFreshness::Disabled => "disabled",
            RepositorySemanticFreshness::MissingManifestSnapshot => "missing_manifest_snapshot",
            RepositorySemanticFreshness::StaleManifestSnapshot => "stale_manifest_snapshot",
            RepositorySemanticFreshness::NoEligibleEntries => "no_eligible_entries",
            RepositorySemanticFreshness::MissingForActiveModel => "missing_for_active_model",
            RepositorySemanticFreshness::Ready => "ready",
        }
    }

    pub(super) fn workspace_manifest_entry_count(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Option<usize> {
        let db_path = resolve_provenance_db_path(&workspace.root).ok()?;
        if db_path.exists() {
            let storage = Storage::new(db_path.clone());
            if let Some(snapshot) =
                crate::manifest_validation::latest_validated_manifest_snapshot_shared(
                    &storage,
                    &workspace.repository_id,
                    &workspace.root,
                    Some(&self.runtime_state.validated_manifest_candidate_cache),
                )
            {
                return Some(snapshot.digests.len());
            }
        }

        Self::load_latest_manifest_snapshot(&workspace.root, &workspace.repository_id)
            .map(|snapshot| snapshot.entries.len())
    }

    pub(super) fn workspace_lexical_index_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexComponentSummary {
        if let Some(summary) = Self::storage_error_health_summary(storage) {
            return summary;
        }

        let mut manifest_only_runtime = self.config.semantic_runtime.clone();
        manifest_only_runtime.enabled = false;
        let freshness =
            match self.workspace_repository_freshness_status(workspace, &manifest_only_runtime) {
                Ok(freshness) => freshness,
                Err(err) => {
                    return WorkspaceIndexComponentSummary {
                        state: WorkspaceIndexComponentState::Error,
                        reason: Some(err),
                        snapshot_id: None,
                        compatible_snapshot_id: None,
                        provider: None,
                        model: None,
                        artifact_count: None,
                    };
                }
            };
        let manifest_entry_count = self.workspace_manifest_entry_count(workspace);
        let dirty_root = self.workspace_has_dirty_root(workspace);
        if dirty_root && freshness.snapshot_id.is_some() {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Stale,
                reason: Some("dirty_root".to_owned()),
                snapshot_id: freshness.snapshot_id,
                compatible_snapshot_id: None,
                provider: None,
                model: None,
                artifact_count: manifest_entry_count
                    .or_else(|| freshness.validated_manifest_digests.as_ref().map(Vec::len)),
            };
        }
        let manifest_state = freshness.manifest.clone();
        let (state, reason) = match manifest_state {
            RepositoryManifestFreshness::MissingSnapshot => (
                WorkspaceIndexComponentState::Missing,
                Some("missing_manifest_snapshot".to_owned()),
            ),
            RepositoryManifestFreshness::StaleSnapshot => (
                WorkspaceIndexComponentState::Stale,
                Some("stale_manifest_snapshot".to_owned()),
            ),
            RepositoryManifestFreshness::Ready => (WorkspaceIndexComponentState::Ready, None),
        };
        WorkspaceIndexComponentSummary {
            state,
            reason,
            snapshot_id: freshness.snapshot_id,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: match freshness.manifest {
                RepositoryManifestFreshness::MissingSnapshot => None,
                RepositoryManifestFreshness::StaleSnapshot => manifest_entry_count,
                RepositoryManifestFreshness::Ready => manifest_entry_count
                    .or_else(|| freshness.validated_manifest_digests.as_ref().map(Vec::len)),
            },
        }
    }

    pub(super) fn workspace_semantic_index_summary(
        &self,
        workspace: &AttachedWorkspace,
        storage: &WorkspaceStorageSummary,
    ) -> WorkspaceIndexComponentSummary {
        if !self.config.semantic_runtime.enabled {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Disabled,
                reason: Some("semantic_runtime_disabled".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider: None,
                model: None,
                artifact_count: None,
            };
        }

        let provider = self
            .config
            .semantic_runtime
            .provider
            .map(|value| value.as_str().to_owned());
        let model = self
            .config
            .semantic_runtime
            .normalized_model()
            .map(ToOwned::to_owned);
        if self.config.semantic_runtime.validate().is_err() || provider.is_none() || model.is_none()
        {
            return WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Error,
                reason: Some("semantic_runtime_invalid_config".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            };
        }
        if let Some(summary) = Self::storage_error_health_summary(storage) {
            return WorkspaceIndexComponentSummary {
                provider,
                model,
                ..summary
            };
        }

        let freshness = match self
            .workspace_repository_freshness_status(workspace, &self.config.semantic_runtime)
        {
            Ok(freshness) => freshness,
            Err(err) => {
                return WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Error,
                    reason: Some(err),
                    snapshot_id: None,
                    compatible_snapshot_id: None,
                    provider,
                    model,
                    artifact_count: None,
                };
            }
        };
        let storage_reader = Storage::new(&workspace.db_path);
        let provider_ref = provider
            .as_deref()
            .expect("semantic provider should exist after config validation");
        let model_ref = model
            .as_deref()
            .expect("semantic model should exist after config validation");
        let semantic_health = storage_reader
            .collect_semantic_storage_health_for_repository_model(
                &workspace.repository_id,
                provider_ref,
                model_ref,
            )
            .ok();
        let semantic_state = freshness.semantic.clone();
        match semantic_state {
            RepositorySemanticFreshness::MissingManifestSnapshot => {
                WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Missing,
                    reason: Some("missing_manifest_snapshot".to_owned()),
                    snapshot_id: None,
                    compatible_snapshot_id: None,
                    provider,
                    model,
                    artifact_count: None,
                }
            }
            RepositorySemanticFreshness::StaleManifestSnapshot => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Stale,
                reason: Some("stale_manifest_snapshot".to_owned()),
                snapshot_id: freshness.snapshot_id,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            },
            RepositorySemanticFreshness::NoEligibleEntries => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Ready,
                reason: Some("manifest_valid_no_semantic_eligible_entries".to_owned()),
                snapshot_id: freshness.snapshot_id,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: Some(0),
            },
            RepositorySemanticFreshness::Ready => {
                let snapshot_id = freshness
                    .snapshot_id
                    .expect("ready semantic freshness should carry a snapshot id");
                if semantic_health
                    .as_ref()
                    .is_some_and(|health| !health.vector_consistent)
                {
                    return WorkspaceIndexComponentSummary {
                        state: WorkspaceIndexComponentState::Error,
                        reason: Some("semantic_vector_partition_out_of_sync".to_owned()),
                        snapshot_id: Some(snapshot_id),
                        compatible_snapshot_id: None,
                        provider: provider.clone(),
                        model: model.clone(),
                        artifact_count: semantic_health
                            .as_ref()
                            .map(|health| health.live_embedding_rows),
                    };
                }
                WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Ready,
                    reason: None,
                    snapshot_id: Some(snapshot_id.clone()),
                    compatible_snapshot_id: None,
                    provider: provider.clone(),
                    model: model.clone(),
                    artifact_count: semantic_health
                        .as_ref()
                        .map(|health| health.live_embedding_rows)
                        .or_else(|| {
                            storage_reader
                                .count_semantic_embeddings_for_repository_snapshot_model(
                                    &workspace.repository_id,
                                    &snapshot_id,
                                    provider_ref,
                                    model_ref,
                                )
                                .ok()
                        }),
                }
            }
            RepositorySemanticFreshness::MissingForActiveModel => {
                let snapshot_id = freshness.snapshot_id.clone();
                WorkspaceIndexComponentSummary {
                    state: WorkspaceIndexComponentState::Missing,
                    reason: Some("semantic_snapshot_missing_for_active_model".to_owned()),
                    snapshot_id,
                    compatible_snapshot_id: None,
                    provider: provider.clone(),
                    model: model.clone(),
                    artifact_count: None,
                }
            }
            RepositorySemanticFreshness::Disabled => WorkspaceIndexComponentSummary {
                state: WorkspaceIndexComponentState::Disabled,
                reason: Some("semantic_runtime_disabled".to_owned()),
                snapshot_id: None,
                compatible_snapshot_id: None,
                provider,
                model,
                artifact_count: None,
            },
        }
    }

    pub(super) fn workspace_scip_index_summary(
        &self,
        workspace: &AttachedWorkspace,
    ) -> WorkspaceIndexComponentSummary {
        let discovery = Self::collect_scip_artifact_digests(&workspace.root);
        let artifact_count = discovery.artifact_digests.len();
        WorkspaceIndexComponentSummary {
            state: if artifact_count == 0 {
                WorkspaceIndexComponentState::Missing
            } else {
                WorkspaceIndexComponentState::Ready
            },
            reason: if artifact_count == 0 {
                Some("no_scip_artifacts_discovered".to_owned())
            } else {
                None
            },
            snapshot_id: None,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: Some(artifact_count),
        }
    }

    pub(super) fn workspace_semantic_refresh_plan(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Option<WorkspaceSemanticRefreshPlan> {
        if !self.config.semantic_runtime.enabled {
            return None;
        }

        self.config.semantic_runtime.validate().ok()?;
        let freshness = self
            .workspace_repository_freshness_status(workspace, &self.config.semantic_runtime)
            .ok()?;
        let latest_snapshot_id = freshness.snapshot_id?;
        match (freshness.manifest.clone(), freshness.semantic.clone()) {
            (RepositoryManifestFreshness::StaleSnapshot, _) => Some(WorkspaceSemanticRefreshPlan {
                latest_snapshot_id,
                reason: "stale_manifest_snapshot",
            }),
            (
                RepositoryManifestFreshness::Ready,
                RepositorySemanticFreshness::MissingForActiveModel,
            ) => Some(WorkspaceSemanticRefreshPlan {
                latest_snapshot_id,
                reason: "semantic_snapshot_missing_for_active_model",
            }),
            _ => None,
        }
    }

    pub(super) fn refresh_workspace_semantic_snapshot_with_plan(
        &self,
        workspace: &AttachedWorkspace,
        _plan: &WorkspaceSemanticRefreshPlan,
    ) -> Result<(), String> {
        let credentials = SemanticRuntimeCredentials::from_process_env();
        self.config
            .semantic_runtime
            .validate_startup(&credentials)
            .map_err(|err| err.to_string())?;

        reindex_repository_with_runtime_config(
            &workspace.repository_id,
            &workspace.root,
            &workspace.db_path,
            ReindexMode::Full,
            &self.config.semantic_runtime,
            &credentials,
        )
        .map(|_| ())
        .map_err(|err| err.to_string())
    }

    pub(super) fn maybe_refresh_workspace_semantic_snapshot(&self, workspace: &AttachedWorkspace) {
        let Some(plan) = self.workspace_semantic_refresh_plan(workspace) else {
            return;
        };
        if plan.reason != "semantic_snapshot_missing_for_active_model" {
            return;
        }
        if self
            .runtime_state
            .runtime_task_registry
            .read()
            .expect("runtime task registry poisoned")
            .has_active_task_for_repository(
                crate::mcp::types::RuntimeTaskKind::SemanticRefresh,
                &workspace.repository_id,
            )
        {
            return;
        }
        if let Err(err) = self.refresh_workspace_semantic_snapshot_with_plan(workspace, &plan) {
            warn!(
                repository_id = workspace.repository_id,
                snapshot_id = %plan.latest_snapshot_id,
                reason = plan.reason,
                error = %err,
                "workspace semantic refresh failed during attach"
            );
        }
    }

    pub(super) fn maybe_spawn_workspace_runtime_prewarm(&self, workspace: &AttachedWorkspace) {
        let semantic_plan = self.workspace_semantic_refresh_plan(workspace);
        let should_refresh_semantic = semantic_plan
            .as_ref()
            .is_some_and(|plan| plan.reason == "stale_manifest_snapshot");
        let should_prewarm_precise = !Self::collect_scip_artifact_digests(&workspace.root)
            .artifact_digests
            .is_empty();
        if !should_refresh_semantic && !should_prewarm_precise {
            return;
        }

        let semantic_refresh_already_running = should_refresh_semantic
            && self
                .runtime_state
                .runtime_task_registry
                .read()
                .expect("runtime task registry poisoned")
                .has_active_task_for_repository(
                    crate::mcp::types::RuntimeTaskKind::SemanticRefresh,
                    &workspace.repository_id,
                );

        if should_refresh_semantic && !semantic_refresh_already_running {
            let server = self.clone();
            let workspace = workspace.clone();
            let semantic_plan = semantic_plan.clone();
            let task_id = self
                .runtime_state
                .runtime_task_registry
                .write()
                .expect("runtime task registry poisoned")
                .start_task(
                    crate::mcp::types::RuntimeTaskKind::SemanticRefresh,
                    workspace.repository_id.clone(),
                    "semantic_attach_refresh",
                    semantic_plan.as_ref().map(|plan| {
                        format!(
                            "attach root {} snapshot {} reason {}",
                            workspace.root.display(),
                            plan.latest_snapshot_id,
                            plan.reason
                        )
                    }),
                );
            let task_registry = Arc::clone(&self.runtime_state.runtime_task_registry);
            let task_id_for_thread = task_id.clone();
            let spawn_result = std::thread::Builder::new()
                .name(format!(
                    "frigg-semantic-refresh-{}",
                    workspace.repository_id
                ))
                .spawn(move || {
                    let result = semantic_plan
                        .as_ref()
                        .ok_or_else(|| "missing semantic refresh plan".to_owned())
                        .and_then(|plan| {
                            server.refresh_workspace_semantic_snapshot_with_plan(&workspace, plan)
                        });
                    let (status, detail) = match result {
                        Ok(()) => (crate::mcp::types::RuntimeTaskStatus::Succeeded, None),
                        Err(err) => {
                            warn!(
                                repository_id = workspace.repository_id,
                                error = %err,
                                "workspace semantic refresh failed during runtime prewarm"
                            );
                            (crate::mcp::types::RuntimeTaskStatus::Failed, Some(err))
                        }
                    };
                    task_registry
                        .write()
                        .expect("runtime task registry poisoned")
                        .finish_task(&task_id_for_thread, status, detail);
                });
            if let Err(err) = spawn_result {
                self.runtime_state
                    .runtime_task_registry
                    .write()
                    .expect("runtime task registry poisoned")
                    .finish_task(
                        &task_id,
                        crate::mcp::types::RuntimeTaskStatus::Failed,
                        Some(format!("failed to spawn semantic prewarm thread: {err}")),
                    );
            }
        }

        if should_prewarm_precise {
            let server = self.clone();
            let workspace = workspace.clone();
            let task_id = self
                .runtime_state
                .runtime_task_registry
                .write()
                .expect("runtime task registry poisoned")
                .start_task(
                    crate::mcp::types::RuntimeTaskKind::PrecisePrewarm,
                    workspace.repository_id.clone(),
                    "precise_attach_prewarm",
                    Some(format!("attach root {}", workspace.root.display())),
                );
            let task_registry = Arc::clone(&self.runtime_state.runtime_task_registry);
            let task_id_for_thread = task_id.clone();
            let spawn_result = std::thread::Builder::new()
                .name(format!("frigg-precise-prewarm-{}", workspace.repository_id))
                .spawn(move || {
                    let result = server.prewarm_precise_graph_for_workspace(&workspace);
                    let (status, detail) = match result {
                        Ok(()) => (crate::mcp::types::RuntimeTaskStatus::Succeeded, None),
                        Err(err) => {
                            warn!(
                                repository_id = workspace.repository_id,
                                error = %err,
                                "failed to prewarm precise graph during workspace attach"
                            );
                            (crate::mcp::types::RuntimeTaskStatus::Failed, Some(err))
                        }
                    };
                    task_registry
                        .write()
                        .expect("runtime task registry poisoned")
                        .finish_task(&task_id_for_thread, status, detail);
                });
            if let Err(err) = spawn_result {
                self.runtime_state
                    .runtime_task_registry
                    .write()
                    .expect("runtime task registry poisoned")
                    .finish_task(
                        &task_id,
                        crate::mcp::types::RuntimeTaskStatus::Failed,
                        Some(format!("failed to spawn precise prewarm thread: {err}")),
                    );
            }
        }
    }

    pub(super) fn maybe_spawn_workspace_precise_generation_for_paths(
        &self,
        workspace: &AttachedWorkspace,
        changed_paths: &[String],
        deleted_paths: &[String],
    ) -> WorkspacePreciseGenerationAction {
        self.maybe_spawn_workspace_precise_generation(workspace, changed_paths, deleted_paths)
    }

    pub(super) fn workspace_precise_summary_for_workspace(
        &self,
        workspace: &AttachedWorkspace,
        generation_action: Option<WorkspacePreciseGenerationAction>,
    ) -> WorkspacePreciseSummary {
        let storage = Self::workspace_storage_summary(workspace);
        let health = self.workspace_index_health_summary(workspace, &storage);
        let default_action =
            generation_action.or(Some(WorkspacePreciseGenerationAction::NotApplicable));

        if health.scip.state == WorkspaceIndexComponentState::Ready {
            return WorkspacePreciseSummary {
                state: WorkspacePreciseState::Ok,
                failure_tool: None,
                failure_class: None,
                failure_summary: None,
                recommended_action: None,
                generation_action: default_action,
            };
        }

        let failed_generator =
            health.precise_generators.iter().find(|generator| {
                generator.last_generation.as_ref().is_some_and(|summary| {
                    summary.status == WorkspacePreciseGenerationStatus::Failed
                }) || generator.state == WorkspacePreciseGeneratorState::Error
            });
        if let Some(generator) = failed_generator {
            let last_generation = generator.last_generation.as_ref();
            let failure_class = last_generation.and_then(|summary| summary.failure_class);
            let failure_detail = last_generation
                .and_then(|summary| summary.detail.as_deref())
                .or(generator.reason.as_deref());
            return WorkspacePreciseSummary {
                state: if health.scip.state == WorkspaceIndexComponentState::Stale {
                    WorkspacePreciseState::Partial
                } else {
                    WorkspacePreciseState::Failed
                },
                failure_tool: generator.tool.clone(),
                failure_class,
                failure_summary: Self::concise_precise_failure_summary(
                    generator.tool.as_deref(),
                    failure_class,
                    failure_detail,
                ),
                recommended_action: last_generation
                    .and_then(|summary| summary.recommended_action)
                    .or(Some(WorkspaceRecommendedAction::UseHeuristicMode)),
                generation_action: default_action,
            };
        }

        let missing_tool_generator = health.precise_generators.iter().find(|generator| {
            generator.state == WorkspacePreciseGeneratorState::MissingTool
                || generator.last_generation.as_ref().is_some_and(|summary| {
                    summary.status == WorkspacePreciseGenerationStatus::MissingTool
                })
        });
        if let Some(generator) = missing_tool_generator {
            let failure_detail = generator.reason.as_deref().or_else(|| {
                generator
                    .last_generation
                    .as_ref()
                    .and_then(|summary| summary.detail.as_deref())
            });
            return WorkspacePreciseSummary {
                state: WorkspacePreciseState::Unavailable,
                failure_tool: generator.tool.clone(),
                failure_class: Some(WorkspacePreciseFailureClass::MissingTool),
                failure_summary: Self::concise_precise_failure_summary(
                    generator.tool.as_deref(),
                    Some(WorkspacePreciseFailureClass::MissingTool),
                    failure_detail,
                ),
                recommended_action: Some(WorkspaceRecommendedAction::InstallTool),
                generation_action: default_action,
            };
        }

        WorkspacePreciseSummary {
            state: if health.scip.state == WorkspaceIndexComponentState::Stale {
                WorkspacePreciseState::Partial
            } else {
                WorkspacePreciseState::Unavailable
            },
            failure_tool: None,
            failure_class: None,
            failure_summary: health.scip.reason.clone(),
            recommended_action: Some(WorkspaceRecommendedAction::UseHeuristicMode),
            generation_action: default_action,
        }
    }

    pub(super) fn storage_error_health_summary(
        storage: &WorkspaceStorageSummary,
    ) -> Option<WorkspaceIndexComponentSummary> {
        let (state, reason) = match storage.index_state {
            WorkspaceStorageIndexState::MissingDb => (
                WorkspaceIndexComponentState::Missing,
                Some("missing_db".to_owned()),
            ),
            WorkspaceStorageIndexState::Uninitialized => (
                WorkspaceIndexComponentState::Missing,
                Some(if storage.initialized {
                    "missing_manifest_snapshot".to_owned()
                } else {
                    "uninitialized_db".to_owned()
                }),
            ),
            WorkspaceStorageIndexState::Ready => return None,
            WorkspaceStorageIndexState::Error => (
                WorkspaceIndexComponentState::Error,
                storage
                    .error
                    .clone()
                    .or_else(|| Some("storage_error".to_owned())),
            ),
        };
        Some(WorkspaceIndexComponentSummary {
            state,
            reason,
            snapshot_id: None,
            compatible_snapshot_id: None,
            provider: None,
            model: None,
            artifact_count: None,
        })
    }
}
