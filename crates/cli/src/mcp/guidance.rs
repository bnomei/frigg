//! MCP policy resources and routing prompts that describe language support, tool-surface profiles,
//! and when agents should prefer Frigg tools over shell reads.

use rmcp::model::{
    GetPromptResult, Prompt, PromptArgument, PromptMessage, ReadResourceResult, Resource,
    ResourceContents, Role,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::languages::{LanguageSupportCapability, SymbolLanguage};
use crate::mcp::tool_surface::{ToolSurfaceProfile, manifest_for_tool_surface_profile};

pub(crate) const SUPPORT_MATRIX_RESOURCE_URI: &str = "frigg://policy/support-matrix.json";
pub(crate) const TOOL_SURFACE_RESOURCE_URI: &str = "frigg://policy/tool-surface.json";
pub(crate) const SHELL_REPLACEMENT_MAP_RESOURCE_URI: &str =
    "frigg://policy/shell-replacement-map.json";
pub(crate) const SHELL_GUIDANCE_RESOURCE_URI: &str = "frigg://guidance/shell-vs-frigg.md";
pub(crate) const ROUTING_STATS_RESOURCE_URI: &str =
    crate::mcp::routing_stats::ROUTING_STATS_RESOURCE_URI;
pub(crate) const ROUTING_GUIDE_PROMPT_NAME: &str = "frigg-routing-guide";

#[derive(Debug, Clone, Serialize)]
struct LanguageSupportEntry {
    id: &'static str,
    display_name: &'static str,
    capabilities: Value,
    search_outline: &'static str,
    navigation: &'static str,
    semantic_retrieval: &'static str,
    capability_note: &'static str,
}

fn support_matrix_json() -> String {
    let languages = SymbolLanguage::ALL
        .into_iter()
        .map(|language| LanguageSupportEntry {
            id: support_matrix_language_id(language),
            display_name: language.display_name(),
            capabilities: language_capabilities_json(language),
            search_outline: support_matrix_search_outline(language),
            navigation: support_matrix_navigation(language),
            semantic_retrieval: support_matrix_semantic_retrieval(language),
            capability_note: support_matrix_capability_note(language),
        })
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.support_matrix.v4",
        "product": "frigg",
        "product_boundary": "local-first deterministic code-evidence engine delivered through MCP",
        "stable_core": [
            "repository discovery and attach",
            "safe file reads",
            "text, symbol, and hybrid search",
            "read-only navigation",
            "evidence-backed auditing"
        ],
        "optional_accelerators": [
            "semantic retrieval",
            "external SCIP ingestion",
            "built-in watch mode"
        ],
        "advanced_consumers": support_matrix_advanced_consumers(),
        "language_support_notes": [
            "Frigg currently supports the listed languages for source-backed search, outline, structural, and hybrid retrieval workflows.",
            "Navigation stays read-only and may combine source heuristics, graph evidence, and optional external artifacts.",
            "Semantic retrieval is optional acceleration only and never the grounding layer."
        ],
        "capability_tiers": {
            "core": "capability is part of FRIGG's stable read-only core contract for that language",
            "optional_accelerator": "capability is an optional accelerator that only contributes when runtime configuration and repository state make it available",
            "unsupported": "capability is not currently provided for that language in the runtime registry"
        },
        "languages": languages
    }))
    .expect("support matrix JSON should serialize")
}

