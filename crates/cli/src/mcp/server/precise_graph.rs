use super::*;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::process::{Command, Stdio};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use walkdir::WalkDir;

use crate::mcp::types::{
    WorkspacePreciseFailureClass, WorkspacePreciseGenerationAction, WorkspaceRecommendedAction,
};

mod config;
mod generation;
mod ingest;

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
pub(super) struct PreciseGeneratorSpec {
    pub(super) language: SymbolLanguage,
    pub(super) generator_id: &'static str,
    pub(super) tool_name: &'static str,
    pub(super) tool_candidates: &'static [&'static str],
    pub(super) version_arg_sets: &'static [&'static [&'static str]],
    pub(super) generate_args: &'static [&'static str],
    pub(super) infer_tsconfig: bool,
    pub(super) trigger_markers: &'static [&'static str],
    pub(super) output_artifact_name: &'static str,
    pub(super) stdout_artifact_fallback: bool,
    pub(super) output_flag: Option<&'static str>,
}

#[derive(Debug)]
pub(super) enum PreciseToolProbeError {
    MissingTool,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(super) struct ResolvedPreciseGeneratorTool {
    pub(super) command: String,
    pub(super) display: String,
}

pub(in crate::mcp::server) fn workspace_is_laravel_php_precise_workspace(
    workspace_root: &Path,
) -> bool {
    (workspace_root.join("composer.json").is_file()
        || workspace_root.join("composer.lock").is_file())
        && workspace_root.join("bootstrap/app.php").is_file()
}

pub(in crate::mcp::server) fn php_precise_generator_tool_candidates(
    workspace_root: &Path,
) -> Vec<&'static str> {
    if workspace_is_laravel_php_precise_workspace(workspace_root) {
        vec!["vendor/bin/scip-laravel", "vendor/bin/scip-php", "scip-php"]
    } else {
        vec!["vendor/bin/scip-php", "scip-php"]
    }
}
