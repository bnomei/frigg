//! Canonical agent-facing Frigg directive text and render helpers.

/// Version of the canonical Frigg-first directive contract.
pub const FRIGG_DIRECTIVE_VERSION: &str = "2026-07-01";

/// Canonical Frigg-first directive body shared by agent-facing surfaces.
pub const FRIGG_FIRST_DIRECTIVE: &str = include_str!("../assets/frigg-directive.md");

/// Opening marker for generated Frigg directive blocks.
pub const MANAGED_BLOCK_START: &str = "<!-- frigg-directive:start version=2026-07-01 -->";

/// Closing marker for generated Frigg directive blocks.
pub const MANAGED_BLOCK_END: &str = "<!-- frigg-directive:end -->";

/// Short nudge text suitable for hook output.
pub const HOOK_NUDGE: &str = "Frigg is the default for code discovery, file listing, navigation, exact code search, and bounded source reads.";

/// Renders the canonical directive inside stable managed-block markers.
pub fn render_managed_block() -> String {
    format!(
        "{MANAGED_BLOCK_START}\n{}\n{MANAGED_BLOCK_END}",
        FRIGG_FIRST_DIRECTIVE.trim()
    )
}

/// Composes MCP instructions from the canonical directive plus runtime-specific guidance.
pub fn mcp_instructions(runtime_tail: &str) -> String {
    let directive = FRIGG_FIRST_DIRECTIVE.trim();
    let runtime_tail = runtime_tail.trim();

    if runtime_tail.is_empty() {
        directive.to_owned()
    } else {
        format!("{directive}\n\n{runtime_tail}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const CORE_SENTENCES: [&str; 3] = [
        "Frigg is the default for code discovery, file listing, navigation, exact code search, and bounded source reads.",
        "Before using shell `rg`, `grep`, `find`, `fd`, `cat`, or `sed` for code exploration, use the matching Frigg MCP tool in attached or attachable code repositories.",
        "Shell tools are fallback only for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, or when Frigg is unavailable.",
    ];

    fn assert_core_sentences(surface_name: &str, text: &str) {
        for sentence in CORE_SENTENCES {
            assert!(
                text.contains(sentence),
                "{surface_name} should contain canonical sentence: {sentence}"
            );
        }
    }

    fn workspace_file(relative_path: &str) -> Option<String> {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(relative_path);
        if !path.exists() {
            return None;
        }

        Some(std::fs::read_to_string(&path).expect("workspace file should be readable"))
    }

    #[test]
    fn render_managed_block_wraps_canonical_directive_with_markers() {
        let rendered = render_managed_block();

        assert!(rendered.starts_with(MANAGED_BLOCK_START));
        assert!(rendered.ends_with(MANAGED_BLOCK_END));
        assert!(rendered.contains(FRIGG_FIRST_DIRECTIVE.trim()));
        assert_eq!(
            rendered,
            format!(
                "{MANAGED_BLOCK_START}\n{}\n{MANAGED_BLOCK_END}",
                FRIGG_FIRST_DIRECTIVE.trim()
            )
        );
    }

    #[test]
    fn mcp_instructions_preserves_runtime_tail_after_directive() {
        let rendered =
            mcp_instructions("Runtime profile is `extended`.\nResource: `frigg://policy`.");

        assert!(rendered.starts_with(FRIGG_FIRST_DIRECTIVE.trim()));
        assert!(rendered.contains("\n\nRuntime profile is `extended`."));
        assert!(rendered.ends_with("Resource: `frigg://policy`."));
    }

    #[test]
    fn mcp_instructions_omits_blank_tail_spacing() {
        assert_eq!(mcp_instructions(" \n\t "), FRIGG_FIRST_DIRECTIVE.trim());
    }

    #[test]
    fn agent_directive_core_sentences_are_in_canonical_asset() {
        assert_core_sentences("canonical directive", FRIGG_FIRST_DIRECTIVE);
    }

    #[test]
    fn agent_directive_core_sentences_are_in_skill_and_openai_prompt() {
        let skill = workspace_file("skills/frigg-first-code-search/SKILL.md");
        let openai_prompt = workspace_file("skills/frigg-first-code-search/agents/openai.yaml");
        let (Some(skill), Some(openai_prompt)) = (skill.as_deref(), openai_prompt.as_deref())
        else {
            assert!(
                skill.is_none() && openai_prompt.is_none(),
                "bundled skill and OpenAI prompt drift files should be present together"
            );
            return;
        };

        assert_core_sentences("bundled skill", skill);
        assert_core_sentences("OpenAI prompt", openai_prompt);
        assert!(
            !skill.contains("Prefer local shell tools"),
            "bundled skill should not return to shell-first code-search guidance"
        );
        assert!(
            !openai_prompt.contains("Use shell tools for quick local inspection"),
            "OpenAI prompt should not return to shell-first code-search guidance"
        );
    }
}
