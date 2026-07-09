//! Shared recovery composer types for empty and failed search/navigation/read paths (`FUT-016`).
//!
//! Recovery fields are designed for compact mode (top-level, optional, skip-when-empty) so agents
//! get actionable next steps without requesting `response_mode=full`. Builders cover the common
//! situations from `docs/futura.md` §15, including structured zero-hit diagnostics (`FUT-006`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Stable zero-hit reason codes for search/navigation empty results (`FUT-006` / 2.2).
///
/// Defined here so recovery builders and later zero-hit contracts share one serde-stable enum.
/// Values are snake_case on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ZeroHitReason {
    /// Indexed search finished; no match within the stated scope.
    IndexedSearchComplete,
    /// Session default or explicit repository may not be the intended repo.
    WrongRepositoryPossible,
    /// Requested path class is not covered by the index.
    PathClassNotIndexed,
    /// Filters (path_regex/glob/path_class/exclusions) eliminated all candidates.
    ScopeExcludedAllCandidates,
    /// Working tree may have changed since the last successful index.
    IndexStalePossible,
    /// Literal query contains regex metacharacters; retry as regex may help.
    QueryLooksLikeRegex,
    /// Requested tool is unavailable on this surface/profile/runtime.
    ToolUnavailable,
    /// Precise graph/SCIP data is not available for this navigation request.
    PreciseGraphUnavailable,
    /// No index coverage for the adopted repository or path set.
    NoIndexCoverage,
    /// Query simply did not match; no stronger diagnostic applies yet.
    QueryMiss,
}

/// One suggested follow-up tool invocation for recovery.
///
/// Optional fields are only set when relevant so compact payloads stay small.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct SuggestedNext {
    /// MCP tool name to call next (for example `search_text`, `workspace`).
    pub tool: String,
    /// Text or hybrid query payload when the next tool accepts `query`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// `search_text` / explore pattern mode (`literal` or `regex`) when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern_type: Option<String>,
    /// Repository-relative path regex scope.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_regex: Option<String>,
    /// Repository-relative include glob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    /// Path class filter (`runtime`, `project`, `support`) when applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_class: Option<String>,
    /// Symbol name for symbol/navigation tools.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Explicit repository scope when wrong-repo recovery applies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    /// Absolute or workspace path for `workspace` / `read_file` style next steps.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Result handle when re-binding reads after a handle failure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_handle: Option<String>,
    /// Human-readable why this step is the next action.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl SuggestedNext {
    /// Builds a minimal suggested-next row for `tool`.
    pub fn tool(tool: impl Into<String>) -> Self {
        Self {
            tool: tool.into(),
            ..Self::default()
        }
    }

    pub fn with_query(mut self, query: impl Into<String>) -> Self {
        self.query = Some(query.into());
        self
    }

    pub fn with_pattern_type(mut self, pattern_type: impl Into<String>) -> Self {
        self.pattern_type = Some(pattern_type.into());
        self
    }

    pub fn with_path_regex(mut self, path_regex: impl Into<String>) -> Self {
        self.path_regex = Some(path_regex.into());
        self
    }

    pub fn with_glob(mut self, glob: impl Into<String>) -> Self {
        self.glob = Some(glob.into());
        self
    }

    pub fn with_path_class(mut self, path_class: impl Into<String>) -> Self {
        self.path_class = Some(path_class.into());
        self
    }

    pub fn with_symbol(mut self, symbol: impl Into<String>) -> Self {
        self.symbol = Some(symbol.into());
        self
    }

    pub fn with_repository_id(mut self, repository_id: impl Into<String>) -> Self {
        self.repository_id = Some(repository_id.into());
        self
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_result_handle(mut self, result_handle: impl Into<String>) -> Self {
        self.result_handle = Some(result_handle.into());
        self
    }

    pub fn with_reason(mut self, reason: impl Into<String>) -> Self {
        self.reason = Some(reason.into());
        self
    }
}

/// Applied search/navigation scope echo for zero-hit diagnostics (`FUT-006`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct ZeroHitScope {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_regex: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glob: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path_class: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_by_policy: Vec<String>,
}

impl ZeroHitScope {
    /// True when no scope fields would serialize.
    pub fn is_empty(&self) -> bool {
        self.path_regex.is_none()
            && self.glob.is_none()
            && self.path_class.is_none()
            && self.repository_id.is_none()
            && self.excluded_by_policy.is_empty()
    }

    pub fn with_path_regex(mut self, path_regex: impl Into<String>) -> Self {
        let value = path_regex.into();
        if !value.is_empty() {
            self.path_regex = Some(value);
        }
        self
    }

    pub fn with_glob(mut self, glob: impl Into<String>) -> Self {
        let value = glob.into();
        if !value.is_empty() {
            self.glob = Some(value);
        }
        self
    }

    pub fn with_path_class(mut self, path_class: impl Into<String>) -> Self {
        let value = path_class.into();
        if !value.is_empty() {
            self.path_class = Some(value);
        }
        self
    }

    pub fn with_repository_id(mut self, repository_id: impl Into<String>) -> Self {
        let value = repository_id.into();
        if !value.is_empty() {
            self.repository_id = Some(value);
        }
        self
    }

    pub fn with_excluded_by_policy(mut self, excluded: impl IntoIterator<Item = String>) -> Self {
        self.excluded_by_policy = excluded.into_iter().filter(|v| !v.is_empty()).collect();
        self
    }
}

/// Index freshness block for zero-hit diagnostics (`FUT-006`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct ZeroHitIndex {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index_state: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_index_success_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_tree_dirty: Option<bool>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_paths_since_snapshot: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stale_warning: Option<String>,
}

impl ZeroHitIndex {
    /// True when no index fields would serialize.
    pub fn is_empty(&self) -> bool {
        self.index_state.is_none()
            && self.last_index_success_at.is_none()
            && self.working_tree_dirty.is_none()
            && self.changed_paths_since_snapshot.is_empty()
            && self.stale_warning.is_none()
    }
}

/// Optional scope + index diagnostics bundled for zero-hit responses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct ZeroHitDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ZeroHitScope>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<ZeroHitIndex>,
}

impl ZeroHitDiagnostics {
    pub fn is_empty(&self) -> bool {
        self.scope.as_ref().is_none_or(ZeroHitScope::is_empty)
            && self.index.as_ref().is_none_or(ZeroHitIndex::is_empty)
    }
}

/// Inputs for composing a structured zero-hit recovery payload.
#[derive(Debug, Clone, Default)]
pub struct ZeroHitInput<'a> {
    /// Tool that returned zero hits (`search_text`, `search_symbol`, …).
    pub tool: &'a str,
    /// Original query / symbol when available.
    pub query: Option<&'a str>,
    /// When `Some(true)`, the request used literal matching (default for search_text).
    pub pattern_type_is_literal: Option<bool>,
    /// Optional applied scope echo.
    pub scope: Option<ZeroHitScope>,
    /// Optional index freshness block.
    pub index: Option<ZeroHitIndex>,
    /// Force a specific reason when the caller already classified the zero.
    pub reason_override: Option<ZeroHitReason>,
}

