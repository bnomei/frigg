//! Canonical agent-facing Frigg directive text and render helpers.

/// Version of the canonical Frigg-first directive contract.
pub const FRIGG_DIRECTIVE_VERSION: &str = "2026-07-08";

/// Canonical lightweight Frigg-first directive body shared by agent-facing surfaces.
pub const FRIGG_FIRST_DIRECTIVE: &str = include_str!("../assets/frigg-directive.md");

/// Opt-in expanded managed-policy body for `frigg adopt --policy expanded`.
pub const FRIGG_FIRST_DIRECTIVE_EXPANDED: &str =
    include_str!("../assets/frigg-directive-expanded.md");

/// Opening marker for generated Frigg directive blocks.
pub const MANAGED_BLOCK_START: &str = "<!-- frigg-directive:start version=2026-07-08 -->";

/// Closing marker for generated Frigg directive blocks.
pub const MANAGED_BLOCK_END: &str = "<!-- frigg-directive:end -->";

/// Soft PreToolUse `additionalContext` only — never a hard allow/deny decision.
///
/// Compact checklist aligned with `policy-pack/.../shell-indexed-src-justification.md`.
/// Kept short so hosts do not ignore over-long hook context the way they ignore long skills.
pub const HOOK_NUDGE: &str = "\
Frigg is the default for code discovery, file listing, navigation, exact code search, and bounded source reads. \
Advisory context only — does not block or auto-approve shell tools.
Preferred next step (indexed source while Frigg is registered):
- exact string/regex → search_text
- several guesses → search_batch (else parallel search_text/search_symbol)
- known symbol → search_symbol → go_to_definition
- vague \"where is X?\" → search_hybrid → exact proof (not rank-1 alone)
- list/outline → list_files / document_symbols; proof → read_match / read_file
Shell still OK: Frigg missing from tools/list; ignored/generated/unindexed path; live-disk when workspace advised; git/build/test output; non-source artifact.
";

/// Policy body size for managed markdown adoption targets (`AGENTS.md`, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentsPolicy {
    /// Lightweight pointer: core Frigg-first rules plus skill reference.
    #[default]
    Lightweight,
    /// Expanded compact routing policy without dumping the full skill.
    Expanded,
}

impl AgentsPolicy {
    /// Returns the directive body for this policy mode.
    pub fn directive_body(self) -> &'static str {
        match self {
            Self::Lightweight => FRIGG_FIRST_DIRECTIVE,
            Self::Expanded => FRIGG_FIRST_DIRECTIVE_EXPANDED,
        }
    }
}

/// Renders the default (lightweight) directive inside stable managed-block markers.
pub fn render_managed_block() -> String {
    render_managed_block_for_policy(AgentsPolicy::Lightweight)
}

/// Renders the directive for the selected adoption policy inside managed-block markers.
pub fn render_managed_block_for_policy(policy: AgentsPolicy) -> String {
    format!(
        "{MANAGED_BLOCK_START}\n{}\n{MANAGED_BLOCK_END}",
        policy.directive_body().trim()
    )
}

