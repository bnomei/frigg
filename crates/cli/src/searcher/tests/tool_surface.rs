use super::*;

#[test]
fn hybrid_lexical_recall_tokens_preserve_signal_order_for_tool_surface_queries() {
    let tokens = hybrid_lexical_recall_tokens(
        "which MCP tools are core versus extended and where is tool surface gating enforced in runtime docs and tests",
    );

    assert_eq!(
        tokens,
        vec![
            "tools", "core", "versus", "extended", "tool", "surface", "gating", "enforced",
            "runtime", "docs", "tests",
        ]
    );
}

#[test]
fn hybrid_ranking_query_aware_lexical_hits_promote_tool_contract_docs_over_generic_readmes()
-> FriggResult<()> {
    let query = "which MCP tools are core versus extended and where is tool surface gating enforced in runtime docs and tests";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "README.md",
                1,
                1,
                "FRIGG_MCP_TOOL_SURFACE_PROFILE core extended tools list",
            ),
            text_match(
                "repo-001",
                "contracts/tools/v1/README.md",
                1,
                1,
                "tool surface profile core extended_only tools/list",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/mcp/tool_surface.rs",
                1,
                1,
                "ToolSurfaceProfile::Core ToolSurfaceProfile::Extended",
            ),
            text_match(
                "repo-001",
                "crates/cli/tests/tool_surface_parity.rs",
                1,
                1,
                "runtime_tool_surface_parity",
            ),
        ],
        query,
    );
    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        4,
        query,
    )?;

    assert!(
        ranked[0].document.path == "contracts/tools/v1/README.md"
            || ranked[1].document.path == "contracts/tools/v1/README.md",
        "tool contract docs should land at the top of the ranked set"
    );
    assert!(
        ranked
            .iter()
            .position(|entry| entry.document.path == "contracts/tools/v1/README.md")
            < ranked
                .iter()
                .position(|entry| entry.document.path == "README.md"),
        "tool contract docs should outrank the generic README for tool-surface queries"
    );
    Ok(())
}

#[test]
fn hybrid_ranking_tool_surface_queries_prefer_mcp_runtime_surface_over_searcher_noise()
-> FriggResult<()> {
    let query = "which MCP tools are core versus extended and where are tool surface types and runtime gating defined";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "contracts/tools/v1/README.md",
                1,
                1,
                "tool surface profile core extended_only tools/list",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/mcp/tool_surface.rs",
                1,
                1,
                "ToolSurfaceProfile::Core ToolSurfaceProfile::Extended",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/mcp/types.rs",
                1,
                1,
                "tools/list tool metadata runtime response types",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/mcp/mod.rs",
                1,
                1,
                "pub mod server pub mod types pub mod tool_surface",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/searcher/mod.rs",
                1,
                1,
                "search_hybrid ranking intent tool surface docs",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/embeddings/mod.rs",
                1,
                1,
                "embedding runtime provider",
            ),
        ],
        query,
    );
    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        6,
        query,
    )?;

    assert!(
        ranked
            .iter()
            .position(|entry| entry.document.path == "crates/cli/src/mcp/tool_surface.rs")
            < ranked
                .iter()
                .position(|entry| entry.document.path == "crates/cli/src/searcher/mod.rs"),
        "tool-surface runtime file should outrank searcher noise for MCP tool-surface queries"
    );
    assert!(
        ranked
            .iter()
            .position(|entry| entry.document.path == "crates/cli/src/mcp/types.rs")
            < ranked
                .iter()
                .position(|entry| entry.document.path == "crates/cli/src/embeddings/mod.rs"),
        "MCP runtime types should outrank unrelated embedding runtime files"
    );
    Ok(())
}

#[test]
fn hybrid_ranking_mcp_http_startup_queries_prefer_http_runtime_entrypoint() -> FriggResult<()> {
    let query = "where does MCP HTTP startup happen and which runtime entrypoint wires the loopback HTTP server";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "crates/cli/src/main.rs",
                1,
                1,
                "mcp http startup cli command wires runtime",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/http_runtime.rs",
                1,
                1,
                "loopback http server startup runtime tool surface",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/embeddings/mod.rs",
                1,
                1,
                "http client embedding provider runtime",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/searcher/mod.rs",
                1,
                1,
                "runtime startup ranking path",
            ),
            text_match(
                "repo-001",
                "docs/overview.md",
                1,
                1,
                "mcp http runtime overview",
            ),
        ],
        query,
    );
    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        5,
        query,
    )?;

    assert!(
        ranked[0].document.path == "crates/cli/src/http_runtime.rs"
            || ranked[1].document.path == "crates/cli/src/http_runtime.rs",
        "http_runtime.rs should land at the top for MCP HTTP startup queries"
    );
    assert!(
        ranked
            .iter()
            .position(|entry| entry.document.path == "crates/cli/src/http_runtime.rs")
            < ranked
                .iter()
                .position(|entry| entry.document.path == "crates/cli/src/embeddings/mod.rs"),
        "HTTP runtime entrypoint should outrank unrelated embedding runtime files"
    );
    Ok(())
}

