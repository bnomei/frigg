use super::*;

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
fn hybrid_ranking_firecrawl_js_sdk_queries_prefer_typescript_sdk_over_python_drift()
-> FriggResult<()> {
    let root = temp_workspace_root("hybrid-firecrawl-js-sdk-vs-python-drift");
    prepare_workspace(
        &root,
        &[
            (
                "apps/js-sdk/firecrawl/src/client.ts",
                "export class FirecrawlClient {\n    batchCrawlSearchExtract() {\n        return 'firecrawl js sdk client batch crawl search scrape extract';\n    }\n}\n",
            ),
            (
                "apps/js-sdk/firecrawl/src/__tests__/unit/v2/agent.test.ts",
                "describe('firecrawl js sdk batch crawl search scrape extract agent', () => {});\n",
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
    let python_doc_position = ranked_paths
        .iter()
        .position(|path| *path == "docs/python-sdk.md")
        .expect("python docs drift should still be ranked");

    assert!(
        js_sdk_position <= 1 && js_sdk_position < python_doc_position,
        "broad js-sdk queries should keep same-language typescript witnesses prominent and ahead of python docs drift: {ranked_paths:?}"
    );

    cleanup_workspace(&root);
    Ok(())
}
