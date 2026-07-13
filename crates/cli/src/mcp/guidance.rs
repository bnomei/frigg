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
use crate::settings::{
    DEFAULT_GOOGLE_EMBEDDING_MODEL, DEFAULT_LOCAL_EMBEDDING_MODEL,
    DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL, DEFAULT_OPENAI_EMBEDDING_MODEL, GEMINI_API_KEY_ENV_VAR,
    OPENAI_API_KEY_ENV_VAR, OPENAI_COMPAT_API_KEY_ENV_VAR, OPENAI_COMPAT_ENDPOINT_ENV_VAR,
};
use crate::storage::DEFAULT_VECTOR_DIMENSIONS;

pub(crate) const SUPPORT_MATRIX_RESOURCE_URI: &str = "frigg://policy/support-matrix.json";
pub(crate) const TOOL_SURFACE_RESOURCE_URI: &str = "frigg://policy/tool-surface.json";
pub(crate) const SHELL_REPLACEMENT_MAP_RESOURCE_URI: &str =
    "frigg://policy/shell-replacement-map.json";
/// Machine schema for skill-composed multi-claim review packets (not a callable MCP tool).
pub(crate) const EVIDENCE_PACKET_RESOURCE_URI: &str = "frigg://policy/evidence-packet.json";
/// Curated embedding-model scoreboard (peer to support-matrix; not live quality metrics).
pub(crate) const SEMANTIC_MODELS_RESOURCE_URI: &str = "frigg://policy/semantic-models.json";
pub(crate) const SHELL_GUIDANCE_RESOURCE_URI: &str = "frigg://guidance/shell-vs-frigg.md";
pub(crate) const ROUTING_STATS_RESOURCE_URI: &str =
    crate::mcp::routing_stats::ROUTING_STATS_RESOURCE_URI;
pub(crate) const ROUTING_GUIDE_PROMPT_NAME: &str = "frigg-routing-guide";

/// Native output width of the default local MiniLM alias (parity with
/// `embeddings::local_model::DEFAULT_LOCAL_MODEL_ALIAS.dimensions` when local-embeddings is on).
const LOCAL_DEFAULT_NATIVE_DIMENSIONS: usize = 384;
/// Dimensions Frigg requests/stores for OpenAI default model (matches projection; no pad).
const OPENAI_DEFAULT_NATIVE_DIMENSIONS: usize = DEFAULT_VECTOR_DIMENSIONS;
/// Dimensions Frigg requests for Google default model via `output_dimensionality`
/// (index/query paths pass `Some(DEFAULT_VECTOR_DIMENSIONS)` — not API catalog default 3072 / MRL 768).
const GOOGLE_DEFAULT_NATIVE_DIMENSIONS: usize = DEFAULT_VECTOR_DIMENSIONS;

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