#[test]
fn hybrid_ranking_navigation_fallback_queries_promote_mcp_runtime_witnesses() -> FriggResult<()> {
    let query = "find EmbeddingProvider implementations and fallback when precise navigation data is missing";
    let ranked = rank_hybrid_evidence_for_query(
        &[
            hybrid_hit(
                "repo-001",
                "crates/cli/src/embeddings/mod.rs",
                1.00,
                "lex-impl-runtime",
            ),
            hybrid_hit(
                "repo-001",
                "crates/cli/src/searcher/mod.rs",
                0.98,
                "lex-searcher-runtime",
            ),
            hybrid_hit(
                "repo-001",
                "skills/frigg-mcp-search-navigation/references/navigation-fallbacks.md",
                0.97,
                "lex-nav-doc",
            ),
            hybrid_hit(
                "repo-001",
                "crates/cli/tests/tool_handlers.rs",
                0.96,
                "lex-tests",
            ),
            hybrid_hit(
                "repo-001",
                "contracts/tools/v1/README.md",
                0.95,
                "lex-tool-contract",
            ),
            hybrid_hit(
                "repo-001",
                "contracts/semantic.md",
                0.94,
                "lex-semantic-contract",
            ),
            hybrid_hit(
                "repo-001",
                "contracts/errors.md",
                0.93,
                "lex-error-contract",
            ),
            hybrid_hit(
                "repo-001",
                "crates/cli/src/indexer/mod.rs",
                0.92,
                "lex-indexer-runtime",
            ),
            hybrid_hit(
                "repo-001",
                "crates/cli/src/mcp/server.rs",
                0.88,
                "lex-mcp-server-runtime",
            ),
            hybrid_hit(
                "repo-001",
                "crates/cli/src/mcp/types.rs",
                0.87,
                "lex-mcp-types-runtime",
            ),
        ],
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        8,
        query,
    )?;

    let ranked_paths = ranked
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.contains(&"crates/cli/src/mcp/server.rs")
            || ranked_paths.contains(&"crates/cli/src/mcp/types.rs"),
        "navigation-fallback queries should surface at least one MCP runtime witness in top-k"
    );
    assert!(
        ranked
            .iter()
            .position(|entry| entry.document.path == "crates/cli/src/mcp/server.rs")
            < ranked.iter().position(|entry| {
                entry.document.path
                    == "skills/frigg-mcp-search-navigation/references/navigation-fallbacks.md"
            }),
        "MCP runtime witness should outrank the secondary navigation reference doc"
    );

    Ok(())
}

#[test]
fn hybrid_ranking_query_aware_lexical_hits_promote_benchmark_docs_for_replay_queries()
-> FriggResult<()> {
    let query = "how does Frigg turn a multi-step suite playbook fixture into a deterministic trace artifact replay and citations";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "README.md",
                1,
                1,
                "deterministic replay provenance auditing deep_search_replay",
            ),
            text_match(
                "repo-001",
                "benchmarks/deep-search.md",
                1,
                1,
                "deterministic trace artifact replay citations playbook fixture benchmark",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/mcp/deep_search.rs",
                1,
                1,
                "DeepSearchTraceArtifact deep_search_compose_citations",
            ),
        ],
        query,
    );
    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        3,
        query,
    )?;

    assert!(
        ranked
            .iter()
            .position(|entry| entry.document.path == "benchmarks/deep-search.md")
            < ranked
                .iter()
                .position(|entry| entry.document.path == "README.md"),
        "benchmark docs should outrank the generic README for replay/citation queries"
    );
    Ok(())
}

