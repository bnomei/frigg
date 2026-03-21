use super::*;
use std::process::{Command, Stdio};
use std::thread;

impl FriggMcpServer {
    pub(in crate::mcp::server) fn invalidate_repository_precise_generator_probe_cache(
        &self,
        repository_id: &str,
    ) {
        self.cache_state
            .precise_generator_probe_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|key, _| key.repository_id != repository_id);
    }

    fn cached_precise_generator_probe(
        &self,
        repository_id: &str,
        generator_id: &str,
    ) -> Option<CachedPreciseGeneratorProbe> {
        let cache = self
            .cache_state
            .precise_generator_probe_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = cache.get(&PreciseGeneratorProbeCacheKey {
            repository_id: repository_id.to_owned(),
            generator_id: generator_id.to_owned(),
        })?;
        (entry.generated_at.elapsed() <= Self::PRECISE_GENERATOR_PROBE_CACHE_TTL)
            .then(|| entry.clone())
    }

    fn cache_precise_generator_probe(
        &self,
        repository_id: &str,
        generator_id: &str,
        state: WorkspacePreciseGeneratorState,
        tool: Option<String>,
        version: Option<String>,
        reason: Option<String>,
    ) {
        let mut cache = self
            .cache_state
            .precise_generator_probe_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.insert(
            PreciseGeneratorProbeCacheKey {
                repository_id: repository_id.to_owned(),
                generator_id: generator_id.to_owned(),
            },
            CachedPreciseGeneratorProbe {
                state,
                tool,
                version,
                reason,
                generated_at: Instant::now(),
            },
        );
        while cache.len() > Self::PRECISE_GENERATOR_PROBE_CACHE_MAX_ENTRIES {
            let _ = cache.pop_first();
        }
    }

    pub(in crate::mcp::server) fn concise_precise_failure_summary(
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

    #[allow(dead_code)]
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
        let tool_candidates = generator.tool_candidates(workspace_root);
        for args in generator.version_arg_sets() {
            for tool in Self::resolve_precise_generator_tools(workspace_root, &tool_candidates) {
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

    pub(in crate::mcp::server) fn workspace_precise_generator_summaries(
        &self,
        workspace: &AttachedWorkspace,
    ) -> Vec<WorkspacePreciseGeneratorSummary> {
        let precise_config = Self::load_workspace_precise_config(&workspace.root);
        Self::precise_generator_specs()
            .into_iter()
            .filter(|spec| Self::workspace_has_precise_generator_markers(&workspace.root, spec))
            .map(|spec| {
                if Self::workspace_precise_generator_disabled(&precise_config, spec.generator_id) {
                    return WorkspacePreciseGeneratorSummary {
                        state: WorkspacePreciseGeneratorState::NotConfigured,
                        language: Some(Self::precise_generator_language_label(&spec).to_owned()),
                        tool: Some(spec.tool_name.to_owned()),
                        version: None,
                        expected_output_path: Some(
                            Self::precise_generator_expected_output_path(&workspace.root, &spec)
                                .display()
                                .to_string(),
                        ),
                        last_generation: self.scip_cached_workspace_precise_generation(
                            &workspace.repository_id,
                            spec.generator_id,
                        ),
                        reason: Some("disabled_by_workspace_precise_config".to_owned()),
                    };
                }
                let cached_probe = self
                    .cached_precise_generator_probe(&workspace.repository_id, spec.generator_id);
                let (state, resolved_tool, version, reason) = if let Some(cached) = cached_probe {
                    (cached.state, cached.tool, cached.version, cached.reason)
                } else {
                    let (state, resolved_tool, version, reason) =
                        match Self::probe_precise_generator_tool(&workspace.root, &spec) {
                            Ok((tool, version)) => (
                                WorkspacePreciseGeneratorState::Available,
                                Some(tool.display),
                                Some(version),
                                None,
                            ),
                            Err(super::precise_graph::PreciseToolProbeError::MissingTool) => (
                                WorkspacePreciseGeneratorState::MissingTool,
                                None,
                                None,
                                Some(format!(
                                    "{} is not installed or not on PATH",
                                    spec.tool_name
                                )),
                            ),
                            Err(super::precise_graph::PreciseToolProbeError::Failed(error)) => (
                                WorkspacePreciseGeneratorState::Error,
                                None,
                                None,
                                Some(error),
                            ),
                        };
                    self.cache_precise_generator_probe(
                        &workspace.repository_id,
                        spec.generator_id,
                        state,
                        resolved_tool.clone(),
                        version.clone(),
                        reason.clone(),
                    );
                    (state, resolved_tool, version, reason)
                };
                WorkspacePreciseGeneratorSummary {
                    state,
                    language: Some(Self::precise_generator_language_label(&spec).to_owned()),
                    tool: Some(resolved_tool.unwrap_or_else(|| spec.tool_name.to_owned())),
                    version,
                    expected_output_path: Some(
                        Self::precise_generator_expected_output_path(&workspace.root, &spec)
                            .display()
                            .to_string(),
                    ),
                    last_generation: self.scip_cached_workspace_precise_generation(
                        &workspace.repository_id,
                        spec.generator_id,
                    ),
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
                let language = summary.language.as_deref()?;
                let generator = PreciseGeneratorKind::FIRST_WAVE
                    .into_iter()
                    .find(|candidate| candidate.language() == language)?;
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
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
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
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
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
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
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

        let tool_candidates = generator.tool_candidates(&workspace.root);
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
                    used_tool = Some(tool);
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
                        artifact_count: None,
                        artifact_sample_paths: Vec::new(),
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
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
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
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
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
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
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
                artifact_count: None,
                artifact_sample_paths: Vec::new(),
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
            artifact_count: None,
            artifact_sample_paths: Vec::new(),
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

    pub(in crate::mcp::server) fn workspace_has_dirty_root(
        &self,
        workspace: &AttachedWorkspace,
    ) -> bool {
        self.runtime_state
            .validated_manifest_candidate_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .is_dirty_root(&workspace.root)
    }
}
