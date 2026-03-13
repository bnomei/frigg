use crate::domain::SourceClass;

#[path = "surfaces/artifacts.rs"]
mod artifacts;
#[path = "surfaces/runtime.rs"]
mod runtime;
#[path = "surfaces/support.rs"]
mod support;
#[path = "surfaces/tokens.rs"]
mod tokens;

pub(in crate::searcher) type HybridSourceClass = SourceClass;

pub(in crate::searcher) use artifacts::*;
pub(in crate::searcher) use runtime::*;
pub(in crate::searcher) use support::*;

#[cfg(test)]
mod tests {
    use crate::domain::SourceClass;

    use super::{
        hybrid_source_class, is_cli_command_entrypoint_path, is_entrypoint_runtime_path,
        is_fixture_support_path, is_frontend_runtime_noise_path, is_go_entrypoint_runtime_path,
        is_kotlin_android_entrypoint_runtime_path, is_kotlin_android_ui_runtime_surface_path,
        is_python_test_witness_path, is_root_scoped_runtime_config_path,
        is_runtime_adjacent_python_test_path, is_runtime_anchor_test_support_path,
        is_runtime_config_artifact_path, is_rust_workspace_config_path, is_test_support_path,
        is_typescript_runtime_module_index_path,
    };

    #[test]
    fn hybrid_source_class_respects_specific_precedence_before_path_class() {
        assert_eq!(
            hybrid_source_class("contracts/errors.md"),
            SourceClass::ErrorContracts
        );
        assert_eq!(
            hybrid_source_class("contracts/tools/v1/search_hybrid.v1.schema.json"),
            SourceClass::ToolContracts
        );
        assert_eq!(
            hybrid_source_class("playbooks/runtime/deep-search.md"),
            SourceClass::Project
        );
    }

    #[test]
    fn hybrid_source_class_falls_back_to_typed_path_classification() {
        assert_eq!(
            hybrid_source_class("crates/cli/src/mcp/server.rs"),
            SourceClass::Runtime
        );
        assert_eq!(
            hybrid_source_class("crates/cli/examples/server.rs"),
            SourceClass::Support
        );
    }

