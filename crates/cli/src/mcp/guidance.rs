use rmcp::model::{
    AnnotateAble, GetPromptResult, Prompt, PromptArgument, PromptMessage, PromptMessageRole,
    RawResource, ReadResourceResult, Resource, ResourceContents,
};
use serde::Serialize;
use serde_json::{Map, Value, json};

use crate::mcp::tool_surface::{ToolSurfaceProfile, manifest_for_tool_surface_profile};

pub(crate) const SUPPORT_MATRIX_RESOURCE_URI: &str = "frigg://policy/support-matrix.json";
pub(crate) const TOOL_SURFACE_RESOURCE_URI: &str = "frigg://policy/tool-surface.json";
pub(crate) const SHELL_GUIDANCE_RESOURCE_URI: &str = "frigg://guidance/shell-vs-frigg.md";
pub(crate) const ROUTING_GUIDE_PROMPT_NAME: &str = "frigg-routing-guide";

#[derive(Debug, Clone, Serialize)]
struct LanguageStage {
    id: &'static str,
    description: &'static str,
}

#[derive(Debug, Clone, Serialize)]
struct LanguageSupportEntry {
    id: &'static str,
    display_name: &'static str,
    status: &'static str,
    search_outline: &'static str,
    navigation: &'static str,
    semantic_retrieval: &'static str,
    rollout_stage: &'static str,
    next_priority: bool,
}

fn baseline_runtime_support_entry(
    id: &'static str,
    display_name: &'static str,
    next_priority: bool,
) -> LanguageSupportEntry {
    LanguageSupportEntry {
        id,
        display_name,
        status: "baseline_runtime_surface",
        search_outline: "baseline_runtime_surface",
        navigation: "bounded_heuristic_navigation",
        semantic_retrieval: "experimental_when_enabled",
        rollout_stage: "runtime_l1_l2",
        next_priority,
    }
}

fn support_matrix_json() -> String {
    let stages = vec![
        LanguageStage {
            id: "witness_only",
            description: "Hybrid/path witness support only. Useful, but not enough for first-class claims.",
        },
        LanguageStage {
            id: "runtime_l1_l2",
            description: "Runtime symbol + outline + heuristic navigation support with stable public semantics.",
        },
        LanguageStage {
            id: "precise_l3",
            description: "Precise SCIP-backed navigation is validated and part of the supported public story.",
        },
        LanguageStage {
            id: "semantic_parity",
            description: "Semantic chunking, indexing, ranking, watch/reindex, and provenance behavior are aligned too.",
        },
    ];
    let languages = vec![
        LanguageSupportEntry {
            id: "rust",
            display_name: "Rust",
            status: "first_class",
            search_outline: "first_class",
            navigation: "first_class",
            semantic_retrieval: "first_class_when_enabled",
            rollout_stage: "semantic_parity",
            next_priority: false,
        },
        LanguageSupportEntry {
            id: "php",
            display_name: "PHP",
            status: "first_class",
            search_outline: "first_class",
            navigation: "first_class",
            semantic_retrieval: "first_class_when_enabled",
            rollout_stage: "semantic_parity",
            next_priority: false,
        },
        LanguageSupportEntry {
            id: "blade",
            display_name: "Blade",
            status: "first_class_template_surface",
            search_outline: "first_class_template_surface",
            navigation: "bounded_source_template_navigation",
            semantic_retrieval: "bounded_template_retrieval",
            rollout_stage: "runtime_l1_l2",
            next_priority: false,
        },
        baseline_runtime_support_entry("typescript_tsx", "TypeScript / TSX", true),
        baseline_runtime_support_entry("python", "Python", false),
        baseline_runtime_support_entry("go", "Go", false),
        baseline_runtime_support_entry("kotlin", "Kotlin / KTS", false),
        baseline_runtime_support_entry("lua", "Lua", false),
        baseline_runtime_support_entry("roc", "Roc", false),
        baseline_runtime_support_entry("nim", "Nim", false),
    ];
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.support_matrix.v1",
        "product": "frigg",
        "product_boundary": "local-first deterministic code-evidence engine delivered through MCP",
        "next_language_priority": "typescript_tsx",
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
        "advanced_consumers": [
            "explore",
            "deep_search_*",
            "self-improvement loop"
        ],
        "language_rollout_policy": stages,
        "languages": languages
    }))
    .expect("support matrix JSON should serialize")
}

