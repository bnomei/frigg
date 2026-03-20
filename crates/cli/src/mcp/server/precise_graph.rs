use super::*;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::process::{Command, Stdio};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use walkdir::WalkDir;

use crate::mcp::types::{
    WorkspacePreciseFailureClass, WorkspacePreciseGenerationAction, WorkspaceRecommendedAction,
};

#[cfg(test)]
static TEST_PRECISE_GENERATOR_BIN_OVERRIDE: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

const PRECISE_WORKSPACE_CONFIG_FILE: &str = ".frigg/precise.json";

#[derive(Debug, Default, Clone, Deserialize)]
struct WorkspacePreciseConfigFile {
    #[serde(default)]
    precise: WorkspacePreciseConfig,
}

#[derive(Debug, Default, Clone, Deserialize)]
pub(super) struct WorkspacePreciseConfig {
    #[serde(default)]
    disabled_generators: Vec<String>,
    #[serde(default)]
    generation_excludes: Vec<String>,
    #[serde(default)]
    ingest_excludes: Vec<String>,
    #[serde(default)]
    generator_extra_args: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Copy)]
struct PreciseGeneratorSpec {
    language: SymbolLanguage,
    generator_id: &'static str,
    tool_candidates: &'static [&'static str],
    version_args: &'static [&'static str],
    generate_args: &'static [&'static str],
    infer_tsconfig: bool,
    trigger_markers: &'static [&'static str],
    output_artifact_name: &'static str,
    stdout_artifact_fallback: bool,
    quiet_arg: Option<&'static str>,
}

