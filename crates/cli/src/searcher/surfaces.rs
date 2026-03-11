use std::path::Path;

use crate::domain::{PathClass, SourceClass};
use crate::path_class::classify_repository_path;

pub(super) type HybridSourceClass = SourceClass;

pub(super) fn hybrid_source_class(path: &str) -> HybridSourceClass {
    if is_error_contract_path(path) {
        return HybridSourceClass::ErrorContracts;
    }
    if is_tool_contract_path(path) {
        return HybridSourceClass::ToolContracts;
    }
    if path.starts_with("benchmarks/") {
        return HybridSourceClass::BenchmarkDocs;
    }
    if is_readme_path(path) {
        return HybridSourceClass::Readme;
    }
    if path.starts_with("docs/") {
        return HybridSourceClass::Documentation;
    }
    if path.starts_with("fixtures/") {
        return HybridSourceClass::Fixtures;
    }
    if is_ci_workflow_path(path) {
        return HybridSourceClass::Support;
    }
    if path.starts_with("specs/") {
        return HybridSourceClass::Specs;
    }
    if is_test_support_path(path) {
        return HybridSourceClass::Tests;
    }

    match classify_repository_path(path) {
        PathClass::Runtime => HybridSourceClass::Runtime,
        PathClass::Project => HybridSourceClass::Project,
        PathClass::Support => HybridSourceClass::Support,
    }
}

pub(super) fn is_example_support_path(path: &str) -> bool {
    path.starts_with("examples/") || path.contains("/examples/")
}

pub(super) fn is_bench_support_path(path: &str) -> bool {
    path.starts_with("benches/")
        || path.contains("/benches/")
        || path.starts_with("bench/")
        || path.contains("/bench/")
}

pub(super) fn is_test_harness_path(path: &str) -> bool {
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

pub(super) fn is_test_support_path(path: &str) -> bool {
    path.starts_with("tests/")
        || path.contains("/tests/")
        || path.ends_with("_test.rs")
        || path.ends_with("_tests.rs")
}

pub(super) fn is_cli_test_support_path(path: &str) -> bool {
    is_test_support_path(path)
        && (path.starts_with("tests/cli/")
            || path.contains("/tests/cli/")
            || path.ends_with("/cli.rs")
            || path.contains("/cli/"))
}

pub(super) fn has_generic_runtime_anchor_stem(path: &str) -> bool {
    Path::new(path)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim().to_ascii_lowercase())
        .is_some_and(|stem| matches!(stem.as_str(), "server" | "discoverer"))
}

pub(super) fn is_error_contract_path(path: &str) -> bool {
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

pub(super) fn is_tool_contract_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.starts_with("contracts/tools/") || normalized.contains("/contracts/tools/")
}

pub(super) fn is_readme_path(path: &str) -> bool {
    path == "README.md" || path.ends_with("/README.md")
}

pub(super) fn is_generic_runtime_witness_doc_path(path: &str) -> bool {
    if is_readme_path(path) {
        return true;
    }

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
        "index.md" | "overview.md" | "examples.md"
    ) && (normalized == format!("docs/{file_name}")
        || normalized.ends_with(&format!("/docs/{file_name}")))
}

pub(super) fn is_repo_metadata_path(path: &str) -> bool {
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
        "composer.json"
            | "cargo.toml"
            | "cargo.lock"
            | "package.json"
            | "package-lock.json"
            | "pnpm-lock.yaml"
            | "yarn.lock"
            | "pyproject.toml"
            | "poetry.lock"
    )
}

#[cfg(test)]
mod tests {
    use crate::domain::SourceClass;

    use super::{
        hybrid_source_class, is_runtime_config_artifact_path, is_rust_workspace_config_path,
    };

    #[test]
    fn hybrid_source_class_respects_specific_precedence_before_path_class() {
        assert_eq!(
            hybrid_source_class("contracts/errors.md"),
            SourceClass::ErrorContracts
        );
        assert_eq!(
            hybrid_source_class("contracts/tools/v1/search_hybrid.v1.schema.json"),
            SourceClass::ToolContracts
        );
        assert_eq!(
            hybrid_source_class("playbooks/runtime/deep-search.md"),
            SourceClass::Project
        );
    }

    #[test]
    fn hybrid_source_class_falls_back_to_typed_path_classification() {
        assert_eq!(
            hybrid_source_class("crates/cli/src/mcp/server.rs"),
            SourceClass::Runtime
        );
        assert_eq!(
            hybrid_source_class("crates/cli/examples/server.rs"),
            SourceClass::Support
        );
    }

    #[test]
    fn rust_workspace_config_paths_are_detected_as_runtime_config_artifacts() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            "rustfmt.toml",
            "clippy.toml",
            "crates/tooling/.cargo/config.toml",
        ] {
            assert!(
                is_rust_workspace_config_path(path),
                "{path} should be detected as a rust workspace config path"
            );
            assert!(
                is_runtime_config_artifact_path(path),
                "{path} should participate in runtime config artifact ranking"
            );
        }
    }
}

pub(super) fn is_python_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    matches!(
        normalized.as_str(),
        "__main__.py" | "main.py" | "app.py" | "manage.py" | "cli.py" | "run.py"
    ) || [
        "/__main__.py",
        "/main.py",
        "/app.py",
        "/manage.py",
        "/cli.py",
        "/run.py",
    ]
    .iter()
    .any(|suffix| normalized.ends_with(suffix))
}

