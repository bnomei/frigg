use super::*;

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