/// Curated embedding model catalog (EXP-scoreboard B).
///
/// Static rows: defaults and contract facts, not a live CI leaderboard. Product was validated
/// hands-on across multi-repo playbooks; this resource does not retain published rank scores.
/// Semantic retrieval remains optional acceleration (see support-matrix), never the grounding layer.
fn semantic_models_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.semantic_models.v1",
        "product": "frigg",
        "product_role": "optional_accelerator_model_catalog",
        "live": true,
        "quality_scores": "curated",
        "quality_scores_note": "Rows are curated defaults and contract facts (dims, pad, offline, credentials). Frigg does not ship a retained public embedding leaderboard; early multi-repo playbook validation is not re-exported as scores. Prefer exact Frigg tools over hybrid rank-1 even when semantic is on.",
        "semantic_default": {
            "enabled": false,
            "note": "Semantic runtime is off by default. When enabled without a cloud provider, Frigg resolves to local MiniLM."
        },
        "projection_dimensions": DEFAULT_VECTOR_DIMENSIONS,
        "projection_note": "sqlite-vec table width (embedding float[N]). Fixed so one vector index can hold multiple providers. Short model vectors are zero-padded on write/query to fit N; oversize is rejected. Padding is storage interoperability only — it does not add semantic signal or make MiniLM '1536-quality'.",
        "dimensions_contract": {
            "model_field": "native_dimensions",
            "model_field_meaning": "REAL model output width before any storage pad (never the padded store length)",
            "store_field": "projection_dimensions",
            "pad_flag": "pad_to_projection",
            "pad_flag_meaning": "true only when native_dimensions < projection_dimensions; zeros fill the gap at the DB boundary",
            "why_pad": "vec0 requires every row to have exactly projection_dimensions floats; local MiniLM is 384-d so it is padded to fit. Partitions (repository_id, provider, model) keep different models from sharing one cosine space.",
            "reindex": "Changing provider or model requires a semantic reindex (frigg index); partitions do not auto-heal the active head"
        },
        "reindex_on_change": true,
        "source_of_truth": {
            "defaults": "crates/cli/src/settings/semantic_runtime.rs DEFAULT_*_EMBEDDING_MODEL",
            "projection": "crates/cli/src/storage/mod.rs DEFAULT_VECTOR_DIMENSIONS",
            "local_dims": "crates/cli/src/embeddings/local_model.rs DEFAULT_LOCAL_MODEL_ALIAS.dimensions",
            "normalize": "crates/cli/src/storage/semantic_store_support.rs::normalize_embedding_for_vector_projection"
        },
        "models": [
            {
                "id": "local-minilm-l6-v2",
                "provider": "local",
                "model": DEFAULT_LOCAL_EMBEDDING_MODEL,
                "role": "default",
                "quality_tier": "offline_smoke",
                "offline": true,
                "native_dimensions": LOCAL_DEFAULT_NATIVE_DIMENSIONS,
                "pad_to_projection": LOCAL_DEFAULT_NATIVE_DIMENSIONS < DEFAULT_VECTOR_DIMENSIONS,
                "credential_env": null,
                "reindex_on_change": true,
                "quality": "curated",
                "known_limits": [
                    "Offline smoke / zero-key accelerator — general-purpose MiniLM, not a code-specialized embedder",
                    "Product/natural phrases may map weakly to API identifiers (prefer search_symbol / search_text after hybrid)",
                    "Requires local model preparation at startup when provider=local",
                    "native_dimensions is 384 (real); store pads with zeros to projection_dimensions — pad is storage only, not quality",
                    "Embed documents use path+language envelope; after template upgrade run full frigg index (not changed-only)"
                ]
            },
            {
                "id": "openai-text-embedding-3-small",
                "provider": "openai",
                "model": DEFAULT_OPENAI_EMBEDDING_MODEL,
                "role": "recommended",
                "quality_tier": "cloud",
                "offline": false,
                "native_dimensions": OPENAI_DEFAULT_NATIVE_DIMENSIONS,
                "pad_to_projection": OPENAI_DEFAULT_NATIVE_DIMENSIONS < DEFAULT_VECTOR_DIMENSIONS,
                "credential_env": OPENAI_API_KEY_ENV_VAR,
                "reindex_on_change": true,
                "quality": "curated",
                "known_limits": [
                    "Requires OPENAI_API_KEY and network",
                    "Cloud embeddings leave the machine; use local when zero-cloud is required"
                ]
            },
            {
                "id": "google-gemini-embedding-001",
                "provider": "google",
                "model": DEFAULT_GOOGLE_EMBEDDING_MODEL,
                "role": "recommended",
                "quality_tier": "credential_peer",
                "recommended_when": "GEMINI_API_KEY already present (bring-your-key); not Frigg's preferred cloud default over OpenAI",
                "offline": false,
                "native_dimensions": GOOGLE_DEFAULT_NATIVE_DIMENSIONS,
                "pad_to_projection": GOOGLE_DEFAULT_NATIVE_DIMENSIONS < DEFAULT_VECTOR_DIMENSIONS,
                "credential_env": GEMINI_API_KEY_ENV_VAR,
                "reindex_on_change": true,
                "quality": "curated",
                "known_limits": [
                    "Credential-ecosystem peer: use when GEMINI_API_KEY is already present — not Frigg's preferred cloud default over OpenAI",
                    "Requires GEMINI_API_KEY and network",
                    "native_dimensions is the width Frigg requests (output_dimensionality), not a padded value",
                    "API catalog default may be 3072; Frigg requests native_dimensions — no storage pad when equal to projection",
                    "OpenAI-only shops need not configure Google; multi-key is never required"
                ]
            },
            {
                "id": "openai-compat-protocol",
                "provider": "openai_compat",
                "model": DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL,
                "role": "experimental",
                "quality_tier": "selfhost_protocol",
                "offline": false,
                "native_dimensions": OPENAI_DEFAULT_NATIVE_DIMENSIONS,
                "pad_to_projection": OPENAI_DEFAULT_NATIVE_DIMENSIONS < DEFAULT_VECTOR_DIMENSIONS,
                "credential_env": OPENAI_COMPAT_API_KEY_ENV_VAR,
                "endpoint_env": OPENAI_COMPAT_ENDPOINT_ENV_VAR,
                "reindex_on_change": true,
                "quality": "curated",
                "known_limits": [
                    "Requires FRIGG_SEMANTIC_RUNTIME_OPENAI_COMPAT_ENDPOINT (full embeddings POST URL)",
                    "Requires FRIGG_OPENAI_COMPAT_API_KEY (or OPENAI_API_KEY fallback) as Bearer token",
                    "Same OpenAI HTTP wire format; backend quality/dims are operator-owned",
                    "Storage partition is provider=openai_compat + model string — not openai",
                    "Set FRIGG_SEMANTIC_RUNTIME_MODEL to the backend model id when it is not text-embedding-3-small"
                ]
            }
        ],
        "presets": [
            {
                "id": "offline-small",
                "intent": "Zero-cloud offline_smoke MiniLM when you enable semantic without API keys",
                "provider": "local",
                "model": DEFAULT_LOCAL_EMBEDDING_MODEL,
                "model_id": "local-minilm-l6-v2",
                "quality": "curated",
                "quality_tier": "offline_smoke",
                "cli_alias": false,
                "expands_to": {
                    "FRIGG_SEMANTIC_RUNTIME_ENABLED": "true",
                    "FRIGG_SEMANTIC_RUNTIME_PROVIDER": "local",
                    "FRIGG_SEMANTIC_RUNTIME_MODEL": DEFAULT_LOCAL_EMBEDDING_MODEL
                },
                "required_credential_env": null,
                "storage_keys": {
                    "provider": "local",
                    "model": DEFAULT_LOCAL_EMBEDDING_MODEL,
                    "note": "Partition identity is always provider+model strings — never the preset id alone"
                },
                "failure_modes": [
                    "Semantic still off by product default until FRIGG_SEMANTIC_RUNTIME_ENABLED=true",
                    "Local model prepare failure at startup (cache / HF artifacts)",
                    "Product-phrase → API mapping can be weak; still pivot hybrid to search_text / search_symbol",
                    "semantic_status ok under MiniLM means vectors ran — still exact-pivot for proof",
                    "Changing model later requires frigg index semantic pass (reindex_on_change)"
                ]
            },
            {
                "id": "cloud-openai",
                "intent": "Cloud OpenAI embeddings when OPENAI_API_KEY is available",
                "provider": "openai",
                "model": DEFAULT_OPENAI_EMBEDDING_MODEL,
                "model_id": "openai-text-embedding-3-small",
                "quality": "curated",
                "quality_tier": "cloud",
                "cli_alias": false,
                "expands_to": {
                    "FRIGG_SEMANTIC_RUNTIME_ENABLED": "true",
                    "FRIGG_SEMANTIC_RUNTIME_PROVIDER": "openai",
                    "FRIGG_SEMANTIC_RUNTIME_MODEL": DEFAULT_OPENAI_EMBEDDING_MODEL
                },
                "required_credential_env": OPENAI_API_KEY_ENV_VAR,
                "storage_keys": {
                    "provider": "openai",
                    "model": DEFAULT_OPENAI_EMBEDDING_MODEL,
                    "note": "Partition identity is always provider+model strings — never the preset id alone"
                },
                "failure_modes": [
                    "Missing or empty OPENAI_API_KEY (fail-fast at semantic startup)",
                    "Network / provider outage → semantic degraded; lexical/graph still work",
                    "Cloud embeddings leave the machine (use offline-small for zero-cloud)",
                    "Changing model later requires frigg index semantic pass (reindex_on_change)"
                ]
            },
            {
                "id": "cloud-google",
                "intent": "Cloud Google embeddings when GEMINI_API_KEY is already present (credential peer, not preferred cloud default)",
                "provider": "google",
                "model": DEFAULT_GOOGLE_EMBEDDING_MODEL,
                "model_id": "google-gemini-embedding-001",
                "quality": "curated",
                "quality_tier": "credential_peer",
                "cli_alias": false,
                "expands_to": {
                    "FRIGG_SEMANTIC_RUNTIME_ENABLED": "true",
                    "FRIGG_SEMANTIC_RUNTIME_PROVIDER": "google",
                    "FRIGG_SEMANTIC_RUNTIME_MODEL": DEFAULT_GOOGLE_EMBEDDING_MODEL
                },
                "required_credential_env": GEMINI_API_KEY_ENV_VAR,
                "storage_keys": {
                    "provider": "google",
                    "model": DEFAULT_GOOGLE_EMBEDDING_MODEL,
                    "note": "Partition identity is always provider+model strings — never the preset id alone"
                },
                "failure_modes": [
                    "Missing or empty GEMINI_API_KEY (fail-fast at semantic startup)",
                    "Network / provider outage → semantic degraded; lexical/graph still work",
                    "Frigg requests output_dimensionality = projection_dimensions for this model",
                    "Credential peer only — not Frigg's preferred cloud default over OpenAI",
                    "Changing model later requires frigg index semantic pass (reindex_on_change)"
                ]
            },
            {
                "id": "openai-compat-selfhost",
                "intent": "OpenAI-protocol embeddings at a configured endpoint (vLLM, LM Studio, Azure-compatible, gateways)",
                "provider": "openai_compat",
                "model": DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL,
                "model_id": "openai-compat-protocol",
                "quality": "curated",
                "cli_alias": false,
                "expands_to": {
                    "FRIGG_SEMANTIC_RUNTIME_ENABLED": "true",
                    "FRIGG_SEMANTIC_RUNTIME_PROVIDER": "openai_compat",
                    "FRIGG_SEMANTIC_RUNTIME_MODEL": DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL,
                    "FRIGG_SEMANTIC_RUNTIME_OPENAI_COMPAT_ENDPOINT": "<full embeddings POST URL>"
                },
                "required_credential_env": OPENAI_COMPAT_API_KEY_ENV_VAR,
                "required_endpoint_env": OPENAI_COMPAT_ENDPOINT_ENV_VAR,
                "storage_keys": {
                    "provider": "openai_compat",
                    "model": DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL,
                    "note": "Partition identity is always provider+model strings — never the preset id alone; endpoint is not part of the storage key"
                },
                "failure_modes": [
                    "Missing/blank/invalid openai_compat endpoint URL (fail-fast at semantic startup)",
                    "Missing FRIGG_OPENAI_COMPAT_API_KEY (OPENAI_API_KEY is accepted as fallback Bearer)",
                    "Backend model id mismatch — set FRIGG_SEMANTIC_RUNTIME_MODEL explicitly",
                    "Native vector width may differ from 1536 — pad/reject follows store contract; reindex on model change"
                ]
            }
        ],
        "presets_note": "Soft intent aliases over models[] (EXP-code-presets C + openai_compat self-host). Not CLI flags (B deferred). Not brand embedding vendors (Voyage/Cohere deferred). Not auto local-vs-cloud by key presence (E rejected). Set provider+model (+ endpoint for openai_compat) env/config explicitly; preset id is documentation only.",
        "guidance": [
            "Semantic is optional acceleration — never the sole grounding layer for code claims",
            "Local MiniLM is offline_smoke (zero-key general embedder); prefer exact tools after hybrid",
            "Google Gemini is a credential_peer when GEMINI_API_KEY exists — not Frigg's preferred cloud default over OpenAI",
            "After hybrid, pivot to exact search_text / search_symbol before answering",
            "After changing provider or model (or embed template upgrades), run frigg index for a semantic pass",
            "Do not invent unlisted providers or treat preset id as a storage partition key",
            "Prefer presets for intent (offline smoke vs cloud peer keys vs openai_compat); always apply expands_to provider+model strings",
            "openai_compat requires a full embeddings POST URL; storage partition is openai_compat+model, not openai"
        ]
    }))
    .expect("semantic models JSON should serialize")
}

