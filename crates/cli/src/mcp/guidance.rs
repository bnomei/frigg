use rmcp::model::{
    AnnotateAble, GetPromptResult, Prompt, PromptArgument, PromptMessage, PromptMessageRole,
    RawResource, ReadResourceResult, Resource, ResourceContents,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::languages::{LanguageSupportCapability, SymbolLanguage};
use crate::mcp::tool_surface::{ToolSurfaceProfile, manifest_for_tool_surface_profile};

pub(crate) const SUPPORT_MATRIX_RESOURCE_URI: &str = "frigg://policy/support-matrix.json";
pub(crate) const TOOL_SURFACE_RESOURCE_URI: &str = "frigg://policy/tool-surface.json";
pub(crate) const SHELL_GUIDANCE_RESOURCE_URI: &str = "frigg://guidance/shell-vs-frigg.md";
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
            "provenance-backed auditing"
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
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.tool_surface.v1",
        "default_profile": ToolSurfaceProfile::Core.as_str(),
        "active_profile": active_profile.as_str(),
        "core_tools": core.tool_names,
        "extended_only_tools": extended_only_tool_names(),
        "guidance": [
            "Use shell tools for trivial local literal scans and one-off file reads in the checked-out workspace.",
            "Use Frigg when repository-aware evidence, symbols, navigation, provenance, or multi-repo context matter.",
            "Use include_follow_up_structural=true when you want replayable search_structural follow-ups from inspect_syntax_tree, search_structural, or anchored navigation and outline results.",
            "Treat extended tools as advanced consumers of the stable runtime surface, not the product center."
        ]
    }))
    .expect("tool surface JSON should serialize")
}

fn shell_vs_frigg_markdown(active_profile: ToolSurfaceProfile) -> String {
    let explore_guidance = if active_profile == ToolSurfaceProfile::Extended {
        "`explore` is available for bounded single-artifact follow-up after discovery."
    } else {
        "`explore` is intentionally absent from the active `core` profile."
    };
    format!(
        "# Shell vs Frigg\n\n\
Use shell tools when the task is a trivial local operation in the checked-out workspace.\n\n\
- exact literal scans such as `rg foo`\n\
- quick one-off file reads such as `sed -n '1,120p' file`\n\
- generic filesystem or git inspection\n\n\
Use Frigg when the task needs repository-aware evidence.\n\n\
- symbol, definition, reference, implementation, or call navigation\n\
- mixed doc/runtime questions where lexical, graph, witness, and semantic channels may all matter\n\
- provenance-backed answers or replayable evidence\n\
- attached multi-repo context instead of one current shell directory\n\n\
Structural follow-up suggestions are opt-in. Use `include_follow_up_structural=true` on `inspect_syntax_tree`, `search_structural`, or anchored navigation and outline tools when you want replayable `search_structural` follow-ups derived from the resolved AST focus.\n\n\
Semantic retrieval remains an optional accelerator, not the grounding layer.\n\
If semantic status is disabled, degraded, or unavailable, treat the answer as lexical/graph/witness-only.\n\n\
{explore_guidance}\n"
    )
}

pub(crate) fn policy_resources() -> Vec<Resource> {
    vec![
        RawResource::new(SUPPORT_MATRIX_RESOURCE_URI, "FRIGG Support Matrix")
            .with_description("Machine-readable supported languages and capability notes.")
            .with_mime_type("application/json")
            .no_annotation(),
        RawResource::new(TOOL_SURFACE_RESOURCE_URI, "FRIGG Tool Surface Policy")
            .with_description("Machine-readable core vs extended tool-surface policy.")
            .with_mime_type("application/json")
            .no_annotation(),
        RawResource::new(SHELL_GUIDANCE_RESOURCE_URI, "Shell vs Frigg Guidance")
            .with_description(
                "Guidance for when to use shell tools versus repo-aware Frigg surfaces.",
            )
            .with_mime_type("text/markdown")
            .no_annotation(),
    ]
}

pub(crate) fn read_policy_resource(
    uri: &str,
    active_profile: ToolSurfaceProfile,
) -> Option<ReadResourceResult> {
    let (content, mime_type) = match uri {
        SUPPORT_MATRIX_RESOURCE_URI => (support_matrix_json(), "application/json"),
        TOOL_SURFACE_RESOURCE_URI => (tool_surface_json(active_profile), "application/json"),
        SHELL_GUIDANCE_RESOURCE_URI => (shell_vs_frigg_markdown(active_profile), "text/markdown"),
        _ => return None,
    };

    Some(ReadResourceResult::new(vec![
        ResourceContents::text(content, uri).with_mime_type(mime_type),
    ]))
}

pub(crate) fn guidance_prompts() -> Vec<Prompt> {
    vec![
        Prompt::new(
            ROUTING_GUIDE_PROMPT_NAME,
            Some(
                "Route a code question toward shell tools, core Frigg tools, or extended follow-up.",
            ),
            Some(vec![
                PromptArgument::new("task")
                    .with_description("Optional task or question to route.")
                    .with_required(false),
            ]),
        )
        .with_title("FRIGG Routing Guide"),
    ]
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
1. Prefer shell tools for trivial local scans, file reads, or git/filesystem inspection.\n\
2. Prefer Frigg core tools when repository-aware evidence, symbols, navigation, provenance, or multi-repo context matter.\n\
3. Treat semantic retrieval as optional acceleration only; degraded or unavailable semantic status means lexical/graph/witness evidence is carrying the answer.\n\
4. Treat the current supported-language set as one public list: Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, and Nim. Describe differences in concrete capability terms, not first-class or baseline badges.\n\
5. Use `include_follow_up_structural=true` when you want replayable `search_structural` follow-ups from `inspect_syntax_tree`, `search_structural`, or anchored navigation and outline results.\n\
6. Use `explore` only after discovery and only when the active profile includes it.\n\n",
    );
    text.push_str(profile_note);

    Some(
        GetPromptResult::new(vec![
            PromptMessage::new_text(PromptMessageRole::Assistant, text),
            PromptMessage::new_resource_link(
                PromptMessageRole::Assistant,
                RawResource::new(SUPPORT_MATRIX_RESOURCE_URI, "FRIGG Support Matrix")
                    .no_annotation(),
            ),
            PromptMessage::new_resource_link(
                PromptMessageRole::Assistant,
                RawResource::new(TOOL_SURFACE_RESOURCE_URI, "FRIGG Tool Surface Policy")
                    .no_annotation(),
            ),
            PromptMessage::new_resource_link(
                PromptMessageRole::Assistant,
                RawResource::new(SHELL_GUIDANCE_RESOURCE_URI, "Shell vs Frigg Guidance")
                    .no_annotation(),
            ),
        ])
        .with_description("Guide shell-vs-FRIGG routing and link the relevant policy resources."),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        ROUTING_GUIDE_PROMPT_NAME, SUPPORT_MATRIX_RESOURCE_URI, TOOL_SURFACE_RESOURCE_URI,
        read_guidance_prompt, read_policy_resource,
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
            Some(ToolSurfaceProfile::Core.as_str())
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
        assert_eq!(prompt.messages.len(), 4);
    }
}
