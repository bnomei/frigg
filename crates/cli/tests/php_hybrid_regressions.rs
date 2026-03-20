#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::searcher::{SearchHybridExecutionOutput, SearchHybridQuery, TextSearcher};
use frigg::settings::FriggConfig;

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-php-hybrid-regressions-{test_name}-{}-{nanos_since_epoch}",
        std::process::id()
    ))
}

fn cleanup_workspace_root(workspace_root: &Path) {
    if workspace_root.exists() {
        fs::remove_dir_all(workspace_root).expect("temporary workspace should be removable");
    }
}

fn prepare_workspace(workspace_root: &Path, files: &[(&str, &str)]) {
    for (relative_path, contents) in files {
        let absolute_path = workspace_root.join(relative_path);
        if let Some(parent) = absolute_path.parent() {
            fs::create_dir_all(parent).expect("failed to create temporary fixture directory");
        }
        fs::write(&absolute_path, contents).expect("failed to seed temporary fixture source");
    }
}

fn searcher_for_workspace_root(workspace_root: &Path) -> TextSearcher {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    TextSearcher::new(config)
}

fn hybrid_output(
    searcher: &TextSearcher,
    query: &str,
    limit: usize,
) -> SearchHybridExecutionOutput {
    searcher
        .search_hybrid(SearchHybridQuery {
            query: query.to_owned(),
            limit,
            weights: Default::default(),
            semantic: Some(false),
        })
        .expect("search_hybrid should succeed for php regression harness")
}

fn top_paths(searcher: &TextSearcher, query: &str, limit: usize) -> Vec<String> {
    hybrid_output(searcher, query, limit)
        .matches
        .into_iter()
        .map(|matched| matched.document.path)
        .collect()
}

fn assert_witness_groups(paths: &[String], groups: &[(&str, &[&str])]) {
    for (label, expected_paths) in groups {
        assert!(
            expected_paths
                .iter()
                .any(|expected| paths.iter().any(|candidate| candidate == expected)),
            "missing witness group '{label}'; expected one of {:?}, got {:?}",
            expected_paths,
            paths
        );
    }
}

fn prepare_erpsaas_laravel_ui_pressure_fixture(workspace_root: &Path) {
    prepare_workspace(
        workspace_root,
        &[
            (
                "resources/views/filament/forms/components/phone-builder.blade.php",
                "<x-dynamic-component>\n<x-slot name=\"trigger\">blade livewire flux component view slot section update password profile transaction</x-slot>\n<div>blade livewire flux component view slot section update password profile transaction blade livewire flux component view slot section update password profile transaction</div>\n</x-dynamic-component>\n",
            ),
            (
                "resources/views/filament/forms/components/linear-wizard.blade.php",
                "<x-dynamic-component>\n<x-slot name=\"trigger\">blade livewire flux component view slot section update password profile transaction</x-slot>\n<div>blade livewire flux component view slot section update password profile transaction blade livewire flux component view slot section update password profile transaction</div>\n</x-dynamic-component>\n",
            ),
            (
                "resources/views/livewire/update-password-form.blade.php",
                "<x-filament::section>\n<x-slot name=\"description\">blade livewire flux component view slot section update password</x-slot>\n<form>{{ $this->form }}</form>\n</x-filament::section>\n",
            ),
            (
                "resources/views/livewire/update-profile-information.blade.php",
                "<x-filament::section>\n<x-slot name=\"description\">blade livewire flux component view slot section profile information</x-slot>\n<form>{{ $this->form }}</form>\n</x-filament::section>\n",
            ),
            (
                "resources/views/filament/company/pages/setting/company-profile.blade.php",
                "<section>blade livewire flux component view slot section profile information update password</section>\n",
            ),
            (
                "resources/views/filament/company/pages/setting/localization.blade.php",
                "<section>blade livewire flux component view slot section livewire profile update password</section>\n",
            ),
            (
                "app/Livewire/UpdatePassword.php",
                "<?php\nnamespace App\\Livewire;\nuse Livewire\\Component;\nclass UpdatePassword extends Component {\n    public function render() {\n        return view('livewire.update-password-form');\n    }\n}\n",
            ),
            (
                "app/Livewire/UpdateProfileInformation.php",
                "<?php\nnamespace App\\Livewire;\nuse Livewire\\Component;\nclass UpdateProfileInformation extends Component {\n    public function render() {\n        return view('livewire.update-profile-information');\n    }\n}\n",
            ),
            (
                "app/Filament/Company/Resources/Accounting/TransactionResource.php",
                "<?php\nnamespace App\\Filament\\Company\\Resources\\Accounting;\nclass TransactionResource {}\n",
            ),
            (
                "app/Models/Accounting/Transaction.php",
                "<?php\nnamespace App\\Models\\Accounting;\nclass Transaction {}\n",
            ),
            (
                "resources/views/components/actions/delete-bank-connection-modal.blade.php",
                "<span>Delete the selected connected account.</span>\n",
            ),
            (
                "resources/views/components/actions/transaction-import-modal.blade.php",
                "<div>Import transactions from the selected connected account.</div>\n",
            ),
            (
                "resources/views/components/company/tables/reports/account-transactions.blade.php",
                "<div>blade livewire flux component view slot section transaction report</div>\n",
            ),
            (
                "tests/CreatesApplication.php",
                "<?php\ntrait CreatesApplication {\n    public function createApplication() {}\n}\n",
            ),
            (
                "tests/Pest.php",
                "<?php\nuses(Tests\\\\TestCase::class)->in('Feature');\n",
            ),
            ("tests/TestCase.php", "<?php\nabstract class TestCase {}\n"),
            (
                "tests/Feature/Accounting/TransactionTest.php",
                "<?php\n// tests fixtures integration transaction transaction tests fixtures integration transaction transaction\n",
            ),
        ],
    );
}

