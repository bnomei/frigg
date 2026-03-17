#[derive(Debug, Clone)]
pub(super) struct QueryContext {
    query: String,
}

impl QueryContext {
    pub(super) fn new(query_text: &str) -> Self {
        Self {
            query: query_text.trim().to_ascii_lowercase(),
        }
    }

    pub(super) fn has_any(&self, needles: &[&str]) -> bool {
        needles.iter().any(|needle| self.query.contains(needle))
    }

    pub(super) fn has_any_token(&self, needles: &[&str]) -> bool {
        needles.iter().any(|needle| self.has_token(needle))
    }

    fn has_token(&self, needle: &str) -> bool {
        self.query
            .split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-')))
            .any(|token| !token.is_empty() && token == needle)
    }

    pub(super) fn has_blade_ui_surface_terms(&self) -> bool {
        self.has_any(&[
            "component",
            "components",
            "view",
            "views",
            "slot",
            "section",
        ])
    }

    pub(super) fn has_ui_runtime_surface_terms(&self) -> bool {
        self.has_any(&[
            "canvas",
            "dashboard",
            "editor",
            "layout",
            "message",
            "messages",
            "node details",
            "panel",
            "panels",
            "screen",
            "viewmodel",
        ])
    }

    pub(super) fn has_blade_form_action_terms(&self) -> bool {
        self.has_any(&[
            "form", "forms", "modal", "modals", "partial", "partials", "table", "tables",
        ])
    }

    pub(super) fn has_manifest_hint(&self) -> bool {
        self.has_any(&[
            "cargo",
            "pyproject",
            "composer",
            "requirements",
            "pipfile",
            "tsconfig",
            "go.mod",
            "go.sum",
            "pnpm-workspace",
            "turbo",
            "gradle",
            "nimble",
            "rockspec",
            "mix.exs",
        ])
    }

    pub(super) fn mentions_rust_family(&self) -> bool {
        self.has_any(&[
            "cargo",
            "cargo.toml",
            "cargo.lock",
            "rust",
            "crate",
            "crates",
        ])
    }

    pub(super) fn mentions_php_family(&self) -> bool {
        self.has_any(&[
            "php", "composer", "artisan", "laravel", "blade", "livewire", "flux",
        ])
    }

    pub(super) fn mentions_typescript_family(&self) -> bool {
        self.has_any(&[
            "typescript",
            "tsconfig",
            "tsx",
            "vitest",
            "vite",
            "jest",
            "playwright",
            "nextjs",
            "deno",
            "vue",
            "js sdk",
            "pnpm",
            "turbo",
            "node-cli",
        ])
    }

    pub(super) fn mentions_python_family(&self) -> bool {
        self.has_any(&[
            "python",
            "pyproject",
            "pipfile",
            "requirements.txt",
            "pytest",
        ])
    }

    pub(super) fn mentions_go_family(&self) -> bool {
        self.has_any(&["main.go", "go.mod", "go.sum", "go module"]) || self.has_token("golang")
    }

    pub(super) fn mentions_kotlin_family(&self) -> bool {
        self.has_any(&[
            "kotlin",
            "gradle",
            "gradle.kts",
            "android",
            "viewmodel",
            "compose",
        ])
    }

    pub(super) fn mentions_lua_family(&self) -> bool {
        self.has_any(&["lua", "luarocks", "rockspec", "neovim", "nvim"])
    }

    pub(super) fn mentions_roc_family(&self) -> bool {
        self.has_any(&["roc-lang", "main.roc"]) || self.has_token("roc")
    }

    pub(super) fn mentions_nim_family(&self) -> bool {
        self.has_any(&["nimble", ".nim", ".nims"]) || self.has_token("nim")
    }

    pub(super) fn is_runtime_config_shorthand(&self) -> bool {
        matches!(self.query.as_str(), "config" | "configuration")
    }

    pub(super) fn mentions_laravel_ui(&self) -> bool {
        self.has_any(&["blade", "livewire", "flux"])
            && (self.has_blade_ui_surface_terms() || self.has_blade_form_action_terms())
    }

    pub(super) fn has_strong_test_focus_terms(&self) -> bool {
        self.has_any(&[
            "fixture",
            "fixtures",
            "integration",
            "scenario",
            "assert",
            "coverage",
            "parity",
            "replay",
            "conformance",
            "inspector",
        ])
    }

    pub(super) fn mentions_model_data_surface(&self) -> bool {
        self.has_any(&[
            "model",
            "models",
            "migration",
            "migrations",
            "seeder",
            "seeders",
            "factory",
            "factories",
            "policy",
            "policies",
            "validation",
            "database",
            "schema",
            "table",
            "tables",
        ])
    }

    pub(super) fn mentions_playbooks(&self) -> bool {
        self.has_any(&["playbook", "playbooks"])
    }
}