#[derive(Debug)]
enum PreciseToolProbeError {
    MissingTool,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedPreciseGeneratorTool {
    pub(super) command: String,
    pub(super) display: String,
}

impl FriggMcpServer {
    pub(super) fn load_workspace_precise_config(root: &Path) -> WorkspacePreciseConfig {
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

    pub(super) fn workspace_precise_generator_disabled(
        config: &WorkspacePreciseConfig,
        generator_id: &str,
    ) -> bool {
        config
            .disabled_generators
            .iter()
            .any(|value| value.eq_ignore_ascii_case(generator_id))
    }

    fn workspace_precise_generator_extra_args(
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

    fn compile_workspace_precise_exclude_matcher(
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

    fn workspace_precise_excludes_path(
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

    fn create_precise_generation_workspace(
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
    pub(super) fn set_test_precise_generator_bin_override(bin_dir: Option<PathBuf>) {
        *TEST_PRECISE_GENERATOR_BIN_OVERRIDE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("test precise generator override lock should not be poisoned") = bin_dir;
    }

    #[cfg(test)]
    fn test_precise_generator_bin_override() -> Option<PathBuf> {
        TEST_PRECISE_GENERATOR_BIN_OVERRIDE
            .get_or_init(|| Mutex::new(None))
            .lock()
            .expect("test precise generator override lock should not be poisoned")
            .clone()
    }

    fn precise_generator_specs() -> [PreciseGeneratorSpec; 4] {
        [
            PreciseGeneratorSpec {
                language: SymbolLanguage::Rust,
                generator_id: "rust",
                tool_candidates: &["rust-analyzer"],
                version_args: &["--version"],
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
                tool_candidates: &["$GOPATH/bin/scip-go", "scip-go"],
                version_args: &["--version"],
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
                tool_candidates: &[
                    "node_modules/.bin/scip-typescript",
                    "$NPM_PREFIX/bin/scip-typescript",
                    "$PNPM_BIN/scip-typescript",
                    "$BUN_BIN/scip-typescript",
                    "scip-typescript",
                ],
                version_args: &["--version"],
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
                tool_candidates: &["vendor/bin/scip-php", "scip-php"],
                version_args: &["--help"],
                generate_args: &[],
                infer_tsconfig: false,
                trigger_markers: &["composer.json", "composer.lock"],
                output_artifact_name: "php.scip",
                stdout_artifact_fallback: true,
                quiet_arg: None,
            },
        ]
    }

    fn scip_precise_generation_cache_key(repository_id: &str, generator_id: &str) -> String {
        format!("{repository_id}:{generator_id}")
    }

    fn scip_now_unix_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or(0)
    }

    fn workspace_has_precise_generator_markers(
        workspace_root: &Path,
        spec: &PreciseGeneratorSpec,
    ) -> bool {
        spec.trigger_markers
            .iter()
            .any(|marker| workspace_root.join(marker).exists())
    }

    fn scip_cached_workspace_precise_generation(
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

    pub(super) fn scip_invalidate_repository_precise_generation_cache(&self, repository_id: &str) {
        let mut cache = self
            .runtime_state
            .precise_generation_status_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let prefix = format!("{repository_id}:");
        cache.retain(|key, _| !key.starts_with(&prefix));
    }

    pub(super) fn invalidate_repository_precise_graph_caches(&self, repository_id: &str) {
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

    pub(super) fn resolve_precise_generator_tools(
        workspace_root: &Path,
        tool_candidates: &[&str],
    ) -> Vec<ResolvedPreciseGeneratorTool> {
        let mut resolved = Vec::new();
        let mut seen = BTreeSet::new();
        for candidate in tool_candidates {
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
                        continue;
                    }
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

    fn probe_precise_generator_tool(
        workspace_root: &Path,
        tool_candidates: &[&str],
        version_args: &[&str],
    ) -> Result<(ResolvedPreciseGeneratorTool, String), PreciseToolProbeError> {
        for tool in Self::resolve_precise_generator_tools(workspace_root, tool_candidates) {
            let output = Command::new(&tool.command)
                .current_dir(workspace_root)
                .args(version_args)
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
                return Err(PreciseToolProbeError::Failed(
                    String::from_utf8_lossy(&output.stderr).trim().to_owned(),
                ));
            }
            if version.is_empty() {
                version = "unknown".to_owned();
            }
            return Ok((tool, version));
        }
        Err(PreciseToolProbeError::MissingTool)
    }

    fn generator_dirty_path_matches(spec: &PreciseGeneratorSpec, path: &str) -> bool {
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
            _ => false,
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

        let (tool, version) = match Self::probe_precise_generator_tool(
            &workspace.root,
            spec.tool_candidates,
            spec.version_args,
        ) {
            Ok((tool, version)) => (tool, version),
            Err(PreciseToolProbeError::MissingTool) => {
                return WorkspacePreciseGenerationSummary {
                    status: WorkspacePreciseGenerationStatus::MissingTool,
                    generated_at_ms,
                    artifact_path: None,
                    failure_class: Some(WorkspacePreciseFailureClass::MissingTool),
                    recommended_action: Some(WorkspaceRecommendedAction::InstallTool),
                    detail: Some(format!(
                        "precise generator tool '{}' is not installed",
                        spec.tool_candidates.join(" or ")
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
            command.args(spec.generate_args);
            if spec.infer_tsconfig {
                let has_tsconfig = generator_workspace_root.join("tsconfig.json").is_file()
                    || generator_workspace_root.join("jsconfig.json").is_file();
                if !has_tsconfig {
                    command.arg("--infer-tsconfig");
                }
            }
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
                &workspace.root,
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

    pub(super) fn maybe_spawn_workspace_precise_generation(
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

    pub(super) fn scip_candidate_directories(root: &Path) -> [PathBuf; 1] {
        [root.join(".frigg/scip")]
    }

    pub(super) fn system_time_to_unix_nanos(system_time: SystemTime) -> Option<u64> {
        system_time
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|duration| u64::try_from(duration.as_nanos()).ok())
    }

    pub(super) fn root_signature(file_digests: &[FileMetadataDigest]) -> String {
        let mut hasher = DeterministicSignatureHasher::new();
        for digest in file_digests {
            hasher.write_str(&digest.path.to_string_lossy());
            hasher.write_u64(digest.size_bytes);
            hasher.write_optional_u64(digest.mtime_ns);
        }
        hasher.finish_hex()
    }

    pub(super) fn scip_signature(artifact_digests: &[ScipArtifactDigest]) -> String {
        let mut hasher = DeterministicSignatureHasher::new();
        for artifact in artifact_digests {
            hasher.write_str(&artifact.path.to_string_lossy());
            hasher.write_str(artifact.format.as_str());
            hasher.write_u64(artifact.size_bytes);
            hasher.write_optional_u64(artifact.mtime_ns);
        }
        hasher.finish_hex()
    }

    pub(super) fn collect_scip_artifact_digests(root: &Path) -> ScipArtifactDiscovery {
        let mut artifacts = Vec::new();
        let mut candidate_directories = Vec::new();
        let mut candidate_directory_digests = Vec::new();
        for directory in Self::scip_candidate_directories(root) {
            candidate_directories.push(directory.display().to_string());
            let directory_metadata = fs::metadata(&directory).ok();
            let directory_mtime_ns = directory_metadata
                .as_ref()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(Self::system_time_to_unix_nanos);
            candidate_directory_digests.push(ScipCandidateDirectoryDigest {
                path: directory.clone(),
                exists: directory_metadata.is_some(),
                mtime_ns: directory_mtime_ns,
            });
            let read_dir = match fs::read_dir(&directory) {
                Ok(read_dir) => read_dir,
                Err(_) => continue,
            };

            for entry in read_dir {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(_) => continue,
                };
                let path = entry.path();
                let Some(format) = ScipArtifactFormat::from_path(&path) else {
                    continue;
                };
                let metadata = match entry.metadata() {
                    Ok(metadata) => metadata,
                    Err(_) => continue,
                };
                if !metadata.is_file() {
                    continue;
                }
                let mtime_ns = metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_unix_nanos);
                artifacts.push(ScipArtifactDigest {
                    path,
                    format,
                    size_bytes: metadata.len(),
                    mtime_ns,
                });
            }
        }

        artifacts.sort_by(|left, right| {
            left.path
                .cmp(&right.path)
                .then(left.size_bytes.cmp(&right.size_bytes))
                .then(left.mtime_ns.cmp(&right.mtime_ns))
        });
        artifacts.dedup_by(|left, right| left.path == right.path);
        ScipArtifactDiscovery {
            candidate_directories,
            candidate_directory_digests,
            artifact_digests: artifacts,
        }
    }

    fn ingest_precise_artifacts_for_repository(
        graph: &mut SymbolGraph,
        workspace_root: &Path,
        repository_id: &str,
        discovery: &ScipArtifactDiscovery,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<PreciseIngestStats, ErrorData> {
        let precise_config = Self::load_workspace_precise_config(workspace_root);
        let ingest_matcher = Self::compile_workspace_precise_exclude_matcher(
            workspace_root,
            &precise_config.ingest_excludes,
        );
        let artifact_digests = discovery
            .artifact_digests
            .iter()
            .filter(|digest| {
                !Self::workspace_precise_excludes_path(
                    workspace_root,
                    &digest.path,
                    ingest_matcher.as_ref(),
                    false,
                )
            })
            .collect::<Vec<_>>();
        let discovered_bytes = artifact_digests
            .iter()
            .fold(0u64, |acc, digest| acc.saturating_add(digest.size_bytes));
        let mut stats = PreciseIngestStats {
            candidate_directories: discovery.candidate_directories.clone(),
            discovered_artifacts: artifact_digests
                .iter()
                .take(Self::PRECISE_DISCOVERY_SAMPLE_LIMIT)
                .map(|digest| digest.path.display().to_string())
                .collect(),
            artifacts_discovered: artifact_digests.len(),
            artifacts_discovered_bytes: discovered_bytes,
            ..PreciseIngestStats::default()
        };
        let max_artifacts = Self::usize_to_u64(budgets.scip_max_artifacts);
        if stats.artifacts_discovered > budgets.scip_max_artifacts {
            return Err(Self::find_references_resource_budget_error(
                "scip",
                "scip_artifact_count",
                "find_references SCIP artifact count exceeds configured budget",
                json!({
                    "repository_id": repository_id,
                    "actual": Self::usize_to_u64(stats.artifacts_discovered),
                    "limit": max_artifacts,
                }),
            ));
        }

        let max_artifact_bytes = Self::usize_to_u64(budgets.scip_max_artifact_bytes);
        let max_total_bytes = Self::usize_to_u64(budgets.scip_max_total_bytes);
        if discovered_bytes > max_total_bytes {
            warn!(
                repository_id,
                discovered_bytes,
                max_total_bytes,
                "scip discovery bytes exceed configured budget; precise ingest may degrade to heuristic fallback"
            );
        }

        let started_at = Instant::now();
        let max_elapsed = Duration::from_millis(budgets.scip_max_elapsed_ms);
        let mut processed_artifacts = 0usize;
        let mut processed_bytes = 0u64;

        for artifact_digest in artifact_digests {
            if started_at.elapsed() > max_elapsed {
                let elapsed_ms =
                    u64::try_from(started_at.elapsed().as_millis()).unwrap_or(u64::MAX);
                warn!(
                    repository_id,
                    actual_elapsed_ms = elapsed_ms,
                    limit_elapsed_ms = budgets.scip_max_elapsed_ms,
                    processed_artifacts,
                    bytes_processed = processed_bytes,
                    "scip processing exceeded time budget; degrading precise path to heuristic fallback"
                );
                Self::push_precise_failure_sample(
                    &mut stats,
                    "<scip-processing-budget>".to_owned(),
                    "ingest_budget_elapsed_ms",
                    format!(
                        "scip processing elapsed_ms={} exceeded limit={} after processing {} artifacts and {} bytes",
                        elapsed_ms,
                        budgets.scip_max_elapsed_ms,
                        processed_artifacts,
                        processed_bytes
                    ),
                );
                break;
            }

            if artifact_digest.size_bytes > max_artifact_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    actual_bytes = artifact_digest.size_bytes,
                    limit_bytes = max_artifact_bytes,
                    "skipping scip artifact that exceeds per-file byte budget"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes = stats
                    .artifacts_failed_bytes
                    .saturating_add(artifact_digest.size_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "artifact_budget_bytes",
                    format!(
                        "artifact bytes {} exceed configured per-file limit {}",
                        artifact_digest.size_bytes, max_artifact_bytes
                    ),
                );
                continue;
            }
            let projected_processed_bytes =
                processed_bytes.saturating_add(artifact_digest.size_bytes);
            if projected_processed_bytes > max_total_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    projected_processed_bytes,
                    limit_bytes = max_total_bytes,
                    "skipping scip artifact because cumulative byte budget would be exceeded"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes = stats
                    .artifacts_failed_bytes
                    .saturating_add(artifact_digest.size_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "artifact_budget_total_bytes",
                    format!(
                        "projected cumulative bytes {} exceed configured total limit {}",
                        projected_processed_bytes, max_total_bytes
                    ),
                );
                continue;
            }
            processed_bytes = projected_processed_bytes;

            let payload = match fs::read(&artifact_digest.path) {
                Ok(payload) => payload,
                Err(err) => {
                    warn!(
                        repository_id,
                        path = %artifact_digest.path.display(),
                        error = %err,
                        "failed to read scip artifact payload while resolving references"
                    );
                    stats.artifacts_failed += 1;
                    stats.artifacts_failed_bytes = stats
                        .artifacts_failed_bytes
                        .saturating_add(artifact_digest.size_bytes);
                    Self::push_precise_failure_sample(
                        &mut stats,
                        artifact_digest.path.display().to_string(),
                        "read_payload",
                        err.to_string(),
                    );
                    continue;
                }
            };
            let payload_bytes = Self::usize_to_u64(payload.len());
            if payload_bytes > max_artifact_bytes {
                warn!(
                    repository_id,
                    path = %artifact_digest.path.display(),
                    actual_bytes = payload_bytes,
                    limit_bytes = max_artifact_bytes,
                    "skipping scip artifact payload that exceeds per-file byte budget after read"
                );
                stats.artifacts_failed += 1;
                stats.artifacts_failed_bytes =
                    stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                Self::push_precise_failure_sample(
                    &mut stats,
                    artifact_digest.path.display().to_string(),
                    "payload_budget_bytes",
                    format!(
                        "payload bytes {} exceed configured per-file limit {}",
                        payload_bytes, max_artifact_bytes
                    ),
                );
                continue;
            }

            let artifact_label = artifact_digest.path.to_string_lossy().into_owned();
            let ingest_result = match artifact_digest.format {
                ScipArtifactFormat::Json => graph.overlay_scip_json_with_budgets(
                    repository_id,
                    &artifact_label,
                    &payload,
                    ScipResourceBudgets {
                        max_payload_bytes: budgets.scip_max_artifact_bytes,
                        max_documents: budgets.scip_max_documents_per_artifact,
                        max_elapsed_ms: budgets.scip_max_elapsed_ms,
                    },
                ),
                ScipArtifactFormat::Protobuf => graph.overlay_scip_protobuf_with_budgets(
                    repository_id,
                    &artifact_label,
                    &payload,
                    ScipResourceBudgets {
                        max_payload_bytes: budgets.scip_max_artifact_bytes,
                        max_documents: budgets.scip_max_documents_per_artifact,
                        max_elapsed_ms: budgets.scip_max_elapsed_ms,
                    },
                ),
            };
            match ingest_result {
                Ok(_) => {
                    stats.artifacts_ingested += 1;
                    stats.artifacts_ingested_bytes =
                        stats.artifacts_ingested_bytes.saturating_add(payload_bytes);
                }
                Err(err) => {
                    if let ScipIngestError::ResourceBudgetExceeded { diagnostic } = &err {
                        warn!(
                            repository_id,
                            path = %artifact_digest.path.display(),
                            budget_code = diagnostic.code.as_str(),
                            actual = diagnostic.actual,
                            limit = diagnostic.limit,
                            detail = %diagnostic.message,
                            "scip ingest exceeded resource budget; degrading precise path to heuristic fallback"
                        );
                        stats.artifacts_failed += 1;
                        stats.artifacts_failed_bytes =
                            stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                        Self::push_precise_failure_sample(
                            &mut stats,
                            artifact_digest.path.display().to_string(),
                            &format!("ingest_budget_{}", diagnostic.code.as_str()),
                            format!(
                                "ingest budget {} exceeded (actual={}, limit={}): {}",
                                diagnostic.code.as_str(),
                                diagnostic.actual,
                                diagnostic.limit,
                                diagnostic.message
                            ),
                        );
                        continue;
                    }
                    warn!(
                        repository_id,
                        path = %artifact_digest.path.display(),
                        error = %err,
                        "failed to ingest scip artifact while resolving references"
                    );
                    stats.artifacts_failed += 1;
                    stats.artifacts_failed_bytes =
                        stats.artifacts_failed_bytes.saturating_add(payload_bytes);
                    Self::push_precise_failure_sample(
                        &mut stats,
                        artifact_digest.path.display().to_string(),
                        "ingest_payload",
                        err.to_string(),
                    );
                }
            }
            processed_artifacts = processed_artifacts.saturating_add(1);
        }

        Ok(stats)
    }

    fn try_reuse_cached_precise_graph(
        &self,
        corpus: &RepositorySymbolCorpus,
    ) -> Option<CachedPreciseGraph> {
        let cached = self
            .cache_state
            .latest_precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&corpus.repository_id)
            .cloned()?;
        if cached.corpus_signature != corpus.root_signature {
            return None;
        }
        if !Self::cached_scip_discovery_is_current(&corpus.root, &cached.discovery) {
            return None;
        }
        Some((*cached).clone())
    }

    pub(super) fn try_reuse_latest_precise_graph_for_repository(
        &self,
        repository_id: &str,
        root: &Path,
    ) -> Option<CachedPreciseGraph> {
        let current_root_signature =
            Self::current_root_signature_for_repository(root, repository_id)?;
        let cached = self
            .cache_state
            .latest_precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(repository_id)
            .cloned()?;
        if cached.corpus_signature != current_root_signature {
            return None;
        }
        if !Self::cached_scip_discovery_is_current(root, &cached.discovery) {
            return None;
        }
        Some((*cached).clone())
    }

    fn cached_scip_discovery_is_current(root: &Path, discovery: &ScipArtifactDiscovery) -> bool {
        let expected_directories = Self::scip_candidate_directories(root);
        if discovery.candidate_directory_digests.len() != expected_directories.len() {
            return false;
        }

        for (expected_path, cached_digest) in expected_directories
            .iter()
            .zip(discovery.candidate_directory_digests.iter())
        {
            if cached_digest.path != *expected_path {
                return false;
            }
            let metadata = fs::metadata(expected_path).ok();
            let exists = metadata.is_some();
            let mtime_ns = metadata
                .as_ref()
                .and_then(|value| value.modified().ok())
                .and_then(Self::system_time_to_unix_nanos);
            if cached_digest.exists != exists || cached_digest.mtime_ns != mtime_ns {
                return false;
            }
        }

        discovery.artifact_digests.iter().all(|artifact| {
            let metadata = match fs::metadata(&artifact.path) {
                Ok(metadata) => metadata,
                Err(_) => return false,
            };
            metadata.is_file()
                && metadata.len() == artifact.size_bytes
                && metadata
                    .modified()
                    .ok()
                    .and_then(Self::system_time_to_unix_nanos)
                    == artifact.mtime_ns
        })
    }

    pub(super) fn precise_graph_for_corpus(
        &self,
        corpus: &RepositorySymbolCorpus,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<CachedPreciseGraph, ErrorData> {
        if let Some(cached) = self.try_reuse_cached_precise_graph(corpus) {
            return Ok(cached);
        }

        let discovery = Self::collect_scip_artifact_digests(&corpus.root);
        let scip_signature = Self::scip_signature(&discovery.artifact_digests);
        let cache_key = PreciseGraphCacheKey {
            repository_id: corpus.repository_id.clone(),
            scip_signature: scip_signature.clone(),
            corpus_signature: corpus.root_signature.clone(),
        };

        if let Some(cached) = self
            .cache_state
            .precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            self.cache_state
                .latest_precise_graph_cache
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(corpus.repository_id.clone(), cached.clone());
            return Ok((*cached).clone());
        }

        let mut graph = SymbolGraph::default();
        register_symbol_definitions(&mut graph, &corpus.repository_id, &corpus.symbols);
        Self::register_php_declaration_relations(&mut graph, corpus);
        Self::register_php_target_evidence_relations(&mut graph, corpus);
        Self::register_blade_relation_evidence(&mut graph, corpus);
        let ingest_stats = Self::ingest_precise_artifacts_for_repository(
            &mut graph,
            &corpus.root,
            &corpus.repository_id,
            &discovery,
            budgets,
        )?;
        let coverage_mode = Self::precise_coverage_mode(&ingest_stats);
        if coverage_mode == PreciseCoverageMode::Partial {
            warn!(
                repository_id = corpus.repository_id,
                artifacts_ingested = ingest_stats.artifacts_ingested,
                artifacts_failed = ingest_stats.artifacts_failed,
                "retaining partial precise graph because some SCIP artifacts ingested successfully"
            );
        }
        if coverage_mode == PreciseCoverageMode::None && ingest_stats.artifacts_failed > 0 {
            warn!(
                repository_id = corpus.repository_id,
                artifacts_ingested = ingest_stats.artifacts_ingested,
                artifacts_failed = ingest_stats.artifacts_failed,
                "precise graph has no usable artifact data after SCIP ingest failures"
            );
        }
        let cached_graph = CachedPreciseGraph {
            graph: Arc::new(graph),
            ingest_stats,
            corpus_signature: corpus.root_signature.clone(),
            discovery: discovery.clone(),
            coverage_mode,
        };

        let mut cache = self
            .cache_state
            .precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| {
            key.repository_id != corpus.repository_id
                || (key.scip_signature == scip_signature
                    && key.corpus_signature == corpus.root_signature)
        });
        let cached_graph = Arc::new(cached_graph);
        cache.insert(cache_key, cached_graph.clone());
        self.cache_state
            .latest_precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(corpus.repository_id.clone(), cached_graph.clone());

        Ok((*cached_graph).clone())
    }

    pub(super) fn precise_graph_for_repository_root(
        &self,
        repository_id: &str,
        root: &Path,
        budgets: FindReferencesResourceBudgets,
    ) -> Result<CachedPreciseGraph, ErrorData> {
        if let Some(cached) =
            self.try_reuse_latest_precise_graph_for_repository(repository_id, root)
        {
            return Ok(cached);
        }

        let discovery = Self::collect_scip_artifact_digests(root);
        let current_root_signature =
            Self::current_root_signature_for_repository(root, repository_id).ok_or_else(|| {
                Self::internal(
                    "failed to compute current root signature for precise graph",
                    Some(json!({
                        "repository_id": repository_id,
                        "root": root.display().to_string(),
                    })),
                )
            })?;
        let scip_signature = Self::scip_signature(&discovery.artifact_digests);
        let cache_key = PreciseGraphCacheKey {
            repository_id: repository_id.to_owned(),
            scip_signature: scip_signature.clone(),
            corpus_signature: current_root_signature.clone(),
        };

        if let Some(cached) = self
            .cache_state
            .precise_graph_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            self.cache_state
                .latest_precise_graph_cache
                .write()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .insert(repository_id.to_owned(), cached.clone());
            return Ok((*cached).clone());
        }

        let mut graph = SymbolGraph::default();
        let ingest_stats = Self::ingest_precise_artifacts_for_repository(
            &mut graph,
            root,
            repository_id,
            &discovery,
            budgets,
        )?;
        let coverage_mode = Self::precise_coverage_mode(&ingest_stats);
        let cached_graph = CachedPreciseGraph {
            graph: Arc::new(graph),
            ingest_stats,
            corpus_signature: current_root_signature,
            discovery: discovery.clone(),
            coverage_mode,
        };

        let mut cache = self
            .cache_state
            .precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.retain(|key, _| {
            key.repository_id != repository_id
                || (key.scip_signature == scip_signature
                    && key.corpus_signature == cached_graph.corpus_signature)
        });
        let cached_graph = Arc::new(cached_graph);
        cache.insert(cache_key, cached_graph.clone());
        self.cache_state
            .latest_precise_graph_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(repository_id.to_owned(), cached_graph.clone());

        Ok((*cached_graph).clone())
    }
}