/// Embeddable recovery grammar shared by empty/failed search, navigation, and read paths.
///
/// Prefer flattening onto tool responses so compact JSON exposes recovery fields at the top
/// level without a nested envelope. All fields are optional / omit-empty for additive clients.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct RecoveryFields {
    /// Stable machine code (SCREAMING_SNAKE), for example `ZERO_HIT_SCOPE_TOO_TIGHT`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    /// Short human summary of what happened.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// How to correct the request or re-plan without leaving Frigg.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub correction_hint: Option<String>,
    /// Related MCP tools agents should consider next.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related_tools: Vec<String>,
    /// Concrete follow-up tool invocations with optional params.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_next: Vec<SuggestedNext>,
    /// Structured zero-hit reason when the result set is empty (`FUT-006`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub zero_hit_reason: Option<ZeroHitReason>,
    /// Echo of applied scope filters for zero-hit trust (`FUT-006`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<ZeroHitScope>,
    /// Index freshness block for zero-hit trust (`FUT-006`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<ZeroHitIndex>,
}

impl RecoveryFields {
    /// True when no recovery fields would serialize.
    pub fn is_empty(&self) -> bool {
        self.error_code.is_none()
            && self.message.is_none()
            && self.correction_hint.is_none()
            && self.related_tools.is_empty()
            && self.suggested_next.is_empty()
            && self.zero_hit_reason.is_none()
            && self.scope.as_ref().is_none_or(ZeroHitScope::is_empty)
            && self.index.as_ref().is_none_or(ZeroHitIndex::is_empty)
    }

    /// Attach optional scope + index diagnostics without clobbering recovery text.
    pub fn with_diagnostics(mut self, diagnostics: ZeroHitDiagnostics) -> Self {
        if let Some(scope) = diagnostics.scope.filter(|scope| !scope.is_empty()) {
            self.scope = Some(scope);
        }
        if let Some(index) = diagnostics.index.filter(|index| !index.is_empty()) {
            self.index = Some(index);
        }
        self
    }

    /// When a non-recursive glob produced zero hits, suggest a recursive `**` form (`FUT-009`).
    pub fn with_non_recursive_glob_hint(mut self, query: &str, glob: &str) -> Self {
        let glob = glob.trim();
        if glob.is_empty() || glob.contains("**") {
            return self;
        }
        let recursive = if glob.starts_with("**/") {
            glob.to_owned()
        } else if let Some(stripped) = glob.strip_prefix("*/") {
            format!("**/{stripped}")
        } else {
            format!("**/{glob}")
        };
        let query = query.trim();
        self.suggested_next.insert(
            0,
            SuggestedNext::tool("search_text")
                .with_query(query)
                .with_glob(recursive.clone())
                .with_reason("non-recursive glob zero; retry with recursive ** form"),
        );
        if self
            .correction_hint
            .as_ref()
            .is_none_or(|hint| !hint.contains("**"))
        {
            self.correction_hint = Some(format!(
                "glob {glob:?} has no recursive ** segment; retry with glob={recursive:?}."
            ));
        }
        self
    }

    fn attach_scope_index(
        mut self,
        scope: Option<ZeroHitScope>,
        index: Option<ZeroHitIndex>,
    ) -> Self {
        if let Some(scope) = scope.filter(|scope| !scope.is_empty()) {
            self.scope = Some(scope);
        }
        if let Some(index) = index.filter(|index| !index.is_empty()) {
            self.index = Some(index);
        }
        self
    }

    /// Returns `true` when a literal `search_text` query looks like the agent meant regex.
    ///
    /// Heuristic aligned with the production skill: `|`, `.*`, `^`, `$`, or character classes.
    pub fn query_looks_like_regex(query: &str) -> bool {
        let q = query.trim();
        if q.is_empty() {
            return false;
        }
        if q.contains('|') {
            return true;
        }
        if q.contains(".*") || q.contains(".+") {
            return true;
        }
        if q.contains('^') || q.contains('$') {
            return true;
        }
        // Character class `[...]` with at least one interior character.
        if let Some(open) = q.find('[')
            && let Some(close_rel) = q[open + 1..].find(']')
            && close_rel > 0
        {
            return true;
        }
        false
    }

    /// Recovery for a `search_text` zero-hit given the requested pattern mode.
    ///
    /// `pattern_type_is_literal` should reflect the caller-requested mode (default literal),
    /// not an internal rewrite from `ignore_case` / `word` flags. Every search zero is
    /// actionable (`FUT-006`): regex trap, scoped miss, or complete indexed miss.
    pub fn for_search_text_zero_hit(query: &str, pattern_type_is_literal: bool) -> Self {
        Self::for_zero_hit(ZeroHitInput {
            tool: "search_text",
            query: Some(query),
            pattern_type_is_literal: Some(pattern_type_is_literal),
            scope: None,
            index: None,
            reason_override: None,
        })
    }

    /// Structured zero-hit recovery for search and navigation empty results (`FUT-006`).
    ///
    /// Always returns non-empty `message`, `correction_hint`, `suggested_next`, and a
    /// `zero_hit_reason` so agents can re-plan without shell confirmation.
    pub fn for_zero_hit(input: ZeroHitInput<'_>) -> Self {
        let query = input.query.map(str::trim).filter(|value| !value.is_empty());
        let scope = input.scope.filter(|scope| !scope.is_empty());
        let index = input.index.filter(|index| !index.is_empty());
        let tool = if input.tool.trim().is_empty() {
            "search_text"
        } else {
            input.tool.trim()
        };

        if let Some(reason) = input.reason_override {
            return Self::for_zero_hit_reason(tool, query, reason, scope, index);
        }

        let pattern_type_is_literal = input.pattern_type_is_literal.unwrap_or(true);
        if matches!(tool, "search_text" | "search_hybrid" | "explore")
            && pattern_type_is_literal
            && query.is_some_and(Self::query_looks_like_regex)
        {
            return Self::literal_looks_like_regex(query.unwrap_or(""))
                .attach_scope_index(scope, index);
        }

        let scope_is_tight = scope.as_ref().is_some_and(|scope| {
            scope.path_regex.is_some() || scope.glob.is_some() || scope.path_class.is_some()
        });
        if scope_is_tight {
            let path_regex = scope
                .as_ref()
                .and_then(|scope| scope.path_regex.as_deref());
            let path_class = scope
                .as_ref()
                .and_then(|scope| scope.path_class.as_deref());
            return Self::scoped_miss(query.unwrap_or(""), path_regex, path_class)
                .attach_scope_index(scope, index);
        }

        let index_stale = index.as_ref().is_some_and(|index| {
            index.working_tree_dirty == Some(true)
                || index.stale_warning.is_some()
                || !index.changed_paths_since_snapshot.is_empty()
        });
        if index_stale {
            let changed = index
                .as_ref()
                .map(|index| index.changed_paths_since_snapshot.as_slice())
                .unwrap_or(&[]);
            return Self::stale_dirty_paths(changed).attach_scope_index(scope, index);
        }

        Self::indexed_search_complete(tool, query).attach_scope_index(scope, index)
    }

