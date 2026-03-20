use super::*;

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