/// Composes MCP instructions from the canonical lightweight directive plus runtime-specific guidance.
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
    fn render_managed_block_for_policy_selects_lightweight_or_expanded_body() {
        let lightweight = render_managed_block_for_policy(AgentsPolicy::Lightweight);
        let expanded = render_managed_block_for_policy(AgentsPolicy::Expanded);

        assert_eq!(lightweight, render_managed_block());
        assert!(lightweight.contains("frigg-first-code-search"));
        assert!(!lightweight.contains("## Compact scenario picker"));
        assert!(!lightweight.contains("Hard anti-patterns"));
        assert!(!lightweight.contains("Known string or regex -> search_text"));

        assert!(expanded.contains(FRIGG_FIRST_DIRECTIVE_EXPANDED.trim()));
        assert!(expanded.contains("## Compact scenario picker"));
        assert!(expanded.contains("Known string or regex -> search_text"));
        assert!(expanded.contains("Shell → Frigg"));
        assert!(!expanded.contains("BAD: hybrid -> grep"));
        assert_core_sentences("expanded directive", &expanded);
    }

    #[test]
    fn hook_nudge_teaches_next_steps_and_stays_soft() {
        assert!(
            HOOK_NUDGE.contains(CORE_SENTENCES[0]),
            "hook nudge should open with the canonical default sentence"
        );
        for tool in [
            "search_text",
            "search_batch",
            "search_symbol",
            "search_hybrid",
            "go_to_definition",
            "list_files",
            "document_symbols",
            "read_match",
            "read_file",
        ] {
            assert!(
                HOOK_NUDGE.contains(tool),
                "hook nudge should teach preferred tool `{tool}`"
            );
        }
        assert!(
            HOOK_NUDGE.contains("Advisory context only"),
            "hook must state advisory/soft-only (no product hard deny)"
        );
        assert!(
            HOOK_NUDGE.contains("Shell still OK"),
            "hook must keep positive shell fallback boundary"
        );
        // Soft wording may say "does not … deny"; forbid hard-decision product fields.
        assert!(
            !HOOK_NUDGE.contains("permissionDecision"),
            "hook nudge must not reference permissionDecision"
        );
        // Cap noise: hosts ignore over-long additionalContext the way they ignore long skills.
        // Current text is ~0.7k; fail if it bloats toward skill-length context.
        assert!(
            HOOK_NUDGE.len() < 800,
            "hook nudge should stay compact (got {} chars)",
            HOOK_NUDGE.len()
        );
    }

    #[test]
    fn default_managed_block_is_lightweight_pointer() {
        let rendered = render_managed_block();
        assert_core_sentences("default managed block", &rendered);
        assert!(rendered.contains("frigg-first-code-search"));
        assert!(
            !rendered.contains("## Compact scenario picker"),
            "default AGENTS policy must not embed the expanded picker table"
        );
        assert!(
            !rendered.contains("Hard anti-patterns"),
            "default AGENTS policy must not dump skill hard anti-patterns"
        );
        assert!(
            !rendered.contains("| Shell habit | Frigg call |"),
            "default AGENTS policy must not embed the full shell translation card"
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
        assert_core_sentences("expanded directive asset", FRIGG_FIRST_DIRECTIVE_EXPANDED);
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

    /// Live-surface SSOT: scenario tools the skill routes to must stay on the public MCP set.
    /// Inventory freezes and non-public `#[tool]` handlers are not a second catalog.
    #[test]
    fn skill_scenario_tools_are_subset_of_public_tool_names() {
        use crate::mcp::types::PUBLIC_TOOL_NAMES;

        let skill = workspace_file("skills/frigg-first-code-search/SKILL.md")
            .expect("bundled skill skills/frigg-first-code-search/SKILL.md must exist for SSOT guard");

        // Scenario tools the skill actually routes to (must stay ⊆ public SSOT).
        // Not every PUBLIC_TOOL_NAMES entry is required to appear in the skill prose.
        const SCENARIO_TOOLS: &[&str] = &[
            "workspace",
            "list_files",
            "read_file",
            "read_match",
            "search_text",
            "search_hybrid",
            "search_symbol",
            "search_batch",
            "find_references",
            "go_to_definition",
            "find_implementations",
            "incoming_calls",
            "outgoing_calls",
            "document_symbols",
            "inspect_syntax_tree",
            "search_structural",
            "impact_bundle",
            "explore",
            "find_declarations",
        ];

        for tool_name in SCENARIO_TOOLS {
            // Prefer code-span citations so prose like "code exploration" or phantom
            // names like workspace_index cannot satisfy the SSOT guard. Accept either
            // a bare `` `tool` `` span or a call form `` `tool(...)` ``.
            let cited = skill.contains(&format!("`{tool_name}`"))
                || skill.contains(&format!("`{tool_name}("));
            assert!(
                cited,
                "bundled skill should cite scenario tool `{tool_name}` in a markdown code span"
            );
            assert!(
                PUBLIC_TOOL_NAMES.contains(tool_name),
                "skill scenario tool `{tool_name}` must be listed in PUBLIC_TOOL_NAMES (live SSOT)"
            );
        }

        for phantom in [
            "workspace_index",
            "workspace_attach",
            "workspace_reindex",
            "deep_search",
        ] {
            assert!(
                !PUBLIC_TOOL_NAMES.contains(&phantom),
                "phantom `{phantom}` must stay off PUBLIC_TOOL_NAMES"
            );
        }
    }

    #[test]
    fn managed_block_version_matches_directive_version_constant() {
        assert!(MANAGED_BLOCK_START.contains(FRIGG_DIRECTIVE_VERSION));
        assert_eq!(FRIGG_DIRECTIVE_VERSION, "2026-07-08");
    }
}
