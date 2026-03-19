use super::*;

#[test]
fn hybrid_ranking_cli_entrypoint_queries_prefer_cli_test_witnesses_over_runtime_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-cli-test-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff/src/commands/analyze_graph.rs",
                "pub fn analyze_graph() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/checkers/ast/analyze/expression.rs",
                "pub fn analyze_expression() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/checkers/ast/analyze/module.rs",
                "pub fn analyze_module() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/checkers/ast/analyze/suite.rs",
                "pub fn analyze_suite() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/src/lib.rs",
                "pub fn lib_runtime() { let _ = \"ruff analyze\"; }\n",
            ),
            (
                "crates/ruff_linter/resources/test/fixtures/isort/pyproject.toml",
                "[tool.ruff]\nline-length = 88\n",
            ),
            (
                "crates/ruff/tests/integration_test.rs",
                "mod integration_test {}\n",
            ),
            (
                ".github/workflows/ci.yaml",
                "ruff analyze cli entrypoint workflow\n",
            ),
            (
                "crates/ruff/tests/cli/analyze_graph.rs",
                "mod cli_analyze_graph {}\n",
            ),
            ("crates/ruff/tests/cli/main.rs", "mod cli_main {}\n"),
            ("crates/ruff/tests/cli/format.rs", "mod cli_format {}\n"),
            ("crates/ruff/tests/cli/lint.rs", "mod cli_lint {}\n"),
            ("crates/ruff/tests/config.rs", "mod config_test {}\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "ruff analyze ruff cli entrypoint".to_owned(),
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
            .any(|path| *path == "crates/ruff/tests/cli/analyze_graph.rs"),
        "CLI analyze_graph test witness should land near the top for the saved query: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(5)
            .any(|path| *path == "crates/ruff/tests/cli/main.rs"),
        "secondary CLI test witness should remain visible near the top: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_ci_workflow_queries_surface_hidden_workflow_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-ci-workflow-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                ".github/workflows/autofix.yml",
                "name: autofix.ci\njobs:\n  autofix:\n    steps:\n      - run: cargo codegen\n",
            ),
            (
                ".github/workflows/bench_cli.yml",
                "name: Bench CLI\njobs:\n  bench:\n    steps:\n      - run: cargo bench\n",
            ),
            (
                "crates/noise/src/github.rs",
                "pub fn github_reporter() { let _ = \"github workflow autofix\"; }\n",
            ),
            (
                "crates/noise/src/autofix.rs",
                "pub fn autofix_runtime() { let _ = \"autofix runtime\"; }\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "github workflow autofix bench cli".to_owned(),
            limit: 4,
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
        ranked_paths.iter().take(4).any(|path| matches!(
            *path,
            ".github/workflows/autofix.yml" | ".github/workflows/bench_cli.yml"
        )),
        "CI workflow query should surface a hidden workflow witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_scripts_ops_queries_surface_script_and_justfile_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-scripts-ops-witnesses");
    prepare_workspace(
        &root,
        &[
            ("justfile", "fmt:\n\tcargo fmt\n"),
            (
                "scripts/print-changelog.sh",
                "#!/usr/bin/env bash\necho changelog\n",
            ),
            (
                "scripts/update-manifests.mjs",
                "console.log('update manifests');\n",
            ),
            ("docs/changelog.md", "# Changelog\nupdate notes\n"),
            ("src/version.rs", "pub const VERSION: &str = \"1.0.0\";\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "scripts justfile changelog manifests".to_owned(),
            limit: 4,
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
        ranked_paths.iter().take(4).any(|path| matches!(
            *path,
            "justfile" | "scripts/print-changelog.sh" | "scripts/update-manifests.mjs"
        )),
        "scripts/ops query should surface a concrete script witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_firecrawl_queries_prefer_typescript_runtime_over_python_drift() -> FriggResult<()>
{
    let root = temp_workspace_root("hybrid-firecrawl-typescript-locality");
    prepare_workspace(
        &root,
        &[
            (
                "apps/api/src/workers/playwright_service.ts",
                "export function playwrightService() { return 'typescript runtime'; }\n",
            ),
            (
                "apps/api/tests/playwright_service.test.ts",
                "describe('playwright service tests', () => {});\n",
            ),
            (
                "sdk/python/firecrawl/client.py",
                "def playwright_service_client():\n    return 'python drift'\n",
            ),
            (
                "docs/python-sdk.md",
                "# Python SDK\nplaywright service runtime tests\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "api workers playwright service typescript runtime tests".to_owned(),
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
        ranked_paths.iter().take(2).any(|path| {
            matches!(
                *path,
                "apps/api/src/workers/playwright_service.ts"
                    | "apps/api/tests/playwright_service.test.ts"
            )
        }),
        "typescript runtime/test witnesses should land near the top: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
#[ignore = "open TS path-locality escalation target"]
fn hybrid_ranking_firecrawl_js_sdk_queries_prefer_typescript_sdk_over_python_drift()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-firecrawl-js-sdk-vs-python-drift");
    prepare_workspace(
        &root,
        &[
            (
                "apps/js-sdk/firecrawl/src/client.ts",
                "export class FirecrawlClient {}\n",
            ),
            (
                "apps/js-sdk/firecrawl/src/__tests__/unit/v2/agent.test.ts",
                "describe('agent', () => {});\n",
            ),
            (
                "apps/python-sdk/firecrawl/tests/test_batch_scrape.py",
                "def test_batch_scrape():\n    return 'firecrawl js sdk client batch crawl search scrape extract tests'\n",
            ),
            (
                "docs/python-sdk.md",
                "# Python SDK\nfirecrawl js sdk client batch crawl search scrape extract tests\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "firecrawl js sdk client batch crawl search scrape extract tests".to_owned(),
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
    let js_sdk_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "apps/js-sdk/firecrawl/src/client.ts"
                    | "apps/js-sdk/firecrawl/src/__tests__/unit/v2/agent.test.ts"
            )
        })
        .expect("a js-sdk witness should be ranked");
    let python_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "apps/python-sdk/firecrawl/tests/test_batch_scrape.py" | "docs/python-sdk.md"
            )
        })
        .expect("python drift should still be ranked");

    assert!(
        js_sdk_position < python_position,
        "broad js-sdk queries should keep same-language typescript witnesses ahead of python drift: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_n8n_queries_prefer_same_workspace_subtree_over_sibling_packages()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-n8n-workspace-subtree");
    prepare_workspace(
        &root,
        &[
            (
                "packages/cli/src/workflow_runner.ts",
                "export function workflowRunner() { return 'cli runtime'; }\n",
            ),
            (
                "packages/cli/test/integration/workflow_runner.test.ts",
                "describe('workflow runner integration', () => {});\n",
            ),
            (
                "packages/core/src/execution_engine.ts",
                "export function executionEngine() { return 'core runtime'; }\n",
            ),
            (
                "packages/editor-ui/src/workflow_editor.tsx",
                "export function WorkflowEditor() { return null; }\n",
            ),
            (
                "packages/editor-ui/cypress/e2e/workflow_editor.cy.ts",
                "describe('workflow editor playwright', () => {});\n",
            ),
            (
                ".github/workflows/build-base-image.yml",
                "name: build image\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                ".github/workflows/test-workflow-scripts-reusable.yml",
                "name: reusable workflow\njobs:\n  test:\n    steps:\n      - run: pnpm test\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "workflow runtime executions cli integrations typescript tests".to_owned(),
            limit: 6,
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
    let cli_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "packages/cli/src/workflow_runner.ts"
                    | "packages/cli/test/integration/workflow_runner.test.ts"
            )
        })
        .expect("a cli subtree witness should be ranked");
    let sibling_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "packages/core/src/execution_engine.ts"
                    | "packages/editor-ui/src/workflow_editor.tsx"
            )
        })
        .expect("sibling workspace noise should still be ranked");

    assert!(
        cli_position < sibling_position,
        "same-subtree cli witnesses should outrank sibling workspace noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_n8n_broad_execution_queries_keep_cli_runtime_sources_visible() -> FriggResult<()>
{
    let root = temp_workspace_root("hybrid-n8n-broad-execution-runtime");
    prepare_workspace(
        &root,
        &[
            (
                "packages/cli/src/executions/execution.service.ts",
                "export class ExecutionService {}\n",
            ),
            (
                "packages/cli/src/executions/execution.types.ts",
                "export interface ExecutionRecord {}\n",
            ),
            (
                "packages/cli/test/integration/task-runners/task-runner-process.test.ts",
                "describe('task runner process', () => {});\n",
            ),
            (
                "packages/cli/test/integration/webhooks.test.ts",
                "describe('webhooks', () => {});\n",
            ),
            (
                "packages/core/src/execution_engine.ts",
                "export class ExecutionEngine {}\n",
            ),
            (
                ".github/workflows/test-workflow-scripts-reusable.yml",
                "name: reusable workflow\njobs:\n  test:\n    steps:\n      - run: pnpm test\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "n8n executions execution lifecycle task runner webhook cli integration"
                .to_owned(),
            limit: 6,
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
            .any(|path| path.starts_with("packages/cli/src/executions/")),
        "broad execution queries should keep cli execution runtime sources visible near the top: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
#[ignore = "workstream-c escalation target"]
fn hybrid_ranking_n8n_editor_queries_demote_ci_workflow_noise() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-n8n-editor-vs-workflow-noise");
    prepare_workspace(
        &root,
        &[
            (
                "packages/editor-ui/src/components/canvas/NodeDetails.vue",
                "export const canvasNodeDetails = 'editor ui vue canvas workflow node details playwright';\n",
            ),
            (
                "packages/editor-ui/cypress/e2e/canvas/node-details.cy.ts",
                "describe('editor ui vue canvas workflow node details playwright', () => {});\n",
            ),
            (
                "packages/core/src/workflow_runner.ts",
                "export const workflowRunner = 'workflow runtime';\n",
            ),
            (
                ".github/workflows/build-base-image.yml",
                "name: build image\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                ".github/workflows/test-workflow-scripts-reusable.yml",
                "name: reusable workflow\njobs:\n  test:\n    steps:\n      - run: pnpm test\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "editor ui vue canvas workflow node details playwright".to_owned(),
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
    let editor_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "packages/editor-ui/src/components/canvas/NodeDetails.vue"
                    | "packages/editor-ui/cypress/e2e/canvas/node-details.cy.ts"
            )
        })
        .expect("an editor-ui witness should be ranked");
    let workflow_position = ranked_paths
        .iter()
        .position(|path| path.starts_with(".github/workflows/"));

    if let Some(workflow_position) = workflow_position {
        assert!(
            editor_position < workflow_position,
            "editor-ui witnesses should outrank CI workflow noise for UI workflow queries: {ranked_paths:?}"
        );
    }

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_supabase_queries_keep_studio_ui_and_tests_above_docs() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-supabase-studio-ui");
    prepare_workspace(
        &root,
        &[
            (
                "apps/studio/pages/dashboard.tsx",
                "export default function Dashboard() { return <div>studio dashboard</div>; }\n",
            ),
            (
                "apps/studio/tests/e2e/dashboard.spec.ts",
                "test('studio dashboard', async () => {});\n",
            ),
            (
                "docs/guides/studio.md",
                "# Studio guide\nstudio ui tests dashboard\n",
            ),
            (
                "supabase/functions/hello/index.ts",
                "export const hello = () => 'edge function';\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export const templateServer = 'studio ui tests dashboard tsconfig typescript';\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "studio ui tests dashboard tsconfig typescript".to_owned(),
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
    let studio_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "apps/studio/pages/dashboard.tsx" | "apps/studio/tests/e2e/dashboard.spec.ts"
            )
        })
        .expect("a studio witness should be ranked");
    let docs_position = ranked_paths
        .iter()
        .position(|path| *path == "docs/guides/studio.md")
        .expect("docs drift should still be ranked");

    assert!(
        studio_position < docs_position,
        "studio ui/test witnesses should outrank docs drift: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
#[ignore = "workstream-c escalation target"]
fn hybrid_ranking_supabase_runtime_queries_demote_repo_meta_and_template_noise() -> FriggResult<()>
{
    let root = temp_workspace_root("hybrid-supabase-runtime-vs-meta-noise");
    prepare_workspace(
        &root,
        &[
            (
                "supabase/functions/hello/index.ts",
                "export const hello = () => 'edge functions self hosted api runtime docker typescript';\n",
            ),
            (
                "apps/studio/pages/dashboard.tsx",
                "export default function Dashboard() { return <div>edge functions self hosted api runtime docker typescript</div>; }\n",
            ),
            (
                "apps/studio/tests/e2e/dashboard.spec.ts",
                "test('edge functions self hosted api runtime docker typescript', async () => {});\n",
            ),
            (
                "examples/auth/nextjs-full/lib/supabase/server.ts",
                "export const exampleServer = 'edge functions self hosted api runtime docker typescript';\n",
            ),
            (
                "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts",
                "export const templateServer = 'edge functions self hosted api runtime docker typescript';\n",
            ),
            (
                "DEVELOPERS.md",
                "# Developers\nedge functions self hosted api runtime docker typescript\n",
            ),
            (
                "CONTRIBUTING.md",
                "# Contributing\nedge functions self hosted api runtime docker typescript\n",
            ),
            ("Makefile", "docker:\n\tdocker compose up\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "edge functions self hosted api runtime docker typescript".to_owned(),
            limit: 6,
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
    let runtime_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "supabase/functions/hello/index.ts"
                    | "apps/studio/pages/dashboard.tsx"
                    | "apps/studio/tests/e2e/dashboard.spec.ts"
            )
        })
        .expect("a runtime or nearby test witness should be ranked");
    let noise_position = ranked_paths
        .iter()
        .position(|path| {
            matches!(
                *path,
                "DEVELOPERS.md"
                    | "CONTRIBUTING.md"
                    | "Makefile"
                    | "examples/auth/nextjs-full/lib/supabase/server.ts"
                    | "apps/ui-library/registry/default/clients/react-router/lib/supabase/server.ts"
            )
        })
        .expect("meta or template noise should still be ranked");

    assert!(
        runtime_position < noise_position,
        "runtime witnesses should outrank repo-meta and template noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_queries_surface_build_workflow_configs() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-entrypoint-build-workflows");
    prepare_workspace(
        &root,
        &[
            (
                "src-tauri/src/main.rs",
                "fn main() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
            ),
            (
                "src-tauri/src/lib.rs",
                "pub fn run() {\n\
                     let config = AppConfig::load();\n\
                     run_pipeline(&config);\n\
                     }\n",
            ),
            (
                "src-tauri/src/proxy/config.rs",
                "pub struct ProxyConfig;\n\
                     impl ProxyConfig { pub fn load() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/modules/config.rs",
                "pub struct ModuleConfig;\n\
                     impl ModuleConfig { pub fn load() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/models/config.rs",
                "pub struct AppConfig;\n\
                     impl AppConfig { pub fn load() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/proxy/proxy_pool.rs",
                "pub struct ProxyPool;\n\
                     impl ProxyPool { pub fn runner() -> Self { Self } }\n",
            ),
            (
                "src-tauri/src/commands/security.rs",
                "pub fn security_command_runner() {}\n",
            ),
            ("src-tauri/build.rs", "fn main() { tauri_build::build() }\n"),
            (
                ".github/workflows/deploy-pages.yml",
                "name: Deploy static content to Pages\n\
                     jobs:\n\
                       deploy:\n\
                         steps:\n\
                           - name: Deploy to GitHub Pages\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: Release\n\
                     jobs:\n\
                       build-tauri:\n\
                         steps:\n\
                           - name: Build the app\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor_with_trace(
        SearchHybridQuery {
            query: "entry point bootstrap build flow command runner main config".to_owned(),
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
    let anchor_paths = output
        .ranked_anchors
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let grouped_paths = output
        .coverage_grouped_pool
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths
            .iter()
            .take(8)
            .any(|path| *path == "src-tauri/src/main.rs"),
        "entrypoint runtime witness should remain visible near the top: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.iter().take(8).any(|path| {
            matches!(
                *path,
                ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
            )
        }),
        "entrypoint/build-flow queries should surface at least one GitHub workflow config witness in top-k: ranked={ranked_paths:?} witness={witness_paths:?} anchors={anchor_paths:?} grouped={grouped_paths:?} trace={:?}",
        output.post_selection_trace
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_entrypoint_build_flow_queries_keep_runtime_entrypoints_visible_under_workflow_crowding()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-entrypoint-vs-workflow-crowding");
    prepare_workspace(
        &root,
        &[
            (
                "crates/ruff/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ruff_dev/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ruff_python_formatter/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ty/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "crates/ty_completion_bench/src/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                ".github/workflows/build-binaries.yml",
                "name: Build binaries\njobs:\n  build:\n    steps:\n      - run: cargo build --release --bin ruff\n",
            ),
            (
                ".github/workflows/build-docker.yml",
                "name: Build docker\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                ".github/workflows/build-wasm.yml",
                "name: Build wasm\njobs:\n  build:\n    steps:\n      - run: cargo build --target wasm32-unknown-unknown\n",
            ),
            (
                ".github/workflows/publish-playground.yml",
                "name: Publish playground\njobs:\n  publish:\n    steps:\n      - run: cargo run --bin playground\n",
            ),
            (
                ".github/workflows/publish-ty-playground.yml",
                "name: Publish ty playground\njobs:\n  publish:\n    steps:\n      - run: cargo run --bin ty-playground\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: Release\njobs:\n  release:\n    steps:\n      - run: cargo build --release\n",
            ),
            (
                ".github/workflows/publish-docs.yml",
                "name: Publish docs\njobs:\n  publish:\n    steps:\n      - run: cargo doc --no-deps\n",
            ),
            (
                ".github/workflows/publish-mirror.yml",
                "name: Publish mirror\njobs:\n  publish:\n    steps:\n      - run: echo mirror\n",
            ),
            (
                ".github/workflows/publish-pypi.yml",
                "name: Publish pypi\njobs:\n  publish:\n    steps:\n      - run: maturin publish\n",
            ),
            (
                ".github/workflows/publish-versions.yml",
                "name: Publish versions\njobs:\n  publish:\n    steps:\n      - run: cargo metadata --format-version 1\n",
            ),
            (
                ".github/workflows/publish-wasm.yml",
                "name: Publish wasm\njobs:\n  publish:\n    steps:\n      - run: wasm-pack build\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap build flow command runner main".to_owned(),
            limit: 11,
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
        ranked_paths.iter().take(11).any(|path| {
            matches!(
                *path,
                "crates/ruff/src/main.rs"
                    | "crates/ruff_dev/src/main.rs"
                    | "crates/ruff_python_formatter/src/main.rs"
                    | "crates/ty/src/main.rs"
                    | "crates/ty_completion_bench/src/main.rs"
            )
        }),
        "a runtime entrypoint witness should remain visible under workflow crowding: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(11)
            .any(|path| path.starts_with(".github/workflows/")),
        "workflow witnesses should remain visible for entrypoint build-flow queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

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
fn hybrid_ranking_entrypoint_build_flow_queries_recover_bat_build_config_witnesses()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-bat-build-config-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"bat\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
            ),
            ("rustfmt.toml", "edition = \"2021\"\n"),
            (
                ".github/workflows/CICD.yml",
                "name: CICD\njobs:\n  build:\n    steps:\n      - run: cargo build --locked\n",
            ),
            (
                ".github/workflows/require-changelog-for-PRs.yml",
                "name: Require changelog\njobs:\n  check:\n    steps:\n      - run: ./tests/scripts/license-checks.sh\n",
            ),
            (
                "src/lib.rs",
                "pub fn run() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            (
                "src/bin/bat/main.rs",
                "fn main() { let _ = \"entry point bootstrap build flow command runner main\"; }\n",
            ),
            ("src/bin/bat/app.rs", "pub fn build_app() {}\n"),
            ("src/bin/bat/assets.rs", "pub fn build_assets() {}\n"),
            ("src/bin/bat/clap_app.rs", "pub fn clap_app() {}\n"),
            (
                "src/bin/bat/completions.rs",
                "pub fn generate_completions() {}\n",
            ),
            ("src/bin/bat/config.rs", "pub fn load_bat_config() {}\n"),
            ("src/config.rs", "pub struct RuntimeConfig;\n"),
            ("tests/scripts/license-checks.sh", "#!/bin/sh\necho check\n"),
            (
                "tests/examples/system_config/bat/config",
                "--theme=\"TwoDark\"\n",
            ),
            (
                "tests/syntax-tests/highlighted/Elixir/command.ex",
                "defmodule Command do\nend\n",
            ),
            (
                "tests/syntax-tests/highlighted/Go/main.go",
                "package main\nfunc main() {}\n",
            ),
            (
                "tests/syntax-tests/source/Elixir/command.ex",
                "defmodule Command do\nend\n",
            ),
            (
                "tests/syntax-tests/source/Go/main.go",
                "package main\nfunc main() {}\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query: "entry point bootstrap build flow command runner main config cargo github workflow cicd require changelog".to_owned(),
                limit: 11,
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
        ranked_paths.iter().take(11).any(|path| {
            matches!(
                *path,
                "Cargo.toml"
                    | ".github/workflows/CICD.yml"
                    | ".github/workflows/require-changelog-for-PRs.yml"
            )
        }),
        "build-config entrypoint queries should recover a Cargo/workflow witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_go_entrypoint_queries_surface_cmd_command_packages() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-go-cmd-entrypoint-witnesses");
    prepare_workspace(
        &root,
        &[
            ("cmd/frpc/main.go", "package main\nfunc main() {}\n"),
            ("cmd/frps/main.go", "package main\nfunc main() {}\n"),
            ("cmd/frps/root.go", "package frps\nfunc Execute() {}\n"),
            ("cmd/frps/verify.go", "package frps\nfunc Verify() {}\n"),
            ("cmd/frpc/sub/admin.go", "package sub\nfunc Admin() {}\n"),
            (
                "cmd/frpc/sub/nathole.go",
                "package sub\nfunc NatHole() {}\n",
            ),
            ("cmd/frpc/sub/proxy.go", "package sub\nfunc Proxy() {}\n"),
            ("cmd/frpc/sub/root.go", "package sub\nfunc Root() {}\n"),
            ("go.mod", "module github.com/example/frp\n"),
            ("go.sum", "github.com/example/dependency v1.0.0 h1:test\n"),
            (
                ".github/workflows/build-and-push-image.yml",
                "name: build and push\njobs:\n  build:\n    steps:\n      - run: docker build .\n",
            ),
            (
                "pkg/config/legacy/server.go",
                "package legacy\nfunc Server() {}\n",
            ),
            (
                "pkg/config/v1/validation/server.go",
                "package validation\nfunc Server() {}\n",
            ),
            (
                "pkg/metrics/mem/server.go",
                "package mem\nfunc Server() {}\n",
            ),
            (
                "web/frpc/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/api/server.ts",
                "export const api = 'server';\n",
            ),
            (
                "web/frps/src/types/server.ts",
                "export const server = 'type';\n",
            ),
            (
                "web/frpc/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "web/frps/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "test/e2e/mock/server/httpserver/server.go",
                "package httpserver\nfunc Server() {}\n",
            ),
            (
                "test/e2e/mock/server/streamserver/server.go",
                "package streamserver\nfunc Server() {}\n",
            ),
            (
                "test/e2e/legacy/basic/server.go",
                "package basic\nfunc Server() {}\n",
            ),
            (
                "test/e2e/legacy/plugin/server.go",
                "package plugin\nfunc Server() {}\n",
            ),
            (
                "test/e2e/v1/basic/server.go",
                "package basic\nfunc Server() {}\n",
            ),
            (
                "test/e2e/v1/plugin/server.go",
                "package plugin\nfunc Server() {}\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point bootstrap server api main cli command".to_owned(),
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
            .any(|path| path.starts_with("cmd/")),
        "go entrypoint queries should recover a cmd/ command witness in top-k: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_roc_entrypoint_queries_prefer_platform_main_over_host_crates_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-roc-platform-entrypoints");
    prepare_workspace(
        &root,
        &[
            (
                "platform/main.roc",
                "# entry point main app package platform runtime\nplatform \"cli\"\npackages {}\nprovides [main_for_host!]\n",
            ),
            ("platform/Arg.roc", "# platform arg runtime package\n"),
            ("platform/Cmd.roc", "# platform cmd runtime package\n"),
            ("platform/Host.roc", "# platform host runtime package\n"),
            (
                "examples/command.roc",
                "# example command package\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            (
                "crates/roc_host_bin/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                "crates/roc_host/src/lib.rs",
                "pub fn host_runtime() { let _ = \"main app package runtime\"; }\n",
            ),
            (
                "ci/rust_http_server/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                ".github/workflows/deploy-docs.yml",
                "name: deploy docs\njobs:\n  deploy:\n    steps:\n      - run: cargo doc\n",
            ),
            (
                ".github/workflows/test_latest_release.yml",
                "name: test latest release\njobs:\n  test:\n    steps:\n      - run: cargo test\n",
            ),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "entry point main app package platform runtime".to_owned(),
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
    let platform_main_rank = ranked_paths
        .iter()
        .position(|path| *path == "platform/main.roc")
        .expect("platform/main.roc should be ranked for Roc entrypoint queries");
    let host_lib_rank = ranked_paths
        .iter()
        .position(|path| *path == "crates/roc_host/src/lib.rs")
        .expect("host runtime lib.rs should be ranked as competing noise");

    assert!(
        platform_main_rank < 6,
        "platform/main.roc should stay visible near the top for Roc platform queries: {ranked_paths:?}"
    );
    assert!(
        platform_main_rank < host_lib_rank,
        "platform/main.roc should outrank generic host runtime library noise for Roc platform queries: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_roc_mixed_entrypoint_example_queries_recover_example_witnesses_under_runtime_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-roc-entrypoints-with-example-hints");
    prepare_workspace(
        &root,
        &[
            (
                "platform/main.roc",
                "# entry point main app package platform runtime\nplatform \"cli\"\npackages {}\nprovides [main_for_host!]\n",
            ),
            ("platform/Arg.roc", "# platform arg runtime package\n"),
            ("platform/Cmd.roc", "# platform cmd runtime package\n"),
            ("platform/Host.roc", "# platform host runtime package\n"),
            ("platform/Stdin.roc", "# stdin bytes runtime package\n"),
            (
                "examples/command.roc",
                "# example command line package\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            (
                "examples/command-line-args.roc",
                "# command line args bytes stdin example\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            (
                "examples/bytes-stdin-stdout.roc",
                "# bytes stdin stdout example\napp [main!] { pf: platform \"../platform/main.roc\" }\n",
            ),
            ("tests/cmd-test.roc", "# tests command bytes stdin\n"),
            (
                "crates/roc_host_bin/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                "crates/roc_host/src/lib.rs",
                "pub fn host_runtime() { let _ = \"main app package runtime\"; }\n",
            ),
            (
                "crates/roc_command/src/lib.rs",
                "pub fn command_runtime() { let _ = \"command runtime\"; }\n",
            ),
            (
                "crates/roc_env/src/lib.rs",
                "pub fn env_runtime() { let _ = \"env runtime\"; }\n",
            ),
            (
                "ci/rust_http_server/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "entry point main app package platform runtime tests bytes stdin command line examples benches benchmark"
                        .to_owned(),
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
                "platform/main.roc"
                    | "crates/roc_host_bin/src/main.rs"
                    | "ci/rust_http_server/src/main.rs"
            )
        }),
        "Roc mixed entrypoint/example queries should keep at least one entrypoint witness visible: {ranked_paths:?}"
    );
    assert!(
        ranked_paths
            .iter()
            .take(14)
            .any(|path| path.starts_with("examples/")),
        "Roc mixed entrypoint/example queries should recover an example witness under runtime/test noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_roc_saved_wave_queries_prefer_specific_example_witnesses_over_temp_dir_noise()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-roc-saved-wave-specific-example-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "platform/main.roc",
                "# entry point main app package platform runtime\nplatform \"web\"\npackages {}\nprovides [main_for_host!]\n",
            ),
            ("platform/Cmd.roc", "# platform cmd runtime package\n"),
            ("platform/Dir.roc", "# platform dir runtime package\n"),
            ("platform/Env.roc", "# platform env runtime package\n"),
            ("platform/File.roc", "# platform file runtime package\n"),
            ("platform/Host.roc", "# platform host runtime package\n"),
            (
                "examples/command.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Cmd\n# command example\n",
            ),
            (
                "examples/dir.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Dir\nimport pf.Env\n# examples directory listing\n",
            ),
            (
                "examples/env.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Env\n# environment example\n",
            ),
            (
                "examples/temp-dir.roc",
                "app [Model, init!, respond!] { pf: platform \"../platform/main.roc\" }\nimport pf.Env\n# temp dir example\n",
            ),
            ("tests/cmd-test.roc", "# tests command integration\n"),
            (
                "crates/roc_host_bin/src/main.rs",
                "fn main() { let _ = \"entry point main app package platform runtime\"; }\n",
            ),
            (
                "crates/roc_host/src/lib.rs",
                "pub fn host_runtime() { let _ = \"main app package runtime\"; }\n",
            ),
            (
                ".github/workflows/test_latest_release.yml",
                "name: test latest release\njobs:\n  test:\n    steps:\n      - run: cargo test\n",
            ),
            (
                ".github/workflows/deploy-docs.yml",
                "name: deploy docs\njobs:\n  deploy:\n    steps:\n      - run: cargo doc\n",
            ),
            ("rust-toolchain.toml", "[toolchain]\nchannel = \"stable\"\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
            SearchHybridQuery {
                query:
                    "tests fixtures integration entry point main app package platform runtime command dir examples benches benchmark"
                        .to_owned(),
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
            .any(|path| { matches!(*path, "examples/command.roc" | "examples/dir.roc") }),
        "saved-wave Roc entrypoint/example queries should recover a specific example witness instead of only broad temp-dir noise: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}

#[test]
fn hybrid_ranking_go_package_queries_surface_pkg_test_witnesses() -> FriggResult<()> {
    let root = temp_workspace_root("hybrid-go-package-witnesses");
    prepare_workspace(
        &root,
        &[
            (
                "client/http/controller.go",
                "package http\nfunc Controller() {}\n",
            ),
            (
                "client/http/controller_test.go",
                "package http\nfunc TestController() {}\n",
            ),
            (
                "client/config_manager.go",
                "package client\nfunc ConfigManager() {}\n",
            ),
            (
                "client/config_manager_test.go",
                "package client\nfunc TestConfigManager() {}\n",
            ),
            (
                "client/proxy/proxy_manager.go",
                "package proxy\nfunc Manager() {}\n",
            ),
            (
                "client/visitor/visitor_manager.go",
                "package visitor\nfunc Manager() {}\n",
            ),
            (
                "pkg/config/source/aggregator_test.go",
                "package source\nfunc TestAggregator() {}\n",
            ),
            (
                "pkg/config/source/base_source_test.go",
                "package source\nfunc TestBaseSource() {}\n",
            ),
            (
                "pkg/config/source/config_source_test.go",
                "package source\nfunc TestConfigSource() {}\n",
            ),
            (
                "pkg/auth/oidc_test.go",
                "package auth\nfunc TestOIDC() {}\n",
            ),
            (
                "pkg/config/load_test.go",
                "package config\nfunc TestLoad() {}\n",
            ),
            (
                "pkg/config/source/aggregator.go",
                "package source\nfunc NewAggregator() {}\n",
            ),
            (
                "pkg/config/source/base_source.go",
                "package source\nfunc NewBaseSource() {}\n",
            ),
            (
                "pkg/config/source/clone.go",
                "package source\nfunc Clone() {}\n",
            ),
            ("pkg/config/flags.go", "package config\nfunc Flags() {}\n"),
            ("go.mod", "module github.com/example/frp\n"),
            ("go.sum", "github.com/example/dependency v1.0.0 h1:test\n"),
            ("cmd/frpc/sub/root.go", "package sub\nfunc Root() {}\n"),
            ("cmd/frps/root.go", "package frps\nfunc Execute() {}\n"),
            ("web/frpc/tsconfig.json", "{ \"compilerOptions\": {} }\n"),
            ("web/frps/tsconfig.json", "{ \"compilerOptions\": {} }\n"),
            (
                "web/frpc/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/main.ts",
                "export const mount = 'frontend main';\n",
            ),
            (
                "web/frps/src/api/server.ts",
                "export const api = 'frontend server';\n",
            ),
            (
                "web/frps/src/types/server.ts",
                "export const server = 'frontend type';\n",
            ),
            (
                "web/frpc/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "web/frps/src/router/index.ts",
                "export const router = 'frontend router';\n",
            ),
            (
                "test/e2e/legacy/basic/config.go",
                "package basic\nfunc Config() {}\n",
            ),
            ("package.sh", "#!/bin/sh\necho package\n"),
        ],
    )?;

    let searcher = TextSearcher::new(FriggConfig::from_workspace_roots(vec![root.clone()])?);
    let output = searcher.search_hybrid_with_filters_using_executor(
        SearchHybridQuery {
            query: "tests packages internal library integration config manager controller"
                .to_owned(),
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
                "pkg/config/source/aggregator_test.go"
                    | "pkg/config/source/base_source_test.go"
                    | "pkg/config/source/config_source_test.go"
                    | "pkg/auth/oidc_test.go"
                    | "pkg/config/load_test.go"
                    | "pkg/config/source/aggregator.go"
                    | "pkg/config/source/base_source.go"
                    | "pkg/config/source/clone.go"
            )
        }),
        "go package/test queries should recover a pkg/ witness in top-k: {ranked_paths:?}"
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

#[test]
fn hybrid_ranking_entrypoint_queries_surface_build_workflow_configs_with_semantic_runtime()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-rust-entrypoint-build-workflows-semantic");
    prepare_workspace(
        &root,
        &[
            (
                "src-tauri/src/main.rs",
                "fn main() {\n\
                     // entry point bootstrap build flow command runner main config\n\
                     let config = load_config();\n\
                     run_build_flow(config);\n\
                     }\n",
            ),
            (
                "src-tauri/src/proxy/config.rs",
                "pub struct ProxyConfig;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/lib.rs",
                "pub fn run() {\n// entry point bootstrap build flow command runner main config\n}\n",
            ),
            (
                "src-tauri/src/modules/config.rs",
                "pub struct ModuleConfig;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/models/config.rs",
                "pub struct ModelConfig;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/src/proxy/proxy_pool.rs",
                "pub struct ProxyPool;\n// entry point bootstrap build flow command runner main config\n",
            ),
            (
                "src-tauri/build.rs",
                "fn main() {\n tauri_build::build();\n}\n",
            ),
            (
                "src-tauri/src/commands/security.rs",
                "pub fn security_command() {\n// entry point bootstrap build flow command runner main config\n}\n",
            ),
            (
                ".github/workflows/deploy-pages.yml",
                "name: Deploy static content to Pages\n\
                     jobs:\n\
                       deploy:\n\
                         steps:\n\
                           - name: Upload artifact\n\
                             run: echo upload build artifacts\n\
                           - name: Deploy to GitHub Pages\n\
                             run: echo deploy release pages\n",
            ),
            (
                ".github/workflows/release.yml",
                "name: Release\n\
                     jobs:\n\
                       build-tauri:\n\
                         steps:\n\
                           - name: Build the app\n\
                             run: cargo build --release\n\
                           - name: Publish release artifacts\n\
                             run: echo publish release artifacts\n",
            ),
        ],
    )?;
    seed_semantic_embeddings(
        &root,
        "repo-001",
        "snapshot-001",
        &[
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/main.rs",
                0,
                vec![1.0, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/proxy/config.rs",
                0,
                vec![0.99, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/lib.rs",
                0,
                vec![0.98, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/modules/config.rs",
                0,
                vec![0.97, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/models/config.rs",
                0,
                vec![0.96, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/proxy/proxy_pool.rs",
                0,
                vec![0.95, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/build.rs",
                0,
                vec![0.94, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                "src-tauri/src/commands/security.rs",
                0,
                vec![0.93, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                ".github/workflows/release.yml",
                0,
                vec![0.82, 0.0],
            ),
            semantic_record(
                "repo-001",
                "snapshot-001",
                ".github/workflows/deploy-pages.yml",
                0,
                vec![0.81, 0.0],
            ),
        ],
    )?;

    let mut config = FriggConfig::from_workspace_roots(vec![root.clone()])?;
    config.semantic_runtime = semantic_runtime_enabled(false);
    config.max_search_results = 8;
    let searcher = TextSearcher::new(config);
    let output = searcher.search_hybrid_with_filters_using_executor_with_trace(
        SearchHybridQuery {
            query: "entry point bootstrap build flow command runner main config".to_owned(),
            limit: 8,
            weights: HybridChannelWeights::default(),
            semantic: Some(true),
        },
        SearchFilters::default(),
        &SemanticRuntimeCredentials {
            openai_api_key: Some("test-openai-key".to_owned()),
            gemini_api_key: None,
        },
        &MockSemanticQueryEmbeddingExecutor::success(vec![1.0, 0.0]),
    )?;

    assert_eq!(output.note.semantic_status, HybridSemanticStatus::Ok);

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
    let anchor_paths = output
        .ranked_anchors
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    let grouped_paths = output
        .coverage_grouped_pool
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();
    assert!(
        ranked_paths.iter().take(8).any(|path| {
            matches!(
                *path,
                ".github/workflows/deploy-pages.yml" | ".github/workflows/release.yml"
            )
        }),
        "entrypoint/build-flow queries should keep a workflow config witness visible even under semantic runtime pressure: ranked={ranked_paths:?} witness={witness_paths:?} anchors={anchor_paths:?} grouped={grouped_paths:?} trace={:?}",
        output.post_selection_trace
    );

    cleanup_workspace(&root);
    Ok(())
}