pub(super) fn is_python_runtime_config_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("pyproject.toml" | "setup.py")
    )
}

pub(super) fn is_rust_workspace_config_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if normalized == ".cargo/config.toml" || normalized.ends_with("/.cargo/config.toml") {
        return true;
    }

    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("cargo.toml" | "cargo.lock" | "rust-toolchain.toml" | "rustfmt.toml" | "clippy.toml")
    )
}

pub(super) fn is_runtime_config_artifact_path(path: &str) -> bool {
    if is_rust_workspace_config_path(path) {
        return true;
    }

    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str()),
        Some(
            "pyproject.toml"
                | "setup.py"
                | "cargo.toml"
                | "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "composer.json"
                | "composer.lock"
                | "tsconfig.json"
                | "go.mod"
                | "go.sum"
                | "requirements.txt"
                | "pipfile"
                | "mix.exs"
        )
    )
}

pub(super) fn is_python_test_witness_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let Some(file_name) = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    normalized.ends_with(".py")
        && (normalized.contains("/tests/")
            || normalized.starts_with("tests/")
            || file_name == "conftest.py")
}

pub(super) fn is_loose_python_test_module_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let Some(file_name) = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    normalized.ends_with(".py")
        && !is_python_test_witness_path(path)
        && (file_name.starts_with("test_") || file_name.ends_with("_test.py"))
}

pub(super) fn is_non_code_test_doc_path(path: &str) -> bool {
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

pub(super) fn is_frontend_runtime_noise_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if normalized.contains("/frontend/") {
        return true;
    }

    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str()),
        Some(
            "package.json"
                | "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "openapi.json"
                | "contributing.md"
                | "claude.md"
                | "hierarchy.md"
        )
    )
}

pub(super) fn is_navigation_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    let path_class = classify_repository_path(normalized);
    if !matches!(path_class, PathClass::Runtime | PathClass::Support) {
        return false;
    }

    let Some(stem) = Path::new(normalized)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.to_ascii_lowercase())
    else {
        return false;
    };

    hybrid_identifier_tokens(&stem).into_iter().any(|token| {
        matches!(
            token.as_str(),
            "api"
                | "client"
                | "discoverer"
                | "handler"
                | "handlers"
                | "protocol"
                | "route"
                | "router"
                | "routes"
                | "server"
                | "transport"
        )
    })
}

pub(super) fn is_navigation_reference_doc_path(path: &str) -> bool {
    matches!(
        hybrid_source_class(path),
        HybridSourceClass::Documentation | HybridSourceClass::Readme | HybridSourceClass::Specs
    )
}

pub(super) fn is_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    matches!(normalized, "src/main.rs" | "src/lib.rs")
        || normalized.ends_with("/src/main.rs")
        || normalized.ends_with("/src/lib.rs")
        || is_python_entrypoint_runtime_path(normalized)
}

pub(super) fn is_entrypoint_reference_doc_path(path: &str) -> bool {
    path.starts_with("specs/")
        || matches!(hybrid_source_class(path), HybridSourceClass::Documentation)
}

pub(super) fn is_ci_workflow_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with(".github/workflows/")
        && matches!(
            Path::new(normalized)
                .extension()
                .and_then(|ext| ext.to_str()),
            Some("yml" | "yaml")
        )
}

pub(super) fn is_entrypoint_build_workflow_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    if !is_ci_workflow_path(normalized) {
        return false;
    }

    let Some(stem) = Path::new(normalized)
        .file_stem()
        .and_then(|stem| stem.to_str())
    else {
        return false;
    };

    hybrid_identifier_tokens(stem).into_iter().any(|token| {
        matches!(
            token.as_str(),
            "build" | "bundle" | "deploy" | "pages" | "publish" | "release"
        )
    })
}

pub(super) fn is_scripts_ops_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized == "justfile"
        || normalized == "makefile"
        || normalized.starts_with("scripts/")
        || normalized.starts_with("xtask/")
        || normalized.contains("/scripts/")
}

fn hybrid_identifier_tokens(raw: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut previous_was_lowercase = false;

    for ch in raw.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            push_hybrid_identifier_token(&mut tokens, &mut current);
            previous_was_lowercase = false;
            continue;
        }
        if !ch.is_ascii_alphanumeric() {
            push_hybrid_identifier_token(&mut tokens, &mut current);
            previous_was_lowercase = false;
            continue;
        }
        if ch.is_ascii_uppercase() && previous_was_lowercase {
            push_hybrid_identifier_token(&mut tokens, &mut current);
        }
        current.push(ch.to_ascii_lowercase());
        previous_was_lowercase = ch.is_ascii_lowercase();
    }

    push_hybrid_identifier_token(&mut tokens, &mut current);
    tokens
}

fn push_hybrid_identifier_token(tokens: &mut Vec<String>, current: &mut String) {
    let normalized = current
        .trim_matches(|ch: char| !ch.is_ascii_alphanumeric())
        .to_ascii_lowercase();
    if normalized.len() >= 2 {
        tokens.push(normalized);
    }
    current.clear();
}
