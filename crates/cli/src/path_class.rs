use crate::domain::PathClass;

pub(crate) fn classify_repository_path(relative_path: &str) -> PathClass {
    let normalized = relative_path.trim_start_matches("./");
    let components = normalized
        .split('/')
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>();
    if components.iter().any(|component| {
        matches!(
            *component,
            "benches" | "bench" | "examples" | "example" | "tests" | "test"
        )
    }) {
        PathClass::Support
    } else if normalized == "bootstrap/app.php"
        || normalized.starts_with("app/")
        || normalized.starts_with("bootstrap/")
        || normalized.starts_with("routes/")
        || normalized.starts_with("database/migrations/")
        || normalized.starts_with("database/seeders/")
        || normalized.starts_with("database/factories/")
    {
        PathClass::Runtime
    } else if normalized.starts_with("resources/views/") {
        PathClass::Support
    } else if components.iter().any(|component| *component == "src") {
        PathClass::Runtime
    } else {
        PathClass::Project
    }
}

pub(crate) fn repository_path_class(relative_path: &str) -> &'static str {
    classify_repository_path(relative_path).as_str()
}

pub(crate) fn repository_path_class_rank(path_class: &str) -> u8 {
    PathClass::from_str(path_class)
        .map(PathClass::rank)
        .unwrap_or(3)
}

#[cfg(test)]
mod tests {
    use crate::domain::PathClass;

    use super::{classify_repository_path, repository_path_class};

    #[test]
    fn repository_path_class_treats_laravel_runtime_surfaces_as_runtime() {
        assert_eq!(
            repository_path_class("app/Livewire/Dashboard.php"),
            "runtime"
        );
        assert_eq!(repository_path_class("bootstrap/app.php"), "runtime");
        assert_eq!(repository_path_class("routes/web.php"), "runtime");
        assert_eq!(
            repository_path_class("database/migrations/2014_10_12_000000_create_users_table.php"),
            "runtime"
        );
        assert_eq!(
            repository_path_class("database/seeders/DatabaseSeeder.php"),
            "runtime"
        );
        assert_eq!(
            repository_path_class("database/factories/UserFactory.php"),
            "runtime"
        );
    }

    #[test]
    fn classify_repository_path_returns_typed_classes() {
        assert_eq!(
            classify_repository_path("crates/cli/src/mcp/server.rs"),
            PathClass::Runtime
        );
        assert_eq!(
            classify_repository_path("crates/cli/examples/server.rs"),
            PathClass::Support
        );
        assert_eq!(classify_repository_path("Cargo.toml"), PathClass::Project);
    }

    #[test]
    fn repository_path_class_treats_laravel_views_and_tests_as_support() {
        assert_eq!(
            repository_path_class("resources/views/auth/login.blade.php"),
            "support"
        );
        assert_eq!(repository_path_class("tests/DuskTestCase.php"), "support");
    }
}
