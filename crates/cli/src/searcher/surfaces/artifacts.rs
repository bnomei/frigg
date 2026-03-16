use std::path::Path;

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
        matches!(name, ".luarc.json" | ".luarc.jsonc" | ".luarc.doc.json")
            || name.ends_with(".rockspec")
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
