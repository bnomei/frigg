#![allow(clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use frigg::searcher::{SearchHybridQuery, TextSearcher};
use frigg::settings::FriggConfig;

fn temp_workspace_root(test_name: &str) -> PathBuf {
    let nanos_since_epoch = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "frigg-lua-hybrid-regressions-{test_name}-{}-{nanos_since_epoch}",
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

fn top_paths(searcher: &TextSearcher, query: &str, limit: usize) -> Vec<String> {
    searcher
        .search_hybrid(SearchHybridQuery {
            query: query.to_owned(),
            limit,
            weights: Default::default(),
            semantic: Some(false),
        })
        .expect("search_hybrid should succeed for lua regression harness")
        .matches
        .into_iter()
        .map(|matched| matched.document.path)
        .collect()
}

#[test]
fn lua_entrypoint_queries_surface_cli_dispatch_over_test_init_noise() {
    let workspace_root = temp_workspace_root("lua-entrypoints");
    prepare_workspace(
        &workspace_root,
        &[
            ("main.lua", "require 'cli'\nrequire 'service'\n"),
            (
                "script/cli/init.lua",
                "if _G['CHECK'] then require 'cli.check' end\nif _G['HELP'] then require 'cli.help' end\n",
            ),
            (
                "script/cli/check.lua",
                "local M = {}\nfunction M.runCLI() end\nreturn M\n",
            ),
            ("script/cli/help.lua", "return function() end\n"),
            ("script/cli/version.lua", "return function() end\n"),
            ("script/cli/doc/export.lua", "return function() end\n"),
            ("script/config/init.lua", "return require 'config.loader'\n"),
            ("script/service/init.lua", "return require 'service'\n"),
            (
                "make/bootstrap.lua",
                "package.path = root .. '/script/?.lua'\n",
            ),
            ("test/command/init.lua", "require 'command.auto-require'\n"),
            ("test/code_action/init.lua", "require 'core.code-action'\n"),
            ("test/definition/init.lua", "require 'core.definition'\n"),
            ("test/hover/init.lua", "require 'core.hover'\n"),
        ],
    );

    let ranked_paths = top_paths(
        &searcher_for_workspace_root(&workspace_root),
        "entry point bootstrap init cli command runtime server",
        14,
    );

    let first_cli_entrypoint = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                path.as_str(),
                "script/cli/init.lua"
                    | "script/cli/check.lua"
                    | "script/cli/help.lua"
                    | "script/cli/version.lua"
                    | "script/cli/doc/export.lua"
            )
        })
        .expect("at least one Lua CLI entrypoint witness should be ranked");
    let first_non_cli_test_init = ranked_paths
        .iter()
        .position(|path| path == "test/code_action/init.lua")
        .expect("non-cli test init noise should still be ranked");

    assert!(
        ranked_paths
            .iter()
            .take(5)
            .any(|path| path == "script/cli/init.lua"),
        "saved Lua entrypoint queries should recover script/cli/init.lua near the top: {ranked_paths:?}"
    );
    assert!(
        first_cli_entrypoint < first_non_cli_test_init,
        "Lua CLI entrypoints should outrank generic non-cli test/init noise: {ranked_paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}

#[test]
fn lua_entrypoint_queries_keep_repo_root_runtime_config_visible() {
    let workspace_root = temp_workspace_root("lua-entrypoints-config");
    prepare_workspace(
        &workspace_root,
        &[
            ("main.lua", "require 'cli'\nrequire 'service'\n"),
            (
                "script/cli/init.lua",
                "if _G['CHECK'] then require 'cli.check' end\nif _G['HELP'] then require 'cli.help' end\n",
            ),
            (
                "script/cli/check.lua",
                "local M = {}\nfunction M.runCLI() end\nreturn M\n",
            ),
            ("script/cli/help.lua", "return function() end\n"),
            ("script/cli/version.lua", "return function() end\n"),
            ("script/cli/doc/export.lua", "return function() end\n"),
            ("script/service/init.lua", "return require 'service'\n"),
            (
                "script/core/command/reloadFFIMeta.lua",
                "return function() end\n",
            ),
            (
                ".luarc.json",
                "{ \"runtime\": { \"version\": \"LuaJIT\" } }\n",
            ),
            (
                "lua-language-server-scm-1.rockspec",
                "package = 'lua-language-server'\nversion = 'scm-1'\n",
            ),
            (
                ".github/workflows/build.yml",
                "name: Build\njobs:\n  build:\n    steps:\n      - run: ninja\n",
            ),
            ("test/command/init.lua", "require 'command.auto-require'\n"),
            ("test/code_action/init.lua", "require 'core.code-action'\n"),
            ("test/definition/init.lua", "require 'core.definition'\n"),
            ("test/hover/init.lua", "require 'core.hover'\n"),
        ],
    );

    let ranked_paths = top_paths(
        &searcher_for_workspace_root(&workspace_root),
        "entry point bootstrap init cli command runtime server",
        14,
    );

    assert!(
        ranked_paths.iter().take(14).any(|path| matches!(
            path.as_str(),
            ".luarc.json" | "lua-language-server-scm-1.rockspec"
        )),
        "entrypoint queries should keep a Lua repo-root runtime config visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(6)
            .any(|path| path == "script/cli/init.lua"),
        "entrypoint queries should still keep Lua CLI dispatch near the top: {ranked_paths:?}"
    );

    cleanup_workspace_root(&workspace_root);
}