fn shell_replacement_map_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.shell_replacement_map.v1",
        "product": "frigg",
        "default_for": [
            "source-code discovery",
            "exact text search",
            "symbol lookup",
            "repository-relative source reads",
            "code navigation"
        ],
        "fallback_only_for": [
            "git state and diffs",
            "non-code files or workspace metadata",
            "build/test output",
            "generated or unindexed files",
            "explicit live-disk verification",
            "Frigg unavailable"
        ],
        "replacements": [
            {
                "shell": "rg --files",
                "tool": "list_files",
                "params": ["path_regex", "glob", "language", "path_class", "include_hidden", "limit", "resume_from"]
            },
            {
                "shell": "rg -n PATTERN",
                "tool": "search_text",
                "params": ["query", "pattern_type", "case_sensitive", "ignore_case", "word", "context_lines", "limit"]
            },
            {
                "shell": "rg -n 'foo|bar'",
                "tool": "search_text",
                "params": ["query", "pattern_type=regex"]
            },
            {
                "shell": "rg -n PATTERN path/",
                "tool": "search_text",
                "params": ["query", "path_regex"]
            },
            {
                "shell": "rg -n -g GLOB PATTERN",
                "tool": "search_text",
                "params": ["query", "glob", "exclude_glob"]
            },
            {
                "shell": "rg -l PATTERN",
                "tool": "search_text",
                "params": ["query", "files_with_matches=true"]
            },
            {
                "shell": "rg -c PATTERN",
                "tool": "search_text",
                "params": ["query", "count_only=true"]
            },
            {
                "shell": "identifier/API/type/class/function lookup",
                "tool": "search_symbol",
                "params": ["query", "path_regex", "path_class", "limit"]
            },
            {
                "shell": "parallel multi-grep / multi-hypothesis probes",
                "tool": "search_batch",
                "params": ["probes", "merge", "limit", "repository_id", "response_mode"],
                "note": "2..=8 independent concurrent text/symbol/hybrid probes, then merge/dedupe — not one shared multi-query walk"
            },
            {
                "shell": "usages / callers / blast radius for a known symbol",
                "tool": "impact_bundle",
                "params": ["symbol", "path_class", "repository_id", "response_mode"],
                "note": "prefer before sequential find_references + incoming_calls + implementations"
            },
            {
                "shell": "cat path",
                "tool": "read_file",
                "params": ["path"]
            },
            {
                "shell": "sed -n '10,80p' path",
                "tool": "read_file",
                "params": ["path", "start_line", "end_line", "line_count"]
            },
            {
                "shell": "follow definitions/references/calls",
                "tool": "navigation tools",
                "params": ["go_to_definition", "find_references", "find_implementations", "incoming_calls", "outgoing_calls", "impact_bundle"]
            }
        ]
    }))
    .expect("shell replacement map JSON should serialize")
}

fn support_matrix_language_id(language: SymbolLanguage) -> &'static str {
    match language {
        SymbolLanguage::TypeScript => "typescript_tsx",
        other => other.as_str(),
    }
}

fn support_matrix_search_outline(language: SymbolLanguage) -> &'static str {
    match language {
        SymbolLanguage::Blade => "supported_template_surface",
        _ => "supported_source_language",
    }
}

fn support_matrix_navigation(language: SymbolLanguage) -> &'static str {
    match language {
        SymbolLanguage::Blade => "bounded_source_template_navigation",
        _ => "read_only_source_graph_or_artifact_assisted",
    }
}

fn support_matrix_semantic_retrieval(language: SymbolLanguage) -> &'static str {
    if language.supports_semantic_chunking() {
        "optional_when_enabled"
    } else {
        "unsupported"
    }
}

fn support_matrix_capability_note(language: SymbolLanguage) -> &'static str {
    match language {
        SymbolLanguage::Blade => "template_metadata_livewire_flux",
        _ => "general_source_support",
    }
}

fn language_capabilities_json(language: SymbolLanguage) -> Value {
    let mut capabilities = Map::new();
    for capability in LanguageSupportCapability::ALL {
        capabilities.insert(
            capability.as_str().to_owned(),
            Value::String(language.capability_tier(capability).as_str().to_owned()),
        );
    }
    Value::Object(capabilities)
}

fn extended_only_tool_names() -> Vec<String> {
    let core = manifest_for_tool_surface_profile(ToolSurfaceProfile::Core);
    let extended = manifest_for_tool_surface_profile(ToolSurfaceProfile::Extended);
    extended
        .tool_names
        .into_iter()
        .filter(|tool_name| !core.tool_names.contains(tool_name))
        .collect()
}

fn support_matrix_advanced_consumers() -> Vec<String> {
    let mut consumers = extended_only_tool_names();
    consumers.push("self_improvement_loop".to_owned());
    consumers
}

