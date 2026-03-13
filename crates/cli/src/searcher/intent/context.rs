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
            "mix.exs",
        ])
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