fn prepare_erpsaas_blade_profile_pressure_fixture(workspace_root: &Path) {
    prepare_erpsaas_laravel_ui_pressure_fixture(workspace_root);
    prepare_workspace(
        workspace_root,
        &[
            (
                "resources/views/filament/forms/components/company-info.blade.php",
                "<div>blade form modal action table partial page interaction resources views filament forms forms partials modals company info</div>\n",
            ),
            (
                "resources/views/filament/forms/components/custom-section.blade.php",
                "<section>blade form modal action table partial page interaction resources views filament forms forms partials modals custom section</section>\n",
            ),
            (
                "resources/views/filament/forms/components/custom-table-repeater.blade.php",
                "<div>blade form modal action table partial page interaction resources views filament forms forms partials modals custom table repeater</div>\n",
            ),
            (
                "resources/views/filament/forms/components/document-preview.blade.php",
                "<div>blade form modal action table partial page interaction resources views filament forms forms partials modals document preview</div>\n",
            ),
            (
                "resources/views/filament/forms/components/document-totals.blade.php",
                "<div>blade form modal action table partial page interaction resources views filament forms forms partials modals document totals</div>\n",
            ),
            (
                "resources/views/filament/forms/components/journal-entry-repeater.blade.php",
                "<div>blade form modal action table partial page interaction resources views filament forms forms partials modals journal entry repeater</div>\n",
            ),
            (
                "resources/views/components/company/page/custom-simple.blade.php",
                "<section>blade form modal action table partial page interaction blade layout component slot section render page navigation company page custom simple</section>\n",
            ),
            (
                "resources/views/components/company/layout/custom-simple.blade.php",
                "<div>blade form modal action table partial page interaction company layout custom simple</div>\n",
            ),
            (
                "resources/views/components/company/reports/layout.blade.php",
                "<section><x-slot name=\"header\">blade layout component slot section render page navigation company reports layout</x-slot>{{ $slot }}</section>\n",
            ),
            (
                "resources/views/components/company/reports/account-transactions-report-pdf.blade.php",
                "<div>blade layout component slot section render page navigation company reports account transactions report pdf</div>\n",
            ),
            (
                "resources/views/components/company/reports/cash-flow-statement-pdf.blade.php",
                "<div>blade layout component slot section render page navigation company reports cash flow statement report pdf</div>\n",
            ),
            (
                "resources/views/components/company/reports/income-statement-summary-pdf.blade.php",
                "<div>blade layout component slot section render page navigation company reports income statement summary report pdf</div>\n",
            ),
            (
                "resources/views/components/company/reports/report-pdf.blade.php",
                "<div>blade layout component slot section render page navigation company reports report pdf</div>\n",
            ),
            (
                "resources/views/components/company/reports/summary-report-pdf.blade.php",
                "<div>blade layout component slot section render page navigation company reports summary report pdf</div>\n",
            ),
            (
                "app/Filament/Company/Pages/Reports.php",
                "<?php\nnamespace App\\Filament\\Company\\Pages;\nclass Reports {}\n",
            ),
            (
                "app/Filament/Company/Pages/Reports/BaseReportPage.php",
                "<?php\nnamespace App\\Filament\\Company\\Pages\\Reports;\nclass BaseReportPage {}\n",
            ),
            (
                "app/Models/Accounting/Document.php",
                "<?php\nnamespace App\\Models\\Accounting;\nclass Document {}\n",
            ),
            (
                "app/Enums/Setting/Template.php",
                "<?php\nnamespace App\\Enums\\Setting;\nenum Template: string { case Default = 'default'; }\n",
            ),
        ],
    );
}