fn tool_surface_json(active_profile: ToolSurfaceProfile) -> String {
    let core = manifest_for_tool_surface_profile(ToolSurfaceProfile::Core);
    let extended = manifest_for_tool_surface_profile(ToolSurfaceProfile::Extended);
    let extended_only = extended
        .tool_names
        .iter()
        .filter(|tool_name| !core.tool_names.contains(tool_name))
        .cloned()
        .collect::<Vec<_>>();
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.tool_surface.v1",
        "default_profile": ToolSurfaceProfile::Core.as_str(),
        "active_profile": active_profile.as_str(),
        "core_tools": core.tool_names,
        "extended_only_tools": extended_only,
        "guidance": [
            "Use shell tools for trivial local literal scans and one-off file reads in the checked-out workspace.",
            "Use Frigg when repository-aware evidence, symbols, navigation, provenance, or multi-repo context matter.",
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
Semantic retrieval remains an optional accelerator, not the grounding layer.\n\
If semantic status is disabled, degraded, or unavailable, treat the answer as lexical/graph/witness-only.\n\n\
{explore_guidance}\n"
    )
}

pub(crate) fn policy_resources() -> Vec<Resource> {
    vec![
        RawResource::new(SUPPORT_MATRIX_RESOURCE_URI, "FRIGG Support Matrix")
            .with_description(
                "Machine-readable support policy, rollout stages, and next language priority.",
            )
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
4. Keep public language claims conservative: Rust and PHP are first-class, Blade is a first-class template surface, and TypeScript / TSX, Python, Go, Kotlin / KTS, Lua, Roc, plus Nim are baseline runtime surfaces rather than precise or semantic-parity languages.\n\
5. Use `explore` only after discovery and only when the active profile includes it.\n\n",
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
    use crate::mcp::tool_surface::ToolSurfaceProfile;
    use rmcp::model::ResourceContents;
    use serde_json::{Value, json};

    fn resource_text(uri: &str, profile: ToolSurfaceProfile) -> String {
        let result = read_policy_resource(uri, profile).expect("resource should exist");
        match &result.contents[0] {
            ResourceContents::TextResourceContents { text, .. } => text.clone(),
            other => panic!("expected text resource contents, got {other:?}"),
        }
    }

    #[test]
    fn support_matrix_marks_typescript_as_next_priority() {
        let json = resource_text(SUPPORT_MATRIX_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("support matrix JSON should parse");
        assert_eq!(
            parsed["next_language_priority"],
            Value::String("typescript_tsx".to_owned())
        );
        assert!(
            parsed["languages"]
                .as_array()
                .expect("languages should be an array")
                .iter()
                .any(|entry| {
                    entry["id"] == json!("typescript_tsx") && entry["next_priority"] == json!(true)
                })
        );
        assert!(
            parsed["languages"]
                .as_array()
                .expect("languages should be an array")
                .iter()
                .any(|entry| {
                    entry["id"] == json!("typescript_tsx")
                        && entry["rollout_stage"] == json!("runtime_l1_l2")
                })
        );
        assert!(
            parsed["languages"]
                .as_array()
                .expect("languages should be an array")
                .iter()
                .any(|entry| {
                    entry["id"] == json!("python")
                        && entry["rollout_stage"] == json!("runtime_l1_l2")
                })
        );
        for language_id in ["go", "kotlin", "lua", "roc", "nim"] {
            assert!(
                parsed["languages"]
                    .as_array()
                    .expect("languages should be an array")
                    .iter()
                    .any(|entry| {
                        entry["id"] == json!(language_id)
                            && entry["rollout_stage"] == json!("runtime_l1_l2")
                    }),
                "expected {language_id} to be marked as runtime_l1_l2"
            );
        }
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
        assert_eq!(prompt.messages.len(), 3);
    }
}