/// JSON Schema-shaped policy resource for agent-assembled evidence packets (EXP-evidence-packet A+D).
///
/// Composition stays in the skill; this resource is machine documentation only — not an MCP tool.
fn evidence_packet_json() -> String {
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.evidence_packet.v1",
        "live": true,
        "product_role": "skill_composition_template",
        "not_a_tool": true,
        "not_a_tool_note": "There is no compose_evidence_packet (or similar) public MCP tool. Agents assemble multi-claim packets from search/nav/read witnesses using the skill Technical review / security cards. Rust types EvidencePacket / EvidencePacketClaim mirror this shape for hosts.",
        "source_of_truth": {
            "skill": "skills/frigg-first-code-search/SKILL.md (Technical review evidence packet)",
            "types": "crates/cli/src/mcp/types/navigation.rs::EvidencePacketClaim / EvidencePacket"
        },
        "claim_fields": {
            "claim": { "type": "string", "required": true, "description": "Human claim text" },
            "tool": { "type": "string", "required": true, "description": "Frigg tool that produced the witness" },
            "path": { "type": "string", "required": true, "description": "Repository-relative path" },
            "start_line": { "type": "integer", "required": true, "minimum": 1 },
            "end_line": { "type": "integer", "required": true, "minimum": 1 },
            "match_id": { "type": "string", "required": false, "description": "Scoped match_id from the same call as result_handle" },
            "result_handle": { "type": "string", "required": false, "description": "Session result_handle from the same call" }
        },
        "envelope": {
            "claims": { "type": "array", "items": "claim", "minItems": 1 }
        },
        "example": {
            "claims": [
                {
                    "claim": "catalog_entries registers callable operations",
                    "tool": "search_symbol",
                    "path": "src/catalog/mod.rs",
                    "start_line": 40,
                    "end_line": 72,
                    "match_id": "symbols:m1",
                    "result_handle": "..."
                }
            ]
        },
        "guidance": [
            "Assemble packets after Frigg search/nav/read; do not invent path/line witnesses.",
            "Prefer citation presentation_mode for user-facing fences; packets are multi-claim internal/report structure.",
            "match_id is valid only with its own result_handle from the same tool call.",
            "Do not treat packet assembly as a server-sealed authority; nav mode (heuristic vs precise) still applies."
        ]
    }))
    .expect("evidence packet policy JSON should serialize")
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
        "Product tools including explore are on core. Playbook tools are compile-time opt-in (`--features playbook`) and extended-profile only — not default cargo features. Set FRIGG_MCP_TOOL_SURFACE_PROFILE=core to hide playbook tools even when the binary was built with the playbook feature."
    } else {
        "Product tools including explore are on core. Playbook tools require building with `--features playbook` and the extended profile; they are not on default builds."
    };
    serde_json::to_string_pretty(&json!({
        "schema_id": "frigg.policy.tool_surface.v1",
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
        "active_tools": active.tool_names,
        "guidance": [
            "This resource is the machine-readable live tool surface. Prefer it (or tools/list / workspace.runtime.tools_exposed) over Phase 0 / systems inventory freezes.",
            "Use Frigg as the default for code discovery, file listing, navigation, exact code search, and bounded source reads.",
            "Use workspace for compact workspace status or to adopt a target path/repository; repo-aware tools auto-adopt sensible defaults when possible.",
            "Workspace freshness is authoritative: clean snapshot=ready is usable on HTTP and stdio; wait_for_refresh occurs only for leased debouncing/refreshing dirty work. mode_off, no_lease, retry_backoff, blocked, and notify_degraded cannot converge by waiting, so use live disk for touched paths. Missing/uninitialized/error snapshots require CLI/operator frigg index; there is no public reindex/write tool. Use exact next_actions when present; legacy gate fields remain compatibility projections for two minor releases.",
            "Before shell rg/grep/find/fd/cat/sed for code exploration, use list_files, search_text, search_symbol, search_batch, search_hybrid, read_file, read_match, impact_bundle, or navigation tools.",
            "Use search_hybrid only for broad discovery-style repository questions; use search_text for rg-shaped literal or safe-regex code scans, including grouped alternation and path_regex narrowing; pass the search term as query, not pattern; use search_symbol for known identifiers; use search_batch for multi-hypothesis guesses (2..=8 independent concurrent probes, then fixed consensus-first reciprocal_rank_fusion); inspect evidence/completeness and replay its opaque continuation only unchanged; use impact_bundle with a copied target_ref for usages/callers/blast radius; use list_files for rg --files-shaped listing.",
            "Use shell tools as the exception for git state and diffs, non-code files, build/test output, generated or unindexed files, explicit live-disk verification, and unavailable Frigg results.",
            "Use Frigg when repository-aware evidence, symbols, navigation, provenance, or multi-repo context matter.",
            "Read surfaces are text-first by default: read_file, read_match, and explore(operation=zoom). Request presentation_mode=json when a downstream consumer needs the structured compatibility payload.",
            "next_actions[].tool plus exact next_actions[].arguments is authoritative for follow-ups; execute the named existing MCP tool yourself, respecting role/order/dependencies and host authorization. Compact and full carry identical action data. suggested_next is deprecated and lossy; stale or mixed proof handles require rerunning the typed origin producer and choosing a fresh match_id. No generic executor or automatic chaining endpoint exists.",
            "For navigation and impact, prefer search -> copy a row's target_ref unchanged -> pass it as target to the navigation tool or impact_bundle. result_match {result_handle, match_id, target_scope} is session/source scoped; target_scope is opaque correlation, not authentication. stable_symbol {repository_id, stable_symbol_id, snapshot_token} is repository/corpus scoped and never crosses repositories. On TARGET_SCOPE_MISMATCH, STALE_TARGET_SNAPSHOT, stale handle/proof, or TARGET_NOT_FOUND, rerun the producer and use a fresh target; Frigg does not navigate historical source. Direct symbol/location fields remain compatibility input, but cannot be mixed with target; a matching repository_id is only an assertion. Ambiguous legacy impact does not run children; read sections for execution/trust/completeness, use section proof_targets with next_actions, opt into test evidence with include_test_mentions, and keep outgoing calls separate.",
            "Use include_follow_up_structural=true when you want replayable search_structural follow-ups from inspect_syntax_tree, search_structural, or anchored navigation and outline results.",
            core_guidance
        ]
    }))
    .expect("tool surface JSON should serialize")
}