#[test]
fn hybrid_ranking_query_aware_diversification_avoids_single_class_collapse() -> FriggResult<()> {
    let query = "trace invalid_params typed error from public docs to runtime helper and tests";
    let lexical = vec![
        hybrid_hit("repo-001", "crates/cli/src/a.rs", 1.00, "lex-runtime-a"),
        hybrid_hit("repo-001", "crates/cli/src/b.rs", 0.99, "lex-runtime-b"),
        hybrid_hit("repo-001", "contracts/errors.md", 0.98, "lex-docs"),
        hybrid_hit(
            "repo-001",
            "crates/cli/tests/tool_handlers.rs",
            0.97,
            "lex-tests",
        ),
    ];

    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        3,
        query,
    )?;
    let ranked_paths = ranked
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths
            .iter()
            .any(|path| matches!(hybrid_source_class(path), HybridSourceClass::Runtime)),
        "runtime witness should remain in top-k"
    );
    assert!(
        ranked_paths.contains(&"contracts/errors.md"),
        "docs witness should be promoted into top-k"
    );
    assert!(
        ranked_paths.contains(&"crates/cli/tests/tool_handlers.rs"),
        "test witness should be promoted into top-k"
    );
    Ok(())
}

#[test]
fn hybrid_ranking_error_taxonomy_queries_prefer_exact_anchored_runtime_and_tests_over_auxiliary_noise()
-> FriggResult<()> {
    let query = "invalid_params -32602 public error taxonomy docs contract runtime helper tests";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "docs/error-taxonomy.md",
                1,
                1,
                "invalid_params maps to -32602",
            ),
            text_match(
                "repo-001",
                "src/runtime/jsonrpc/errors.rs",
                1,
                1,
                "invalid_params runtime helper",
            ),
            text_match(
                "repo-001",
                "src/runtime/replay.rs",
                1,
                1,
                "invalid_params replay helper",
            ),
            text_match(
                "repo-001",
                "tests/runtime_errors.rs",
                1,
                1,
                "invalid_params tests coverage",
            ),
            text_match(
                "repo-001",
                "src/domain/error.rs",
                1,
                1,
                "invalid_params internal domain error type",
            ),
            text_match(
                "repo-001",
                "src/main.rs",
                1,
                1,
                "runtime helper tests invalid_params",
            ),
            text_match(
                "repo-001",
                "src/cli_runtime.rs",
                1,
                1,
                "runtime helper tests invalid_params",
            ),
            text_match(
                "repo-001",
                "playbooks/error-contract-alignment.md",
                1,
                1,
                "runtime helper tests invalid_params",
            ),
            text_match(
                "repo-001",
                "fixtures/scip/matrix-invalid-range.json",
                1,
                1,
                "runtime helper tests invalid_params",
            ),
        ],
        query,
    );

    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        5,
        query,
    )?;
    let ranked_paths = ranked
        .iter()
        .map(|entry| entry.document.path.as_str())
        .collect::<Vec<_>>();

    assert!(
        ranked_paths.contains(&"src/runtime/jsonrpc/errors.rs")
            || ranked_paths.contains(&"src/runtime/replay.rs"),
        "exact-anchored runtime helpers should remain in top-k: {ranked_paths:?}"
    );
    assert!(
        ranked_paths.contains(&"tests/runtime_errors.rs"),
        "exact-anchored test witnesses should remain in top-k: {ranked_paths:?}"
    );
    assert!(
        !ranked_paths.contains(&"src/main.rs"),
        "generic runtime entrypoints should not outrank exact-anchored runtime helpers: {ranked_paths:?}"
    );
    assert!(
        !ranked_paths.contains(&"playbooks/error-contract-alignment.md"),
        "playbook self-reference should not outrank exact-anchored runtime witnesses: {ranked_paths:?}"
    );
    assert!(
        !ranked_paths.contains(&"fixtures/scip/matrix-invalid-range.json"),
        "fixtures should not outrank exact-anchored runtime witnesses: {ranked_paths:?}"
    );
    Ok(())
}

#[test]
fn hybrid_ranking_shared_path_class_demotes_support_paths_under_crates_prefixes() -> FriggResult<()>
{
    let query = "builder configuration";
    let lexical = build_hybrid_lexical_hits_for_query(
        &[
            text_match(
                "repo-001",
                "crates/cli/examples/server.rs",
                1,
                1,
                "builder configuration builder configuration builder configuration",
            ),
            text_match(
                "repo-001",
                "crates/cli/src/builder.rs",
                1,
                1,
                "builder configuration",
            ),
        ],
        query,
    );

    let ranked = rank_hybrid_evidence_for_query(
        &lexical,
        &[],
        &[],
        HybridChannelWeights {
            lexical: 1.0,
            graph: 0.0,
            semantic: 0.0,
        },
        2,
        query,
    )?;

    assert_eq!(ranked[0].document.path, "crates/cli/src/builder.rs");
    assert_eq!(ranked[1].document.path, "crates/cli/examples/server.rs");
    Ok(())
}
