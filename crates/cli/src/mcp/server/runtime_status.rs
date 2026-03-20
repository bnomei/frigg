use super::*;
use std::ffi::OsStr;

use crate::mcp::server::precise_graph::php_precise_generator_tool_candidates;
use crate::mcp::types::{
    RepositorySessionSummary, RepositoryWatchSummary, WorkspacePreciseFailureClass,
    WorkspacePreciseGenerationAction, WorkspacePreciseState, WorkspacePreciseSummary,
    WorkspaceRecommendedAction,
};

mod index_health;
mod precise_generation;
mod repository_summary;

#[allow(dead_code)]
const PRECISE_GENERATION_TIMEOUT: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreciseGeneratorKind {
    RustAnalyzer,
    ScipGo,
    ScipTypescript,
    ScipPhp,
}

#[allow(dead_code)]
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

    fn tool_candidates(self, workspace_root: &Path) -> Vec<&'static str> {
        match self {
            Self::RustAnalyzer => vec!["rust-analyzer"],
            Self::ScipGo => vec!["$GOPATH/bin/scip-go", "scip-go"],
            Self::ScipTypescript => vec![
                "node_modules/.bin/scip-typescript",
                "$NPM_PREFIX/bin/scip-typescript",
                "$PNPM_BIN/scip-typescript",
                "$BUN_BIN/scip-typescript",
                "scip-typescript",
            ],
            Self::ScipPhp => php_precise_generator_tool_candidates(workspace_root),
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
