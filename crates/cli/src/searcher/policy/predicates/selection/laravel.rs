use super::{PredicateLeaf, SelectionFacts};
use crate::searcher::laravel::LaravelUiSurfaceClass;

fn is_laravel_core_provider(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_core_provider
}

fn is_laravel_provider(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_provider
}

fn is_laravel_route(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_route
}

fn is_laravel_bootstrap_entrypoint(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_bootstrap_entrypoint
}

fn is_laravel_non_livewire_blade_view(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_non_livewire_blade_view
}

fn is_laravel_livewire_view(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_livewire_view
}

fn is_laravel_blade_component(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_blade_component
}

fn is_laravel_nested_blade_component(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_nested_blade_component
}

fn is_laravel_form_action_blade(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_form_action_blade
}

fn is_laravel_livewire_component(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_livewire_component
}

fn is_laravel_view_component_class(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_view_component_class
}

fn is_laravel_command_or_middleware(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_command_or_middleware
}

fn is_laravel_job_or_listener(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_job_or_listener
}

fn is_laravel_layout_blade_view(ctx: &SelectionFacts) -> bool {
    ctx.is_laravel_layout_blade_view
}

fn laravel_surface_is_blade_view(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(LaravelUiSurfaceClass::BladeView)
}

fn laravel_surface_is_livewire_component(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(LaravelUiSurfaceClass::LivewireComponent)
}

fn laravel_surface_is_livewire_view(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(LaravelUiSurfaceClass::LivewireView)
}

fn laravel_surface_is_blade_component(ctx: &SelectionFacts) -> bool {
    ctx.laravel_surface == Some(LaravelUiSurfaceClass::BladeComponent)
}

macro_rules! leaf {
    ($name:ident, $id:literal, $pred:ident) => {
        pub(crate) const fn $name() -> PredicateLeaf<SelectionFacts> {
            PredicateLeaf::new($id, $pred)
        }
    };
}

leaf!(
    is_laravel_core_provider_leaf,
    "candidate.laravel_core_provider",
    is_laravel_core_provider
);
leaf!(
    is_laravel_provider_leaf,
    "candidate.laravel_provider",
    is_laravel_provider
);
leaf!(
    is_laravel_route_leaf,
    "candidate.laravel_route",
    is_laravel_route
);
leaf!(
    is_laravel_bootstrap_entrypoint_leaf,
    "candidate.laravel_bootstrap_entrypoint",
    is_laravel_bootstrap_entrypoint
);
leaf!(
    is_laravel_non_livewire_blade_view_leaf,
    "candidate.laravel_non_livewire_blade_view",
    is_laravel_non_livewire_blade_view
);
leaf!(
    is_laravel_livewire_view_leaf,
    "candidate.laravel_livewire_view",
    is_laravel_livewire_view
);
leaf!(
    is_laravel_blade_component_leaf,
    "candidate.laravel_blade_component",
    is_laravel_blade_component
);
leaf!(
    is_laravel_nested_blade_component_leaf,
    "candidate.laravel_nested_blade_component",
    is_laravel_nested_blade_component
);
leaf!(
    is_laravel_form_action_blade_leaf,
    "candidate.laravel_form_action_blade",
    is_laravel_form_action_blade
);
leaf!(
    is_laravel_livewire_component_leaf,
    "candidate.laravel_livewire_component",
    is_laravel_livewire_component
);
leaf!(
    is_laravel_view_component_class_leaf,
    "candidate.laravel_view_component_class",
    is_laravel_view_component_class
);
leaf!(
    is_laravel_command_or_middleware_leaf,
    "candidate.laravel_command_or_middleware",
    is_laravel_command_or_middleware
);
leaf!(
    is_laravel_job_or_listener_leaf,
    "candidate.laravel_job_or_listener",
    is_laravel_job_or_listener
);
leaf!(
    is_laravel_layout_blade_view_leaf,
    "candidate.laravel_layout_blade_view",
    is_laravel_layout_blade_view
);
leaf!(
    laravel_surface_is_blade_view_leaf,
    "candidate.laravel_surface.blade_view",
    laravel_surface_is_blade_view
);
leaf!(
    laravel_surface_is_livewire_component_leaf,
    "candidate.laravel_surface.livewire_component",
    laravel_surface_is_livewire_component
);
leaf!(
    laravel_surface_is_livewire_view_leaf,
    "candidate.laravel_surface.livewire_view",
    laravel_surface_is_livewire_view
);
leaf!(
    laravel_surface_is_blade_component_leaf,
    "candidate.laravel_surface.blade_component",
    laravel_surface_is_blade_component
);