fn tool_surface_json(active_profile: ToolSurfaceProfile) -> String {
    let core = manifest_for_tool_surface_profile(ToolSurfaceProfile::Core);
    let active = manifest_for_tool_surface_profile(active_profile);
    let core_guidance = if cfg!(feature = "playbook") {
        "The default runtime surface is extended. Set FRIGG_MCP_TOOL_SURFACE_PROFILE=core when you need the restricted stable subset without explore or playbook tools."
    } else {
        "The default runtime surface is extended. Set FRIGG_MCP_TOOL_SURFACE_PROFILE=core when you need the restricted stable subset without explore."
    };
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.tool_surface.v1",
        // Live SSOT for hosts/operators: generated from code manifests, not inventory freezes.
        "live": true,
        "source_of_truth": {
            "public_tool_names": "crates/cli/src/mcp/types.rs::PUBLIC_TOOL_NAMES",
            "profile_manifest": "crates/cli/src/mcp/tool_surface.rs::manifest_for_tool_surface_profile",
            "process_registered": "workspace.runtime.tools_exposed or MCP tools/list"
        },
        "not_authoritative": [
            "Historical inventory freezes (for example docs/futura-phase0-inventory.md) are forensic only",
            "Host schema caches and non-public #[tool] handlers are not the public surface"
        ],
        "default_profile": ToolSurfaceProfile::Extended.as_str(),
        "active_profile": active_profile.as_str(),
        "core_tools": core.tool_names,
        "extended_only_tools": extended_only_tool_names(),
        // Tools registered for the active profile (same set tools_exposed reports at runtime).
        "active_tools": active.tool_names,
        "guidance": [
            "This resource is the machine-readable live tool surface. Prefer it (or tools/list / workspace.runtime.tools_exposed) over Phase 0 / systems inventory freezes.",
            "Use Frigg as the default for code discovery, file listing, navigation, exact code search, and bounded source reads.",
            "Use workspace for compact workspace status or to adopt a target path/repository; repo-aware tools auto-adopt sensible defaults when possible.",
            "Before shell rg/grep/find/fd/cat/sed for code exploration, use list_files, search_text, search_symbol, search_batch, search_hybrid, read_file, read_match, impact_bundle, or navigation tools.",
            "Use search_hybrid only for broad discovery-style repository questions; use search_text for rg-shaped literal or safe-regex code scans, including grouped alternation and path_regex narrowing; pass the search term as query, not pattern; use search_symbol for known identifiers; use search_batch for multi-hypothesis guesses (2..=8 independent concurrent probes, then merge); use impact_bundle for usages/callers/blast radius of a known symbol; use list_files for rg --files-shaped listing.",
            "Use shell tools as the exception for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, and unavailable Frigg results.",
            "Use Frigg when repository-aware evidence, symbols, navigation, provenance, or multi-repo context matter.",
            "Read surfaces are text-first by default: read_file, read_match, and explore(operation=zoom). Request presentation_mode=json when a downstream consumer needs the structured compatibility payload.",
            "Use include_follow_up_structural=true when you want replayable search_structural follow-ups from inspect_syntax_tree, search_structural, or anchored navigation and outline results.",
            core_guidance
        ]
    }))
    .expect("tool surface JSON should serialize")
}

