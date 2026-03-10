use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) enum LaravelUiSurfaceClass {
    BladeView,
    LivewireComponent,
    LivewireView,
    BladeComponent,
}

pub(super) fn is_laravel_livewire_component_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    (normalized.starts_with("app/Livewire/") || normalized.starts_with("app/Http/Livewire/"))
        && normalized.ends_with(".php")
}

pub(super) fn is_laravel_command_or_middleware_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    (normalized.starts_with("app/Console/Commands/")
        || normalized.starts_with("app/Http/Middleware/"))
        && normalized.ends_with(".php")
}

pub(super) fn is_laravel_job_or_listener_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    (normalized.starts_with("app/Jobs/")
        || normalized.starts_with("app/Listeners/")
        || normalized.starts_with("app/Events/")
        || normalized.starts_with("app/Mail/"))
        && normalized.ends_with(".php")
}

pub(super) fn is_laravel_view_component_class_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with("app/View/Components/") && normalized.ends_with(".php")
}

pub(super) fn is_laravel_provider_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with("app/Providers/") && normalized.ends_with(".php")
}

pub(super) fn is_laravel_core_provider_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    matches!(
        Path::new(&normalized)
            .file_name()
            .and_then(|name| name.to_str()),
        Some(
            "appserviceprovider.php"
                | "authserviceprovider.php"
                | "broadcastserviceprovider.php"
                | "configurationserviceprovider.php"
                | "duskserviceprovider.php"
                | "eventserviceprovider.php"
        )
    )
}

pub(super) fn is_laravel_route_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with("routes/") && normalized.ends_with(".php")
}

pub(super) fn is_laravel_bootstrap_entrypoint_path(path: &str) -> bool {
    path.trim_start_matches("./") == "bootstrap/app.php"
}

pub(super) fn is_laravel_blade_view_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with("resources/views/") && normalized.ends_with(".blade.php")
}

pub(super) fn is_laravel_layout_blade_view_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.starts_with("resources/views/")
        && normalized.ends_with(".blade.php")
        && (normalized.starts_with("resources/views/layouts/")
            || matches!(
                Path::new(&normalized)
                    .file_name()
                    .and_then(|name| name.to_str()),
                Some("layout.blade.php")
            ))
}

pub(super) fn is_laravel_livewire_view_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with("resources/views/livewire/") && normalized.ends_with(".blade.php")
}

pub(super) fn is_laravel_non_livewire_blade_view_path(path: &str) -> bool {
    is_laravel_blade_view_path(path)
        && !is_laravel_livewire_view_path(path)
        && !is_laravel_blade_component_path(path)
}

pub(super) fn is_laravel_blade_component_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with("resources/views/")
        && normalized.ends_with(".blade.php")
        && normalized
            .split('/')
            .any(|component| component.eq_ignore_ascii_case("components"))
}

pub(super) fn is_laravel_root_blade_component_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./");
    normalized.starts_with("resources/views/components/") && normalized.ends_with(".blade.php")
}

pub(super) fn is_laravel_nested_blade_component_path(path: &str) -> bool {
    is_laravel_blade_component_path(path) && !is_laravel_root_blade_component_path(path)
}

pub(super) fn is_laravel_form_action_blade_path(path: &str) -> bool {
    let normalized = path.trim_start_matches("./").to_ascii_lowercase();
    normalized.starts_with("resources/views/")
        && normalized.ends_with(".blade.php")
        && (normalized.contains("/forms/")
            || normalized.contains("/partials/")
            || normalized.contains("/modals/"))
}

pub(super) fn laravel_ui_surface_class(path: &str) -> Option<LaravelUiSurfaceClass> {
    if is_laravel_non_livewire_blade_view_path(path) {
        Some(LaravelUiSurfaceClass::BladeView)
    } else if is_laravel_livewire_component_path(path) {
        Some(LaravelUiSurfaceClass::LivewireComponent)
    } else if is_laravel_livewire_view_path(path) {
        Some(LaravelUiSurfaceClass::LivewireView)
    } else if is_laravel_blade_component_path(path) {
        Some(LaravelUiSurfaceClass::BladeComponent)
    } else {
        None
    }
}

pub(super) fn laravel_ui_surface_novelty_bonus(
    surface: LaravelUiSurfaceClass,
    prefer_blade_components: bool,
) -> f32 {
    if prefer_blade_components {
        return match surface {
            LaravelUiSurfaceClass::BladeComponent => 0.86,
            LaravelUiSurfaceClass::BladeView => 0.24,
            LaravelUiSurfaceClass::LivewireView => 0.14,
            LaravelUiSurfaceClass::LivewireComponent => 0.10,
        };
    }

    match surface {
        LaravelUiSurfaceClass::BladeView => 0.80,
        LaravelUiSurfaceClass::LivewireView => 0.72,
        LaravelUiSurfaceClass::LivewireComponent => 0.22,
        LaravelUiSurfaceClass::BladeComponent => 0.10,
    }
}

pub(super) fn laravel_ui_surface_repeat_penalty(
    surface: LaravelUiSurfaceClass,
    prefer_blade_components: bool,
) -> f32 {
    if prefer_blade_components {
        return match surface {
            LaravelUiSurfaceClass::BladeComponent => 0.18,
            LaravelUiSurfaceClass::BladeView => 0.42,
            LaravelUiSurfaceClass::LivewireView => 0.36,
            LaravelUiSurfaceClass::LivewireComponent => 0.26,
        };
    }

    match surface {
        LaravelUiSurfaceClass::BladeView => 0.10,
        LaravelUiSurfaceClass::LivewireView => 0.14,
        LaravelUiSurfaceClass::LivewireComponent => 0.22,
        LaravelUiSurfaceClass::BladeComponent => 0.58,
    }
}
