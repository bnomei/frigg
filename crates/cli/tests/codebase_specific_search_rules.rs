#![allow(clippy::panic)]

use std::fs;
use std::path::PathBuf;

fn crate_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn repo_root() -> PathBuf {
    crate_root()
        .parent()
        .and_then(|path| path.parent())
        .expect("crate root should live under the repository root")
        .to_path_buf()
}

fn assert_forbidden_literals_absent(relative_path: &str, forbidden_literals: &[&str]) {
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

#[test]
fn production_search_policy_does_not_hardcode_frigg_repo_literals() {
    let cases: [(&str, &[&str]); 6] = [
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
                "path.starts_with(\"skills/frigg-self-improvement-loop/\")",
                "path.starts_with(\"var/self-improvement/\")",
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
                "path.starts_with(\"playbooks/\")",
                "path.starts_with(\"skills/frigg-self-improvement-loop/\")",
                "path.starts_with(\"var/self-improvement/\")",
                "path.starts_with(\"skills/frigg-mcp-search-navigation/\")",
                "SourceClass::Playbooks",
                "\"contracts/semantic.md\" | \"contracts/tools/v1/README.md\"",
            ],
        ),
        (
            "src/searcher/reranker.rs",
            &[
                "document.path == \"crates/cli/src/mcp/server.rs\"",
                "document.path.starts_with(\"skills/frigg-self-improvement-loop/\")",
                "document.path.starts_with(\"var/self-improvement/\")",
                "HybridSourceClass::Playbooks",
            ],
        ),
        (
            "src/searcher/lexical_channel.rs",
            &[
                "HybridSourceClass::Playbooks",
                "path.starts_with(\"skills/frigg-self-improvement-loop/\")",
                "path.starts_with(\"var/self-improvement/\")",
            ],
        ),
        ("src/searcher/intent.rs", &["SourceClass::Playbooks =>"]),
        (
            "src/searcher/mod.rs",
            &[
                "should_scrub_playbook_metadata",
                "\"<!-- frigg-playbook\"",
                "path.starts_with(\"playbooks/\") && path.ends_with(\".md\")",
            ],
        ),
    ];

    for (relative_path, forbidden_literals) in cases {
        assert_forbidden_literals_absent(relative_path, forbidden_literals);
    }
}

#[test]
fn workspace_ignore_boundary_keeps_loop_auxiliary_trees_out_of_indexing() {
    let ignore_path = repo_root().join(".ignore");
    let ignore = fs::read_to_string(&ignore_path)
        .unwrap_or_else(|err| panic!("failed reading {}: {err}", ignore_path.display()));

    for required in [
        "var/self-improvement/",
        "skills/frigg-mcp-search-navigation/",
        "skills/frigg-self-improvement-loop/",
        "vendor/",
    ] {
        assert!(
            ignore.contains(required),
            "{} should keep `{required}` behind the workspace-local indexing boundary",
            ignore_path.display()
        );
    }
}
