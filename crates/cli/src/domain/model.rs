//! Repository identity and match payloads exchanged between indexing, search, and MCP tools.
//!
//! Holds stable repository ids, manifest match records, and structural follow-up shapes that stay
//! serde- and schema-stable across storage, retrieval, and MCP response envelopes.

use std::path::Path;

use blake3::Hasher;
use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable repository identifier derived from a workspace root path.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct RepositoryId(pub String);

impl RepositoryId {
    /// Builds the canonical repository id for a filesystem root.
    pub fn for_root(root: &Path) -> Self {
        stable_repository_id_for_root(root)
    }
}

/// Repository metadata surfaced to workspace and MCP callers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RepositoryRecord {
    pub repository_id: RepositoryId,
    pub display_name: String,
    pub root_path: String,
}

/// Opaque manifest snapshot identifier for a repository epoch.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct SnapshotId(pub String);

/// Lexical text hit with repository path coordinates and optional witness provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TextMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub excerpt: String,
    #[serde(skip, default)]
    pub witness_score_hint_millis: Option<u32>,
    #[serde(skip, default)]
    pub witness_provenance_ids: Option<Vec<String>>,
}

/// Planner strategy for a generated structural follow-up search.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedStructuralFollowUpStrategy {
    FocusNamedNodeFileScoped,
    FocusNamedNodeRepoScoped,
    AncestorNamedNodeRepoScoped,
}

/// Confidence tier attached to a generated structural follow-up suggestion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GeneratedStructuralFollowUpConfidence {
    High,
    Medium,
}

/// Parameters for a follow-up structural search emitted by navigation tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedStructuralSearchParams {
    pub query: String,
    pub language: String,
    pub repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_regex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

/// Structural focus metadata explaining why a follow-up search was suggested.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedStructuralFollowUpBasis {
    pub focus_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw_focus_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ancestor_kind: Option<String>,
}

/// Suggested structural search follow-up bundled with a reference match.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedStructuralFollowUp {
    pub strategy: GeneratedStructuralFollowUpStrategy,
    pub confidence: GeneratedStructuralFollowUpConfidence,
    pub basis: GeneratedStructuralFollowUpBasis,
    pub params: GeneratedStructuralSearchParams,
}

/// Symbol lookup hit with repository coordinates and optional container context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SymbolMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_symbol_id: Option<String>,
    pub repository_id: String,
    pub symbol: String,
    pub kind: String,
    pub path: String,
    pub line: usize,
    /// 1-based column when indexed (`FUT-011` navigation handoff).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    /// Short excerpt or signature snippet when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
    /// Path class (`runtime` / `project` / `support`) when classified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// Reference role for a symbol occurrence returned by navigation tools.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReferenceMatchKind {
    Definition,
    Declaration,
    Reference,
}

/// Symbol reference hit with precision metadata and optional structural follow-ups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReferenceMatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stable_symbol_id: Option<String>,
    pub repository_id: String,
    pub symbol: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub match_kind: ReferenceMatchKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub precision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fallback_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow_up_structural: Vec<GeneratedStructuralFollowUp>,
}

/// Record of a single MCP tool invocation used for provenance and replay.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolInvocation {
    pub tool_name: String,
    pub trace_id: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
}

/// Derives a stable slug-plus-hash repository id from a canonical workspace root.
pub fn stable_repository_id_for_root(root: &Path) -> RepositoryId {
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let display = canonical_root
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("repo");
    let slug = stable_repository_slug(display);
    let mut hasher = Hasher::new();
    hasher.update(canonical_root.to_string_lossy().as_bytes());
    let hash = hasher.finalize().to_hex().to_string();
    RepositoryId(format!("{slug}-{}", &hash[..12]))
}

fn stable_repository_slug(value: &str) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_dash = false;
        } else if !last_dash {
            slug.push('-');
            last_dash = true;
        }
    }
    let slug = slug.trim_matches('-');
    if slug.is_empty() {
        "repo".to_owned()
    } else {
        slug.to_owned()
    }
}
