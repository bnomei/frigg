use std::path::Path;

pub(in crate::searcher) const NESTED_ROOT_SCOPED_RUNTIME_CONFIG_PATHS: &[&str] = &[
    ".cargo/config.toml",
    "gradle/init.gradle",
    "gradle/init.gradle.kts",
    "gradle/wrapper/gradle-wrapper.properties",
];

fn path_has_segment(path: &str, segments: &[&str]) -> bool {
    path.split('/').any(|segment| segments.contains(&segment))
}

pub(in crate::searcher) fn is_python_runtime_config_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str()),
        Some("pyproject.toml" | "setup.py")
    )
}

pub(in crate::searcher) fn is_python_named_test_module_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let Some(file_name) = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
    else {
        return false;
    };

    normalized.ends_with(".py")
        && (file_name.starts_with("test_") || file_name.ends_with("_test.py"))
}

pub(in crate::searcher) fn is_rust_workspace_config_path(path: &str) -> bool {
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

pub(in crate::searcher) fn is_runtime_config_artifact_path(path: &str) -> bool {
    if is_rust_workspace_config_path(path) {
        return true;
    }

    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str());
    if file_name.is_some_and(|name| {
        matches!(
            name,
            ".env" | ".env.example" | ".luarc.json" | ".luarc.jsonc" | ".luarc.doc.json"
        ) || name.ends_with(".rockspec")
            || name.ends_with(".nimble")
    }) {
        return true;
    }

    if matches!(
        file_name,
        Some(
            "androidmanifest.xml"
                | "build.gradle"
                | "build.gradle.kts"
                | "gradle.properties"
                | "gradle-wrapper.properties"
                | "init.gradle"
                | "init.gradle.kts"
                | "settings.gradle"
                | "settings.gradle.kts"
        )
    ) {
        return true;
    }

    matches!(
        file_name,
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

pub(in crate::searcher) fn is_package_surface_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str());

    if file_name.is_some_and(|name| name.ends_with(".rockspec") || name.ends_with(".nimble")) {
        return true;
    }

    matches!(
        file_name,
        Some(
            "cargo.toml"
                | "composer.json"
                | "go.mod"
                | "mix.exs"
                | "package.json"
                | "pyproject.toml"
                | "setup.py"
        )
    )
}

pub(in crate::searcher) fn is_workspace_config_surface_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str());

    is_root_scoped_runtime_config_path(path)
        || matches!(
            file_name,
            Some(
                "pnpm-workspace.yaml"
                    | "turbo.json"
                    | "workspace.json"
                    | "nx.json"
                    | "tsconfig.json"
                    | "tsconfig.base.json"
            )
        )
}

pub(in crate::searcher) fn is_build_config_surface_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    if matches!(normalized.as_str(), "justfile" | "makefile") {
        return true;
    }

    if matches!(
        file_name,
        "build.rs"
            | "build.gradle"
            | "build.gradle.kts"
            | "build.zig"
            | "build.zig.zon"
            | "dockerfile"
    ) {
        return true;
    }

    file_name.starts_with("next.config.")
        || file_name.starts_with("vite.config.")
        || file_name.starts_with("vitest.config.")
        || file_name.starts_with("jest.config.")
        || file_name.starts_with("playwright.config.")
        || file_name.starts_with("astro.config.")
}

pub(in crate::searcher) fn is_root_scoped_runtime_config_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if !is_runtime_config_artifact_path(&normalized) {
        return false;
    }

    !normalized.contains('/')
        || matches!(
            normalized.as_str(),
            ".cargo/config.toml"
                | "gradle/init.gradle"
                | "gradle/init.gradle.kts"
                | "gradle/wrapper/gradle-wrapper.properties"
        )
}

pub(in crate::searcher) fn is_python_test_witness_path(path: &str) -> bool {
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
            || file_name == "conftest.py"
            || is_python_named_test_module_path(path))
}

pub(in crate::searcher) fn is_runtime_adjacent_python_test_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    is_python_test_witness_path(path)
        && !normalized.starts_with("tests/")
        && !normalized.contains("/tests/")
        && !normalized.starts_with("test/")
        && !normalized.contains("/test/")
}

pub(in crate::searcher) fn is_loose_python_test_module_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.ends_with(".py")
        && !is_python_test_witness_path(path)
        && (normalized.starts_with("test/") || normalized.contains("/test/"))
}

pub(in crate::searcher) fn is_non_prefix_python_test_module_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    let file_name = Path::new(&normalized)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();

    is_loose_python_test_module_path(path) || file_name.ends_with("_test.py")
}

pub(in crate::searcher) fn is_frontend_runtime_noise_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    if normalized.contains("/frontend/") {
        return true;
    }

    let candidate = Path::new(&normalized);
    let extension = candidate.extension().and_then(|ext| ext.to_str());
    let file_name = candidate.file_name().and_then(|name| name.to_str());
    let docs_like_segment = path_has_segment(
        &normalized,
        &["doc", "docs", "documentation", "site", "website"],
    );
    let registry_or_template_segment =
        path_has_segment(&normalized, &["registry", "template", "templates"]);

    if normalized.starts_with("web/") || normalized.contains("/web/") {
        if matches!(extension, Some("css" | "scss" | "svg")) {
            return true;
        }

        if matches!(
            file_name,
            Some(
                "openapi.json"
                    | "package-lock.json"
                    | "package.json"
                    | "pnpm-lock.yaml"
                    | "tsconfig.json"
                    | "yarn.lock"
            )
        ) {
            return true;
        }
    }

    if docs_like_segment
        && matches!(
            extension,
            Some(
                "cjs"
                    | "css"
                    | "js"
                    | "json"
                    | "jsx"
                    | "mdx"
                    | "mjs"
                    | "scss"
                    | "svg"
                    | "ts"
                    | "tsx"
            )
        )
    {
        return true;
    }

    if registry_or_template_segment
        && matches!(
            extension,
            Some("js" | "json" | "jsx" | "mjs" | "ts" | "tsx")
        )
    {
        return true;
    }

    matches!(
        file_name,
        Some(
            "package-lock.json"
                | "pnpm-lock.yaml"
                | "yarn.lock"
                | "openapi.json"
                | "contributing.md"
                | "claude.md"
                | "hierarchy.md"
        )
    )
}
