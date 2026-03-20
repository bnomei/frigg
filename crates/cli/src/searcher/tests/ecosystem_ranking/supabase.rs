use super::*;

#[ignore = "workstream-c escalation target"]
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
