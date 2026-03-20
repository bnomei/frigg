use super::*;

#[test]
fn hybrid_ranking_companion_surface_pairs_are_retained_for_typescript_editor_subtrees()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-editor-companion-retention");
    prepare_workspace(
        &root,
        &[
            (
                "apps/editor/src/runtime/session_manager.ts",
                "export function sessionManager() { return 'session runtime'; }\n",
            ),
            (
                "apps/editor/tests/session_manager.test.ts",
                "describe('session manager', () => {});\n",
            ),
            (
                "apps/editor/mocks/session_manager.mock.ts",
                "export const sessionManagerMock = {};\n",
            ),
            (
                "apps/other/src/runtime/worker.ts",
                "export function workerRuntime() { return 'worker'; }\n",
            ),
            (
                "apps/other/tests/worker.test.ts",
                "describe('worker', () => {});\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "session manager runtime tests mock".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths
            .iter()
            .take(4)
            .any(|path| *path == "apps/editor/src/runtime/session_manager.ts"),
        "editor runtime witness should be retained in top-4: {ranked_paths:?}",
    );
    assert!(
        ranked_paths
            .iter()
            .take(4)
            .any(|path| *path == "apps/editor/tests/session_manager.test.ts"),
        "editor companion tests should be retained in top-4: {ranked_paths:?}",
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_type_package_and_workspace_surfaces_keep_localized_coverage() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-package-workspace-locality");
    prepare_workspace(
        &root,
        &[
            (
                "apps/platform/package.json",
                "{\"name\":\"platform\",\"workspaces\":[\"./packages/*\"]}\n",
            ),
            (
                "apps/platform/src/config/build.rs",
                "export const buildConfig = { mode: 'platform' };\n",
            ),
            ("apps/platform/tsconfig.json", "{\"compilerOptions\":{}}\n"),
            ("apps/other/package.json", "{\"name\":\"other\"}\n"),
            (
                "apps/other/src/runtime.ts",
                "export const otherRuntime = true;\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "platform package workspace config build runtime".to_owned(),
            limit: 5,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let witness_paths = output
        .channel_results
        .iter()
        .find(|result| result.channel == crate::domain::EvidenceChannel::PathSurfaceWitness)
        .map(|result| {
            result
                .hits
                .iter()
                .map(|hit| hit.document.path.as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let package_position = ranked_paths
        .iter()
        .position(|path| *path == "apps/platform/package.json")
        .expect("platform package manifest should be ranked");
    let workspace_position = ranked_paths
        .iter()
        .position(|path| *path == "apps/platform/tsconfig.json")
        .unwrap_or_else(|| panic!("workspace config surface should be ranked: ranked={ranked_paths:?} witness={witness_paths:?}"));
    let sibling_package_position = ranked_paths
        .iter()
        .position(|path| *path == "apps/other/package.json")
        .expect("sibling package manifest should still be ranked");

    assert!(
        package_position < sibling_package_position
            && workspace_position < sibling_package_position,
        "platform-localized package/config surfaces should beat sibling package noise: {ranked_paths:?}",
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_typescript_entrypoint_queries_keep_cli_entrypoints_visible_under_workflow_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-entrypoints-vs-workflow-noise");
    prepare_workspace(
        &root,
        &[
            (
                "packages/cli/src/server.ts",
                "export function startServer() { return \"bootstrap server app\"; }\n",
            ),
            (
                "packages/cli/src/index.ts",
                "export { startServer } from \"./server\";\n",
            ),
            (
                "packages/@n8n/node-cli/src/index.ts",
                "export const runCli = \"cli bootstrap app\";\n",
            ),
            (
                "packages/frontend/editor-ui/src/main.ts",
                "export const mount = \"frontend browser app\";\n",
            ),
            (
                "packages/@n8n/task-runner-python/src/main.py",
                "ENTRYPOINT = 'entry point bootstrap server app cli router main'\n",
            ),
            (
                "packages/@n8n/nodes-langchain/nodes/vendors/Anthropic/actions/router.ts",
                "export const router = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "packages/testing/playwright/tests/e2e/building-blocks/workflow-entry-points.spec.ts",
                "test('entry point bootstrap server app cli router main');\n",
            ),
            (
                "packages/testing/playwright/tests/e2e/capabilities/proxy-server.spec.ts",
                "test('entry point bootstrap server app cli router main');\n",
            ),
            (
                ".github/workflows/build-windows.yml",
                "name: Build windows\njobs:\n  build:\n    steps:\n      - run: pnpm build\n",
            ),
            (
                ".github/workflows/docker-build-push.yml",
                "name: Docker build push\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                ".github/workflows/docker-build-smoke.yml",
                "name: Docker build smoke\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                ".github/workflows/release-create-pr.yml",
                "name: Release create pr\njobs:\n  release:\n    steps:\n      - run: pnpm release\n",
            ),
            (
                ".github/workflows/release-merge-tag-to-branch.yml",
                "name: Release merge tag to branch\njobs:\n  release:\n    steps:\n      - run: pnpm release\n",
            ),
            (
                ".github/workflows/sec-publish-fix.yml",
                "name: Security publish fix\njobs:\n  publish:\n    steps:\n      - run: pnpm publish\n",
            ),
            (
                ".github/workflows/create-patch-release-branch.yml",
                "name: Create patch release branch\njobs:\n  release:\n    steps:\n      - run: pnpm release\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap server app cli router main".to_owned(),
            limit: 10,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.iter().take(10).any(|path| {
            matches!(
                *path,
                "packages/cli/src/server.ts"
                    | "packages/cli/src/index.ts"
                    | "packages/@n8n/node-cli/src/index.ts"
            )
        }),
        "typescript runtime entrypoints should remain visible under workflow/test crowding: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_runtime_config_queries_keep_typescript_runtime_entrypoints_visible_under_test_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-config-vs-test-noise");
    prepare_workspace(
        &root,
        &[
            ("package.json", "{ \"name\": \"supabase\" }\n"),
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
            ),
            (
                ".github/workflows/ai-tests.yml",
                "name: AI Unit Tests\njobs:\n  test:\n    steps:\n      - run: pnpm run test\n",
            ),
            (
                "packages/ai-commands/src/sql/index.ts",
                "export * from './functions'\n",
            ),
            (
                "packages/pg-meta/src/index.ts",
                "export { config } from './pg-meta-config'\n",
            ),
            (
                "packages/pg-meta/test/config.test.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "packages/pg-meta/test/functions.test.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "packages/ai-commands/test/extensions.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "packages/ai-commands/test/sql-util.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "apps/studio/tests/config/router.test.tsx",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "apps/studio/tests/config/router.tsx",
                "export const router = 'config package tsconfig github workflow ai tests';\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "config package tsconfig github workflow ai tests".to_owned(),
            limit: 14,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
        "runtime-config queries should keep a config artifact visible in top-k: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "packages/ai-commands/src/sql/index.ts" | "packages/pg-meta/src/index.ts"
            )
        }),
        "runtime-config queries should still surface a runtime entrypoint sibling in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_runtime_config_queries_recover_nimble_manifests_under_nim_test_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-nim-config-vs-test-noise");
    prepare_workspace(
        &root,
        &[
            (
                "cligen.nimble",
                "# Package\nversion = \"1.0.0\"\nauthor = \"Example\"\ndescription = \"Infer & generate command-line interface\"\nrequires \"nim >= 2.0.0\"\nskipDirs = @[\"test\"]\n",
            ),
            (
                "cligen/clCfgInit.nim",
                "proc dispatchCli*() = discard # entry point bootstrap cli command runtime app server\n",
            ),
            (
                "cligen/clCfgToml.nim",
                "proc loadConfig*() = discard # entry point bootstrap cli command runtime app server\n",
            ),
            (
                ".github/workflows/gh-pages.yml",
                "name: gh-pages\njobs:\n  docs:\n    steps:\n      - run: nimble docs\n",
            ),
            (
                ".github/workflows/test.yml",
                "name: test\njobs:\n  test:\n    steps:\n      - run: nimble test\n",
            ),
            (
                "test/OptionT.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
            (
                "test/AllSeqTypes.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
            (
                "test/AllSetTypes.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
            (
                "test/AllTypes.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
            (
                "test/BlockedShort.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
            (
                "test/CustomCmdName.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
            (
                "test/CustomType.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
            (
                "test/MultiMulti.nim",
                "discard \"config github workflow gh pages test\"\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "config github workflow gh pages test".to_owned(),
            limit: 8,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.iter().take(8).any(|path| {
            matches!(
                *path,
                "cligen.nimble" | ".github/workflows/gh-pages.yml" | ".github/workflows/test.yml"
            )
        }),
        "runtime-config queries should recover a saved-wave Nim runtime-config witness in top-k under test noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_queries_recover_typescript_config_artifacts_without_explicit_config_terms()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-entrypoints-with-config-siblings");
    prepare_workspace(
        &root,
        &[
            ("package.json", "{ \"name\": \"supabase\" }\n"),
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "packages/build-icons/src/main.mjs",
                "export const build = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "apps/studio/tests/config/router.tsx",
                "export const router = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                ".github/workflows/braintrust-preview-scorers-deploy.yml",
                "name: Deploy preview scorers\njobs:\n  deploy:\n    steps:\n      - run: pnpm deploy\n",
            ),
            (
                ".github/workflows/publish_image.yml",
                "name: Publish image\njobs:\n  publish:\n    steps:\n      - run: docker build .\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap server app cli router main".to_owned(),
            limit: 14,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.iter().take(14).any(|path| {
            *path == "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
        }),
        "entrypoint queries should still surface the runtime entrypoint witness in top-k: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
        "entrypoint queries should recover a config artifact sibling in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_typescript_config_queries_keep_root_manifests_and_runtime_entrypoints_visible()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-config-vs-test-crowding");
    prepare_workspace(
        &root,
        &[
            (
                "package.json",
                "{\n  \"scripts\": {\n    \"test:ui\": \"pnpm turbo run test --filter=ui\",\n    \"authorize-vercel-deploys\": \"tsx scripts/authorizeVercelDeploys.ts\"\n  }\n}\n",
            ),
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export function createServerClient() { return \"supabase server\"; }\n",
            ),
            (
                "apps/docs/generator/cli.ts",
                "export async function runCli() { return \"docs cli\"; }\n",
            ),
            (
                ".github/workflows/ai-tests.yml",
                "name: AI tests\njobs:\n  test:\n    steps:\n      - run: pnpm test:ui\n",
            ),
            (
                ".github/workflows/authorize-vercel-deploys.yml",
                "name: Authorize vercel deploys\njobs:\n  release:\n    steps:\n      - run: pnpm authorize-vercel-deploys\n",
            ),
            (
                ".github/workflows/autofix_linters.yml",
                "name: Autofix linters\njobs:\n  lint:\n    steps:\n      - run: pnpm lint\n",
            ),
            (
                ".github/workflows/avoid-typos.yml",
                "name: Avoid typos\njobs:\n  docs:\n    steps:\n      - run: pnpm docs:lint\n",
            ),
            (
                ".github/workflows/braintrust-evals.yml",
                "name: Braintrust evals\njobs:\n  evals:\n    steps:\n      - run: pnpm test:ui\n",
            ),
            (
                ".github/workflows/docs-tests.yml",
                "name: Docs tests\njobs:\n  docs:\n    steps:\n      - run: pnpm test:docs\n",
            ),
            (
                ".github/workflows/pg-meta-tests.yml",
                "name: pg-meta tests\njobs:\n  test:\n    steps:\n      - run: pnpm test:ui\n",
            ),
            (
                "packages/pg-meta/test/config.test.ts",
                "describe('config', () => test('package tsconfig github workflow ai tests', () => {}));\n",
            ),
            (
                "packages/pg-meta/test/sql/studio/get-users-common.test.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "apps/studio/tests/config/router.test.tsx",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "apps/studio/tests/config/router.tsx",
                "export const router = 'config package tsconfig github workflow ai tests';\n",
            ),
            (
                "apps/studio/tests/config/msw.test.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "packages/ai-commands/test/extensions.ts",
                "export const extensionTest = 'config package tsconfig github workflow ai tests';\n",
            ),
            (
                "packages/ai-commands/test/sql-util.ts",
                "export const sqlUtilTest = 'config package tsconfig github workflow ai tests';\n",
            ),
            (
                "packages/build-icons/src/main.mjs",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "examples/ai/image_search/image_search/main.py",
                "ENTRYPOINT = 'entry point bootstrap server app cli router main'\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "config package tsconfig github workflow ai tests".to_owned(),
            limit: 14,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
        "typescript config queries should keep a root manifest visible under config-test crowding: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
                    | "apps/docs/generator/cli.ts"
            )
        }),
        "typescript config queries should keep a runtime entrypoint visible under config-test crowding: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_typescript_entrypoint_queries_keep_root_manifests_visible_under_test_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-entrypoints-vs-config-tests");
    prepare_workspace(
        &root,
        &[
            (
                "package.json",
                "{\n  \"scripts\": {\n    \"build\": \"turbo run build\",\n    \"test:ui\": \"turbo run test --filter=ui\"\n  }\n}\n",
            ),
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export function createServerClient() { return \"supabase server\"; }\n",
            ),
            (
                "apps/docs/generator/cli.ts",
                "export async function runCli() { return \"docs cli\"; }\n",
            ),
            (
                "apps/ui-library/registry/default/clients/nextjs/lib/supabase/server.ts",
                "export function createNextClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "packages/build-icons/src/main.mjs",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "apps/studio/tests/config/router.tsx",
                "export const router = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "apps/studio/tests/config/router.test.tsx",
                "test('entry point bootstrap server app cli router main');\n",
            ),
            (
                "packages/pg-meta/test/db/server.crt",
                "entry point bootstrap server app cli router main\n",
            ),
            (
                "packages/pg-meta/test/db/server.key",
                "entry point bootstrap server app cli router main\n",
            ),
            (
                "examples/ai/image_search/image_search/main.py",
                "ENTRYPOINT = 'entry point bootstrap server app cli router main'\n",
            ),
            (
                "examples/auth/nextjs-full/lib/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "examples/auth/nextjs/lib/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "examples/realtime/nextjs-authorization-demo/utils/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "examples/user-management/angular-user-management/src/main.ts",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "examples/user-management/ionic-angular-user-management/src/main.ts",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap server app cli router main".to_owned(),
            limit: 14,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
                    | "apps/docs/generator/cli.ts"
            )
        }),
        "typescript entrypoint queries should keep a runtime entrypoint visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
        "typescript entrypoint queries should keep a root manifest visible under test crowding: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_typescript_config_queries_recover_saved_fix_wave_runtime_siblings()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-saved-wave-config-crowding");
    prepare_workspace(
        &root,
        &[
            (
                "package.json",
                "{\n  \"scripts\": {\n    \"test:ui\": \"pnpm turbo run test --filter=ui\"\n  }\n}\n",
            ),
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export function createServerClient() { return \"supabase server\"; }\n",
            ),
            (
                "apps/docs/generator/cli.ts",
                "export async function runCli() { return \"docs cli\"; }\n",
            ),
            (
                "apps/docs/internals/files/cli.ts",
                "export async function runFilesCli() { return \"files cli\"; }\n",
            ),
            (
                "apps/design-system/app/fonts/index.ts",
                "export * from './registry';\n",
            ),
            (
                "packages/pg-meta/src/index.ts",
                "export * from './pg-meta';\n",
            ),
            (
                "packages/ai-commands/src/sql/index.ts",
                "export * from './functions';\n",
            ),
            (
                "packages/icons/src/icons/index.ts",
                "export * from './library';\n",
            ),
            (
                "packages/marketing/src/crm/index.ts",
                "export * from './hubspot';\n",
            ),
            (
                ".github/workflows/ai-tests.yml",
                "name: AI tests\njobs:\n  test:\n    steps:\n      - run: pnpm test:ui\n",
            ),
            (
                ".github/workflows/authorize-vercel-deploys.yml",
                "name: Authorize vercel deploys\njobs:\n  release:\n    steps:\n      - run: pnpm authorize-vercel-deploys\n",
            ),
            (
                ".github/workflows/autofix_linters.yml",
                "name: Autofix linters\njobs:\n  lint:\n    steps:\n      - run: pnpm lint\n",
            ),
            (
                ".github/workflows/avoid-typos.yml",
                "name: Avoid typos\njobs:\n  docs:\n    steps:\n      - run: pnpm docs:lint\n",
            ),
            (
                ".github/workflows/braintrust-evals.yml",
                "name: Braintrust evals\njobs:\n  evals:\n    steps:\n      - run: pnpm test:ui\n",
            ),
            (
                ".github/workflows/braintrust-preview-scorers-cleanup.yml",
                "name: Braintrust cleanup\njobs:\n  cleanup:\n    steps:\n      - run: pnpm cleanup\n",
            ),
            (
                ".github/workflows/braintrust-preview-scorers-deploy.yml",
                "name: Braintrust deploy\njobs:\n  deploy:\n    steps:\n      - run: pnpm deploy\n",
            ),
            (
                ".github/workflows/docs-lint-v2-comment.yml",
                "name: Docs lint comment\njobs:\n  docs:\n    steps:\n      - run: pnpm docs:lint\n",
            ),
            (
                ".github/workflows/docs-tests.yml",
                "name: Docs tests\njobs:\n  docs:\n    steps:\n      - run: pnpm test:docs\n",
            ),
            (
                ".github/workflows/fix-typos.yml",
                "name: Fix typos\njobs:\n  docs:\n    steps:\n      - run: pnpm docs:lint\n",
            ),
            (
                ".github/workflows/pg-meta-tests.yml",
                "name: pg-meta tests\njobs:\n  test:\n    steps:\n      - run: pnpm test:ui\n",
            ),
            (
                ".github/workflows/prettier.yml",
                "name: Prettier\njobs:\n  lint:\n    steps:\n      - run: pnpm prettier\n",
            ),
            (
                ".github/workflows/dashboard-pr-reminder.yml",
                "name: Dashboard reminder\njobs:\n  docs:\n    steps:\n      - run: pnpm docs:lint\n",
            ),
            (
                ".github/workflows/docs-lint-v2-scheduled.yml",
                "name: Docs lint scheduled\njobs:\n  docs:\n    steps:\n      - run: pnpm docs:lint\n",
            ),
            (
                "packages/pg-meta/test/config.test.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "packages/ai-commands/test/extensions.ts",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "apps/studio/tests/config/router.test.tsx",
                "test('config package tsconfig github workflow ai tests');\n",
            ),
            (
                "apps/studio/tests/config/router.tsx",
                "export const router = 'config package tsconfig github workflow ai tests';\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "config package tsconfig github workflow ai tests".to_owned(),
            limit: 14,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
        "saved-wave config queries should keep a root config artifact visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
                    | "apps/docs/generator/cli.ts"
                    | "apps/docs/internals/files/cli.ts"
                    | "apps/design-system/app/fonts/index.ts"
                    | "packages/pg-meta/src/index.ts"
                    | "packages/ai-commands/src/sql/index.ts"
                    | "packages/icons/src/icons/index.ts"
                    | "packages/marketing/src/crm/index.ts"
            )
        }),
        "saved-wave config queries should recover a runtime sibling witness under workflow crowding: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_typescript_entrypoint_queries_recover_saved_fix_wave_nested_indexes()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-typescript-saved-wave-entrypoint-crowding");
    prepare_workspace(
        &root,
        &[
            (
                "package.json",
                "{\n  \"scripts\": {\n    \"build\": \"turbo run build\",\n    \"test:ui\": \"turbo run test --filter=ui\"\n  }\n}\n",
            ),
            (
                "tsconfig.json",
                "{ \"compilerOptions\": { \"jsx\": \"react\" } }\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export function createServerClient() { return \"supabase server\"; }\n",
            ),
            (
                "apps/docs/generator/cli.ts",
                "export async function runCli() { return \"docs cli\"; }\n",
            ),
            (
                "apps/docs/internals/files/cli.ts",
                "export async function runFilesCli() { return \"files cli\"; }\n",
            ),
            (
                "apps/design-system/app/fonts/index.ts",
                "export * from './registry';\n",
            ),
            (
                "packages/pg-meta/src/index.ts",
                "export * from './pg-meta';\n",
            ),
            (
                "packages/ai-commands/src/sql/index.ts",
                "export * from './functions';\n",
            ),
            (
                "packages/icons/src/icons/index.ts",
                "export * from './library';\n",
            ),
            (
                "packages/marketing/src/crm/index.ts",
                "export * from './hubspot';\n",
            ),
            (
                "packages/build-icons/src/main.mjs",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "apps/studio/tests/config/router.tsx",
                "export const router = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "examples/auth/nextjs-full/lib/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "packages/pg-meta/test/db/server.crt",
                "entry point bootstrap server app cli router main\n",
            ),
            (
                "packages/pg-meta/test/db/server.key",
                "entry point bootstrap server app cli router main\n",
            ),
            (
                "apps/ui-library/registry/default/clients/nextjs/lib/supabase/server.ts",
                "export function createNextClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "examples/auth/nextjs/lib/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "examples/realtime/nextjs-authorization-demo/utils/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "examples/user-management/angular-user-management/src/main.ts",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "examples/user-management/ionic-angular-user-management/src/main.ts",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "examples/user-management/nextjs-user-management/lib/supabase/server.ts",
                "export function createClient() { return 'entry point bootstrap server app cli router main'; }\n",
            ),
            (
                "examples/auth/quickstarts/react/src/main.jsx",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "examples/todo-list/sveltejs-todo-list/src/main.ts",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
            (
                "examples/user-management/react-user-management/src/main.jsx",
                "export const main = 'entry point bootstrap server app cli router main';\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap server app cli router main".to_owned(),
            limit: 14,
            weights: HybridChannelWeights::default(),
            semantic: Some(false),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials::default(),
        &PanicSemanticQueryEmbeddingExecutor,
    )?;

    let ranked_paths = output
        .matches
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
                    | "apps/docs/generator/cli.ts"
                    | "apps/docs/internals/files/cli.ts"
                    | "apps/design-system/app/fonts/index.ts"
                    | "packages/pg-meta/src/index.ts"
                    | "packages/ai-commands/src/sql/index.ts"
                    | "packages/icons/src/icons/index.ts"
                    | "packages/marketing/src/crm/index.ts"
            )
        }),
        "saved-wave entrypoint queries should recover a required runtime witness under example crowding: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(14).any(|path| {
            matches!(
                *path,
                "apps/design-system/app/fonts/index.ts"
                    | "packages/pg-meta/src/index.ts"
                    | "packages/ai-commands/src/sql/index.ts"
                    | "packages/icons/src/icons/index.ts"
                    | "packages/marketing/src/crm/index.ts"
            )
        }),
        "saved-wave entrypoint queries should surface a nested runtime index witness: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| matches!(*path, "package.json" | "tsconfig.json")),
        "saved-wave entrypoint queries should keep a root config artifact visible: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