    fn for_zero_hit_reason(
        tool: &str,
        query: Option<&str>,
        reason: ZeroHitReason,
        scope: Option<ZeroHitScope>,
        index: Option<ZeroHitIndex>,
    ) -> Self {
        let recovery = match reason {
            ZeroHitReason::QueryLooksLikeRegex => {
                Self::literal_looks_like_regex(query.unwrap_or(""))
            }
            ZeroHitReason::ScopeExcludedAllCandidates => {
                let path_regex = scope
                    .as_ref()
                    .and_then(|scope| scope.path_regex.as_deref());
                let path_class = scope
                    .as_ref()
                    .and_then(|scope| scope.path_class.as_deref());
                Self::scoped_miss(query.unwrap_or(""), path_regex, path_class)
            }
            ZeroHitReason::IndexStalePossible => {
                let changed = index
                    .as_ref()
                    .map(|index| index.changed_paths_since_snapshot.as_slice())
                    .unwrap_or(&[]);
                Self::stale_dirty_paths(changed)
            }
            ZeroHitReason::WrongRepositoryPossible => Self::wrong_repo_possible(None),
            ZeroHitReason::NoIndexCoverage => Self::detached_session(),
            ZeroHitReason::ToolUnavailable => Self::tool_unavailable(tool),
            ZeroHitReason::PreciseGraphUnavailable => {
                Self::precise_graph_unavailable(tool, query)
            }
            ZeroHitReason::PathClassNotIndexed => {
                Self::path_class_not_indexed(query, scope.as_ref())
            }
            ZeroHitReason::QueryMiss => Self::query_miss(tool, query),
            ZeroHitReason::IndexedSearchComplete => Self::indexed_search_complete(tool, query),
        };
        recovery.attach_scope_index(scope, index)
    }

    /// Indexed search completed with zero matches inside the applied scope.
    pub fn indexed_search_complete(tool: &str, query: Option<&str>) -> Self {
        let tool = tool.trim();
        let query_label = query.unwrap_or("<empty>");
        let mut suggested_next = vec![
            SuggestedNext::tool("workspace")
                .with_reason("confirm repository adoption and index freshness"),
        ];
        if let Some(query) = query {
            if tool == "search_symbol" {
                suggested_next.push(
                    SuggestedNext::tool("search_text")
                        .with_query(query)
                        .with_reason("textual fallback after symbol zero"),
                );
                suggested_next.push(
                    SuggestedNext::tool("search_symbol")
                        .with_symbol(query)
                        .with_path_class("project")
                        .with_reason("broaden path_class after runtime-first zero"),
                );
            } else if tool == "search_hybrid" {
                suggested_next.push(
                    SuggestedNext::tool("search_symbol")
                        .with_query(query)
                        .with_path_class("runtime")
                        .with_reason("exact symbol pivot after hybrid zero"),
                );
                suggested_next.push(
                    SuggestedNext::tool("search_text")
                        .with_query(query)
                        .with_reason("exact text pivot after hybrid zero"),
                );
            } else if matches!(tool, "find_references" | "go_to_definition" | "find_declarations") {
                suggested_next.push(
                    SuggestedNext::tool("search_symbol")
                        .with_symbol(query)
                        .with_path_class("runtime")
                        .with_reason("resolve symbol before navigation retry"),
                );
                suggested_next.push(
                    SuggestedNext::tool("search_text")
                        .with_query(query)
                        .with_reason("textual fallback after navigation zero"),
                );
            } else {
                suggested_next.push(
                    SuggestedNext::tool("search_text")
                        .with_query(query)
                        .with_path_regex("^src/")
                        .with_reason("retry with an explicit runtime path_regex"),
                );
                suggested_next.push(
                    SuggestedNext::tool("search_symbol")
                        .with_query(query)
                        .with_path_class("runtime")
                        .with_reason("try symbol search if the query is a known name"),
                );
            }
        } else {
            suggested_next.push(
                SuggestedNext::tool(tool).with_reason("retry with a more specific query or symbol"),
            );
        }
        Self {
            error_code: Some("ZERO_HIT".to_owned()),
            message: Some(format!(
                "Indexed search via {tool} returned no matches for {query_label:?}."
            )),
            correction_hint: Some(
                "Trust this zero when scope and index look right; broaden path_regex/path_class, check workspace, or pivot tools before shell grep."
                    .to_owned(),
            ),
            related_tools: vec![
                tool.to_owned(),
                "workspace".to_owned(),
                "search_text".to_owned(),
                "search_symbol".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::IndexedSearchComplete),
            scope: None,
            index: None,
        }
    }

