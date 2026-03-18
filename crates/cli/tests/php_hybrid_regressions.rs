#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::mcp::FriggMcpServer;
use frigg::mcp::types::FindImplementationsParams;
use frigg::searcher::{SearchHybridExecutionOutput, SearchHybridQuery, TextSearcher};
use frigg::settings::FriggConfig;
use rmcp::handler::server::wrapper::Parameters;

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

fn server_for_workspace_root(workspace_root: &Path) -> FriggMcpServer {
    let config = FriggConfig::from_workspace_roots(vec![workspace_root.to_path_buf()])
        .expect("workspace root must produce valid config");
    FriggMcpServer::new(config)
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

#[test]
fn php_hybrid_regression_harness_supports_top_k_witness_groups() {
    let workspace_root = temp_workspace_root("hybrid-witness-groups");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/ProviderInterface.php",
                "<?php\ninterface ProviderInterface {}\n",
            ),
            (
                "src/ListCompletionProvider.php",
                "<?php\nclass ListCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "README.md",
                "# Providers\nListCompletionProvider implements ProviderInterface.\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "ProviderInterface ListCompletionProvider implementation",
        5,
    );

    assert_witness_groups(
        &paths,
        &[
            ("interface-runtime", &["src/ProviderInterface.php"]),
            (
                "implementation-runtime",
                &["src/ListCompletionProvider.php"],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_graph_channel_surfaces_related_implementations_from_exact_anchor() {
    let workspace_root = temp_workspace_root("graph-related-implementations");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/ProviderInterface.php",
                "<?php\ninterface ProviderInterface {}\n",
            ),
            (
                "src/ListCompletionProvider.php",
                "<?php\nclass ListCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "src/EnumCompletionProvider.php",
                "<?php\nenum EnumCompletionProvider implements ProviderInterface { case Default; }\n",
            ),
            (
                "src/UserIdCompletionProvider.php",
                "<?php\nclass UserIdCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "README.md",
                "# Providers\nProviderInterface and ListCompletionProvider quick reference.\n",
            ),
            (
                "docs/providers.md",
                "# Providers\nProviderInterface and ListCompletionProvider overview.\n",
            ),
            (
                "examples/provider.php",
                "<?php\n// ProviderInterface example wiring for ListCompletionProvider.\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "ProviderInterface ListCompletionProvider implementation",
        5,
    );

    assert_witness_groups(
        &paths,
        &[(
            "related-implementations",
            &[
                "src/EnumCompletionProvider.php",
                "src/UserIdCompletionProvider.php",
            ],
        )],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_runtime_paths_outrank_example_support_paths() {
    let workspace_root = temp_workspace_root("runtime-over-examples");
    prepare_workspace(
        &workspace_root,
        &[
            ("packages/app/src/Prompt.php", "<?php\nclass Prompt {}\n"),
            (
                "packages/app/examples/Prompt.php",
                "<?php\nclass Prompt {}\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "Prompt", 2);

    assert_eq!(paths[0], "packages/app/src/Prompt.php");

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_prefers_canonical_type_over_reference_sibling() {
    let workspace_root = temp_workspace_root("canonical-over-reference");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/Prompt.php", "<?php\nclass Prompt {}\n"),
            (
                "src/PromptReference.php",
                "<?php\nclass PromptReference {\n    public Prompt $prompt;\n    public function fromPrompt(Prompt $prompt): PromptReference {\n        return $this;\n    }\n}\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "Prompt", 2);

    assert_eq!(paths[0], "src/Prompt.php");

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_transport_onboarding_queries_surface_readme_witnesses() {
    let workspace_root = temp_workspace_root("onboarding-readme");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Quickstart\nInstall and setup the calculator server.\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder wiring discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            ("docs/install.md", "# Install\nCalculator setup guide.\n"),
            ("src/Calculator.php", "<?php\nclass Calculator {}\n"),
            (
                "src/Server.php",
                "<?php\nclass Server { public function boot(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "examples/server.php",
                "<?php\n// calculator quickstart setup example server\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "quickstart setup calculator", 5);

    assert!(
        paths.iter().any(|path| path == "README.md"),
        "expected onboarding query to surface README witness, got {paths:?}"
    );
    assert!(
        paths.iter().any(|path| path == "examples/server.php"),
        "expected onboarding query to surface concrete example server witness, got {paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_exact_anchor_graph_hits_surface_php_implementer_witnesses() {
    let workspace_root = temp_workspace_root("exact-anchor-graph-implementers");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/ProviderInterface.php",
                "<?php\ninterface ProviderInterface {}\n",
            ),
            (
                "src/ListCompletionProvider.php",
                "<?php\nclass ListCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "docs/completion-overview.md",
                "completion providers completion providers completion providers\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "ProviderInterface completion providers", 5);
    let implementation = output
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/ListCompletionProvider.php")
        .expect("implementation runtime witness should surface in top-k");

    assert!(
        implementation.graph_score > 0.0,
        "implementation runtime witness should receive graph evidence once bounded exact-anchor expansion is active: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_exact_anchor_prefers_canonical_runtime_symbol_over_reference_sibling() {
    let workspace_root = temp_workspace_root("canonical-vs-reference");
    prepare_workspace(
        &workspace_root,
        &[
            ("src/Prompt.php", "<?php\nclass Prompt {}\n"),
            (
                "src/PromptReference.php",
                "<?php\nclass PromptReference {\n\
                 public Prompt $primaryPrompt;\n\
                 public Prompt $fallbackPrompt;\n\
                 public Prompt $activePrompt;\n\
                 }\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "Prompt", 3);

    assert_eq!(
        output.matches[0].document.path, "src/Prompt.php",
        "the canonical runtime definition should outrank its noisier reference sibling: {:?}",
        output.matches
    );
    assert!(
        output.matches[0].graph_score > 0.0,
        "the canonical definition should receive graph evidence for exact-anchor queries: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_onboarding_queries_surface_readme_witnesses() {
    let workspace_root = temp_workspace_root("onboarding-readme");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Quickstart\nUse StdioTransport for setup and install the server wiring.\n",
            ),
            (
                "examples/server.php",
                "<?php\n// stdio transport setup install configure server example\n",
            ),
            ("src/StdioTransport.php", "<?php\nclass StdioTransport {}\n"),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "quickstart stdio transport setup install", 3);
    eprintln!("{:#?}", output.matches);
    let paths = output
        .matches
        .iter()
        .map(|matched| matched.document.path.clone())
        .collect::<Vec<_>>();

    assert!(
        paths.iter().any(|path| path == "README.md"),
        "onboarding queries should surface the README witness in top-k: {paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_initialize_queries_surface_runtime_and_example_witnesses() {
    let workspace_root = temp_workspace_root("initialize-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "docs/initialize.md",
                "initialize server capabilities initialize result initialize server capabilities initialize result\n",
            ),
            (
                "composer.json",
                "{\n  \"description\": \"initialize server capabilities initialize result initialize server capabilities\"\n}\n",
            ),
            (
                "src/ServerCapabilities.php",
                "<?php\nclass ServerCapabilities {}\n",
            ),
            (
                "src/InitializeResult.php",
                "<?php\nclass InitializeResult {}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server {\n    public function initialize(): InitializeResult {\n        return new InitializeResult();\n    }\n}\n",
            ),
            (
                "examples/initialize-server.php",
                "<?php\n// initialize server wiring example\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "initialize server capabilities result", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-initialize",
                &[
                    "src/InitializeResult.php",
                    "src/ServerCapabilities.php",
                    "src/Server.php",
                ],
            ),
            ("support-example", &["examples/initialize-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_subscription_queries_surface_runtime_and_test_witnesses() {
    let workspace_root = temp_workspace_root("subscriptions-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "docs/subscriptions.md",
                "resource updated subscription notification resource updated subscription notification\n",
            ),
            (
                "composer.json",
                "{\n  \"description\": \"resource updated subscription notification resource updated\"\n}\n",
            ),
            (
                "src/SessionSubscriptionManager.php",
                "<?php\n// subscription manager runtime witness\nclass SessionSubscriptionManager {}\n",
            ),
            (
                "src/SubscribeRequest.php",
                "<?php\n// subscription request runtime witness\nclass SubscribeRequest {}\n",
            ),
            (
                "src/ResourceUpdatedNotification.php",
                "<?php\n// resource updated notification runtime witness\nclass ResourceUpdatedNotification {}\n",
            ),
            (
                "tests/ResourceUpdatedNotificationTest.php",
                "<?php\n// resource updated notification test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "resource updated subscription notification", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-subscriptions",
                &[
                    "src/SessionSubscriptionManager.php",
                    "src/SubscribeRequest.php",
                    "src/ResourceUpdatedNotification.php",
                ],
            ),
            (
                "tests-subscriptions",
                &["tests/ResourceUpdatedNotificationTest.php"],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_custom_handler_queries_surface_runtime_and_example_witnesses() {
    let workspace_root = temp_workspace_root("custom-handler-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "docs/handlers.md",
                "custom method handlers custom method handlers custom method handlers\n",
            ),
            (
                "composer.json",
                "{\n  \"description\": \"custom method handlers custom method handlers\"\n}\n",
            ),
            (
                "src/CustomMethodHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass CustomMethodHandler {}\n",
            ),
            (
                "src/HealthCheckHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass HealthCheckHandler extends CustomMethodHandler {}\n",
            ),
            (
                "examples/custom-method-server.php",
                "<?php\n// custom method handlers example server\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "custom method handlers", 4);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-handler",
                &["src/CustomMethodHandler.php", "src/HealthCheckHandler.php"],
            ),
            ("support-example", &["examples/custom-method-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_initialize_queries_surface_runtime_and_support_witnesses() {
    let workspace_root = temp_workspace_root("initialize-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Initialize\ninitialize server capabilities example initialize result initialize server capabilities example\n",
            ),
            (
                "docs/index.md",
                "# Initialize\ninitialize server capabilities example initialize result initialize server capabilities example\n",
            ),
            (
                "docs/overview.md",
                "# Overview\ninitialize server capabilities example initialize result initialize server capabilities example\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"initialize server capabilities example initialize result initialize server capabilities example\"\n}\n",
            ),
            (
                "src/InitializeResult.php",
                "<?php\n// initialize result runtime witness\nclass InitializeResult {}\n",
            ),
            (
                "src/ServerCapabilities.php",
                "<?php\n// initialize server capabilities runtime witness\nclass ServerCapabilities {}\n",
            ),
            (
                "examples/initialize.php",
                "<?php\n// initialize server capabilities example runtime wiring\n",
            ),
            (
                "tests/InitializeResultTest.php",
                "<?php\n// initialize result example test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "initialize server capabilities example", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-initialize",
                &["src/InitializeResult.php", "src/ServerCapabilities.php"],
            ),
            (
                "support-initialize",
                &["examples/initialize.php", "tests/InitializeResultTest.php"],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_completion_provider_queries_surface_runtime_and_example_witnesses() {
    let workspace_root = temp_workspace_root("completion-provider-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Completion Providers\ncompletion providers example completion providers example completion providers example\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "docs/index.md",
                "# Completion Providers\ncompletion providers example completion providers example completion providers example\n",
            ),
            (
                "docs/overview.md",
                "# Providers\ncompletion providers example completion providers example completion providers example\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"completion providers example completion providers example\"\n}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function build(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/ProviderInterface.php",
                "<?php\n// completion providers runtime witness\ninterface ProviderInterface {}\n",
            ),
            (
                "src/ListCompletionProvider.php",
                "<?php\n// completion providers runtime witness\nclass ListCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "examples/providers.php",
                "<?php\n// completion providers example runtime wiring\n",
            ),
            (
                "tests/ListCompletionProviderTest.php",
                "<?php\n// completion providers example test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "completion providers example", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-provider",
                &[
                    "src/ProviderInterface.php",
                    "src/ListCompletionProvider.php",
                ],
            ),
            (
                "support-provider",
                &[
                    "examples/providers.php",
                    "tests/ListCompletionProviderTest.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_subscription_queries_surface_runtime_update_witnesses() {
    let workspace_root = temp_workspace_root("subscription-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Resource Subscriptions\nresource subscriptions notification resource updated resource subscriptions notification resource updated\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "docs/index.md",
                "# Resource Subscriptions\nresource subscriptions notification resource updated resource subscriptions notification resource updated\n",
            ),
            (
                "docs/overview.md",
                "# Overview\nresource subscriptions notification resource updated resource subscriptions notification resource updated\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"resource subscriptions notification resource updated resource subscriptions notification resource updated\"\n}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function initialize(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/SessionSubscriptionManager.php",
                "<?php\n// resource subscriptions runtime witness\nclass SessionSubscriptionManager {}\n",
            ),
            (
                "src/SubscribeRequest.php",
                "<?php\n// resource subscriptions request runtime witness\nclass SubscribeRequest {}\n",
            ),
            (
                "src/ResourceUpdatedNotification.php",
                "<?php\n// resource updated notification runtime witness\nclass ResourceUpdatedNotification {}\n",
            ),
            (
                "examples/resource-updates.php",
                "<?php\n// resource updated notification example wiring\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "resource subscriptions notification resource updated",
        5,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-subscriptions",
                &[
                    "src/SessionSubscriptionManager.php",
                    "src/SubscribeRequest.php",
                    "src/ResourceUpdatedNotification.php",
                ],
            ),
            ("support-subscriptions", &["examples/resource-updates.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_custom_handler_queries_surface_handler_and_example_witnesses() {
    let workspace_root = temp_workspace_root("custom-handler-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Custom Method Handlers\ncustom method handlers example custom method handlers example custom method handlers example\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "docs/index.md",
                "# Custom Method Handlers\ncustom method handlers example custom method handlers example custom method handlers example\n",
            ),
            (
                "docs/overview.md",
                "# Overview\ncustom method handlers example custom method handlers example custom method handlers example\n",
            ),
            (
                "composer.json",
                "{\n  \"name\": \"acme/php-sdk\",\n  \"description\": \"custom method handlers example custom method handlers example\"\n}\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function wire(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/CustomMethodHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass CustomMethodHandler {}\n",
            ),
            (
                "src/CustomNotificationHandler.php",
                "<?php\n// custom method handlers runtime witness\nclass CustomNotificationHandler {}\n",
            ),
            (
                "examples/custom-handlers.php",
                "<?php\n// custom method handlers example wiring\n",
            ),
            (
                "tests/CustomMethodHandlerTest.php",
                "<?php\n// custom method handlers example test witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "custom method handlers example", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "runtime-handlers",
                &[
                    "src/CustomMethodHandler.php",
                    "src/CustomNotificationHandler.php",
                ],
            ),
            (
                "support-handlers",
                &[
                    "examples/custom-handlers.php",
                    "tests/CustomMethodHandlerTest.php",
                ],
            ),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_client_communication_queries_surface_gateway_and_example_witnesses() {
    let workspace_root = temp_workspace_root("client-communication-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Client Communication\nclient communication back to client client communication back to client\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function run(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "src/ClientGateway.php",
                "<?php\n// client communication runtime witness\nclass ClientGateway {}\n",
            ),
            (
                "examples/client-aware-server.php",
                "<?php\n// client communication example witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "client communication back to client", 5);

    assert_witness_groups(
        &paths,
        &[
            ("runtime-client", &["src/ClientGateway.php"]),
            ("support-client", &["examples/client-aware-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_conformance_queries_surface_test_and_transport_witnesses() {
    let workspace_root = temp_workspace_root("conformance-runtime-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "README.md",
                "# Conformance\nconformance inspector stdio http conformance inspector stdio http\n",
            ),
            (
                "docs/server-builder.md",
                "# Server Builder\nbuilder discovery runtime docs\n",
            ),
            (
                "docs/examples.md",
                "# Examples\nexample server example runtime docs\n",
            ),
            (
                "src/Server.php",
                "<?php\nclass Server { public function serve(): void {} }\n",
            ),
            (
                "src/Capability/Discovery/Discoverer.php",
                "<?php\nclass Discoverer {}\n",
            ),
            (
                "tests/Conformance/server.php",
                "<?php\n// conformance stdio server witness\n",
            ),
            (
                "tests/Inspector/http_transport_test.php",
                "<?php\n// inspector http transport witness\n",
            ),
            (
                "examples/stdio-server.php",
                "<?php\n// stdio server example witness\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(&searcher, "conformance inspector stdio http", 5);

    assert_witness_groups(
        &paths,
        &[
            (
                "conformance-test",
                &[
                    "tests/Conformance/server.php",
                    "tests/Inspector/http_transport_test.php",
                ],
            ),
            ("support-transport", &["examples/stdio-server.php"]),
        ],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_graph_channel_uses_php_target_evidence_edges() {
    let workspace_root = temp_workspace_root("target-evidence-graph-edges");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/Handlers/OrderHandler.php",
                "<?php\n\
                 namespace App\\Handlers;\n\
                 class OrderHandler {\n\
                     public function handle(): void {}\n\
                 }\n",
            ),
            (
                "src/Listeners/OrderListener.php",
                "<?php\n\
                 namespace App\\Listeners;\n\
                 use App\\Handlers\\OrderHandler;\n\
                 class OrderListener {\n\
                     public function handlers(): array {\n\
                         return [[OrderHandler::class, 'handle']];\n\
                     }\n\
                 }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrder handler listener overview.\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "OrderHandler handle listener", 5);
    let listener_match = output
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/Listeners/OrderListener.php")
        .unwrap_or_else(|| {
            panic!(
                "listener runtime witness should surface in top-k; matches={:?}; ranked_anchors={:?}; channel_results={:?}",
                output.matches,
                output.ranked_anchors,
                output.channel_results
            )
        });

    assert!(
        listener_match.graph_score > 0.0,
        "callable-literal target evidence should add graph score for listener witness: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_graph_channel_seeds_from_canonical_runtime_paths_without_exact_symbol_terms() {
    let workspace_root = temp_workspace_root("canonical-runtime-path-seeds");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/Handlers/OrderHandler.php",
                "<?php\n\
                 namespace App\\Handlers;\n\
                 class OrderHandler {\n\
                     public function handle(): void {}\n\
                 }\n",
            ),
            (
                "src/Listeners/OrderListener.php",
                "<?php\n\
                 namespace App\\Listeners;\n\
                 use App\\Handlers\\OrderHandler;\n\
                 class OrderListener {\n\
                     public function handlers(): array {\n\
                         return [[OrderHandler::class, 'handle']];\n\
                     }\n\
                 }\n",
            ),
            (
                "docs/handlers.md",
                "# Handlers\nOrder listener wiring overview.\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let output = hybrid_output(&searcher, "order listener wiring", 5);
    let handler_match = output
        .matches
        .iter()
        .find(|entry| entry.document.path == "src/Handlers/OrderHandler.php")
        .expect("canonical runtime path seed should surface the handler runtime witness");

    assert!(
        handler_match.graph_score > 0.0,
        "canonical runtime path seed should contribute graph evidence without exact symbol terms: {:?}",
        output.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn php_find_implementations_interface_falls_back_without_precise_data() {
    let workspace_root = temp_workspace_root("interface-implementations");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/ProviderInterface.php",
                "<?php\ninterface ProviderInterface {}\n",
            ),
            (
                "src/EnumCompletionProvider.php",
                "<?php\nenum EnumCompletionProvider implements ProviderInterface { case Default; }\n",
            ),
            (
                "src/ListCompletionProvider.php",
                "<?php\nclass ListCompletionProvider implements ProviderInterface {}\n",
            ),
            (
                "src/UserIdCompletionProvider.php",
                "<?php\nclass UserIdCompletionProvider implements ProviderInterface {}\n",
            ),
        ],
    );
    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("ProviderInterface".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should return deterministic php heuristic fallback")
        .0;

    assert_eq!(
        response
            .matches
            .iter()
            .map(|matched| matched.symbol.as_str())
            .collect::<Vec<_>>(),
        vec![
            "EnumCompletionProvider",
            "ListCompletionProvider",
            "UserIdCompletionProvider",
        ]
    );
    assert!(
        response.matches.iter().all(|matched| {
            matched.relation.as_deref() == Some("implements")
                && matched.precision.as_deref() == Some("heuristic")
                && matched.fallback_reason.as_deref() == Some("precise_absent")
        }),
        "all php interface implementations should be heuristic implements matches: {:?}",
        response.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn php_find_implementations_base_class_falls_back_without_precise_data() {
    let workspace_root = temp_workspace_root("base-class-implementations");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "src/StatsServiceBase.php",
                "<?php\nabstract class StatsServiceBase {}\n",
            ),
            (
                "src/CachedStatsService.php",
                "<?php\nclass CachedStatsService extends StatsServiceBase {}\n",
            ),
            (
                "src/SystemStatsService.php",
                "<?php\nclass SystemStatsService extends StatsServiceBase {}\n",
            ),
        ],
    );
    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("StatsServiceBase".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should return deterministic php extends fallback")
        .0;

    assert_eq!(
        response
            .matches
            .iter()
            .map(|matched| matched.symbol.as_str())
            .collect::<Vec<_>>(),
        vec!["CachedStatsService", "SystemStatsService"]
    );
    assert!(
        response.matches.iter().all(|matched| {
            matched.relation.as_deref() == Some("extends")
                && matched.precision.as_deref() == Some("heuristic")
                && matched.fallback_reason.as_deref() == Some("precise_absent")
        }),
        "all php base-class implementations should be heuristic extends matches: {:?}",
        response.matches
    );

    cleanup_workspace_root(&workspace_root);
}

#[tokio::test]
async fn rust_find_implementations_guard_remains_intact() {
    let workspace_root = temp_workspace_root("rust-guard");
    prepare_workspace(
        &workspace_root,
        &[(
            "src/lib.rs",
            "pub trait Service {}\n\
             pub struct Impl;\n\
             impl Service for Impl {}\n",
        )],
    );
    let server = server_for_workspace_root(&workspace_root);
    let response = server
        .find_implementations(Parameters(FindImplementationsParams {
            symbol: Some("Service".to_owned()),
            repository_id: Some("repo-001".to_owned()),
            path: None,
            line: None,
            column: None,
            limit: Some(20),
        }))
        .await
        .expect("find_implementations should preserve rust impl fallback")
        .0;

    assert_eq!(response.matches.len(), 1);
    assert_eq!(response.matches[0].symbol, "Impl");
    assert_eq!(response.matches[0].relation.as_deref(), Some("implements"));
    assert_eq!(response.matches[0].precision.as_deref(), Some("heuristic"));
    assert_eq!(
        response.matches[0].fallback_reason.as_deref(),
        Some("precise_absent")
    );

    cleanup_workspace_root(&workspace_root);
}

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
fn php_hybrid_bookstack_blade_surface_queries_keep_hidden_views_and_tests_visible() {
    let workspace_root = temp_workspace_root("bookstack-blade-surface-hidden-witnesses");
    prepare_workspace(
        &workspace_root,
        &[
            (
                "resources/views/api-docs/index.blade.php",
                "@extends('layouts.base')\n@section('content')\n<div>API docs index</div>\n@endsection\n",
            ),
            (
                "resources/views/api-docs/parts/endpoint.blade.php",
                "<section>API docs endpoint</section>\n",
            ),
            (
                "resources/views/api-docs/parts/getting-started.blade.php",
                "<section>API docs getting started</section>\n",
            ),
            (
                "resources/views/attachments/list.blade.php",
                "<div>Attachment list</div>\n",
            ),
            (
                "resources/views/attachments/manager-edit-form.blade.php",
                "<form>Attachment manager edit form</form>\n",
            ),
            (
                "resources/views/attachments/manager-link-form.blade.php",
                "<form>Attachment manager link form</form>\n",
            ),
            (
                "resources/views/attachments/manager-list.blade.php",
                "<div>Attachment manager list</div>\n",
            ),
            (
                "resources/views/attachments/manager.blade.php",
                "<section>Attachment manager</section>\n",
            ),
            (
                "tests/Activity/AuditLogApiTest.php",
                "<?php\nclass AuditLogApiTest extends TestCase {}\n",
            ),
            (
                "tests/Activity/AuditLogTest.php",
                "<?php\nclass AuditLogTest extends TestCase {}\n",
            ),
            (
                "tests/Activity/CommentDisplayTest.php",
                "<?php\nclass CommentDisplayTest extends TestCase {}\n",
            ),
            (
                "tests/Activity/CommentMentionTest.php",
                "<?php\nclass CommentMentionTest extends TestCase {}\n",
            ),
            (
                "tests/Activity/CommentSettingTest.php",
                "<?php\nclass CommentSettingTest extends TestCase {}\n",
            ),
            (
                "tests/Activity/CommentStoreTest.php",
                "<?php\nclass CommentStoreTest extends TestCase {}\n",
            ),
            (
                "resources/views/books/parts/index-sidebar-section-actions.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/layouts/parts/header-links.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/layouts/parts/header.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/layouts/parts/header-logo.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/layouts/parts/header-search.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/layouts/parts/header-user-menu.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/layouts/parts/notifications.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/books/parts/index-sidebar-section-new.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/books/parts/index-sidebar-section-popular.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/books/parts/index-sidebar-section-recents.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/books/parts/show-sidebar-section-actions.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/books/parts/show-sidebar-section-activity.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/books/parts/show-sidebar-section-details.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
            (
                "resources/views/books/parts/show-sidebar-section-shelves.blade.php",
                "<div>blade component layout slot section view render tests audit log resources views api docs docs parts</div>\n",
            ),
        ],
    );
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade component layout slot section view render tests audit log resources views api docs docs parts",
        14,
    );

    assert_witness_groups(
        &paths,
        &[
            (
                "blade-views",
                &[
                    "resources/views/api-docs/index.blade.php",
                    "resources/views/api-docs/parts/endpoint.blade.php",
                    "resources/views/api-docs/parts/getting-started.blade.php",
                    "resources/views/attachments/list.blade.php",
                    "resources/views/attachments/manager-edit-form.blade.php",
                    "resources/views/attachments/manager-link-form.blade.php",
                    "resources/views/attachments/manager-list.blade.php",
                    "resources/views/attachments/manager.blade.php",
                ],
            ),
            (
                "tests",
                &[
                    "tests/Activity/AuditLogApiTest.php",
                    "tests/Activity/AuditLogTest.php",
                    "tests/Activity/CommentDisplayTest.php",
                    "tests/Activity/CommentMentionTest.php",
                    "tests/Activity/CommentSettingTest.php",
                    "tests/Activity/CommentStoreTest.php",
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

#[test]
fn php_hybrid_bookstack_entrypoint_queries_keep_bootstrap_witnesses_visible() {
    let workspace_root = temp_workspace_root("bookstack-entrypoint-wave");
    prepare_bookstack_blade_fix_wave_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade bootstrap providers routes middleware app entrypoint",
        8,
    );

    assert_witness_groups(
        &paths,
        &[("providers", &["bootstrap/app.php", "bootstrap/phpstan.php"])],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_bookstack_blade_view_queries_keep_api_docs_views_visible() {
    let workspace_root = temp_workspace_root("bookstack-blade-views-wave");
    prepare_bookstack_blade_fix_wave_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade component layout slot section view render resources views api docs docs parts tests audit log",
        14,
    );

    assert_witness_groups(
        &paths,
        &[(
            "blade-views",
            &[
                "resources/views/api-docs/index.blade.php",
                "resources/views/api-docs/parts/endpoint.blade.php",
                "resources/views/api-docs/parts/getting-started.blade.php",
                "resources/views/attachments/list.blade.php",
                "resources/views/attachments/manager-edit-form.blade.php",
                "resources/views/attachments/manager-link-form.blade.php",
                "resources/views/attachments/manager-list.blade.php",
                "resources/views/attachments/manager.blade.php",
            ],
        )],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_bookstack_blade_test_queries_keep_audit_log_tests_visible() {
    let workspace_root = temp_workspace_root("bookstack-tests-wave");
    prepare_bookstack_blade_fix_wave_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "blade component layout slot section view render tests audit log resources views api docs docs parts",
        14,
    );

    assert_witness_groups(
        &paths,
        &[(
            "tests",
            &[
                "tests/Activity/AuditLogApiTest.php",
                "tests/Activity/AuditLogTest.php",
                "tests/Activity/CommentDisplayTest.php",
                "tests/Activity/CommentMentionTest.php",
                "tests/Activity/CommentSettingTest.php",
                "tests/Activity/CommentStoreTest.php",
            ],
        )],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_bookstack_playbook_test_queries_keep_audit_log_tests_visible() {
    let workspace_root = temp_workspace_root("bookstack-playbook-tests-wave");
    prepare_bookstack_playbook_test_pressure_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "tests fixtures integration audit log resources views api docs docs parts",
        26,
    );

    assert_witness_groups(
        &paths,
        &[(
            "tests",
            &[
                "tests/Activity/AuditLogApiTest.php",
                "tests/Activity/AuditLogTest.php",
                "tests/Activity/CommentDisplayTest.php",
                "tests/Activity/CommentMentionTest.php",
                "tests/Activity/CommentSettingTest.php",
                "tests/Activity/CommentStoreTest.php",
            ],
        )],
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn php_hybrid_bookstack_models_data_playbook_queries_keep_migrations_visible() {
    let workspace_root = temp_workspace_root("bookstack-models-data-wave");
    prepare_bookstack_models_data_pressure_fixture(&workspace_root);
    let searcher = searcher_for_workspace_root(&workspace_root);
    let paths = top_paths(
        &searcher,
        "model migration seeder factory data app models database users table resets table",
        11,
    );

    assert_witness_groups(
        &paths,
        &[(
            "models-data",
            &[
                "database/migrations/2014_10_12_000000_create_users_table.php",
                "database/migrations/2014_10_12_100000_create_password_resets_table.php",
                "database/migrations/2015_07_12_114933_create_books_table.php",
                "database/migrations/2015_07_12_190027_create_pages_table.php",
            ],
        )],
    );

    cleanup_workspace_root(&workspace_root);
}
