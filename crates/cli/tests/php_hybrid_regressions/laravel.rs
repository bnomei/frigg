use super::*;

#[test]
fn php_hybrid_laravel_ui_queries_surface_livewire_components_and_blade_views() {
    let workspace_root = temp_workspace_root("laravel-ui-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "resources/views/livewire/subscription/show.blade.php",
                "<div>livewire flux component view slot section livewire flux component view slot section</div>\n",
            ),
            (
                "resources/views/livewire/dashboard.blade.php",
                "<div>livewire flux component view slot section livewire flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/applications/advanced.blade.php",
                "<x-dropdown>\n<x-slot:title>Advanced</x-slot:title>\n<div>blade component view slot section</div>\n</x-dropdown>\n",
            ),
            (
                "resources/views/auth/login.blade.php",
                "<x-layout>\n<x-slot:title>Login</x-slot:title>\n@section('content') blade view section @endsection\n</x-layout>\n",
            ),
            (
                "app/Livewire/Dashboard.php",
                "<?php\nnamespace App\\Livewire;\nuse Livewire\\Component;\nclass Dashboard extends Component {\n    public function render() {\n        return view('livewire.dashboard');\n    }\n}\n",
            ),
            (
                "app/Livewire/MonacoEditor.php",
                "<?php\nnamespace App\\Livewire;\nuse Livewire\\Component;\nclass MonacoEditor extends Component {\n    public function render() {\n        return view('livewire.subscription.show');\n    }\n}\n",
            ),
            (
                "app/Livewire/Boarding/Index.php",
                "<?php\nnamespace App\\Livewire\\Boarding;\nuse Livewire\\Component;\nclass Index extends Component {\n    public function render() {\n        return view('livewire.subscription.show');\n    }\n}\n",
            ),
            (
                "app/Livewire/Team/AdminView.php",
                "<?php\nnamespace App\\Livewire\\Team;\nuse Livewire\\Component;\nclass AdminView extends Component {\n    public function render() {\n        return view('livewire.subscription.show');\n    }\n}\n",
            ),
            (
                "app/Livewire/Project/Index.php",
                "<?php\nnamespace App\\Livewire\\Project;\nuse Livewire\\Component;\nclass Index extends Component {\n    public function render() {\n        return view('livewire.subscription.show');\n    }\n}\n",
            ),
            (
                "app/Livewire/SharedVariables/Index.php",
                "<?php\nnamespace App\\Livewire\\SharedVariables;\nuse Livewire\\Component;\nclass Index extends Component {\n    public function render() {\n        return view('livewire.subscription.show');\n    }\n}\n",
            ),
            (
                "app/Livewire/Destination/Index.php",
                "<?php\nnamespace App\\Livewire\\Destination;\nuse Livewire\\Component;\nclass Index extends Component {\n    public function render() {\n        return view('livewire.subscription.show');\n    }\n}\n",
            ),
            (
                "app/Livewire/Subscription/Show.php",
                "<?php\nnamespace App\\Livewire\\Subscription;\nuse Livewire\\Component;\nclass Show extends Component {\n    public function render() {\n        return view('livewire.subscription.show');\n    }\n}\n",
            ),
            (
                "resources/views/layouts/app.blade.php",
                "<div>blade view layout section</div>\n",
            ),
            ("TECH_STACK.md", "Blade Livewire Flux reference docs.\n"),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade livewire flux component view slot section",
        8,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/components/applications/advanced.blade.php",
                    "resources/views/auth/login.blade.php",
                ],
            ),
            (
                "livewire-components",
                &[
                    "app/Livewire/Dashboard.php",
                    "app/Livewire/MonacoEditor.php",
                    "app/Livewire/Boarding/Index.php",
                    "app/Livewire/Team/AdminView.php",
                    "app/Livewire/Project/Index.php",
                    "app/Livewire/SharedVariables/Index.php",
                    "app/Livewire/Destination/Index.php",
                    "app/Livewire/Subscription/Show.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_ui_queries_keep_staff_blade_views_and_livewire_views_under_component_pressure()
 {
    let workspace_root = temp_workspace_root("laravel-ui-component-pressure");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "resources/views/components/forum/post.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/torrent/row.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/forum/topic-listing.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/torrent/comment-listing.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/tv/card.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/forum/subforum-listing.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/user-tag.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/components/playlist/card.blade.php",
                "<x-user-tag />\n<x-slot:toolbar>blade flux component view slot section</x-slot:toolbar>\n<div>blade flux component view slot section blade flux component view slot section</div>\n",
            ),
            (
                "resources/views/livewire/announce-search.blade.php",
                "<div>\n    <input wire:model.live=\"torrentId\" type=\"search\" />\n    <button wire:click=\"sortBy('id')\">Announces</button>\n</div>\n",
            ),
            (
                "resources/views/livewire/apikey-search.blade.php",
                "<div>\n    <input wire:model.live=\"token\" type=\"search\" />\n    <button wire:click=\"sortBy('token')\">API keys</button>\n</div>\n",
            ),
            (
                "resources/views/Staff/announce/index.blade.php",
                "@extends('layout.with-main')\n@section('title') Announces @endsection\n@section('main')\n    @livewire('announce-search')\n@endsection\n",
            ),
            (
                "resources/views/Staff/apikey/index.blade.php",
                "@extends('layout.with-main')\n@section('title') API keys @endsection\n@section('main')\n    @livewire('apikey-search')\n@endsection\n",
            ),
            ("TECH_STACK.md", "Blade Livewire Flux reference docs.\n"),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade livewire flux component view slot section",
        8,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/Staff/announce/index.blade.php",
                    "resources/views/Staff/apikey/index.blade.php",
                ],
            ),
            (
                "livewire-components",
                &[
                    "resources/views/livewire/announce-search.blade.php",
                    "resources/views/livewire/apikey-search.blade.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_ui_playbook_queries_keep_livewire_views_visible_with_staff_and_test_hints() {
    let workspace_root = temp_workspace_root("laravel-ui-playbook-livewire-views");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "resources/views/livewire/announce-search.blade.php",
                "<div>\n    <input wire:model.live=\"torrentId\" type=\"search\" />\n    <button wire:click=\"sortBy('id')\">Announces</button>\n</div>\n",
            ),
            (
                "resources/views/livewire/apikey-search.blade.php",
                "<div>\n    <input wire:model.live=\"token\" type=\"search\" />\n    <button wire:click=\"sortBy('token')\">API keys</button>\n</div>\n",
            ),
            (
                "app/Http/Livewire/ApikeySearch.php",
                "<?php\nnamespace App\\Http\\Livewire;\nuse Livewire\\Component;\nclass ApikeySearch extends Component {\n    public function render() {\n        return view('livewire.apikey-search');\n    }\n}\n",
            ),
            (
                "resources/views/Staff/announce/index.blade.php",
                "@extends('layout.with-main')\n@section('main')\n    @livewire('announce-search')\n@endsection\n",
            ),
            (
                "resources/views/Staff/apikey/index.blade.php",
                "@extends('layout.with-main')\n@section('main')\n    @livewire('apikey-search')\n@endsection\n",
            ),
            (
                "resources/views/Staff/application/index.blade.php",
                "<div>Applications</div>\n",
            ),
            (
                "resources/views/Staff/email-update/index.blade.php",
                "<div>Email update</div>\n",
            ),
            (
                "resources/views/Staff/passkey/index.blade.php",
                "<div>Passkeys</div>\n",
            ),
            (
                "resources/views/Staff/rsskey/index.blade.php",
                "<div>RSS keys</div>\n",
            ),
            (
                "resources/views/Staff/user/index.blade.php",
                "<div>Users</div>\n",
            ),
            ("tests/CreatesApplication.php", "<?php\n"),
            ("tests/CreatesUsers.php", "<?php\n"),
            ("tests/TestCase.php", "<?php\n"),
            ("TECH_STACK.md", "Blade Livewire Flux reference docs.\n"),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade livewire flux component view slot section resources views livewire announce livewire apikey app livewire staff announce staff apikey tests creates application creates users",
        16,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/Staff/announce/index.blade.php",
                    "resources/views/Staff/apikey/index.blade.php",
                ],
            ),
            (
                "livewire-components",
                &[
                    "resources/views/livewire/announce-search.blade.php",
                    "resources/views/livewire/apikey-search.blade.php",
                ],
            ),
            (
                "tests",
                &["tests/CreatesApplication.php", "tests/CreatesUsers.php"],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_command_queries_keep_commands_and_jobs_visible_together() {
    let workspace_root = temp_workspace_root("laravel-command-and-jobs");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "app/Console/Commands/AutoBanDisposableUsers.php",
                "<?php\nclass AutoBanDisposableUsers {}\n",
            ),
            (
                "app/Console/Commands/AutoBonAllocation.php",
                "<?php\nclass AutoBonAllocation {}\n",
            ),
            ("app/Events/Chatter.php", "<?php\nclass Chatter {}\n"),
            (
                "app/Jobs/ProcessAnnounce.php",
                "<?php\nclass ProcessAnnounce {}\n",
            ),
            (
                "app/Jobs/ProcessMassPM.php",
                "<?php\nclass ProcessMassPM {}\n",
            ),
            ("app/Models/Event.php", "<?php\nclass Event {}\n"),
            (
                "app/Services/Tmdb/TMDBScraper.php",
                "<?php\nclass TMDBScraper {}\n",
            ),
            (
                "book/src/local_development_macos.md",
                "queue listener event middleware runtime task chatter\n",
            ),
            (
                "book/src/local_development_arch_linux.md",
                "queue listener event middleware runtime task chatter\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "command scheduler queue job listener event middleware runtime task artisan middleware disposable users bon allocation queue listeners chatter",
        14,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "commands-middleware",
                &[
                    "app/Console/Commands/AutoBanDisposableUsers.php",
                    "app/Console/Commands/AutoBonAllocation.php",
                ],
            ),
            (
                "jobs-listeners",
                &["app/Events/Chatter.php", "app/Jobs/ProcessAnnounce.php"],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_blade_component_queries_keep_erpsaas_component_witnesses_under_nested_component_pressure()
 {
    let workspace_root = temp_workspace_root("laravel-erpsaas-view-components");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "resources/views/filament/forms/components/phone-builder.blade.php",
                "<x-dynamic-component>\n<x-slot name=\"trigger\">blade component layout slot section view render</x-slot>\n<div>blade component layout slot section view render blade component layout slot section view render</div>\n</x-dynamic-component>\n",
            ),
            (
                "resources/views/filament/forms/components/linear-wizard.blade.php",
                "<x-dynamic-component>\n<x-slot name=\"trigger\">blade component layout slot section view render</x-slot>\n<div>blade component layout slot section view render blade component layout slot section view render</div>\n</x-dynamic-component>\n",
            ),
            (
                "resources/views/filament/forms/components/journal-entry-repeater.blade.php",
                "<x-dynamic-component>\n<x-slot name=\"trigger\">blade component layout slot section view render</x-slot>\n<div>blade component layout slot section view render blade component layout slot section view render</div>\n</x-dynamic-component>\n",
            ),
            (
                "resources/views/filament/company/components/page/custom-simple.blade.php",
                "<section>\n<x-slot name=\"header\">blade component layout slot section view render</x-slot>\n<div>blade component layout slot section view render blade component layout slot section view render</div>\n</section>\n",
            ),
            (
                "resources/views/vendor/filament-clusters/components/field-wrapper.blade.php",
                "<div>\n<x-slot name=\"label\">blade component layout slot section view render</x-slot>\n{{ $slot }}\n</div>\n",
            ),
            (
                "resources/views/livewire/update-password-form.blade.php",
                "<x-filament::section>\n<x-slot name=\"description\">Update password form</x-slot>\n<form>{{ $this->form }}</form>\n</x-filament::section>\n",
            ),
            (
                "resources/views/livewire/update-profile-information.blade.php",
                "<x-filament::section>\n<x-slot name=\"description\">Update profile form</x-slot>\n<form>{{ $this->form }}</form>\n</x-filament::section>\n",
            ),
            (
                "app/Livewire/Company/Service/ConnectedAccount/ListInstitutions.php",
                "<?php\nnamespace App\\Livewire\\Company\\Service\\ConnectedAccount;\nuse Livewire\\Component;\nclass ListInstitutions extends Component {\n    public function transactionImportModal() {\n        return view('components.actions.transaction-import-modal');\n    }\n    public function deleteBankConnectionModal() {\n        return view('components.actions.delete-bank-connection-modal');\n    }\n    public function render() {\n        return view('livewire.update-password-form');\n    }\n}\n",
            ),
            (
                "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                "<span>Are you sure you want to delete this bank connection?</span>\n<ul><li>{{ $institution->name }}</li></ul>\n",
            ),
            (
                "resources/views/components/actions/transaction-import-modal.blade.php",
                "<div>\n    {{ __('Import transactions from the selected account.') }}\n</div>\n",
            ),
            (
                "resources/views/components/avatar.blade.php",
                "<img {{ $attributes }} src=\"{{ $user->avatar }}\" />\n",
            ),
            (
                "resources/views/components/company/document-template/container.blade.php",
                "@props(['preview' => false])\n<div class=\"doc-template-container\">{{ $slot }}</div>\n",
            ),
            (
                "resources/views/components/company/document-template/footer.blade.php",
                "<footer class=\"doc-template-footer\">{{ $slot }}</footer>\n",
            ),
            (
                "resources/views/components/company/document-template/header.blade.php",
                "<header class=\"doc-template-header\">{{ $slot }}</header>\n",
            ),
            (
                "resources/views/components/company/document-template/line-items.blade.php",
                "<section class=\"doc-template-line-items\">{{ $slot }}</section>\n",
            ),
            (
                "resources/views/components/company/document-template/logo.blade.php",
                "<img class=\"doc-template-logo\" alt=\"Document logo\" />\n",
            ),
            (
                "resources/views/components/company/document-template/metadata.blade.php",
                "<div class=\"doc-template-metadata\">{{ $documentNumber }}</div>\n",
            ),
            (
                "resources/views/filament/company/components/document-templates/default.blade.php",
                "<x-company.document-template.container>\n    <x-company.document-template.header></x-company.document-template.header>\n    <x-company.document-template.metadata></x-company.document-template.metadata>\n    <x-company.document-template.line-items></x-company.document-template.line-items>\n    <x-company.document-template.footer></x-company.document-template.footer>\n</x-company.document-template.container>\n",
            ),
            ("TECH_STACK.md", "Blade Livewire Flux reference docs.\n"),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade component layout slot section view render",
        8,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                    "resources/views/components/actions/transaction-import-modal.blade.php",
                    "resources/views/components/avatar.blade.php",
                    "resources/views/components/company/document-template/container.blade.php",
                    "resources/views/components/company/document-template/footer.blade.php",
                    "resources/views/components/company/document-template/header.blade.php",
                    "resources/views/components/company/document-template/line-items.blade.php",
                    "resources/views/components/company/document-template/logo.blade.php",
                ],
            ),
            (
                "view-components",
                &[
                    "resources/views/components/company/document-template/container.blade.php",
                    "resources/views/components/company/document-template/footer.blade.php",
                    "resources/views/components/company/document-template/header.blade.php",
                    "resources/views/components/company/document-template/line-items.blade.php",
                    "resources/views/components/company/document-template/logo.blade.php",
                    "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                    "resources/views/components/actions/transaction-import-modal.blade.php",
                    "resources/views/components/company/document-template/metadata.blade.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_ui_queries_surface_erpsaas_action_components_and_test_harnesses() {
    let workspace_root = temp_workspace_root("laravel-erpsaas-ui-action-components-and-tests");
    prepare_erpsaas_laravel_ui_pressure_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade livewire flux component view slot section resources views actions delete actions transaction view components livewire password livewire profile update password app livewire",
        16,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                    "resources/views/components/actions/transaction-import-modal.blade.php",
                ],
            ),
            (
                "livewire-components",
                &[
                    "resources/views/livewire/update-password-form.blade.php",
                    "resources/views/livewire/update-profile-information.blade.php",
                    "app/Livewire/UpdatePassword.php",
                    "app/Livewire/UpdateProfileInformation.php",
                ],
            ),
            (
                "tests",
                &[
                    "tests/CreatesApplication.php",
                    "tests/Pest.php",
                    "tests/TestCase.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_test_queries_keep_erpsaas_action_components_visible() {
    let workspace_root = temp_workspace_root("laravel-erpsaas-tests-and-components");
    prepare_erpsaas_laravel_ui_pressure_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "tests fixtures integration creates application pest resources views actions delete actions transaction view components livewire password",
        16,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                    "resources/views/components/actions/transaction-import-modal.blade.php",
                ],
            ),
            (
                "tests",
                &[
                    "tests/CreatesApplication.php",
                    "tests/Pest.php",
                    "tests/TestCase.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_blade_form_action_queries_keep_erpsaas_filament_form_components_visible() {
    let workspace_root = temp_workspace_root("laravel-erpsaas-blade-forms-actions");
    prepare_erpsaas_blade_profile_pressure_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade form modal action table partial page interaction resources views filament forms forms partials modals tests creates application",
        14,
    );

    assert_witness_groups(
        &paths,
        &[(
            "forms-actions",
            &[
                "resources/views/filament/forms/components/company-info.blade.php",
                "resources/views/filament/forms/components/custom-section.blade.php",
                "resources/views/filament/forms/components/custom-table-repeater.blade.php",
                "resources/views/filament/forms/components/document-preview.blade.php",
                "resources/views/filament/forms/components/document-totals.blade.php",
                "resources/views/filament/forms/components/journal-entry-repeater.blade.php",
                "resources/views/filament/forms/components/linear-wizard.blade.php",
                "resources/views/filament/forms/components/phone-builder.blade.php",
            ],
        )],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_blade_layout_queries_keep_erpsaas_report_layout_component_visible() {
    let workspace_root = temp_workspace_root("laravel-erpsaas-blade-layouts");
    prepare_erpsaas_blade_profile_pressure_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade layout component slot section render page navigation resources views layouts company reports view components document template",
        14,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "layouts-pages",
                &["resources/views/components/company/reports/layout.blade.php"],
            ),
            (
                "view-components",
                &[
                    "resources/views/components/company/reports/account-transactions-report-pdf.blade.php",
                    "resources/views/components/company/reports/cash-flow-statement-pdf.blade.php",
                    "resources/views/components/company/reports/income-statement-summary-pdf.blade.php",
                    "resources/views/components/company/reports/report-pdf.blade.php",
                    "resources/views/components/company/reports/summary-report-pdf.blade.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_blade_view_surface_test_queries_keep_erpsaas_document_components_visible() {
    let workspace_root = temp_workspace_root("laravel-erpsaas-blade-view-surface-tests");
    prepare_erpsaas_blade_profile_pressure_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade component layout slot section view render tests creates application pest resources views document template actions delete view components",
        16,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                    "resources/views/components/actions/transaction-import-modal.blade.php",
                    "resources/views/components/avatar.blade.php",
                    "resources/views/components/company/document-template/container.blade.php",
                    "resources/views/components/company/document-template/footer.blade.php",
                    "resources/views/components/company/document-template/header.blade.php",
                    "resources/views/components/company/document-template/line-items.blade.php",
                    "resources/views/components/company/document-template/logo.blade.php",
                ],
            ),
            (
                "view-components",
                &[
                    "resources/views/components/company/document-template/container.blade.php",
                    "resources/views/components/company/document-template/footer.blade.php",
                    "resources/views/components/company/document-template/header.blade.php",
                    "resources/views/components/company/document-template/line-items.blade.php",
                    "resources/views/components/company/document-template/logo.blade.php",
                    "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                    "resources/views/components/actions/transaction-import-modal.blade.php",
                    "resources/views/components/company/document-template/metadata.blade.php",
                ],
            ),
            (
                "tests",
                &[
                    "tests/CreatesApplication.php",
                    "tests/Pest.php",
                    "tests/TestCase.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_compound_test_hint_queries_surface_bootstrap_test_files() {
    let workspace_root = temp_workspace_root("laravel-test-hints");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "tests/Unit/Jobs/RestartProxyJobTest.php",
                "<?php\n// tests fixtures integration tests tests fixtures integration tests\n",
            ),
            (
                "tests/Feature/HetznerServerCreationTest.php",
                "<?php\n// tests fixtures integration tests tests fixtures integration tests\n",
            ),
            (
                "tests/Pest.php",
                "<?php\n// tests fixtures integration tests tests fixtures integration tests\n",
            ),
            (
                "tests/CreatesApplication.php",
                "<?php\ntrait CreatesApplication {\n    public function createApplication() {}\n}\n",
            ),
            ("tests/DuskTestCase.php", "<?php\nclass DuskTestCase {}\n"),
            (
                "tests/Browser/LoginTest.php",
                "<?php\nclass LoginTest extends DuskTestCase {}\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "tests fixtures integration tests createsapplication dusktestcase",
        8,
    );

    assert_witness_groups(
        &paths,
        &[(
            "test-bootstrap",
            &[
                "tests/CreatesApplication.php",
                "tests/DuskTestCase.php",
                "tests/Browser/LoginTest.php",
            ],
        )],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_bootstrap_queries_keep_bootstrap_entrypoints_visible_under_provider_crowding()
{
    let workspace_root = temp_workspace_root("laravel-bootstrap-entrypoint-crowding");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "bootstrap/app.php",
                "<?php\nreturn Application::configure(basePath: dirname(__DIR__));\n",
            ),
            ("bootstrap/phpstan.php", "<?php\nreturn [];\n"),
            (
                "routes/web.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::get('/', fn () => view('books.index'));\n",
            ),
            (
                "routes/api.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::get('/ping', fn () => ['ok' => true]);\n",
            ),
            (
                "app/App/Providers/EventServiceProvider.php",
                "<?php\nclass EventServiceProvider {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/App/Providers/AppServiceProvider.php",
                "<?php\nclass AppServiceProvider {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/App/Providers/AuthServiceProvider.php",
                "<?php\nclass AuthServiceProvider {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/App/Providers/BroadcastServiceProvider.php",
                "<?php\nclass BroadcastServiceProvider {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/App/Providers/HorizonServiceProvider.php",
                "<?php\nclass HorizonServiceProvider {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Http/Middleware/ApiAuthenticate.php",
                "<?php\nclass ApiAuthenticate {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Http/Middleware/ApplyCspRules.php",
                "<?php\nclass ApplyCspRules {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Http/Middleware/Authenticate.php",
                "<?php\nclass Authenticate {}\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade bootstrap providers routes middleware app entrypoint",
        8,
    );

    assert_witness_groups(
        &paths,
        &[
            ("routes", &["routes/web.php", "routes/api.php"]),
            ("bootstrap", &["bootstrap/app.php", "bootstrap/phpstan.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_laravel_entrypoint_queries_surface_routes_files() {
    let workspace_root = temp_workspace_root("laravel-routes");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "composer.lock",
                "bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "tests/Feature/TrustHostsMiddlewareTest.php",
                "<?php\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/RouteServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass RouteServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/FortifyServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass FortifyServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/ConfigurationServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass ConfigurationServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/EventServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass EventServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/BroadcastServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass BroadcastServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/AppServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass AppServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/HorizonServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass HorizonServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "app/Providers/DuskServiceProvider.php",
                "<?php\nnamespace App\\Providers;\nclass DuskServiceProvider {}\n// bootstrap providers routes middleware app entrypoint bootstrap providers routes middleware app entrypoint\n",
            ),
            (
                "routes/web.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::middleware(['web'])->group(function () {\n    Route::get('/', fn () => view('welcome'));\n});\n",
            ),
            (
                "routes/api.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::middleware(['api'])->group(function () {\n    Route::get('/ping', fn () => ['ok' => true]);\n});\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "bootstrap providers routes middleware app entrypoint",
        8,
    );

    assert_witness_groups(
        &paths,
        &[
            ("routes", &["routes/web.php", "routes/api.php"]),
            (
                "providers",
                &[
                    "app/Providers/AppServiceProvider.php",
                    "app/Providers/BroadcastServiceProvider.php",
                    "app/Providers/ConfigurationServiceProvider.php",
                    "app/Providers/DuskServiceProvider.php",
                    "app/Providers/EventServiceProvider.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}
