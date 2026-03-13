use std::path::Path;

use crate::domain::PathClass;
use crate::path_class::classify_repository_path;

use super::HybridSourceClass;
use super::support::{
    hybrid_source_class, is_bench_support_path, is_example_support_path, is_test_support_path,
};
use super::tokens::hybrid_identifier_tokens;

pub(in crate::searcher) fn is_python_entrypoint_runtime_path(path: &str) -> bool {
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

pub(in crate::searcher) fn is_lua_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !normalized.ends_with(".lua") || is_test_support_path(&normalized) {
        return false;
    }

    let candidate = Path::new(&normalized);
    let Some(stem) = candidate
        .file_stem()
        .and_then(|stem| stem.to_str())
        .map(|stem| stem.trim().to_ascii_lowercase())
    else {
        return false;
    };
    let parts = candidate
        .components()
        .filter_map(|component| component.as_os_str().to_str())
        .collect::<Vec<_>>();
    let is_repo_root_file = parts.len() == 1;
    if is_repo_root_file {
        return matches!(
            stem.as_str(),
            "main" | "init" | "app" | "bootstrap" | "cli" | "run" | "server"
        );
    }

    let has_loader_root = parts
        .iter()
        .take(parts.len().saturating_sub(1))
        .any(|part| matches!(*part, "bin" | "cli" | "cmd" | "lua" | "script" | "scripts"));
    if !has_loader_root {
        return false;
    }

    let has_cli_context = parts
        .iter()
        .any(|part| matches!(*part, "cli" | "command" | "commands"));
    if has_cli_context {
        return true;
    }

    let has_runtime_context = parts
        .iter()
        .any(|part| matches!(*part, "daemon" | "server" | "service" | "worker"));
    has_runtime_context
        && matches!(
            stem.as_str(),
            "bootstrap" | "cli" | "daemon" | "init" | "main" | "run" | "server" | "service"
        )
}

pub(in crate::searcher) fn is_kotlin_android_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if is_test_support_path(&normalized) {
        return false;
    }

    let candidate = Path::new(path.trim_start_matches("./"));
    let Some(extension) = candidate.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if !matches!(extension.to_ascii_lowercase().as_str(), "java" | "kt") {
        return false;
    }

    if !(normalized.starts_with("src/main/") || normalized.contains("/src/main/")) {
        return false;
    }

    let Some(stem) = candidate.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let stem_tokens = hybrid_identifier_tokens(stem);
    if stem_tokens.is_empty() {
        return false;
    }

    stem_tokens
        .iter()
        .any(|token| matches!(token.as_str(), "activity" | "application" | "navigation"))
        || stem_tokens
            .windows(2)
            .any(|window| matches!(window, [first, second] if first == "nav" && second == "graph"))
}

pub(in crate::searcher) fn is_kotlin_android_ui_runtime_surface_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if is_test_support_path(&normalized) {
        return false;
    }

    let candidate = Path::new(path.trim_start_matches("./"));
    let Some(extension) = candidate.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if !matches!(extension.to_ascii_lowercase().as_str(), "java" | "kt") {
        return false;
    }

    if !(normalized.starts_with("src/main/") || normalized.contains("/src/main/")) {
        return false;
    }

    let Some(stem) = candidate.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let stem_tokens = hybrid_identifier_tokens(stem);
    if stem_tokens.is_empty() {
        return false;
    }

    stem_tokens.last().is_some_and(|token| token == "screen")
        || stem_tokens.len() >= 2
            && matches!(
                stem_tokens[stem_tokens.len() - 2..],
                [ref first, ref second] if first == "view" && second == "model"
            )
}

pub(in crate::searcher) fn is_typescript_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if is_test_support_path(&normalized) {
        return false;
    }

    let candidate = Path::new(&normalized);
    let Some(extension) = candidate.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if !matches!(
        extension,
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts"
    ) {
        return false;
    }

    let Some(stem) = candidate.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    let stem = stem.trim().to_ascii_lowercase();
    if matches!(
        stem.as_str(),
        "main" | "server" | "cli" | "app" | "bootstrap"
    ) {
        return true;
    }

    let looks_like_runtime_tree =
        matches!(classify_repository_path(&normalized), PathClass::Runtime)
            || normalized.starts_with("app/")
            || normalized.contains("/app/")
            || normalized.contains("/lib/");
    if !looks_like_runtime_tree {
        return false;
    }

    let Some(parent_name) = candidate
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
    else {
        return false;
    };
    if parent_name != "src" {
        return false;
    }
    if stem != "index" {
        return false;
    }

    hybrid_identifier_tokens(&normalized)
        .into_iter()
        .any(|token| {
            matches!(
                token.as_str(),
                "app" | "bootstrap" | "cli" | "daemon" | "server" | "service" | "worker"
            )
        })
}