    #[test]
    fn rust_workspace_config_paths_are_detected_as_runtime_config_artifacts() {
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            ".cargo/config.toml",
            "rust-toolchain.toml",
            "rustfmt.toml",
            "clippy.toml",
            "crates/tooling/.cargo/config.toml",
        ] {
            assert!(
                is_rust_workspace_config_path(path),
                "{path} should be detected as a rust workspace config path"
            );
            assert!(
                is_runtime_config_artifact_path(path),
                "{path} should participate in runtime config artifact ranking"
            );
        }
    }

    #[test]
    fn nim_runtime_config_paths_are_detected_as_runtime_config_artifacts() {
        for path in ["cligen.nimble", "packages/tooling/tooling.nimble"] {
            assert!(
                is_runtime_config_artifact_path(path),
                "{path} should be detected as a Nim runtime config artifact"
            );
        }

        for path in ["cligen.nim", "examples/linect.nims"] {
            assert!(
                !is_runtime_config_artifact_path(path),
                "{path} should not be treated as a Nim runtime config artifact"
            );
        }
    }

    #[test]
    fn lua_runtime_config_paths_are_detected_as_runtime_config_artifacts() {
        for path in [
            ".luarc.json",
            ".luarc.jsonc",
            ".luarc.doc.json",
            "lua-language-server-scm-1.rockspec",
            "rocks/my-tool-1.rockspec",
        ] {
            assert!(
                is_runtime_config_artifact_path(path),
                "{path} should be detected as a Lua runtime config artifact"
            );
        }

        for path in ["main.lua", "script/core/command/getConfig.lua"] {
            assert!(
                !is_runtime_config_artifact_path(path),
                "{path} should not be treated as a Lua runtime config artifact"
            );
        }
    }

    #[test]
    fn typescript_entrypoint_runtime_paths_detect_direct_src_entrypoints() {
        for path in [
            "packages/cli/src/server.ts",
            "packages/cli/src/index.ts",
            "packages/@n8n/node-cli/src/index.ts",
            "src/main.ts",
            "apps/docs/generator/cli.ts",
            "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
        ] {
            assert!(
                is_entrypoint_runtime_path(path),
                "{path} should be detected as a runtime entrypoint"
            );
        }

        for path in [
            "packages/core/src/index.ts",
            "packages/cli/src/config/index.ts",
            "packages/testing/playwright/tests/e2e/building-blocks/workflow-entry-points.spec.ts",
            "packages/@n8n/nodes-langchain/nodes/vendors/Anthropic/actions/router.ts",
        ] {
            assert!(
                !is_entrypoint_runtime_path(path),
                "{path} should not be detected as a runtime entrypoint"
            );
        }
    }

    #[test]
    fn typescript_runtime_module_index_paths_detect_generic_runtime_sibling_surfaces() {
        for path in [
            "packages/pg-meta/src/index.ts",
            "packages/ai-commands/src/sql/index.ts",
            "packages/icons/src/icons/index.ts",
            "packages/marketing/src/crm/index.ts",
            "apps/design-system/app/fonts/index.ts",
        ] {
            assert!(
                is_typescript_runtime_module_index_path(path),
                "{path} should be detected as a TypeScript runtime module index surface"
            );
        }

        for path in [
            "packages/cli/src/config/index.ts",
            "packages/testing/playwright/tests/e2e/building-blocks/workflow-entry-points.spec.ts",
            "apps/studio/tests/config/router.tsx",
        ] {
            assert!(
                !is_typescript_runtime_module_index_path(path),
                "{path} should not be detected as a TypeScript runtime module index surface"
            );
        }
    }

    #[test]
    fn lua_entrypoint_runtime_paths_detect_cli_dispatch_and_test_support() {
        for path in [
            "main.lua",
            "lua/cli/init.lua",
            "lua/cli/check.lua",
            "script/cli/init.lua",
            "script/cli/doc/export.lua",
            "script/service/init.lua",
        ] {
            assert!(
                is_entrypoint_runtime_path(path),
                "{path} should be detected as a Lua runtime entrypoint"
            );
        }

        for path in [
            "script/config/init.lua",
            "script/workspace/init.lua",
            "test/command/init.lua",
            "tests/command/init.lua",
        ] {
            assert!(
                !is_entrypoint_runtime_path(path),
                "{path} should not be detected as a Lua runtime entrypoint"
            );
        }

        for path in ["test/command/init.lua", "tests/command/init.lua"] {
            assert!(
                is_test_support_path(path),
                "{path} should be treated as test support"
            );
            assert_eq!(
                hybrid_source_class(path),
                SourceClass::Tests,
                "{path} should surface through the tests source class"
            );
        }
    }

    #[test]
    fn kotlin_android_entrypoint_runtime_paths_detect_activities_and_navigation() {
        for path in [
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoActivity.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoApplication.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoNavGraph.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoNavigation.kt",
            "feature/src/main/java/com/example/app/MainActivity.java",
        ] {
            assert!(
                is_entrypoint_runtime_path(path),
                "{path} should be detected as an Android runtime entrypoint"
            );
        }

        for path in [
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/util/CoroutinesUtils.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskViewModel.kt",
            "app/src/test/java/com/example/android/architecture/blueprints/todoapp/TodoActivityTest.kt",
            "feature/src/debug/java/com/example/app/MainActivity.java",
        ] {
            assert!(
                !is_entrypoint_runtime_path(path),
                "{path} should not be detected as an Android runtime entrypoint"
            );
        }
    }

    #[test]
    fn kotlin_android_ui_runtime_surface_paths_detect_screens_and_viewmodels() {
        for path in [
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskScreen.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskViewModel.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/statistics/StatisticsScreen.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/tasks/TasksViewModel.kt",
        ] {
            assert!(
                is_kotlin_android_ui_runtime_surface_path(path),
                "{path} should be detected as a Kotlin UI runtime surface"
            );
        }

        for path in [
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/TodoNavigation.kt",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/util/CoroutinesUtils.kt",
            "app/src/androidTest/java/com/example/android/architecture/blueprints/todoapp/addedittask/AddEditTaskScreenTest.kt",
            "app/src/main/kotlin/cn/ppps/forwarder/database/viewmodel/BaseViewModelFactory.kt",
            "app/src/main/kotlin/cn/ppps/forwarder/workers/LockScreenWorker.kt",
            "app/src/main/kotlin/cn/ppps/forwarder/fragment/condition/LockScreenFragment.kt",
            "app/src/main/kotlin/cn/ppps/forwarder/entity/condition/LockScreenSetting.kt",
            "app/src/main/kotlin/cn/ppps/forwarder/utils/ProximitySensorScreenHelper.kt",
        ] {
            assert!(
                !is_kotlin_android_ui_runtime_surface_path(path),
                "{path} should not be detected as a Kotlin UI runtime surface"
            );
        }
    }

    #[test]
    fn go_command_entrypoint_and_test_paths_are_detected() {
        for path in [
            "main.go",
            "cmd/frpc/main.go",
            "cmd/frps/root.go",
            "cmd/frpc/sub/root.go",
        ] {
            assert!(
                is_entrypoint_runtime_path(path),
                "{path} should be detected as a Go entrypoint witness"
            );
        }

        for path in [
            "pkg/config/source/aggregator_test.go",
            "pkg/auth/oidc_test.go",
            "internal/transport/router_test.go",
        ] {
            assert!(
                is_test_support_path(path),
                "{path} should be treated as Go test support"
            );
            assert_eq!(
                hybrid_source_class(path),
                SourceClass::Tests,
                "{path} should surface through the tests source class"
            );
        }

        for path in [
            "pkg/config/source/aggregator.go",
            "cmd/frps/verify.go",
            "cmd/frpc/sub/admin.go",
            "pkg/auth/oidc.go",
            "test/e2e/v1/basic/server.go",
            "web/frpc/src/main.ts",
        ] {
            assert!(
                !is_go_entrypoint_runtime_path(path),
                "{path} should not be detected as a Go entrypoint witness"
            );
        }
    }

    #[test]
    fn kotlin_android_entrypoint_runtime_paths_detect_android_startup_surfaces() {
        for path in [
            "app/src/main/java/com/example/android/todoapp/TodoActivity.kt",
            "app/src/main/java/com/example/android/todoapp/TodoApplication.kt",
            "app/src/main/java/com/example/android/todoapp/TodoNavGraph.kt",
            "app/src/main/java/com/example/android/todoapp/TodoNavigation.kt",
        ] {
            assert!(
                is_kotlin_android_entrypoint_runtime_path(path),
                "{path} should be detected as a Kotlin Android entrypoint witness"
            );
            assert!(
                is_entrypoint_runtime_path(path),
                "{path} should be treated as a runtime entrypoint"
            );
        }

        for path in [
            "app/src/main/java/com/example/android/todoapp/TodoTheme.kt",
            "app/src/main/java/com/example/android/todoapp/data/DefaultTaskRepository.kt",
            "app/src/main/java/com/example/android/todoapp/util/CoroutinesUtils.kt",
            "app/src/androidTest/java/com/example/android/todoapp/TodoActivityTest.kt",
        ] {
            assert!(
                !is_kotlin_android_entrypoint_runtime_path(path),
                "{path} should not be detected as a Kotlin Android entrypoint witness"
            );
        }
    }

    #[test]
    fn cli_command_entrypoint_paths_detect_loader_trees_across_languages() {
        for path in [
            "cmd/frpc/main.go",
            "cmd/frps/root.go",
            "packages/cli/src/server.ts",
            "backend/cli.py",
            "bin/server.js",
        ] {
            assert!(
                is_cli_command_entrypoint_path(path),
                "{path} should be treated as a CLI or command entrypoint"
            );
        }

        for path in ["web/frps/src/main.ts", "pkg/config/source/aggregator.go"] {
            assert!(
                !is_cli_command_entrypoint_path(path),
                "{path} should not be treated as a CLI or command entrypoint"
            );
        }
    }

    #[test]
    fn roc_entrypoint_runtime_paths_detect_platform_main_modules() {
        for path in ["main.roc", "platform/main.roc"] {
            assert!(
                is_entrypoint_runtime_path(path),
                "{path} should be detected as a Roc entrypoint witness"
            );
        }

        for path in [
            "platform/Arg.roc",
            "platform/Host.roc",
            "examples/command.roc",
            "examples/main.roc",
            "tests/main.roc",
        ] {
            assert!(
                !is_entrypoint_runtime_path(path),
                "{path} should not be detected as a Roc entrypoint witness"
            );
        }
    }

    #[test]
    fn python_test_witness_paths_include_loose_test_modules() {
        for path in [
            "autogpt_platform/backend/backend/api/test_helpers.py",
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
            "classic/original_autogpt/autogpt/app/helper_test.py",
        ] {
            assert!(
                is_python_test_witness_path(path),
                "{path} should be treated as a python test witness"
            );
            assert!(
                is_test_support_path(path),
                "{path} should be treated as test support for source-class ranking"
            );
            assert_eq!(
                hybrid_source_class(path),
                SourceClass::Tests,
                "{path} should surface through the tests source class"
            );
        }

        assert!(
            !is_python_test_witness_path("autogpt_platform/backend/backend/api/helpers.py"),
            "non-test python helpers should not be treated as test witnesses"
        );
    }

    #[test]
    fn runtime_config_artifacts_inside_test_trees_do_not_count_as_test_support() {
        for path in [
            "tests/sagemaker/scripts/pytorch/requirements.txt",
            "tests/cli/pyproject.toml",
            "test/runtime/setup.py",
        ] {
            assert!(
                is_runtime_config_artifact_path(path),
                "{path} should still be treated as a runtime config artifact"
            );
            assert!(
                !is_test_support_path(path),
                "{path} should not be treated as test support"
            );
        }
    }

    #[test]
    fn android_runtime_config_artifacts_detect_gradle_and_manifest_surfaces() {
        for path in [
            "app/src/main/AndroidManifest.xml",
            "app/build.gradle.kts",
            "build.gradle.kts",
            "gradle.properties",
            "gradle/init.gradle.kts",
            "settings.gradle.kts",
        ] {
            assert!(
                is_runtime_config_artifact_path(path),
                "{path} should be treated as an Android or Gradle runtime config artifact"
            );
        }

        for path in [
            "renovate.json",
            "app/src/main/java/com/example/android/architecture/blueprints/todoapp/util/CoroutinesUtils.kt",
        ] {
            assert!(
                !is_runtime_config_artifact_path(path),
                "{path} should not be treated as a runtime config artifact"
            );
        }
    }

    #[test]
    fn root_scoped_runtime_config_paths_include_tool_subdirectories() {
        for path in [
            ".cargo/config.toml",
            "gradle/init.gradle.kts",
            "gradle/wrapper/gradle-wrapper.properties",
            "settings.gradle.kts",
        ] {
            assert!(
                is_root_scoped_runtime_config_path(path),
                "{path} should be treated as a root-scoped runtime config artifact"
            );
        }

        for path in [
            "app/src/main/AndroidManifest.xml",
            "packages/tooling/gradle/wrapper/gradle-wrapper.properties",
        ] {
            assert!(
                !is_root_scoped_runtime_config_path(path),
                "{path} should not be treated as a root-scoped runtime config artifact"
            );
        }
    }

    #[test]
    fn nested_fixture_directories_surface_as_fixtures_not_tests() {
        for path in [
            "tests/fixtures/config.json",
            "tests/fixtures/sample/input.txt",
            "resources/test/fixtures/isort/pyproject.toml",
        ] {
            assert!(
                is_fixture_support_path(path),
                "{path} should be treated as a fixture surface"
            );
            assert!(
                !is_test_support_path(path),
                "{path} should not be treated as test support"
            );
            assert_eq!(
                hybrid_source_class(path),
                SourceClass::Fixtures,
                "{path} should surface through the fixtures source class"
            );
        }
    }

    #[test]
    fn non_code_assets_inside_test_trees_do_not_count_as_test_support() {
        for path in [
            "tests/trainer/distributed/accelerate_configs/deepspeed_zero2.yaml",
            "tests/data/expected.json",
            "test/runtime/sample.txt",
        ] {
            assert!(
                !is_test_support_path(path),
                "{path} should not be treated as test support"
            );
        }

        for path in [
            "tests/cli/test_chat.py",
            "tests/generation/__init__.py",
            "tests/fixtures.rs",
            "tests/scripts/license-checks.sh",
            "tests/syntax-tests/regression_test.sh",
        ] {
            assert!(
                is_test_support_path(path),
                "{path} should remain test support"
            );
        }

        for path in [
            "tests/syntax-tests/source/bash/simple.sh",
            "tests/syntax-tests/highlighted/javascript/test.js",
        ] {
            assert!(
                !is_test_support_path(path),
                "{path} should remain fixture-like test data, not test support"
            );
        }
    }

    #[test]
    fn web_typescript_surfaces_count_as_frontend_runtime_noise() {
        for path in ["web/frps/tsconfig.json", "docs/frontend/openapi.json"] {
            assert!(
                is_frontend_runtime_noise_path(path),
                "{path} should be treated as frontend runtime noise"
            );
        }

        for path in [
            "web/frps/src/main.ts",
            "web/frps/src/api/server.ts",
            "web/frps/src/router/index.ts",
            "web/server/main.go",
            "cmd/web/root.go",
            "src/web/server.rs",
        ] {
            assert!(
                !is_frontend_runtime_noise_path(path),
                "{path} should remain available until stronger frontend evidence exists"
            );
        }
    }

    #[test]
    fn runtime_adjacent_python_test_paths_distinguish_nested_runtime_modules_from_test_trees() {
        for path in [
            "autogpt_platform/backend/backend/api/test_helpers.py",
            "autogpt_platform/backend/backend/blocks/mcp/test_server.py",
            "classic/original_autogpt/autogpt/app/helper_test.py",
        ] {
            assert!(
                is_runtime_adjacent_python_test_path(path),
                "{path} should be treated as a runtime-adjacent python test path"
            );
        }

        for path in [
            "autogpt_platform/backend/test/agent_generator/test_service.py",
            "classic/original_autogpt/tests/integration/test_setup.py",
            "tests/test_server.py",
        ] {
            assert!(
                !is_runtime_adjacent_python_test_path(path),
                "{path} should remain a dedicated test-tree python path"
            );
        }
    }

    #[test]
    fn runtime_anchor_test_support_paths_strip_common_test_affixes() {
        for path in [
            "backend/tests/test_server.py",
            "backend/tests/server_test.py",
            "tests/cli_test.py",
            "pkg/worker_test.go",
        ] {
            assert!(
                is_runtime_anchor_test_support_path(path),
                "{path} should be treated as a runtime-anchor test support path"
            );
        }

        for path in [
            "backend/tests/test_helpers.py",
            "backend/tests/test_routes.py",
            "backend/app/server.py",
        ] {
            assert!(
                !is_runtime_anchor_test_support_path(path),
                "{path} should not be treated as a runtime-anchor test support path"
            );
        }
    }
}