fn shell_vs_frigg_markdown(active_profile: ToolSurfaceProfile) -> String {
    let explore_guidance = "`explore` is on the core product surface for bounded single-artifact follow-up after discovery. `explore(operation=zoom)` defaults to the same text-first read rendering as `read_file` and `read_match`, while `probe` and `refine` stay structured by default.";
    let playbook_guidance = if cfg!(feature = "playbook") {
        if active_profile == ToolSurfaceProfile::Extended {
            "Playbook tools (`playbook_run`, `playbook_replay`, `playbook_compose_citations`) are present on this extended build — they are trace/dev tooling, not first-line discovery."
        } else {
            "Playbook tools are compiled in but hidden on the `core` profile; set FRIGG_MCP_TOOL_SURFACE_PROFILE=extended only for explicit playbook workflows."
        }
    } else {
        "Playbook tools are not compiled into this binary (build with `--features playbook` only for explicit trace/dev workflows)."
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
    ## Workspace freshness\n\n\
    `workspace.freshness` is authoritative. A clean `snapshot=ready` works on HTTP and stdio; do not wait merely because watch is off. For dirty known paths, use `wait_for_refresh` only when a leased watch is `debouncing` or `refreshing`. `mode_off`, `no_lease`, `retry_backoff`, `blocked`, and `notify_degraded` cannot converge by waiting: direct-read touched paths, or use HTTP/operator recovery. Missing, uninitialized, or error snapshots require CLI `frigg index`; this is not a public MCP write/reindex tool. Follow exact `next_actions` when present; legacy gate fields are compatibility projections retained for two minor releases. After a touched edit, rerun a `read_match` producer before proof.\n\n\
    Shell replacement map:\n\
    - `rg --files` -> `list_files`\n\
    - `rg -n \"text\"` -> `search_text`\n\
    - `rg -n \"foo|bar\"` -> `search_text` with regex mode\n\
    - `rg -n \"text\" path/` -> `search_text` with `path_regex`\n\
    - identifier/API/type/class/function lookup -> `search_symbol`\n\
    - parallel multi-grep / multi-hypothesis probes -> `search_batch` (2..=8 independent concurrent probes, then consensus-first fixed-RRF merge/dedupe; not one shared multi-query walk)\n\
    - usages / callers / blast radius for a known symbol -> prefer `impact_bundle(target)` with a copied target_ref before sequential navigation tools\n\
    - `cat path` -> `read_file`\n\
    - `sed -n '10,80p' path` -> `read_file` with `start_line`, `end_line`, or `line_count`\n\
    - follow definitions/references/calls -> navigation tools (or `impact_bundle` when the symbol is already known)\n\n\
    Use `search_hybrid` only for broad discovery-style repository questions when there is no stable string, symbol, or path anchor yet. Use `search_text` for `rg`-shaped literal or safe-regex scans, including grouped alternation, `path_regex` narrowing, context windows, per-file limits (`max_count_per_file`), and file-containment probes (`files_with_matches`). For `search_text`, pass the search term as `query`, not `pattern`. Frigg may execute those scans with its native scanner, its ripgrep accelerator, or a mixed path while preserving repository-scoped results and result handles. Use `search_symbol` for known identifiers. Use `search_batch` when you would fire several Frigg probes in one turn (text/symbol/hybrid); each probe is a full independent search, then results merge by consensus, fixed equal-weight reciprocal-rank fusion, and derived strength. Read per-row `evidence`, `consensus_count`, `rrf_score`, and `match_strength`, plus per-probe and aggregate `completeness`; `merge_strategy` is fixed to `reciprocal_rank_fusion`, and legacy `merge=rank_by_probe_hit_strength` is a two-minor compatibility input only. Replay its opaque `continuation` only with the same normalized probes, scopes, and snapshots. Prefer `impact_bundle` for impact/refactor questions with a copied target_ref before chaining `find_references` / `incoming_calls` / `find_implementations` by hand.\n\n\
    `read_file` and `read_match` default to text-first output. Ask for `presentation_mode=json` when a caller needs the structured compatibility payload with explicit `content`, and apply the same rule to `explore(operation=zoom)`.\n\n\
    Structural follow-up suggestions are opt-in. Use `include_follow_up_structural=true` on `inspect_syntax_tree`, `search_structural`, or anchored navigation and outline tools when you want replayable `search_structural` follow-ups derived from the resolved AST focus.\n\n\
    For exact navigation, use search -> copy a row's `target_ref` unchanged -> pass it as `target` to navigation or `impact_bundle`. A result-match target has `kind=result_match`, `result_handle`, `match_id`, and `target_scope`, and is session/source scoped; `target_scope` is opaque correlation, not authentication. A stable-symbol target has `kind=stable_symbol`, `repository_id`, `stable_symbol_id`, and `snapshot_token`, and is repository/corpus scoped. Do not combine `target` with direct symbol/location fields (`CONFLICTING_TARGET_INPUT`); a matching top-level `repository_id` is an assertion, and a mismatch returns `TARGET_REPOSITORY_MISMATCH`. On `TARGET_SCOPE_MISMATCH`, `STALE_TARGET_SNAPSHOT`, stale handle/proof, or `TARGET_NOT_FOUND`, rerun the producer and copy a fresh target; Frigg does not navigate historical source. `impact_bundle` accepts exactly one of `target` or legacy non-empty `symbol`; target mode resolves once for all child work, while legacy same-rank symbol results surface disambiguation and run no child section. Its authoritative `sections[]` keeps execution, trust, and completeness separate; use section-qualified `proof_targets` with canonical `next_actions`, opt into exact test evidence with `include_test_mentions=true`, and keep outgoing calls as a separate provisional tool.\n\n\
    Semantic retrieval remains an optional accelerator, not the grounding layer.\n\n\
    {explore_guidance}\n\
    {playbook_guidance}\n"
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
        Resource::new(
            EVIDENCE_PACKET_RESOURCE_URI,
            "FRIGG Evidence Packet Schema",
        )
        .with_description(
            "Skill-composed multi-claim evidence packet shape (schema only; not a callable MCP tool).",
        )
        .with_mime_type("application/json"),
        Resource::new(
            SEMANTIC_MODELS_RESOURCE_URI,
            "FRIGG Semantic Models Catalog",
        )
        .with_description(
            "Curated embedding-model defaults, soft intent presets (offline-small / cloud-*), and contract facts (dims, pad, offline, credentials). No retained public leaderboard; peer to support-matrix. Presets are not CLI flags.",
        )
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
        EVIDENCE_PACKET_RESOURCE_URI => (evidence_packet_json(), "application/json"),
        SEMANTIC_MODELS_RESOURCE_URI => (semantic_models_json(), "application/json"),
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
2. Use `search_hybrid` only for broad discovery-style repository questions when there is no stable string, symbol, or path anchor yet; use `search_text` for `rg`-shaped literal or safe-regex scans, including grouped alternation and `path_regex`; use `search_symbol` for known identifiers; use `search_batch` for multi-hypothesis guesses (2..=8 independent concurrent probes, then consensus-first fixed reciprocal-rank fusion); inspect evidence and per-probe/aggregate completeness, and replay its opaque `continuation` only unchanged; prefer `impact_bundle(target)` with a copied `target_ref` for usages/callers/blast radius before sequential navigation.\n\
3. Use shell tools as the exception for non-code files, git/filesystem inspection, explicit live-disk verification, trivial one-offs, or ripgrep-specific flags outside `search_text`.\n\
4. Prefer Frigg core tools when repository-aware evidence, symbols, navigation, provenance, or multi-repo context matter.\n\
5. Treat semantic retrieval as optional acceleration only. When broad discovery is weak, pivot to lexical, graph, and source-witness evidence instead of diagnosing runtime state by default.\n\
6. Treat the current supported-language set as one public list: Rust, PHP, Blade, TypeScript / TSX, Python, Go, Kotlin / KTS, Java, Lua, Roc, and Nim. Describe differences in concrete capability terms, not first-class or baseline badges.\n\
7. `read_file` and `read_match` default to text-first output; request `presentation_mode=json` only when the caller truly needs the structured compatibility payload. In the extended profile, `explore(operation=zoom)` follows the same text-first default, while `probe` and `refine` stay structured.\n\
8. Use `include_follow_up_structural=true` when you want replayable `search_structural` follow-ups from `inspect_syntax_tree`, `search_structural`, or anchored navigation and outline results.\n\
9. Use `explore` only after discovery and only when the active profile includes it.\n\
10. For navigation/impact, prefer search -> copied `target_ref` -> `target`; result-match targets are session/source scoped and stable-symbol targets are repository/corpus scoped. Target scope is not authentication. Recover `TARGET_SCOPE_MISMATCH`, `STALE_TARGET_SNAPSHOT`, stale proof/handles, or `TARGET_NOT_FOUND` by rerunning the producer for a fresh target. Direct inputs remain compatible but cannot be mixed with `target`; `impact_bundle` accepts target or legacy symbol and resolves supplied targets once. Ambiguous legacy symbols run no child section. Read `sections[]` for independent execution, trust, and completeness; use section-qualified `proof_targets` with canonical `next_actions`; opt into test evidence with `include_test_mentions=true`; keep outgoing calls separate/provisional.\n\n",
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
                Resource::new(
                    SEMANTIC_MODELS_RESOURCE_URI,
                    "FRIGG Semantic Models Catalog",
                ),
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
        EVIDENCE_PACKET_RESOURCE_URI, LOCAL_DEFAULT_NATIVE_DIMENSIONS, ROUTING_GUIDE_PROMPT_NAME,
        SEMANTIC_MODELS_RESOURCE_URI, SHELL_GUIDANCE_RESOURCE_URI,
        SHELL_REPLACEMENT_MAP_RESOURCE_URI, SUPPORT_MATRIX_RESOURCE_URI, TOOL_SURFACE_RESOURCE_URI,
        policy_resources, read_guidance_prompt, read_policy_resource,
    };
    use crate::languages::{LanguageSupportCapability, SymbolLanguage};
    use crate::mcp::tool_surface::ToolSurfaceProfile;
    use crate::settings::{
        DEFAULT_GOOGLE_EMBEDDING_MODEL, DEFAULT_LOCAL_EMBEDDING_MODEL,
        DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL, DEFAULT_OPENAI_EMBEDDING_MODEL,
        GEMINI_API_KEY_ENV_VAR, OPENAI_API_KEY_ENV_VAR, OPENAI_COMPAT_API_KEY_ENV_VAR,
        OPENAI_COMPAT_ENDPOINT_ENV_VAR,
    };
    use crate::storage::DEFAULT_VECTOR_DIMENSIONS;

    const _: () = assert!(LOCAL_DEFAULT_NATIVE_DIMENSIONS < DEFAULT_VECTOR_DIMENSIONS);
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
    fn semantic_models_catalog_matches_runtime_defaults_and_stays_curated() {
        assert!(
            policy_resources()
                .iter()
                .any(|resource| resource.uri == SEMANTIC_MODELS_RESOURCE_URI),
            "policy_resources should list semantic-models"
        );

        let json = resource_text(SEMANTIC_MODELS_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("semantic models JSON should parse");
        assert_eq!(
            parsed["schema_id"],
            json!("frigg.policy.semantic_models.v1")
        );
        assert_eq!(
            parsed["projection_dimensions"],
            json!(DEFAULT_VECTOR_DIMENSIONS)
        );
        assert_eq!(parsed["quality_scores"], json!("curated"));
        assert_eq!(parsed["semantic_default"]["enabled"], json!(false));
        assert_eq!(parsed["reindex_on_change"], json!(true));

        let models = parsed["models"].as_array().expect("models array").to_vec();
        assert_eq!(
            models.len(),
            4,
            "curated defaults + openai_compat protocol — not a brand model zoo"
        );
        let models_by_id: std::collections::BTreeMap<&str, &Value> = models
            .iter()
            .filter_map(|row| row["id"].as_str().map(|id| (id, row)))
            .collect();

        let local = models
            .iter()
            .find(|row| row["provider"] == "local")
            .expect("local row");
        assert_eq!(local["model"], json!(DEFAULT_LOCAL_EMBEDDING_MODEL));
        assert_eq!(local["role"], json!("default"));
        assert_eq!(local["offline"], json!(true));
        assert_eq!(
            local["native_dimensions"],
            json!(LOCAL_DEFAULT_NATIVE_DIMENSIONS),
            "local native_dimensions must be REAL MiniLM width (384), not padded 1536"
        );
        assert_eq!(local["pad_to_projection"], json!(true));
        assert!(local["credential_env"].is_null());
        assert_eq!(local["quality"], json!("curated"));
        assert_eq!(
            local["quality_tier"],
            json!("offline_smoke"),
            "MiniLM is offline smoke / zero-key general embedder"
        );
        let local_limits = local["known_limits"]
            .as_array()
            .expect("local known_limits");
        assert!(
            local_limits.iter().any(|limit| {
                limit.as_str().is_some_and(|text| {
                    text.contains("Offline smoke") || text.contains("not a code-specialized")
                })
            }),
            "local known_limits must state MiniLM offline-smoke / general-embedder role"
        );
        assert!(
            local.get("dimensions").is_none() || local["dimensions"] == local["native_dimensions"]
        );
        assert!(local.get("stored_dimensions").is_none());

        let openai = models
            .iter()
            .find(|row| row["provider"] == "openai")
            .expect("openai row");
        assert_eq!(openai["model"], json!(DEFAULT_OPENAI_EMBEDDING_MODEL));
        assert_eq!(openai["credential_env"], json!(OPENAI_API_KEY_ENV_VAR));
        assert_eq!(
            openai["native_dimensions"],
            json!(DEFAULT_VECTOR_DIMENSIONS)
        );
        assert_eq!(
            openai["pad_to_projection"],
            json!(false),
            "OpenAI default matches projection; no pad"
        );
        assert_eq!(openai["offline"], json!(false));
        assert_eq!(openai["quality"], json!("curated"));

        let google = models
            .iter()
            .find(|row| row["provider"] == "google")
            .expect("google row");
        assert_eq!(google["model"], json!(DEFAULT_GOOGLE_EMBEDDING_MODEL));
        assert_eq!(google["credential_env"], json!(GEMINI_API_KEY_ENV_VAR));
        assert_eq!(
            google["native_dimensions"],
            json!(DEFAULT_VECTOR_DIMENSIONS),
            "Google native_dimensions is Frigg-requested REAL width, not a padded value"
        );
        assert_eq!(
            google["pad_to_projection"],
            json!(false),
            "Google path requests full projection width; storage pad unused on happy path"
        );
        assert_eq!(google["quality"], json!("curated"));
        assert_eq!(
            google["quality_tier"],
            json!("credential_peer"),
            "Gemini is credential ecosystem peer, not Frigg preferred cloud default"
        );
        assert!(
            google["recommended_when"]
                .as_str()
                .is_some_and(|text| text.contains("GEMINI_API_KEY")),
            "google recommended_when must mention GEMINI_API_KEY bring-your-key"
        );
        let google_limits = google["known_limits"]
            .as_array()
            .expect("google known_limits");
        assert!(
            google_limits.iter().any(|limit| {
                limit.as_str().is_some_and(|text| {
                    text.contains("Credential-ecosystem peer") || text.contains("credential peer")
                })
            }),
            "google known_limits must state credential-peer positioning"
        );
        assert_eq!(openai["quality_tier"], json!("cloud"));

        let openai_compat = models
            .iter()
            .find(|row| row["provider"] == "openai_compat")
            .expect("openai_compat row");
        assert_eq!(
            openai_compat["model"],
            json!(DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL)
        );
        assert_eq!(
            openai_compat["credential_env"],
            json!(OPENAI_COMPAT_API_KEY_ENV_VAR)
        );
        assert_eq!(
            openai_compat["endpoint_env"],
            json!(OPENAI_COMPAT_ENDPOINT_ENV_VAR)
        );
        assert_eq!(openai_compat["role"], json!("experimental"));
        assert_eq!(openai_compat["quality"], json!("curated"));

        for row in &models {
            assert!(row.get("score").is_none());
            assert!(row.get("benchmark_score").is_none());
            assert!(row.get("leaderboard_rank").is_none());
            let native = row["native_dimensions"]
                .as_u64()
                .expect("native_dimensions");
            let pad = row["pad_to_projection"]
                .as_bool()
                .expect("pad_to_projection");
            if pad {
                assert!(
                    (native as usize) < DEFAULT_VECTOR_DIMENSIONS,
                    "when pad_to_projection, native_dimensions must be REAL pre-pad width < projection"
                );
            }
            assert!(
                row.get("stored_dimensions").is_none(),
                "do not report padded store length on model rows"
            );
        }
        assert_eq!(
            parsed["dimensions_contract"]["model_field"],
            json!("native_dimensions")
        );

        let presets = parsed["presets"]
            .as_array()
            .expect("presets array (EXP-code-presets C)");
        assert_eq!(
            presets.len(),
            4,
            "soft presets over existing models + openai_compat self-host — not a brand zoo"
        );
        assert!(
            parsed["presets_note"]
                .as_str()
                .is_some_and(|note| note.contains("CLI") && note.contains("deferred")),
            "presets_note should state CLI aliases deferred"
        );

        let expected_presets = [
            (
                "offline-small",
                "local",
                DEFAULT_LOCAL_EMBEDDING_MODEL,
                "local-minilm-l6-v2",
            ),
            (
                "cloud-openai",
                "openai",
                DEFAULT_OPENAI_EMBEDDING_MODEL,
                "openai-text-embedding-3-small",
            ),
            (
                "cloud-google",
                "google",
                DEFAULT_GOOGLE_EMBEDDING_MODEL,
                "google-gemini-embedding-001",
            ),
            (
                "openai-compat-selfhost",
                "openai_compat",
                DEFAULT_OPENAI_COMPAT_EMBEDDING_MODEL,
                "openai-compat-protocol",
            ),
        ];
        for (id, provider, model, model_id) in expected_presets {
            let preset = presets
                .iter()
                .find(|row| row["id"] == id)
                .expect("expected semantic preset must exist");
            assert_eq!(preset["provider"], json!(provider));
            assert_eq!(preset["model"], json!(model));
            assert_eq!(preset["model_id"], json!(model_id));
            assert_eq!(preset["quality"], json!("curated"));
            assert_eq!(
                preset["cli_alias"],
                json!(false),
                "preset {id} is documentation only — not a CLI flag (B deferred)"
            );
            assert_eq!(preset["storage_keys"]["provider"], json!(provider));
            assert_eq!(preset["storage_keys"]["model"], json!(model));
            assert_eq!(
                preset["expands_to"]["FRIGG_SEMANTIC_RUNTIME_PROVIDER"],
                json!(provider)
            );
            assert_eq!(
                preset["expands_to"]["FRIGG_SEMANTIC_RUNTIME_MODEL"],
                json!(model)
            );
            assert_eq!(
                preset["expands_to"]["FRIGG_SEMANTIC_RUNTIME_ENABLED"],
                json!("true")
            );
            assert!(
                preset["expands_to"].get(OPENAI_API_KEY_ENV_VAR).is_none()
                    && preset["expands_to"].get(GEMINI_API_KEY_ENV_VAR).is_none(),
                "preset {id} must not put credential values in expands_to"
            );
            let resolved = models_by_id
                .get(model_id)
                .expect("semantic preset model_id must resolve to models[]");
            assert_eq!(
                resolved["provider"],
                json!(provider),
                "preset {id} provider must match models[]"
            );
            assert_eq!(
                resolved["model"],
                json!(model),
                "preset {id} model must match models[]"
            );
            assert_eq!(
                preset["required_credential_env"], resolved["credential_env"],
                "preset {id} credential env must match models[]"
            );
            assert!(
                preset["failure_modes"]
                    .as_array()
                    .is_some_and(|modes| !modes.is_empty()),
                "preset {id} needs failure_modes"
            );
            assert!(preset.get("score").is_none());
            assert!(preset.get("benchmark_score").is_none());
        }
        let offline = presets
            .iter()
            .find(|row| row["id"] == "offline-small")
            .expect("offline-small");
        assert!(offline["required_credential_env"].is_null());
        assert_eq!(
            offline["quality_tier"],
            json!("offline_smoke"),
            "offline-small preset must carry MiniLM offline_smoke tier"
        );
        let cloud_google = presets
            .iter()
            .find(|row| row["id"] == "cloud-google")
            .expect("cloud-google");
        assert_eq!(
            cloud_google["quality_tier"],
            json!("credential_peer"),
            "cloud-google preset must carry Gemini credential_peer tier"
        );
        assert_eq!(
            cloud_google["required_credential_env"],
            json!(GEMINI_API_KEY_ENV_VAR)
        );

        #[cfg(feature = "local-embeddings")]
        {
            use crate::embeddings::local_model::DEFAULT_LOCAL_MODEL_ALIAS;
            assert_eq!(
                LOCAL_DEFAULT_NATIVE_DIMENSIONS, DEFAULT_LOCAL_MODEL_ALIAS.dimensions,
                "scoreboard local dims must match DEFAULT_LOCAL_MODEL_ALIAS"
            );
            assert_eq!(
                DEFAULT_LOCAL_EMBEDDING_MODEL,
                DEFAULT_LOCAL_MODEL_ALIAS.semantic_model
            );
        }
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
    fn tool_surface_policy_lists_explore_on_core_not_extended_only() {
        let json = resource_text(TOOL_SURFACE_RESOURCE_URI, ToolSurfaceProfile::Extended);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("tool surface policy JSON should parse");
        assert!(
            parsed["core_tools"]
                .as_array()
                .expect("core_tools should be an array")
                .iter()
                .any(|entry| entry == "explore"),
            "explore is product tooling and belongs on core"
        );
        assert!(
            !parsed["extended_only_tools"]
                .as_array()
                .expect("extended_only_tools should be an array")
                .iter()
                .any(|entry| entry == "explore"),
            "explore must not be extended-only"
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
            parsed["active_tools"]
                .as_array()
                .expect("active_tools")
                .iter()
                .any(|entry| entry == "explore"),
            "core active_tools must include product explore tool"
        );
        assert!(
            !parsed["active_tools"]
                .as_array()
                .expect("active_tools")
                .iter()
                .any(|entry| entry.as_str().is_some_and(|s| s.starts_with("playbook_"))),
            "core active_tools must omit playbook tools"
        );
    }

    #[test]
    fn evidence_packet_policy_is_schema_only_not_a_tool() {
        let json = resource_text(EVIDENCE_PACKET_RESOURCE_URI, ToolSurfaceProfile::Core);
        let parsed =
            serde_json::from_str::<Value>(&json).expect("evidence packet policy should parse");
        assert_eq!(
            parsed["schema_id"],
            json!("frigg.policy.evidence_packet.v1")
        );
        assert_eq!(parsed["not_a_tool"], json!(true));
        assert_eq!(parsed["product_role"], json!("skill_composition_template"));
        for field in [
            "claim",
            "tool",
            "path",
            "start_line",
            "end_line",
            "match_id",
            "result_handle",
        ] {
            assert!(
                parsed["claim_fields"][field].is_object(),
                "claim_fields.{field} must be documented"
            );
        }
        assert!(
            parsed["envelope"]["claims"].is_object(),
            "envelope.claims must be documented"
        );
        let example = parsed
            .get("example")
            .cloned()
            .expect("policy must include example packet");
        let packet: crate::mcp::types::EvidencePacket = serde_json::from_value(example)
            .expect("policy example must deserialize as EvidencePacket");
        assert!(!packet.claims.is_empty());
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
        assert_eq!(prompt.messages.len(), 6);
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