pub(in crate::searcher) fn is_typescript_runtime_module_index_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if is_test_support_path(&normalized)
        || is_example_support_path(&normalized)
        || is_bench_support_path(&normalized)
    {
        return false;
    }

    let candidate = Path::new(&normalized);
    let Some(extension) = candidate.extension().and_then(|ext| ext.to_str()) else {
        return false;
    };
    if !matches!(
        extension,
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts"
    ) {
        return false;
    }

    let looks_like_runtime_tree =
        matches!(classify_repository_path(&normalized), PathClass::Runtime)
            || normalized.starts_with("app/")
            || normalized.contains("/app/")
            || normalized.contains("/lib/");
    if !looks_like_runtime_tree {
        return false;
    }

    let Some(stem) = candidate.file_stem().and_then(|stem| stem.to_str()) else {
        return false;
    };
    if !stem.eq_ignore_ascii_case("index") {
        return false;
    }

    let path_tokens = hybrid_identifier_tokens(&normalized);
    if path_tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "bench"
                | "benches"
                | "config"
                | "configs"
                | "fixture"
                | "fixtures"
                | "mock"
                | "mocks"
                | "spec"
                | "specs"
                | "stories"
                | "story"
                | "test"
                | "tests"
        )
    }) {
        return false;
    }

    true
}

pub(in crate::searcher) fn is_go_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !normalized.ends_with(".go") || normalized.ends_with("_test.go") {
        return false;
    }
    if is_test_support_path(&normalized) {
        return false;
    }

    if normalized == "main.go" || normalized.ends_with("/main.go") {
        return true;
    }

    (normalized.starts_with("cmd/") || normalized.contains("/cmd/"))
        && Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("root.go"))
}

pub(in crate::searcher) fn is_roc_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !normalized.ends_with(".roc") || is_test_support_path(&normalized) {
        return false;
    }

    normalized == "main.roc"
        || normalized == "platform/main.roc"
        || normalized.ends_with("/platform/main.roc")
}

pub(in crate::searcher) fn is_navigation_runtime_path(path: &str) -> bool {
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

pub(in crate::searcher) fn is_navigation_reference_doc_path(path: &str) -> bool {
    matches!(
        hybrid_source_class(path),
        HybridSourceClass::Documentation | HybridSourceClass::Readme | HybridSourceClass::Specs
    )
}

pub(in crate::searcher) fn is_entrypoint_runtime_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    matches!(normalized, "src/main.rs" | "src/lib.rs")
        || normalized.ends_with("/src/main.rs")
        || normalized.ends_with("/src/lib.rs")
        || is_go_entrypoint_runtime_path(normalized)
        || is_kotlin_android_entrypoint_runtime_path(normalized)
        || is_roc_entrypoint_runtime_path(normalized)
        || is_lua_entrypoint_runtime_path(normalized)
        || is_python_entrypoint_runtime_path(normalized)
        || is_typescript_entrypoint_runtime_path(normalized)
}

pub(in crate::searcher) fn is_entrypoint_reference_doc_path(path: &str) -> bool {
    path.starts_with("specs/")
        || matches!(hybrid_source_class(path), HybridSourceClass::Documentation)
}

pub(in crate::searcher) fn is_ci_workflow_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with(".github/workflows/")
        && matches!(
            Path::new(normalized)
                .extension()
                .and_then(|ext| ext.to_str()),
            Some("yml" | "yaml")
        )
}

pub(in crate::searcher) fn is_entrypoint_build_workflow_path(path: &str) -> bool {
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

pub(in crate::searcher) fn is_scripts_ops_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized == "justfile"
        || normalized == "makefile"
        || normalized.starts_with("scripts/")
        || normalized.starts_with("xtask/")
        || normalized.contains("/scripts/")
}
