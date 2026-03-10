pub(crate) fn repository_path_class(relative_path: &str) -> &'static str {
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
        "support"
    } else if normalized == "bootstrap/app.php"
        || normalized.starts_with("app/")
        || normalized.starts_with("bootstrap/")
        || normalized.starts_with("routes/")
    {
        "runtime"
    } else if normalized.starts_with("resources/views/") {
        "support"
    } else if components.iter().any(|component| *component == "src") {
        "runtime"
    } else {
        "project"
    }
}

pub(crate) fn repository_path_class_rank(path_class: &str) -> u8 {
    match path_class {
        "runtime" => 0,
        "project" => 1,
        "support" => 2,
        _ => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::repository_path_class;

    #[test]
    fn repository_path_class_treats_laravel_runtime_surfaces_as_runtime() {
        assert_eq!(
            repository_path_class("app/Livewire/Dashboard.php"),
            "runtime"
        );
        assert_eq!(repository_path_class("bootstrap/app.php"), "runtime");
        assert_eq!(repository_path_class("routes/web.php"), "runtime");
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
