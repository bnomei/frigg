#![allow(clippy::panic)]

use std::fs;
use std::path::PathBuf;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn production_search_policy_does_not_hardcode_frigg_repo_literals() {
    let cases: [(&str, &[&str]); 4] = [
        (
            "src/searcher/lexical_channel.rs",
            &[
                "path == \"contracts/errors.md\"",
                "path.starts_with(\"crates/cli/src/mcp/\")",
                "path == \"crates/cli/src/mcp/server.rs\"",
                "path == \"crates/cli/src/mcp/deep_search.rs\"",
                "path.starts_with(\"crates/cli/tests/\")",
                "path == \"crates/cli/src/domain/error.rs\"",
                "path == \"contracts/tools/v1/README.md\"",
                "path == \"crates/cli/src/mcp/tool_surface.rs\"",
                "path == \"crates/cli/tests/tool_surface_parity.rs\"",
                "path == \"crates/cli/src/http_runtime.rs\"",
                "path == \"crates/cli/src/mcp/types.rs\"",
                "path == \"crates/cli/src/mcp/mod.rs\"",
                "path == \"docs/overview.md\"",
                "path.starts_with(\"crates/cli/src/searcher/\")",
                "path.starts_with(\"crates/cli/src/embeddings/\")",
                "path == \"benchmarks/deep-search.md\"",
                "path.starts_with(\"skills/\")",
                "path.starts_with(\"vendor/\")",
                "path.starts_with(\"fixtures/\")",
                "path.starts_with(\"var/\")",
            ],
        ),
        (
            "src/searcher/surfaces.rs",
            &[
                "path == \"contracts/errors.md\"",
                "path.starts_with(\"crates/cli/src/mcp/\")",
                "path.starts_with(\"skills/frigg-mcp-search-navigation/\")",
                "\"contracts/semantic.md\" | \"contracts/tools/v1/README.md\"",
            ],
        ),
        (
            "src/searcher/reranker.rs",
            &["document.path == \"crates/cli/src/mcp/server.rs\""],
        ),
        (
            "src/indexer/semantic.rs",
            &["repository_relative_path.starts_with(\"playbooks/\")"],
        ),
    ];

    for (relative_path, forbidden_literals) in cases {
        let full_path = crate_root().join(relative_path);
        let source = fs::read_to_string(&full_path)
            .unwrap_or_else(|err| panic!("failed reading {}: {err}", full_path.display()));
        for forbidden in forbidden_literals {
            assert!(
                !source.contains(forbidden),
                "{} still contains forbidden repo-specific literal `{forbidden}`",
                full_path.display()
            );
        }
    }
}
