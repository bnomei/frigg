use super::*;

#[ignore = "open TS path-locality escalation target"]
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