fn shell_vs_frigg_markdown(active_profile: ToolSurfaceProfile) -> String {
    let explore_guidance = if active_profile == ToolSurfaceProfile::Extended {
        "`explore` is available for bounded single-artifact follow-up after discovery. `explore(operation=zoom)` defaults to the same text-first read rendering as `read_file` and `read_match`, while `probe` and `refine` stay structured by default."
    } else {
        "`explore` is intentionally absent from the active `core` profile."
    };
    format!(
        "# Shell vs Frigg\n\n\
Use Frigg as the default for code discovery, file listing, navigation, exact code search, and bounded source reads.\n\n\
- repository-aware file listing through `list_files` instead of `rg --files`, `find`, or `fd`\n\
- symbol, definition, reference, implementation, or call navigation\n\
- exact literal, safe-regex, grouped-alternation, or `rg`-shaped code searches with repository scoping and result handles\n\
- bounded source reads through `read_file`, `read_match`, or `explore(operation=zoom)`\n\
- mixed doc/runtime questions where lexical, graph, witness, and semantic channels may all matter\n\
- evidence-backed answers or replayable source references\n\
- attached multi-repo context instead of one current shell directory\n\n\
Use shell tools only as the exception for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, and unavailable Frigg results.\n\n\
Use shell `rg` for explicit live-disk verification, ripgrep-specific flags outside `search_text`, or generated/unindexed files.\n\n\
Shell replacement map:\n\
- `rg --files` -> `list_files`\n\
- `rg -n \"text\"` -> `search_text`\n\
- `rg -n \"foo|bar\"` -> `search_text` with regex mode\n\
- `rg -n \"text\" path/` -> `search_text` with `path_regex`\n\
- identifier/API/type/class/function lookup -> `search_symbol`\n\
- parallel multi-grep / multi-hypothesis probes -> `search_batch` (2..=8 independent concurrent probes, then merge/dedupe; not one shared multi-query walk)\n\
- usages / callers / blast radius for a known symbol -> prefer `impact_bundle(symbol)` before sequential navigation tools\n\
- `cat path` -> `read_file`\n\
- `sed -n '10,80p' path` -> `read_file` with `start_line`, `end_line`, or `line_count`\n\
- follow definitions/references/calls -> navigation tools (or `impact_bundle` when the symbol is already known)\n\n\
Use `search_hybrid` only for broad discovery-style repository questions when there is no stable string, symbol, or path anchor yet. Use `search_text` for `rg`-shaped literal or safe-regex scans, including grouped alternation, `path_regex` narrowing, context windows, per-file limits (`max_count_per_file`), and file-containment probes (`files_with_matches`). For `search_text`, pass the search term as `query`, not `pattern`. Frigg may execute those scans with its native scanner, its ripgrep accelerator, or a mixed path while preserving repository-scoped results and result handles. Use `search_symbol` for known identifiers. Use `search_batch` when you would fire several Frigg probes in one turn (text/symbol/hybrid); each probe is a full independent search, then results merge. Prefer `impact_bundle` for impact/refactor questions with a known symbol before chaining `find_references` / `incoming_calls` / `find_implementations` by hand.\n\n\
`read_file` and `read_match` default to text-first output. Ask for `presentation_mode=json` when a caller needs the structured compatibility payload with explicit `content`, and apply the same rule to `explore(operation=zoom)` in the extended profile.\n\n\
Structural follow-up suggestions are opt-in. Use `include_follow_up_structural=true` on `inspect_syntax_tree`, `search_structural`, or anchored navigation and outline tools when you want replayable `search_structural` follow-ups derived from the resolved AST focus.\n\n\
Semantic retrieval remains an optional accelerator, not the grounding layer.\n\n\
{explore_guidance}\n"
    )
}

pub(crate) fn policy_resources() -> Vec<Resource> {
    vec![
        Resource::new(SUPPORT_MATRIX_RESOURCE_URI, "FRIGG Support Matrix")
            .with_description("Machine-readable supported languages and capability notes.")
            .with_mime_type("application/json"),
        Resource::new(TOOL_SURFACE_RESOURCE_URI, "FRIGG Tool Surface Policy")
            .with_description(
                "Live machine catalog of core vs extended MCP tools (active_tools, profile manifests). Prefer over inventory freezes.",
            )
            .with_mime_type("application/json"),
        Resource::new(
            SHELL_REPLACEMENT_MAP_RESOURCE_URI,
            "FRIGG Shell Replacement Map",
        )
        .with_description("Machine-readable shell-to-Frigg replacement table.")
        .with_mime_type("application/json"),
        Resource::new(SHELL_GUIDANCE_RESOURCE_URI, "Shell vs Frigg Guidance")
            .with_description(
                "Guidance for when to use shell tools versus repo-aware Frigg surfaces.",
            )
            .with_mime_type("text/markdown"),
        Resource::new(ROUTING_STATS_RESOURCE_URI, "Frigg Routing Stats")
            .with_description(
                "Local opt-in session routing stats (tool mix, zero-hits, recovery, handle failures). Enable with FRIGG_ROUTING_STATS=1. No cloud telemetry.",
            )
            .with_mime_type("application/json"),
    ]
}

pub(crate) fn read_policy_resource(
    uri: &str,
    active_profile: ToolSurfaceProfile,
) -> Option<ReadResourceResult> {
    let (content, mime_type) = match uri {
        SUPPORT_MATRIX_RESOURCE_URI => (support_matrix_json(), "application/json"),
        TOOL_SURFACE_RESOURCE_URI => (tool_surface_json(active_profile), "application/json"),
        SHELL_REPLACEMENT_MAP_RESOURCE_URI => (shell_replacement_map_json(), "application/json"),
        SHELL_GUIDANCE_RESOURCE_URI => (shell_vs_frigg_markdown(active_profile), "text/markdown"),
        ROUTING_STATS_RESOURCE_URI => (
            crate::mcp::routing_stats::snapshot_json(),
            "application/json",
        ),
        _ => return None,
    };

    Some(ReadResourceResult::new(vec![
        ResourceContents::text(content, uri).with_mime_type(mime_type),
    ]))
}