fn prepare_bookstack_blade_fix_wave_fixture(workspace_root: &Path) {
    let generic_books_partial = "<div>blade component layout slot section view render resources views parts books sidebar actions</div>\n";
    let generic_header_partial = "<div>blade component layout slot section view render resources views layouts parts header navigation</div>\n";
    let provider_pressure = "<?php\n// blade bootstrap providers routes middleware app entrypoint blade bootstrap providers routes middleware app entrypoint\n";

    prepare_workspace(
        workspace_root,
        &[
            (
                "bootstrap/app.php",
                "<?php\n$app = new \\BookStack\\App\\Application(dirname(__DIR__));\n$app->singleton(Illuminate\\Contracts\\Http\\Kernel::class, BookStack\\Http\\Kernel::class);\n$app->singleton(Illuminate\\Contracts\\Console\\Kernel::class, BookStack\\Console\\Kernel::class);\nreturn $app;\n",
            ),
            (
                "bootstrap/phpstan.php",
                "<?php\nreturn ['bootstrap' => true, 'app' => 'bookstack'];\n",
            ),
            (
                "routes/web.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::middleware(['web'])->group(function () {\n    Route::get('/books', fn () => view('books.parts.list'));\n});\n",
            ),
            (
                "routes/api.php",
                "<?php\nuse Illuminate\\Support\\Facades\\Route;\nRoute::middleware(['api'])->group(function () {\n    Route::get('/api/docs', fn () => ['ok' => true]);\n});\n",
            ),
            (
                "app/App/Providers/EventServiceProvider.php",
                provider_pressure,
            ),
            (
                "app/App/Providers/AppServiceProvider.php",
                provider_pressure,
            ),
            (
                "app/App/Providers/AuthServiceProvider.php",
                provider_pressure,
            ),
            ("app/Http/Middleware/ApiAuthenticate.php", provider_pressure),
            ("app/Http/Middleware/ApplyCspRules.php", provider_pressure),
            ("app/Http/Middleware/Authenticate.php", provider_pressure),
            (
                "resources/views/books/parts/index-sidebar-section-actions.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/books/parts/index-sidebar-section-new.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/books/parts/index-sidebar-section-popular.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/books/parts/index-sidebar-section-recents.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/books/parts/show-sidebar-section-actions.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/books/parts/show-sidebar-section-activity.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/books/parts/show-sidebar-section-details.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/books/parts/show-sidebar-section-shelves.blade.php",
                generic_books_partial,
            ),
            (
                "resources/views/layouts/parts/header-links.blade.php",
                generic_header_partial,
            ),
            (
                "resources/views/layouts/parts/header.blade.php",
                generic_header_partial,
            ),
            (
                "resources/views/layouts/parts/header-logo.blade.php",
                generic_header_partial,
            ),
            (
                "resources/views/layouts/parts/header-search.blade.php",
                generic_header_partial,
            ),
            (
                "resources/views/layouts/parts/header-user-menu.blade.php",
                generic_header_partial,
            ),
            (
                "resources/views/layouts/parts/notifications.blade.php",
                generic_header_partial,
            ),
            (
                "resources/views/api-docs/index.blade.php",
                "@extends('layouts.simple')\n@section('body')\n@include('api-docs.parts.getting-started')\n@include('api-docs.parts.endpoint')\n@stop\n",
            ),
            (
                "resources/views/api-docs/parts/endpoint.blade.php",
                "<article>\n    <h2>API endpoint</h2>\n    <p>Endpoint docs and API response details.</p>\n</article>\n",
            ),
            (
                "resources/views/api-docs/parts/getting-started.blade.php",
                "<section>\n    <h2>API getting started</h2>\n    <p>Authentication, request format, and API docs overview.</p>\n</section>\n",
            ),
            (
                "resources/views/attachments/list.blade.php",
                "<div>Attachment manager list.</div>\n",
            ),
            (
                "resources/views/attachments/manager-edit-form.blade.php",
                "<form>Attachment manager edit form.</form>\n",
            ),
            (
                "resources/views/attachments/manager-link-form.blade.php",
                "<form>Attachment manager link form.</form>\n",
            ),
            (
                "resources/views/attachments/manager-list.blade.php",
                "<div>Attachment manager list.</div>\n",
            ),
            (
                "resources/views/attachments/manager.blade.php",
                "<section>Attachment manager.</section>\n",
            ),
            (
                "tests/Activity/AuditLogApiTest.php",
                "<?php\n// tests audit log api audit log tests audit log api\n",
            ),
            (
                "tests/Activity/AuditLogTest.php",
                "<?php\n// tests audit log settings audit log permissions tests audit log\n",
            ),
            (
                "tests/Activity/CommentDisplayTest.php",
                "<?php\n// tests audit log comment display tests audit log\n",
            ),
            (
                "tests/Activity/CommentMentionTest.php",
                "<?php\n// tests audit log comment mention tests audit log\n",
            ),
            (
                "tests/Activity/CommentSettingTest.php",
                "<?php\n// tests audit log comment settings tests audit log\n",
            ),
            (
                "tests/Activity/CommentStoreTest.php",
                "<?php\n// tests audit log comment store tests audit log\n",
            ),
            (
                "resources/views/settings/audit.blade.php",
                "<h1>Audit Log</h1>\n",
            ),
        ],
    );
}