    /// Generic query miss when a stronger diagnostic does not apply.
    pub fn query_miss(tool: &str, query: Option<&str>) -> Self {
        let tool = tool.trim();
        let query_label = query.unwrap_or("<empty>");
        let mut suggested_next = vec![
            SuggestedNext::tool("workspace")
                .with_reason("confirm repository and freshness if the miss is surprising"),
        ];
        if let Some(query) = query {
            suggested_next.push(
                SuggestedNext::tool("search_text")
                    .with_query(query)
                    .with_reason("retry or rephrase as exact text"),
            );
        }
        Self {
            error_code: Some("QUERY_MISS".to_owned()),
            message: Some(format!(
                "No matches for {query_label:?} via {tool}."
            )),
            correction_hint: Some(
                "Rephrase the query, drop tight filters, or call workspace if adoption may be wrong."
                    .to_owned(),
            ),
            related_tools: vec![tool.to_owned(), "workspace".to_owned(), "search_text".to_owned()],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::QueryMiss),
            scope: None,
            index: None,
        }
    }

    /// Precise graph / SCIP data is unavailable for navigation.
    pub fn precise_graph_unavailable(tool: &str, query: Option<&str>) -> Self {
        let tool = tool.trim();
        let mut suggested_next = vec![
            SuggestedNext::tool("search_symbol")
                .with_reason("heuristic symbol search when precise graph is absent"),
            SuggestedNext::tool("search_text")
                .with_reason("textual fallback when precise navigation is unavailable"),
            SuggestedNext::tool("workspace")
                .with_reason("check precise generation / index readiness"),
        ];
        if let Some(query) = query {
            suggested_next[0] = SuggestedNext::tool("search_symbol")
                .with_symbol(query)
                .with_path_class("runtime")
                .with_reason("heuristic symbol search when precise graph is absent");
            suggested_next[1] = SuggestedNext::tool("search_text")
                .with_query(query)
                .with_reason("textual fallback when precise navigation is unavailable");
        }
        Self {
            error_code: Some("PRECISE_GRAPH_UNAVAILABLE".to_owned()),
            message: Some(format!(
                "{tool} has no precise graph/SCIP data for this request."
            )),
            correction_hint: Some(
                "Use search_symbol/search_text, or wait for precise generation via workspace when SCIP artifacts are expected."
                    .to_owned(),
            ),
            related_tools: vec![
                tool.to_owned(),
                "search_symbol".to_owned(),
                "search_text".to_owned(),
                "workspace".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::PreciseGraphUnavailable),
            scope: None,
            index: None,
        }
    }

    /// Requested path class is not covered by the index.
    pub fn path_class_not_indexed(query: Option<&str>, scope: Option<&ZeroHitScope>) -> Self {
        let path_class = scope
            .and_then(|scope| scope.path_class.as_deref())
            .unwrap_or("requested");
        let mut suggested_next = vec![
            SuggestedNext::tool("search_text")
                .with_reason("search without path_class when class coverage is missing"),
            SuggestedNext::tool("list_files")
                .with_reason("verify which path classes are present"),
        ];
        if let Some(query) = query {
            suggested_next[0] = SuggestedNext::tool("search_text")
                .with_query(query)
                .with_reason("search without path_class when class coverage is missing");
        }
        Self {
            error_code: Some("PATH_CLASS_NOT_INDEXED".to_owned()),
            message: Some(format!(
                "Path class {path_class:?} is not covered by the index for this request."
            )),
            correction_hint: Some(
                "Drop path_class, switch to project/support, or verify list_files under the intended roots."
                    .to_owned(),
            ),
            related_tools: vec![
                "search_symbol".to_owned(),
                "search_text".to_owned(),
                "list_files".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::PathClassNotIndexed),
            scope: None,
            index: None,
        }
    }

    /// Literal query appears regex-shaped and returned zero hits.
    pub fn literal_looks_like_regex(query: &str) -> Self {
        let query = query.trim();
        Self {
            error_code: Some("QUERY_LOOKS_LIKE_REGEX".to_owned()),
            message: Some(format!(
                "No literal matches for query that looks like a regular expression: {query:?}."
            )),
            correction_hint: Some(
                "Retry search_text with pattern_type=regex (default is literal)."
                    .to_owned(),
            ),
            related_tools: vec!["search_text".to_owned()],
            suggested_next: vec![
                SuggestedNext::tool("search_text")
                    .with_query(query)
                    .with_pattern_type("regex")
                    .with_reason("literal query contains regex metacharacters"),
            ],
            zero_hit_reason: Some(ZeroHitReason::QueryLooksLikeRegex),
            scope: None,
            index: None,
        }
    }

    /// Runtime-scoped zero with a known name: broaden path_class or fall back to text.
    pub fn runtime_zero_name_known(name: &str) -> Self {
        let name = name.trim();
        Self {
            error_code: Some("ZERO_HIT_RUNTIME_SCOPE".to_owned()),
            message: Some(format!(
                "No runtime-class hits for known name {name:?}."
            )),
            correction_hint: Some(
                "Broaden path_class beyond runtime, or retry with search_text for textual hits."
                    .to_owned(),
            ),
            related_tools: vec![
                "search_symbol".to_owned(),
                "search_text".to_owned(),
                "list_files".to_owned(),
            ],
            suggested_next: vec![
                SuggestedNext::tool("search_symbol")
                    .with_symbol(name)
                    .with_path_class("project")
                    .with_reason("broaden path_class after runtime zero"),
                SuggestedNext::tool("search_text")
                    .with_query(name)
                    .with_reason("textual search when symbol class filter misses"),
            ],
            zero_hit_reason: Some(ZeroHitReason::ScopeExcludedAllCandidates),
            scope: None,
            index: None,
        }
    }

    /// Wrong repository may explain an empty result.
    pub fn wrong_repo_possible(path_hint: Option<&str>) -> Self {
        let mut next = SuggestedNext::tool("workspace")
            .with_reason("confirm adoption and session default repository");
        if let Some(path) = path_hint.filter(|value| !value.trim().is_empty()) {
            next = next.with_path(path);
        }
        Self {
            error_code: Some("WRONG_REPOSITORY_POSSIBLE".to_owned()),
            message: Some(
                "No matches; the session default or repository_id may not be the intended repo."
                    .to_owned(),
            ),
            correction_hint: Some(
                "Call workspace(path=...) to adopt the correct repository, then retry with that repository_id."
                    .to_owned(),
            ),
            related_tools: vec!["workspace".to_owned(), "search_text".to_owned()],
            suggested_next: vec![next],
            zero_hit_reason: Some(ZeroHitReason::WrongRepositoryPossible),
            scope: None,
            index: None,
        }
    }

    /// Dirty / stale index may explain a surprising zero.
    pub fn stale_dirty_paths(changed_paths: &[String]) -> Self {
        let summary = if changed_paths.is_empty() {
            "working tree may have changed since the last index".to_owned()
        } else {
            let preview = changed_paths
                .iter()
                .take(3)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let extra = changed_paths.len().saturating_sub(3);
            if extra == 0 {
                format!("recently changed paths: {preview}")
            } else {
                format!("recently changed paths: {preview} (+{extra} more)")
            }
        };
        let mut suggested_next = vec![
            SuggestedNext::tool("workspace")
                .with_reason("check dirty paths and index freshness"),
        ];
        if let Some(path) = changed_paths.first() {
            suggested_next.push(
                SuggestedNext::tool("read_file")
                    .with_path(path.clone())
                    .with_reason("live-disk read of a touched path only"),
            );
        }
        Self {
            error_code: Some("INDEX_STALE_POSSIBLE".to_owned()),
            message: Some(format!("No matches; index may be stale ({summary}).")),
            correction_hint: Some(
                "Use workspace for freshness; live-read only touched paths, not repo-wide shell grep."
                    .to_owned(),
            ),
            related_tools: vec![
                "workspace".to_owned(),
                "read_file".to_owned(),
                "search_text".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::IndexStalePossible),
            scope: None,
            index: None,
        }
    }

    /// Multi-hypothesis task should prefer batch or parallel exact probes.
    pub fn multi_hypothesis(probes: &[&str]) -> Self {
        let suggested_next = if probes.is_empty() {
            vec![
                SuggestedNext::tool("search_batch")
                    .with_reason("multi-hypothesis: batch probes when available"),
                SuggestedNext::tool("search_text")
                    .with_reason("interim: same-turn parallel search_text probes"),
            ]
        } else {
            probes
                .iter()
                .take(6)
                .map(|probe| {
                    SuggestedNext::tool("search_text")
                        .with_query(*probe)
                        .with_reason("parallel exact probe for multi-hypothesis task")
                })
                .collect()
        };
        Self {
            error_code: Some("MULTI_HYPOTHESIS".to_owned()),
            message: Some(
                "Several plausible probes fit this task better than a single wide search."
                    .to_owned(),
            ),
            correction_hint: Some(
                "Prefer search_batch when available; otherwise issue same-turn parallel search_text/search_symbol probes."
                    .to_owned(),
            ),
            related_tools: vec![
                "search_batch".to_owned(),
                "search_text".to_owned(),
                "search_symbol".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::QueryMiss),
            scope: None,
            index: None,
        }
    }

    /// All `search_batch` probes returned zero hits after the batch already ran.
    pub fn batch_all_zero(
        strongest_reason: Option<ZeroHitReason>,
        suggested_next: Vec<SuggestedNext>,
    ) -> Self {
        let zero_hit_reason = strongest_reason.unwrap_or(ZeroHitReason::QueryMiss);
        let suggested_next = if suggested_next.is_empty() {
            vec![
                SuggestedNext::tool("workspace")
                    .with_reason("confirm adoption, dirty paths, and index freshness"),
                SuggestedNext::tool("search_text")
                    .with_reason("retry one exact probe with broader scope after reading probe_summary"),
            ]
        } else {
            suggested_next
        };
        Self {
            error_code: Some("BATCH_ALL_ZERO".to_owned()),
            message: Some(
                "All search_batch probes returned zero hits; inspect probe_summary for per-probe diagnostics."
                    .to_owned(),
            ),
            correction_hint: Some(
                "Inspect probe_summary zero_hit_reason/scope; broaden filters, fix the query, or refresh the index. Do not re-issue the same multi-hypothesis batch unchanged."
                    .to_owned(),
            ),
            related_tools: vec![
                "search_text".to_owned(),
                "search_symbol".to_owned(),
                "workspace".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(zero_hit_reason),
            scope: None,
            index: None,
        }
    }

    /// After a symbol hit, impact tools are the natural next step.
    pub fn impact_after_symbol(symbol: &str) -> Self {
        let symbol = symbol.trim();
        Self {
            error_code: Some("IMPACT_AFTER_SYMBOL".to_owned()),
            message: Some(format!(
                "Symbol {symbol:?} is resolved; gather references and callers for impact."
            )),
            correction_hint: Some(
                "Run find_references and incoming_calls (or impact_bundle when available) before whole-repo text search."
                    .to_owned(),
            ),
            related_tools: vec![
                "find_references".to_owned(),
                "incoming_calls".to_owned(),
                "find_implementations".to_owned(),
                "impact_bundle".to_owned(),
            ],
            suggested_next: vec![
                SuggestedNext::tool("find_references")
                    .with_symbol(symbol)
                    .with_reason("usages for impact analysis"),
                SuggestedNext::tool("incoming_calls")
                    .with_symbol(symbol)
                    .with_reason("callers for blast radius"),
            ],
            zero_hit_reason: None,
            scope: None,
            index: None,
        }
    }

    /// Hybrid discovery should pivot to exact tools rather than proof-from-rank-1.
    pub fn hybrid_discovery_exact_pivot(query: &str, pivot_path: Option<&str>) -> Self {
        let query = query.trim();
        let mut suggested_next = vec![
            SuggestedNext::tool("search_symbol")
                .with_query(query)
                .with_path_class("runtime")
                .with_reason("exact symbol pivot after hybrid discovery"),
            SuggestedNext::tool("search_text")
                .with_query(query)
                .with_reason("exact text pivot after hybrid discovery"),
        ];
        if let Some(path) = pivot_path.filter(|value| !value.trim().is_empty()) {
            suggested_next.push(
                SuggestedNext::tool("read_file")
                    .with_path(path)
                    .with_reason("inspect best hybrid pivot path after exact confirmation"),
            );
        }
        Self {
            error_code: Some("HYBRID_DISCOVERY_PIVOT".to_owned()),
            message: Some(
                "Hybrid ranked candidates; pivot to exact search_symbol or search_text before proof."
                    .to_owned(),
            ),
            correction_hint: Some(
                "Do not answer from hybrid rank-1 alone; run an exact pivot then read_match/read_file."
                    .to_owned(),
            ),
            related_tools: vec![
                "search_symbol".to_owned(),
                "search_text".to_owned(),
                "read_match".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: None,
            scope: None,
            index: None,
        }
    }

    /// Scoped search returned zero; broaden path filters.
    pub fn scoped_miss(
        query: &str,
        path_regex: Option<&str>,
        path_class: Option<&str>,
    ) -> Self {
        let query = query.trim();
        let mut scope_bits = Vec::new();
        if let Some(path_regex) = path_regex.filter(|value| !value.is_empty()) {
            scope_bits.push(format!("path_regex={path_regex:?}"));
        }
        if let Some(path_class) = path_class.filter(|value| !value.is_empty()) {
            scope_bits.push(format!("path_class={path_class}"));
        }
        let scope_desc = if scope_bits.is_empty() {
            "the applied scope".to_owned()
        } else {
            scope_bits.join(" and ")
        };
        let broader_path_regex = path_regex.and_then(|raw| broaden_path_regex_hint(raw));
        let mut suggested_next = vec![
            SuggestedNext::tool("search_text")
                .with_query(query)
                .with_reason("retry without tight scope filters"),
        ];
        if let Some(path_regex) = broader_path_regex {
            suggested_next.insert(
                0,
                SuggestedNext::tool("search_text")
                    .with_query(query)
                    .with_path_regex(path_regex)
                    .with_reason("broaden scope after scoped miss"),
            );
        }
        suggested_next.push(
            SuggestedNext::tool("list_files")
                .with_reason("verify the scoped path set still has files"),
        );
        Self {
            error_code: Some("ZERO_HIT_SCOPE_TOO_TIGHT".to_owned()),
            message: Some(format!("No matches under {scope_desc}.")),
            correction_hint: Some(
                "Retry with a broader path_regex, drop path_class, or verify list_files under the scope."
                    .to_owned(),
            ),
            related_tools: vec![
                "workspace".to_owned(),
                "search_text".to_owned(),
                "list_files".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::ScopeExcludedAllCandidates),
            scope: None,
            index: None,
        }
    }

    /// No attached workspace / detached session.
    pub fn detached_session() -> Self {
        Self {
            error_code: Some("DETACHED_SESSION".to_owned()),
            message: Some(
                "No repository is attached for this session; Frigg cannot search indexed source."
                    .to_owned(),
            ),
            correction_hint: Some(
                "Call workspace(path=<repo root>) to adopt a repository, then retry the original tool."
                    .to_owned(),
            ),
            related_tools: vec!["workspace".to_owned()],
            suggested_next: vec![
                SuggestedNext::tool("workspace")
                    .with_reason("attach or adopt a repository for this session"),
            ],
            zero_hit_reason: Some(ZeroHitReason::NoIndexCoverage),
            scope: None,
            index: None,
        }
    }

    /// Tool missing from the live surface/profile.
    pub fn tool_unavailable(tool_name: &str) -> Self {
        let tool_name = tool_name.trim();
        Self {
            error_code: Some("TOOL_UNAVAILABLE".to_owned()),
            message: Some(format!(
                "Tool {tool_name:?} is unavailable on the current Frigg tool surface."
            )),
            correction_hint: Some(
                "Verify tools/list for the live surface profile; use an available core/extended substitute or shell only when Frigg is the wrong tool."
                    .to_owned(),
            ),
            related_tools: vec!["workspace".to_owned(), "search_text".to_owned()],
            suggested_next: vec![
                SuggestedNext::tool("workspace")
                    .with_reason("confirm runtime profile and available tools"),
                SuggestedNext::tool("search_text")
                    .with_reason("core exact search remains available on default surfaces"),
            ],
            zero_hit_reason: Some(ZeroHitReason::ToolUnavailable),
            scope: None,
            index: None,
        }
    }

    /// Empty `go_to_definition({})` without a symbol or location anchor.
    pub fn empty_go_to_definition() -> Self {
        Self {
            error_code: Some("EMPTY_GO_TO_DEFINITION".to_owned()),
            message: Some(
                "go_to_definition requires a symbol, or a path+line location with optional column."
                    .to_owned(),
            ),
            correction_hint: Some(
                "Pass symbol=<name>, or path+line (and column when known). Prefer symbol over path+line alone on dense lines."
                    .to_owned(),
            ),
            related_tools: vec![
                "go_to_definition".to_owned(),
                "search_symbol".to_owned(),
                "document_symbols".to_owned(),
            ],
            suggested_next: vec![
                SuggestedNext::tool("search_symbol")
                    .with_path_class("runtime")
                    .with_reason("resolve a symbol name before go_to_definition"),
                SuggestedNext::tool("go_to_definition")
                    .with_reason("retry with symbol=... or path+line"),
            ],
            zero_hit_reason: Some(ZeroHitReason::QueryMiss),
            scope: None,
            index: None,
        }
    }

    /// Multiple same-rank definition candidates require a tighter anchor.
    pub fn disambiguation_required(query: Option<&str>) -> Self {
        let mut suggested_next = vec![
            SuggestedNext::tool("go_to_definition")
                .with_reason("retry with path+line (and column) from target_selection.candidates"),
            SuggestedNext::tool("search_symbol")
                .with_path_class("runtime")
                .with_reason("list candidate symbols before re-calling go_to_definition"),
        ];
        if let Some(query) = query.filter(|q| !q.trim().is_empty()) {
            suggested_next.insert(
                0,
                SuggestedNext::tool("go_to_definition")
                    .with_symbol(query)
                    .with_reason("disambiguate by re-calling with path+line for this symbol"),
            );
        }
        Self {
            error_code: Some("DISAMBIGUATION_REQUIRED".to_owned()),
            message: Some(
                "go_to_definition found multiple same-rank candidates; pass path+line or a stable_symbol_id."
                    .to_owned(),
            ),
            correction_hint: Some(
                "Inspect target_selection.candidates and retry go_to_definition with path+line or stable_symbol_id. This is not a missing SCIP/precise-graph failure."
                    .to_owned(),
            ),
            related_tools: vec![
                "go_to_definition".to_owned(),
                "search_symbol".to_owned(),
                "document_symbols".to_owned(),
            ],
            suggested_next,
            zero_hit_reason: Some(ZeroHitReason::QueryMiss),
            scope: None,
            index: None,
        }
    }

    /// Syntax inspection missing required line/column pair.
    pub fn missing_line_column_pair(tool_name: &str) -> Self {
        let tool_name = tool_name.trim();
        Self {
            error_code: Some("MISSING_LINE_COLUMN".to_owned()),
            message: Some(format!(
                "{tool_name} requires both line and column (use column=1 if unknown)."
            )),
            correction_hint: Some(
                "Provide line AND column together. When column is unknown, pass column=1 rather than omitting it."
                    .to_owned(),
            ),
            related_tools: vec![
                tool_name.to_owned(),
                "document_symbols".to_owned(),
                "read_file".to_owned(),
            ],
            suggested_next: vec![
                SuggestedNext::tool(tool_name)
                    .with_reason("retry with line and column pair"),
                SuggestedNext::tool("document_symbols")
                    .with_reason("obtain a concrete line/column anchor first"),
            ],
            zero_hit_reason: None,
            scope: None,
            index: None,
        }
    }

    /// Stale result_handle / match_id after reindex or session expiry.
    pub fn stale_handle(result_handle: Option<&str>, match_id: Option<&str>) -> Self {
        let handle = result_handle.unwrap_or("<missing>");
        let match_id = match_id.unwrap_or("<missing>");
        Self {
            error_code: Some("STALE_HANDLE".to_owned()),
            message: Some(format!(
                "Handle is no longer valid (result_handle={handle:?}, match_id={match_id:?})."
            )),
            correction_hint: Some(
                "Re-run the original search/navigation tool to obtain a fresh result_handle and match_id pair from the same call."
                    .to_owned(),
            ),
            related_tools: vec![
                "search_text".to_owned(),
                "search_symbol".to_owned(),
                "read_match".to_owned(),
            ],
            suggested_next: vec![
                SuggestedNext::tool("search_text")
                    .with_reason("refresh handles via a new search call"),
                SuggestedNext::tool("read_file")
                    .with_reason("path-based read when the path is still known"),
            ],
            zero_hit_reason: None,
            scope: None,
            index: None,
        }
    }

    /// match_id from a different result_handle / foreign handle pairing.
    pub fn mixed_handle(result_handle: Option<&str>, match_id: Option<&str>) -> Self {
        let handle = result_handle.unwrap_or("<missing>");
        let match_id = match_id.unwrap_or("<missing>");
        Self {
            error_code: Some("MIXED_HANDLE".to_owned()),
            message: Some(format!(
                "match_id {match_id:?} does not belong to result_handle {handle:?}."
            )),
            correction_hint: Some(
                "match_id is valid only with the result_handle from the same tool call. Do not mix handles across searches."
                    .to_owned(),
            ),
            related_tools: vec!["read_match".to_owned(), "search_text".to_owned()],
            suggested_next: vec![
                SuggestedNext::tool("search_text")
                    .with_reason("re-run search and use the paired handle + match_id"),
            ],
            zero_hit_reason: None,
            scope: None,
            index: None,
        }
    }
}

/// Best-effort broader path_regex hint for scoped-miss recovery.
fn broaden_path_regex_hint(path_regex: &str) -> Option<String> {
    let trimmed = path_regex.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Common tight runtime roots → one level broader.
    if let Some(stripped) = trimmed.strip_prefix('^') {
        if let Some((head, _)) = stripped.split_once('/') {
            if !head.is_empty() && head != ".*" {
                return Some(format!("^{head}/"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn assert_recovery_actionable(recovery: &RecoveryFields) {
        assert!(
            recovery
                .message
                .as_ref()
                .is_some_and(|message| !message.trim().is_empty()),
            "message must be non-empty: {recovery:?}"
        );
        assert!(
            recovery
                .correction_hint
                .as_ref()
                .is_some_and(|hint| !hint.trim().is_empty()),
            "correction_hint must be non-empty: {recovery:?}"
        );
        assert!(
            !recovery.related_tools.is_empty(),
            "related_tools must be non-empty: {recovery:?}"
        );
        assert!(
            !recovery.suggested_next.is_empty(),
            "suggested_next must be non-empty: {recovery:?}"
        );
        for next in &recovery.suggested_next {
            assert!(
                !next.tool.trim().is_empty(),
                "suggested_next.tool must be non-empty: {recovery:?}"
            );
        }
    }

    #[test]
    fn recovery_query_looks_like_regex_detects_skill_traps() {
        assert!(RecoveryFields::query_looks_like_regex(r"foo|bar"));
        assert!(RecoveryFields::query_looks_like_regex(r"register.*tool"));
        assert!(RecoveryFields::query_looks_like_regex(r"^src/"));
        assert!(RecoveryFields::query_looks_like_regex(r"end$"));
        assert!(RecoveryFields::query_looks_like_regex(r"[A-Z]+Error"));
        assert!(!RecoveryFields::query_looks_like_regex("catalog_entries"));
        assert!(!RecoveryFields::query_looks_like_regex(""));
        assert!(!RecoveryFields::query_looks_like_regex("plain.text"));
    }

    #[test]
    fn recovery_literal_looks_like_regex_builder_is_actionable() {
        let recovery = RecoveryFields::literal_looks_like_regex(r"foo|bar");
        assert_recovery_actionable(&recovery);
        assert_eq!(recovery.error_code.as_deref(), Some("QUERY_LOOKS_LIKE_REGEX"));
        assert_eq!(
            recovery.zero_hit_reason,
            Some(ZeroHitReason::QueryLooksLikeRegex)
        );
        assert_eq!(
            recovery.suggested_next[0].pattern_type.as_deref(),
            Some("regex")
        );
        assert_eq!(recovery.suggested_next[0].query.as_deref(), Some("foo|bar"));
    }

    #[test]
    fn recovery_for_search_text_zero_hit_wires_regex_trap_only_for_literals() {
        let recovery = RecoveryFields::for_search_text_zero_hit(r"foo|bar", true);
        assert_recovery_actionable(&recovery);
        assert_eq!(recovery.error_code.as_deref(), Some("QUERY_LOOKS_LIKE_REGEX"));

        let explicit_regex = RecoveryFields::for_search_text_zero_hit(r"foo|bar", false);
        assert_recovery_actionable(&explicit_regex);
        assert_ne!(
            explicit_regex.error_code.as_deref(),
            Some("QUERY_LOOKS_LIKE_REGEX"),
            "explicit pattern_type=regex zero should not emit the literal-regex trap"
        );
        assert_eq!(
            explicit_regex.zero_hit_reason,
            Some(ZeroHitReason::IndexedSearchComplete)
        );

        let plain = RecoveryFields::for_search_text_zero_hit("catalog_entries", true);
        assert_recovery_actionable(&plain);
        assert_eq!(
            plain.zero_hit_reason,
            Some(ZeroHitReason::IndexedSearchComplete)
        );
    }

    #[test]
    fn recovery_for_zero_hit_includes_scope_echo_and_actionable_fields() {
        let recovery = RecoveryFields::for_zero_hit(ZeroHitInput {
            tool: "search_text",
            query: Some("catalog_entries"),
            pattern_type_is_literal: Some(true),
            scope: Some(
                ZeroHitScope::default()
                    .with_path_regex("^src/catalog/")
                    .with_glob("**/*.rs")
                    .with_path_class("runtime")
                    .with_repository_id("repo-1"),
            ),
            index: Some(ZeroHitIndex {
                index_state: Some("ready".to_owned()),
                last_index_success_at: None,
                working_tree_dirty: Some(false),
                changed_paths_since_snapshot: Vec::new(),
                stale_warning: None,
            }),
            reason_override: None,
        });
        assert_recovery_actionable(&recovery);
        assert_eq!(
            recovery.zero_hit_reason,
            Some(ZeroHitReason::ScopeExcludedAllCandidates)
        );
        let scope = recovery.scope.as_ref().expect("scope echo");
        assert_eq!(scope.path_regex.as_deref(), Some("^src/catalog/"));
        assert_eq!(scope.glob.as_deref(), Some("**/*.rs"));
        assert_eq!(scope.path_class.as_deref(), Some("runtime"));
        assert_eq!(scope.repository_id.as_deref(), Some("repo-1"));
        assert_eq!(
            recovery.index.as_ref().and_then(|index| index.index_state.as_deref()),
            Some("ready")
        );

        let value = serde_json::to_value(&recovery).expect("serialize zero-hit");
        assert_eq!(value["zero_hit_reason"], "scope_excluded_all_candidates");
        assert_eq!(value["scope"]["path_regex"], "^src/catalog/");
        assert_eq!(value["index"]["index_state"], "ready");
        assert!(value["message"].as_str().is_some_and(|m| !m.is_empty()));
        assert!(value["correction_hint"].as_str().is_some_and(|m| !m.is_empty()));
        assert!(value["suggested_next"].as_array().is_some_and(|v| !v.is_empty()));
    }

    #[test]
    fn recovery_for_zero_hit_nav_and_symbol_are_actionable() {
        for tool in ["search_symbol", "search_hybrid", "find_references", "go_to_definition"] {
            let recovery = RecoveryFields::for_zero_hit(ZeroHitInput {
                tool,
                query: Some("MissingSymbol"),
                pattern_type_is_literal: None,
                scope: None,
                index: None,
                reason_override: None,
            });
            assert_recovery_actionable(&recovery);
            assert_eq!(
                recovery.zero_hit_reason,
                Some(ZeroHitReason::IndexedSearchComplete),
                "tool={tool}"
            );
        }
    }

    #[test]
    fn recovery_runtime_zero_name_known_builder_is_actionable() {
        let recovery = RecoveryFields::runtime_zero_name_known("CatalogEntries");
        assert_recovery_actionable(&recovery);
        assert!(recovery.related_tools.contains(&"search_symbol".to_owned()));
    }

    #[test]
    fn recovery_wrong_repo_possible_builder_is_actionable() {
        let recovery = RecoveryFields::wrong_repo_possible(Some("/tmp/repo"));
        assert_recovery_actionable(&recovery);
        assert_eq!(
            recovery.zero_hit_reason,
            Some(ZeroHitReason::WrongRepositoryPossible)
        );
        assert_eq!(
            recovery.suggested_next[0].path.as_deref(),
            Some("/tmp/repo")
        );
    }

    #[test]
    fn recovery_stale_dirty_paths_builder_is_actionable() {
        let recovery = RecoveryFields::stale_dirty_paths(&[
            "src/a.rs".to_owned(),
            "src/b.rs".to_owned(),
        ]);
        assert_recovery_actionable(&recovery);
        assert_eq!(
            recovery.zero_hit_reason,
            Some(ZeroHitReason::IndexStalePossible)
        );
        assert!(
            recovery
                .suggested_next
                .iter()
                .any(|next| next.tool == "read_file")
        );
    }

    #[test]
    fn recovery_multi_hypothesis_builder_is_actionable() {
        let recovery = RecoveryFields::multi_hypothesis(&["alpha", "beta"]);
        assert_recovery_actionable(&recovery);
        assert_eq!(recovery.suggested_next.len(), 2);
    }

    #[test]
    fn recovery_impact_after_symbol_builder_is_actionable() {
        let recovery = RecoveryFields::impact_after_symbol("load_config");
        assert_recovery_actionable(&recovery);
        assert!(
            recovery
                .suggested_next
                .iter()
                .any(|next| next.tool == "find_references")
        );
    }

    #[test]
    fn recovery_hybrid_discovery_exact_pivot_builder_is_actionable() {
        let recovery =
            RecoveryFields::hybrid_discovery_exact_pivot("where is catalog", Some("src/catalog.rs"));
        assert_recovery_actionable(&recovery);
        assert!(
            recovery
                .suggested_next
                .iter()
                .any(|next| next.tool == "search_symbol")
        );
    }

    #[test]
    fn recovery_scoped_miss_builder_is_actionable() {
        let recovery = RecoveryFields::scoped_miss(
            "catalog_entries",
            Some("^src/catalog/"),
            Some("runtime"),
        );
        assert_recovery_actionable(&recovery);
        assert_eq!(
            recovery.error_code.as_deref(),
            Some("ZERO_HIT_SCOPE_TOO_TIGHT")
        );
        assert!(
            recovery
                .suggested_next
                .iter()
                .any(|next| next.path_regex.as_deref() == Some("^src/"))
        );
    }

    #[test]
    fn recovery_detached_session_builder_is_actionable() {
        let recovery = RecoveryFields::detached_session();
        assert_recovery_actionable(&recovery);
        assert_eq!(recovery.suggested_next[0].tool, "workspace");
    }

    #[test]
    fn recovery_tool_unavailable_builder_is_actionable() {
        let recovery = RecoveryFields::tool_unavailable("search_batch");
        assert_recovery_actionable(&recovery);
        assert_eq!(
            recovery.zero_hit_reason,
            Some(ZeroHitReason::ToolUnavailable)
        );
    }

    #[test]
    fn recovery_empty_go_to_definition_builder_is_actionable() {
        let recovery = RecoveryFields::empty_go_to_definition();
        assert_recovery_actionable(&recovery);
        assert_eq!(
            recovery.error_code.as_deref(),
            Some("EMPTY_GO_TO_DEFINITION")
        );
    }

    #[test]
    fn recovery_disambiguation_required_builder_is_actionable() {
        let recovery = RecoveryFields::disambiguation_required(Some("Handler"));
        assert_recovery_actionable(&recovery);
        assert_eq!(
            recovery.error_code.as_deref(),
            Some("DISAMBIGUATION_REQUIRED")
        );
        assert!(
            recovery
                .correction_hint
                .as_ref()
                .is_some_and(|hint| hint.contains("not a missing SCIP")),
            "disambiguation recovery must not steer toward precise-graph wait"
        );
    }

    #[test]
    fn recovery_missing_line_column_pair_builder_is_actionable() {
        let recovery = RecoveryFields::missing_line_column_pair("inspect_syntax_tree");
        assert_recovery_actionable(&recovery);
        assert!(recovery.message.as_ref().is_some_and(|m| m.contains("column")));
    }

    #[test]
    fn recovery_stale_handle_builder_is_actionable() {
        let recovery = RecoveryFields::stale_handle(Some("result-000001"), Some("m1"));
        assert_recovery_actionable(&recovery);
        assert_eq!(recovery.error_code.as_deref(), Some("STALE_HANDLE"));
    }

    #[test]
    fn recovery_mixed_handle_builder_is_actionable() {
        let recovery = RecoveryFields::mixed_handle(Some("result-000001"), Some("m9"));
        assert_recovery_actionable(&recovery);
        assert_eq!(recovery.error_code.as_deref(), Some("MIXED_HANDLE"));
    }

    #[test]
    fn recovery_fields_serialize_top_level_and_omit_when_empty() {
        let empty = serde_json::to_value(RecoveryFields::default()).expect("serialize empty");
        assert_eq!(empty, json!({}));

        let value = serde_json::to_value(RecoveryFields::literal_looks_like_regex(r"a|b"))
            .expect("serialize recovery");
        assert_eq!(value["error_code"], "QUERY_LOOKS_LIKE_REGEX");
        assert_eq!(value["zero_hit_reason"], "query_looks_like_regex");
        assert!(value["correction_hint"].as_str().is_some());
        assert!(value["related_tools"].as_array().is_some_and(|v| !v.is_empty()));
        assert!(value["suggested_next"].as_array().is_some_and(|v| !v.is_empty()));
        assert_eq!(value["suggested_next"][0]["pattern_type"], "regex");
    }

    #[test]
    fn zero_hit_reason_serde_snake_case_is_stable() {
        let value = serde_json::to_value(ZeroHitReason::IndexedSearchComplete)
            .expect("serialize enum");
        assert_eq!(value, json!("indexed_search_complete"));
        let parsed: ZeroHitReason =
            serde_json::from_value(json!("wrong_repository_possible")).expect("parse enum");
        assert_eq!(parsed, ZeroHitReason::WrongRepositoryPossible);
    }
}
