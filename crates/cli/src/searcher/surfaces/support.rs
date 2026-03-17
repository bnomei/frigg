use std::path::Path;

use crate::domain::{PathClass, SourceClass};
use crate::languages::{LanguageCapability, supported_language_for_path};
use crate::path_class::classify_repository_path;

use super::artifacts::{
    is_build_config_surface_path, is_package_surface_path, is_python_named_test_module_path,
    is_runtime_config_artifact_path, is_workspace_config_surface_path,
};
use super::runtime::is_entrypoint_runtime_path;
use super::tokens::{hybrid_identifier_tokens, normalize_runtime_anchor_test_stem};

pub(in crate::searcher) fn hybrid_source_class(path: &str) -> SourceClass {
    if is_error_contract_path(path) {
        return SourceClass::ErrorContracts;
    }
    if is_tool_contract_path(path) {
        return SourceClass::ToolContracts;
    }
    if path.starts_with("benchmarks/") {
        return SourceClass::BenchmarkDocs;
    }
    if is_readme_path(path) {
        return SourceClass::Readme;
    }
    if path.starts_with("docs/") {
        return SourceClass::Documentation;
    }
    if is_fixture_support_path(path) {
        return SourceClass::Fixtures;
    }
    if super::runtime::is_ci_workflow_path(path) {
        return SourceClass::Support;
    }
    if path.starts_with("specs/") {
        return SourceClass::Specs;
    }
    if is_test_support_path(path) {
        return SourceClass::Tests;
    }

    match classify_repository_path(path) {
        PathClass::Runtime => SourceClass::Runtime,
        PathClass::Project => SourceClass::Project,
        PathClass::Support => SourceClass::Support,
    }
}

pub(in crate::searcher) fn is_example_support_path(path: &str) -> bool {
    path.starts_with("examples/") || path.contains("/examples/")
}

pub(in crate::searcher) fn is_bench_support_path(path: &str) -> bool {
    path.starts_with("benches/")
        || path.contains("/benches/")
        || path.starts_with("bench/")
        || path.contains("/bench/")
}

pub(in crate::searcher) fn is_test_harness_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str()),
        Some(
            "createsapplication.php"
                | "dusktestcase.php"
                | "pest.php"
                | "testcase.php"
                | "conftest.py"
        )
    )
}

pub(in crate::searcher) fn coverage_subtree_root(path: &str) -> Option<String> {
    const CONTAINER_SEGMENTS: &[&str] = &["apps", "packages", "crates", "libs", "services"];
    const ROLE_SEGMENTS: &[&str] = &[
        "src", "tests", "test", "pages", "routes", "api", "server", "cli", "bin",
    ];

    let segments = path
        .trim_start_matches("./")
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();
    if segments.len() <= 1 {
        return None;
    }

    let mut prefix = Vec::new();
    if CONTAINER_SEGMENTS.contains(&segments[0]) && segments.len() >= 2 {
        prefix.push(segments[0]);
        prefix.push(segments[1]);
        if segments
            .get(2)
            .is_some_and(|segment| ROLE_SEGMENTS.contains(segment))
        {
            prefix.push(segments[2]);
        }
    } else {
        prefix.push(segments[0]);
        if segments
            .get(1)
            .is_some_and(|segment| ROLE_SEGMENTS.contains(segment))
        {
            prefix.push(segments[1]);
        }
    }

    Some(prefix.join("/"))
}

fn is_named_test_script_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let is_script = matches!(
        Path::new(&normalized)
            .extension()
            .and_then(|extension| extension.to_str()),
        Some("sh" | "bash" | "zsh" | "fish" | "bats" | "ps1" | "cmd" | "bat" | "nu")
    );
    if !is_script {
        return false;
    }

    if normalized.contains("/scripts/") {
        return true;
    }

    Path::new(&normalized)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(hybrid_identifier_tokens)
        .is_some_and(|tokens| {
            tokens.into_iter().any(|token| {
                matches!(
                    token.as_str(),
                    "bench"
                        | "benchmark"
                        | "check"
                        | "e2e"
                        | "integration"
                        | "regression"
                        | "run"
                        | "smoke"
                        | "spec"
                        | "test"
                        | "unit"
                )
            })
        })
}

pub(in crate::searcher) fn is_test_support_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if is_runtime_config_artifact_path(&normalized) {
        return false;
    }
    if is_fixture_support_path(&normalized) {
        return false;
    }

    let under_test_tree = normalized.starts_with("tests/")
        || normalized.contains("/tests/")
        || normalized.starts_with("test/")
        || normalized.contains("/test/");
    let is_supported_test_source =
        supported_language_for_path(Path::new(&normalized), LanguageCapability::SourceFilter)
            .is_some();

    (under_test_tree
        && (is_supported_test_source
            || is_named_test_script_path(&normalized)
            || is_test_harness_path(&normalized)))
        || normalized.ends_with("_test.go")
        || normalized.ends_with("_test.rs")
        || normalized.ends_with("_tests.rs")
        || is_python_named_test_module_path(&normalized)
}

pub(in crate::searcher) fn is_fixture_support_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.starts_with("fixtures/") || normalized.contains("/fixtures/")
}