pub(crate) fn guidance_prompts() -> Vec<Prompt> {
    vec![Prompt::new(
        ROUTING_GUIDE_PROMPT_NAME,
        Some("Route a code question toward Frigg tools, shell exceptions, or extended follow-up."),
        Some(vec![PromptArgument::new("task")
            .with_description("Optional task or question to route.")
            .with_required(false)]),
    )
    .with_title("FRIGG Routing Guide")]
}

pub(crate) fn read_guidance_prompt(
    name: &str,
    arguments: Option<&Map<String, Value>>,
    active_profile: ToolSurfaceProfile,
) -> Option<GetPromptResult> {
    if name != ROUTING_GUIDE_PROMPT_NAME {
        return None;
    }

    let task = arguments
        .and_then(|map| map.get("task"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let profile_note = if active_profile == ToolSurfaceProfile::Extended {
        "Active profile: `extended`."
    } else {
        "Active profile: `core`."
    };
    let mut text = String::new();
    if let Some(task) = task {
        text.push_str("Task:\n");
        text.push_str(task);
        text.push_str("\n\n");
    }
    text.push_str(
        "Routing policy:\n\
1. Prefer Frigg for code discovery, navigation, exact code search, and bounded source reads.\n\
2. Use `search_hybrid` only for broad discovery-style repository questions when there is no stable string, symbol, or path anchor yet; use `search_text` for `rg`-shaped literal or safe-regex scans, including grouped alternation and `path_regex`; use `search_symbol` for known identifiers; use `search_batch` for multi-hypothesis guesses (2..=8 independent concurrent probes, then merge); prefer `impact_bundle(symbol)` for usages/callers/blast radius before sequential navigation.\n\
3. Use shell tools as the exception for non-code files, git/filesystem inspection, explicit live-disk verification, trivial one-offs, or ripgrep-specific flags outside `search_text`.\n\
4. Prefer Frigg core tools when repository-aware evidence, symbols, navigation, provenance, or multi-repo context matter.\n\
5. Treat semantic retrieval as optional acceleration only. When broad discovery is weak, pivot to lexical, graph, and source-witness evidence instead of diagnosing runtime state by default.\n\
6. Treat the current supported-language set as one public list: Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Java, Lua, Roc, and Nim. Describe differences in concrete capability terms, not first-class or baseline badges.\n\
7. `read_file` and `read_match` default to text-first output; request `presentation_mode=json` only when the caller truly needs the structured compatibility payload. In the extended profile, `explore(operation=zoom)` follows the same text-first default, while `probe` and `refine` stay structured.\n\
8. Use `include_follow_up_structural=true` when you want replayable `search_structural` follow-ups from `inspect_syntax_tree`, `search_structural`, or anchored navigation and outline results.\n\
9. Use `explore` only after discovery and only when the active profile includes it.\n\n",
    );
    text.push_str(profile_note);

    Some(
        GetPromptResult::new(vec![
            PromptMessage::new_text(Role::Assistant, text),
            PromptMessage::new_resource_link(
                Role::Assistant,
                Resource::new(SUPPORT_MATRIX_RESOURCE_URI, "FRIGG Support Matrix"),
            ),
            PromptMessage::new_resource_link(
                Role::Assistant,
                Resource::new(TOOL_SURFACE_RESOURCE_URI, "FRIGG Tool Surface Policy"),
            ),
            PromptMessage::new_resource_link(
                Role::Assistant,
                Resource::new(
                    SHELL_REPLACEMENT_MAP_RESOURCE_URI,
                    "FRIGG Shell Replacement Map",
                ),
            ),
            PromptMessage::new_resource_link(
                Role::Assistant,
                Resource::new(SHELL_GUIDANCE_RESOURCE_URI, "Shell vs Frigg Guidance"),
            ),
        ])
        .with_description("Guide shell-vs-FRIGG routing and link the relevant policy resources."),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ROUTING_GUIDE_PROMPT_NAME, SHELL_GUIDANCE_RESOURCE_URI, SHELL_REPLACEMENT_MAP_RESOURCE_URI,
        SUPPORT_MATRIX_RESOURCE_URI, TOOL_SURFACE_RESOURCE_URI, read_guidance_prompt,
        read_policy_resource,
    };
    use crate::languages::{LanguageSupportCapability, SymbolLanguage};
    use crate::mcp::tool_surface::ToolSurfaceProfile;
    use rmcp::model::ResourceContents;
    use serde_json::{Value, json};

    fn resource_text(uri: &str, profile: ToolSurfaceProfile) -> String {
        let result = read_policy_resource(uri, profile).expect("resource should exist");
        let ResourceContents::TextResourceContents { text, .. } = &result.contents[0] else {
            unreachable!("expected text resource contents");
        };
        text.clone()
    }

    #[test]
    fn support_matrix_lists_supported_languages_without_rollout_tiers() {
        let json = resource_text(SUPPORT_MATRIX_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("support matrix JSON should parse");
        assert!(parsed.get("next_language_priority").is_none());
        assert!(parsed.get("language_rollout_policy").is_none());
        assert_eq!(parsed["schema_id"], json!("frigg.policy.support_matrix.v4"));
        assert_eq!(
            parsed["capability_tiers"]["core"].as_str(),
            Some("capability is part of FRIGG's stable read-only core contract for that language")
        );
        assert_eq!(
            parsed["capability_tiers"]["optional_accelerator"].as_str(),
            Some(
                "capability is an optional accelerator that only contributes when runtime configuration and repository state make it available"
            )
        );
        for language_id in [
            "rust",
            "php",
            "blade",
            "typescript_tsx",
            "python",
            "go",
            "kotlin",
            "java",
            "lua",
            "roc",
            "nim",
        ] {
            assert!(
                parsed["languages"]
                    .as_array()
                    .expect("languages should be an array")
                    .iter()
                    .any(|entry| entry["id"] == json!(language_id)),
                "expected {language_id} to be listed as supported"
            );
        }
        assert!(
            parsed["languages"]
                .as_array()
                .expect("languages should be an array")
                .iter()
                .any(|entry| {
                    entry["id"] == json!("blade")
                        && entry["capability_note"] == json!("template_metadata_livewire_flux")
                })
        );
        assert_eq!(
            parsed["languages"]
                .as_array()
                .expect("languages should be an array")
                .iter()
                .find(|entry| entry["id"] == json!("typescript_tsx"))
                .and_then(|entry| entry.get("capabilities"))
                .and_then(|value| value.get("semantic_chunking"))
                .and_then(|value| value.as_str()),
            Some("unsupported")
        );
        assert_eq!(
            parsed["languages"]
                .as_array()
                .expect("languages should be an array")
                .iter()
                .find(|entry| entry["id"] == json!("rust"))
                .and_then(|entry| entry.get("capabilities"))
                .and_then(|value| value.get("precise_artifact_assist"))
                .and_then(|value| value.as_str()),
            Some("optional_accelerator")
        );
    }

    #[test]
    fn support_matrix_capabilities_match_language_registry() {
        let json = resource_text(SUPPORT_MATRIX_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("support matrix JSON should parse");
        let languages = parsed["languages"]
            .as_array()
            .expect("languages should be an array");

        for language in SymbolLanguage::ALL {
            let expected_id = if matches!(language, SymbolLanguage::TypeScript) {
                "typescript_tsx"
            } else {
                language.as_str()
            };
            let entry = languages
                .iter()
                .find(|entry| entry["id"] == json!(expected_id))
                .unwrap_or_else(|| unreachable!("expected {expected_id} to be listed"));
            for capability in LanguageSupportCapability::ALL {
                let expected = language.capability_tier(capability).as_str();
                assert_eq!(
                    entry["capabilities"][capability.as_str()].as_str(),
                    Some(expected),
                    "expected {expected_id} capability {} to match the registry",
                    capability.as_str()
                );
            }
        }
    }

    #[test]
    fn support_matrix_advanced_consumers_follow_extended_tool_surface_manifest() {
        let json = resource_text(SUPPORT_MATRIX_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("support matrix JSON should parse");
        let advanced_consumers = parsed["advanced_consumers"]
            .as_array()
            .expect("advanced_consumers should be an array");
        let core =
            crate::mcp::tool_surface::manifest_for_tool_surface_profile(ToolSurfaceProfile::Core);
        let extended = crate::mcp::tool_surface::manifest_for_tool_surface_profile(
            ToolSurfaceProfile::Extended,
        );

        for tool_name in extended
            .tool_names
            .iter()
            .filter(|tool_name| !core.tool_names.contains(tool_name))
        {
            assert!(
                advanced_consumers
                    .iter()
                    .any(|entry| entry.as_str() == Some(tool_name.as_str())),
                "expected advanced_consumers to include extended-only tool {tool_name}"
            );
        }
        assert!(
            !advanced_consumers
                .iter()
                .any(|entry| entry.as_str() == Some("search_text")),
            "stable-core tools must not leak into advanced_consumers"
        );
        assert!(
            advanced_consumers
                .iter()
                .any(|entry| entry.as_str() == Some("self_improvement_loop")),
            "support matrix should keep non-tool advanced consumers explicit"
        );
    }

    #[test]
    fn tool_surface_policy_lists_explore_as_extended_only() {
        let json = resource_text(TOOL_SURFACE_RESOURCE_URI, ToolSurfaceProfile::Extended);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("tool surface policy JSON should parse");
        assert!(
            parsed["extended_only_tools"]
                .as_array()
                .expect("extended_only_tools should be an array")
                .iter()
                .any(|entry| entry == "explore")
        );
    }

    #[test]
    fn tool_surface_policy_matches_profile_manifests() {
        let json = resource_text(TOOL_SURFACE_RESOURCE_URI, ToolSurfaceProfile::Extended);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("tool surface policy JSON should parse");
        let core =
            crate::mcp::tool_surface::manifest_for_tool_surface_profile(ToolSurfaceProfile::Core);
        let extended = crate::mcp::tool_surface::manifest_for_tool_surface_profile(
            ToolSurfaceProfile::Extended,
        );
        let expected_extended_only = extended
            .tool_names
            .iter()
            .filter(|tool_name| !core.tool_names.contains(tool_name))
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>();

        assert_eq!(
            parsed["default_profile"].as_str(),
            Some(ToolSurfaceProfile::Extended.as_str())
        );
        assert_eq!(parsed["live"], json!(true));
        assert_eq!(
            parsed["source_of_truth"]["public_tool_names"].as_str(),
            Some("crates/cli/src/mcp/types.rs::PUBLIC_TOOL_NAMES")
        );
        assert!(
            parsed["not_authoritative"]
                .as_array()
                .is_some_and(|rows| !rows.is_empty()),
            "tool-surface.json should mark inventory freezes as non-authoritative"
        );
        assert_eq!(
            parsed["core_tools"].as_array(),
            Some(
                &core
                    .tool_names
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect::<Vec<_>>()
            )
        );
        assert_eq!(
            parsed["extended_only_tools"].as_array(),
            Some(&expected_extended_only)
        );
        assert_eq!(
            parsed["active_tools"].as_array(),
            Some(
                &extended
                    .tool_names
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect::<Vec<_>>()
            ),
            "active_tools must match the active profile manifest (live SSOT)"
        );
    }

    #[test]
    fn tool_surface_active_tools_follow_core_profile() {
        let json = resource_text(TOOL_SURFACE_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("tool surface policy JSON should parse");
        let core =
            crate::mcp::tool_surface::manifest_for_tool_surface_profile(ToolSurfaceProfile::Core);
        assert_eq!(parsed["active_profile"].as_str(), Some("core"));
        assert_eq!(
            parsed["active_tools"].as_array(),
            Some(
                &core
                    .tool_names
                    .iter()
                    .cloned()
                    .map(Value::String)
                    .collect::<Vec<_>>()
            )
        );
        assert!(
            !parsed["active_tools"]
                .as_array()
                .expect("active_tools")
                .iter()
                .any(|entry| entry == "explore"),
            "core active_tools must omit extended-only explore"
        );
    }

    #[test]
    fn shell_replacement_map_is_machine_readable() {
        let json = resource_text(SHELL_REPLACEMENT_MAP_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("shell replacement map JSON should parse");
        assert_eq!(
            parsed["schema_id"],
            json!("frigg.policy.shell_replacement_map.v1")
        );
        assert!(
            parsed["replacements"]
                .as_array()
                .expect("replacements should be an array")
                .iter()
                .any(|entry| {
                    entry["shell"] == json!("sed -n '10,80p' path")
                        && entry["tool"] == json!("read_file")
                        && entry["params"]
                            .as_array()
                            .expect("params should be an array")
                            .iter()
                            .any(|param| param == "start_line")
                        && entry["params"]
                            .as_array()
                            .expect("params should be an array")
                            .iter()
                            .any(|param| param == "line_count")
                })
        );
        assert!(
            parsed["replacements"]
                .as_array()
                .expect("replacements should be an array")
                .iter()
                .any(|entry| {
                    entry["shell"] == json!("rg -l PATTERN")
                        && entry["tool"] == json!("search_text")
                        && entry["params"]
                            .as_array()
                            .expect("params should be an array")
                            .iter()
                            .any(|param| param == "files_with_matches=true")
                })
        );
        assert!(
            parsed["replacements"]
                .as_array()
                .expect("replacements should be an array")
                .iter()
                .any(|entry| entry["tool"] == json!("search_batch")),
            "shell map should list search_batch for multi-hypothesis work"
        );
        assert!(
            parsed["replacements"]
                .as_array()
                .expect("replacements should be an array")
                .iter()
                .any(|entry| entry["tool"] == json!("impact_bundle")),
            "shell map should list impact_bundle for usages/callers"
        );
    }

    #[test]
    fn guidance_surfaces_mention_search_batch_and_impact_bundle() {
        let shell_guidance = resource_text(SHELL_GUIDANCE_RESOURCE_URI, ToolSurfaceProfile::Core);
        let tool_surface = resource_text(TOOL_SURFACE_RESOURCE_URI, ToolSurfaceProfile::Core);
        let shell_map = resource_text(SHELL_REPLACEMENT_MAP_RESOURCE_URI, ToolSurfaceProfile::Core);
        let prompt = read_guidance_prompt(
            ROUTING_GUIDE_PROMPT_NAME,
            None,
            ToolSurfaceProfile::Extended,
        )
        .expect("routing prompt should exist");
        let prompt_text = format!("{prompt:?}");

        for surface in [&shell_guidance, &tool_surface, &shell_map, &prompt_text] {
            assert!(
                surface.contains("search_batch"),
                "guidance surface missing search_batch: {surface:.200}"
            );
            assert!(
                surface.contains("impact_bundle"),
                "guidance surface missing impact_bundle: {surface:.200}"
            );
        }
        assert!(
            shell_guidance.contains("independent concurrent"),
            "shell guidance should describe batch as independent concurrent probes"
        );
    }

    #[test]
    fn routing_prompt_links_policy_resources() {
        let prompt = read_guidance_prompt(
            ROUTING_GUIDE_PROMPT_NAME,
            Some(&serde_json::Map::from_iter([(
                "task".to_owned(),
                json!("where is runtime state wired"),
            )])),
            ToolSurfaceProfile::Extended,
        )
        .expect("routing prompt should exist");
        assert_eq!(prompt.messages.len(), 5);
    }

    #[test]
    fn agent_directive_core_sentences_are_in_guidance_surfaces() {
        let shell_guidance = resource_text(SHELL_GUIDANCE_RESOURCE_URI, ToolSurfaceProfile::Core);
        let tool_surface = resource_text(TOOL_SURFACE_RESOURCE_URI, ToolSurfaceProfile::Core);
        let prompt = read_guidance_prompt(
            ROUTING_GUIDE_PROMPT_NAME,
            Some(&serde_json::Map::from_iter([(
                "task".to_owned(),
                json!("find every caller of load_config"),
            )])),
            ToolSurfaceProfile::Extended,
        )
        .expect("routing prompt should exist");
        let prompt_debug = format!("{prompt:?}");

        assert!(shell_guidance.contains(
            "Use Frigg as the default for code discovery, file listing, navigation, exact code search, and bounded source reads."
        ));
        assert!(tool_surface.contains(
            "Use Frigg as the default for code discovery, file listing, navigation, exact code search, and bounded source reads."
        ));
        assert!(
            shell_guidance.contains(
                "Use `search_hybrid` only for broad discovery-style repository questions"
            )
        );
        assert!(
            tool_surface
                .contains("Use search_hybrid only for broad discovery-style repository questions")
        );
        assert!(
            prompt_debug.contains(
                "Use `search_hybrid` only for broad discovery-style repository questions"
            )
        );
        assert!(prompt_debug.contains(
            "Prefer Frigg for code discovery, navigation, exact code search, and bounded source reads."
        ));
        assert!(shell_guidance.contains(
            "Use shell tools only as the exception for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, and unavailable Frigg results."
        ));
        assert!(tool_surface.contains(
            "Use shell tools as the exception for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, and unavailable Frigg results."
        ));
        assert!(prompt_debug.contains(
            "Use shell tools as the exception for non-code files, git/filesystem inspection, explicit live-disk verification, trivial one-offs, or ripgrep-specific flags outside `search_text`."
        ));
    }
}