fn prepare_bookstack_playbook_test_pressure_fixture(workspace_root: &Path) {
    prepare_bookstack_blade_fix_wave_fixture(workspace_root);
    prepare_workspace(
        workspace_root,
        &[
            (
                "app/Activity/Controllers/AuditLogController.php",
                "<?php\nclass AuditLogController {\n    public function index() {\n        return 'tests fixtures integration audit log resources views api docs docs parts';\n    }\n}\n",
            ),
            (
                "app/Activity/Controllers/AuditLogApiController.php",
                "<?php\nclass AuditLogApiController {\n    public function index() {\n        return ['docs' => 'tests fixtures integration audit log resources views api docs docs parts'];\n    }\n}\n",
            ),
            (
                "app/Activity/Models/Activity.php",
                "<?php\nclass Activity {\n    public string $audit = 'tests fixtures integration audit log resources views api docs docs parts';\n}\n",
            ),
            (
                "app/Console/Commands/ClearActivityCommand.php",
                "<?php\nclass ClearActivityCommand {\n    protected $signature = 'activity:clear';\n}\n",
            ),
            (
                "app/Theming/ThemeEvents.php",
                "<?php\nclass ThemeEvents {}\n",
            ),
            (
                "app/Config/clockwork.php",
                "<?php\nreturn ['audit' => 'tests fixtures integration audit log resources views api docs docs parts'];\n",
            ),
            (
                "app/Config/logging.php",
                "<?php\nreturn ['audit' => 'tests fixtures integration audit log resources views api docs docs parts'];\n",
            ),
            (
                "app/Config/services.php",
                "<?php\nreturn ['audit' => 'tests fixtures integration audit log resources views api docs docs parts'];\n",
            ),
            (
                "dev/docs/php-testing.md",
                "# PHP testing\ntests fixtures integration audit log resources views api docs docs parts\n",
            ),
            (
                "dev/docs/development.md",
                "# Development\ntests fixtures integration audit log resources views api docs docs parts\n",
            ),
            (
                "dev/docs/permission-scenario-testing.md",
                "# Permission scenario testing\ntests fixtures integration audit log resources views api docs docs parts\n",
            ),
            (
                "lang/ar/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/bg/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/bn/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/bs/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/ca/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/cs/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/cy/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/da/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/de/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            (
                "lang/de_informal/settings.php",
                "<?php\nreturn ['audit' => 'docs'];\n",
            ),
            ("composer.json", "{ \"name\": \"bookstack/bookstack\" }\n"),
            ("composer.lock", "{ \"packages\": [] }\n"),
            ("jest.config.ts", "export default {};\n"),
            (
                "dev/build/svg-blank-transform.js",
                "export default function svgBlankTransform() { return 'audit'; }\n",
            ),
            (
                "dev/docker/db-testing/run.sh",
                "#!/usr/bin/env bash\necho audit\n",
            ),
        ],
    );
}