pub(in crate::searcher) fn is_runtime_companion_surface_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if is_entrypoint_runtime_path(&normalized)
        || is_runtime_config_artifact_path(&normalized)
        || is_package_surface_path(&normalized)
        || is_build_config_surface_path(&normalized)
        || is_workspace_config_surface_path(&normalized)
        || is_test_support_path(&normalized)
        || is_test_harness_path(&normalized)
        || is_fixture_support_path(&normalized)
        || is_example_support_path(&normalized)
        || is_bench_support_path(&normalized)
        || is_generic_runtime_witness_doc_path(&normalized)
        || super::runtime::is_ci_workflow_path(&normalized)
        || super::runtime::is_scripts_ops_path(&normalized)
        || super::artifacts::is_frontend_runtime_noise_path(&normalized)
    {
        return false;
    }

    matches!(hybrid_source_class(&normalized), SourceClass::Runtime)
}

pub(in crate::searcher) fn is_runtime_anchor_test_support_path(path: &str) -> bool {
    if !is_test_support_path(path) {
        return false;
    }

    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let Some(stem) = Path::new(&normalized)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(normalize_runtime_anchor_test_stem)
    else {
        return false;
    };

    matches!(
        stem.as_str(),
        "app"
            | "bootstrap"
            | "cli"
            | "daemon"
            | "main"
            | "manage"
            | "run"
            | "server"
            | "service"
            | "worker"
    )
}

pub(in crate::searcher) fn is_cli_test_support_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    is_test_support_path(&normalized)
        && (normalized.starts_with("tests/cli/")
            || normalized.contains("/tests/cli/")
            || normalized.starts_with("test/cli/")
            || normalized.contains("/test/cli/")
            || normalized.ends_with("/cli.rs")
            || normalized.contains("/cli/"))
}

pub(in crate::searcher) fn is_cli_command_entrypoint_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !is_entrypoint_runtime_path(&normalized) {
        return false;
    }

    let stem_is_cli = Path::new(&normalized)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .is_some_and(|stem| stem.eq_ignore_ascii_case("cli"));

    stem_is_cli
        || normalized.starts_with("bin/")
        || normalized.contains("/bin/")
        || normalized.starts_with("cli/")
        || normalized.contains("/cli/")
        || normalized.starts_with("cmd/")
        || normalized.contains("/cmd/")
        || normalized.starts_with("command/")
        || normalized.contains("/command/")
        || normalized.starts_with("commands/")
        || normalized.contains("/commands/")
}

pub(in crate::searcher) fn has_generic_runtime_anchor_stem(path: &str) -> bool {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim().to_ascii_lowercase())
        .is_some_and(|stem| matches!(stem.as_str(), "server" | "discoverer"))
}

pub(in crate::searcher) fn is_error_contract_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !(normalized.starts_with("contracts/") || normalized.contains("/contracts/")) {
        return false;
    }

    let Some(stem) = Path::new(&normalized)
        .file_stem()
        .and_then(|stem| stem.to_str())
    else {
        return false;
    };

    hybrid_identifier_tokens(stem)
        .into_iter()
        .any(|token| matches!(token.as_str(), "error" | "errors" | "failure" | "failures"))
}

pub(in crate::searcher) fn is_tool_contract_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.starts_with("contracts/tools/") || normalized.contains("/contracts/tools/")
}

pub(in crate::searcher) fn is_readme_path(path: &str) -> bool {
    path == "README.md" || path.ends_with("/README.md")
}

fn has_docs_like_segment(path: &str) -> bool {
    path.split('/').any(|segment| {
        matches!(
            segment,
            "content" | "doc" | "docs" | "documentation" | "guide" | "guides" | "site" | "website"
        )
    })
}

pub(in crate::searcher) fn is_generic_runtime_witness_doc_path(path: &str) -> bool {
    if is_readme_path(path) {
        return true;
    }

    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let candidate = Path::new(&normalized);
    let Some(file_name) = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    if matches!(
        file_name,
        "agents.md"
            | "architecture.md"
            | "changelog.md"
            | "claude.md"
            | "code_of_conduct.md"
            | "contributing.md"
            | "developers.md"
            | "development.md"
            | "examples.md"
            | "faq.md"
            | "index.md"
            | "overview.md"
            | "roadmap.md"
            | "security.md"
            | "support.md"
    ) {
        return true;
    }

    let is_markdownish = matches!(
        candidate.extension().and_then(|ext| ext.to_str()),
        Some("adoc" | "markdown" | "md" | "mdx" | "rst" | "txt")
    );
    is_markdownish && has_docs_like_segment(&normalized)
}

pub(in crate::searcher) fn is_repo_metadata_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    let Some(file_name) = Path::new(normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .map(|name| name.to_ascii_lowercase())
    else {
        return false;
    };

    matches!(
        file_name.as_str(),
        "agents.md"
            | "architecture.md"
            | "changelog.md"
            | "claude.md"
            | "composer.json"
            | "contributing.md"
            | "cargo.toml"
            | "cargo.lock"
            | "developers.md"
            | "development.md"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "pnpm-workspace.yaml"
            | "yarn.lock"
            | "pyproject.toml"
            | "poetry.lock"
            | "makefile"
            | "justfile"
            | "lerna.json"
            | "nx.json"
            | "security.md"
            | "taskfile.yml"
            | "taskfile.yaml"
            | "turbo.json"
    )
}

pub(in crate::searcher) fn is_non_code_test_doc_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !is_test_support_path(&normalized) {
        return false;
    }

    matches!(
        Path::new(&normalized)
            .extension()
            .and_then(|ext| ext.to_str()),
        Some("md" | "markdown" | "rst" | "txt" | "adoc")
    )
}
