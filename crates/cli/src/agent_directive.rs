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
pub const HOOK_NUDGE: &str = "Frigg is the default for code discovery, navigation, exact code search, and bounded source reads.";

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
}