fn prepare_bookstack_models_data_pressure_fixture(workspace_root: &Path) {
    prepare_workspace(
        workspace_root,
        &[
            (
                "database/migrations/2014_10_12_000000_create_users_table.php",
                "<?php\nSchema::create('users', function ($table) {\n    $table->id();\n    $table->string('email');\n});\n",
            ),
            (
                "database/migrations/2014_10_12_100000_create_password_resets_table.php",
                "<?php\nSchema::create('password_resets', function ($table) {\n    $table->string('email');\n    $table->string('token');\n});\n",
            ),
            (
                "database/migrations/2015_07_12_114933_create_books_table.php",
                "<?php\nSchema::create('books', function ($table) {\n    $table->id();\n    $table->string('name');\n});\n",
            ),
            (
                "database/migrations/2015_07_12_190027_create_pages_table.php",
                "<?php\nSchema::create('pages', function ($table) {\n    $table->id();\n    $table->unsignedBigInteger('book_id');\n});\n",
            ),
            (
                "database/seeders/DatabaseSeeder.php",
                "<?php\nclass DatabaseSeeder {\n    public function run() {}\n}\n",
            ),
            (
                "database/factories/UserFactory.php",
                "<?php\nclass UserFactory {}\n",
            ),
            (
                "app/Activity/Tools/WebhookFormatter.php",
                "<?php\nclass WebhookFormatter {\n    public function format(array $data) {\n        return $data;\n    }\n}\n",
            ),
            (
                "app/Exports/ZipExports/Models/ZipExportBook.php",
                "<?php\nclass ZipExportBook {\n    public string $model = 'book';\n}\n",
            ),
            (
                "app/Entities/Tools/MixedEntityListLoader.php",
                "<?php\nclass MixedEntityListLoader {\n    public function load(array $data) {\n        return $data;\n    }\n}\n",
            ),
            (
                "app/Exports/ZipExports/Models/ZipExportAttachment.php",
                "<?php\nclass ZipExportAttachment {\n    public string $model = 'attachment';\n}\n",
            ),
            ("app/Models/User.php", "<?php\nclass User {}\n"),
            (
                "app/Policies/BookPolicy.php",
                "<?php\nclass BookPolicy {}\n",
            ),
        ],
    );
}

#[path = "php_hybrid_regressions/bookstack.rs"]
mod bookstack;
#[path = "php_hybrid_regressions/core.rs"]
mod core;
#[path = "php_hybrid_regressions/laravel.rs"]
mod laravel;
#[path = "php_hybrid_regressions/transport_runtime.rs"]
mod transport_runtime;
