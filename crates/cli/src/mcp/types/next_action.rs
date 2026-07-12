//! Canonical typed, executable follow-up actions for existing core MCP tools.
//!
//! This contract intentionally has no generic executor or dynamic argument map. Callers choose
//! whether to invoke a typed action through the ordinary MCP surface.

use std::collections::{HashMap, HashSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::{
    DocumentSymbolsParams, ExploreParams, FindDeclarationsParams, FindImplementationsParams,
    FindReferencesParams, GoToDefinitionParams, ImpactBundleParams, IncomingCallsParams,
    InspectSyntaxTreeParams, ListFilesParams, OutgoingCallsParams, ReadFileParams, ReadMatchParams,
    SearchBatchParams, SearchHybridParams, SearchStructuralParams, SearchSymbolParams,
    SearchTextParams, SuggestedNext, WorkspaceParams,
};

/// Non-empty, response-local action identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct NextActionId(pub String);

impl NextActionId {
    pub fn new(value: impl Into<String>) -> Option<Self> {
        let value = value.into();
        (!value.trim().is_empty()).then_some(Self(value))
    }

    fn valid(&self) -> bool {
        !self.0.trim().is_empty()
    }
}

/// Closed semantic role for an executable follow-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NextActionRole {
    Retry,
    BroadenScope,
    ResolveTarget,
    VerifyExact,
    ProofRead,
    Inspect,
    Diagnose,
}

impl NextActionRole {
    fn priority(self) -> u8 {
        match self {
            Self::ProofRead => 0,
            Self::VerifyExact => 1,
            Self::Retry => 2,
            Self::ResolveTarget => 3,
            Self::Inspect => 4,
            Self::BroadenScope => 5,
            Self::Diagnose => 6,
        }
    }
}

/// Dependency-group satisfaction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum NextActionDependencyMode {
    All,
    Any,
}

/// A non-empty group of actions from lower order stages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct NextActionDependency {
    pub mode: NextActionDependencyMode,
    pub action_ids: Vec<NextActionId>,
}

/// Exact typed target for every default non-playbook core tool.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tool", content = "arguments", rename_all = "snake_case")]
pub enum NextActionTarget {
    Workspace(WorkspaceParams),
    ListFiles(ListFilesParams),
    ReadFile(ReadFileParams),
    ReadMatch(ReadMatchParams),
    Explore(ExploreParams),
    SearchText(SearchTextParams),
    SearchHybrid(SearchHybridParams),
    SearchSymbol(SearchSymbolParams),
    SearchBatch(SearchBatchParams),
    FindReferences(FindReferencesParams),
    GoToDefinition(GoToDefinitionParams),
    FindDeclarations(FindDeclarationsParams),
    FindImplementations(FindImplementationsParams),
    IncomingCalls(IncomingCallsParams),
    OutgoingCalls(OutgoingCallsParams),
    DocumentSymbols(DocumentSymbolsParams),
    InspectSyntaxTree(InspectSyntaxTreeParams),
    SearchStructural(SearchStructuralParams),
    ImpactBundle(ImpactBundleParams),
}

impl NextActionTarget {
    pub const fn tool_name(&self) -> &'static str {
        match self {
            Self::Workspace(_) => "workspace",
            Self::ListFiles(_) => "list_files",
            Self::ReadFile(_) => "read_file",
            Self::ReadMatch(_) => "read_match",
            Self::Explore(_) => "explore",
            Self::SearchText(_) => "search_text",
            Self::SearchHybrid(_) => "search_hybrid",
            Self::SearchSymbol(_) => "search_symbol",
            Self::SearchBatch(_) => "search_batch",
            Self::FindReferences(_) => "find_references",
            Self::GoToDefinition(_) => "go_to_definition",
            Self::FindDeclarations(_) => "find_declarations",
            Self::FindImplementations(_) => "find_implementations",
            Self::IncomingCalls(_) => "incoming_calls",
            Self::OutgoingCalls(_) => "outgoing_calls",
            Self::DocumentSymbols(_) => "document_symbols",
            Self::InspectSyntaxTree(_) => "inspect_syntax_tree",
            Self::SearchStructural(_) => "search_structural",
            Self::ImpactBundle(_) => "impact_bundle",
        }
    }
}

/// Exact non-recursive producer target for stale-handle replay. `read_match` is deliberately
/// absent, preventing nested origin chains.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "tool", content = "arguments", rename_all = "snake_case")]
pub enum ReplayOriginTarget {
    Explore(ExploreParams),
    SearchText(SearchTextParams),
    SearchHybrid(SearchHybridParams),
    SearchSymbol(SearchSymbolParams),
    SearchBatch(SearchBatchParams),
    FindReferences(FindReferencesParams),
    GoToDefinition(GoToDefinitionParams),
    FindDeclarations(FindDeclarationsParams),
    FindImplementations(FindImplementationsParams),
    IncomingCalls(IncomingCallsParams),
    OutgoingCalls(OutgoingCallsParams),
    DocumentSymbols(DocumentSymbolsParams),
    InspectSyntaxTree(InspectSyntaxTreeParams),
    SearchStructural(SearchStructuralParams),
    ImpactBundle(ImpactBundleParams),
}

/// Tagged producer request carried by a handle-bound `read_match` action.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct NextActionOrigin(pub ReplayOriginTarget);

/// Canonical action. `target` is flattened so wire output is `{tool, arguments}`.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NextAction {
    pub id: NextActionId,
    pub role: NextActionRole,
    pub order: u16,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dependencies: Vec<NextActionDependency>,
    #[serde(flatten)]
    pub target: NextActionTarget,
    pub reason: String,
}

impl NextAction {
    /// One-way, lossy compatibility projection. Canonical actions always remain authoritative.
    pub fn to_legacy_suggestion(&self) -> SuggestedNext {
        let mut suggestion = SuggestedNext::tool(self.target.tool_name()).with_reason(&self.reason);
        match &self.target {
            NextActionTarget::Workspace(params) => {
                suggestion.path = params.path.clone();
                suggestion.repository_id = params.repository_id.clone();
            }
            NextActionTarget::ListFiles(params) => {
                suggestion.repository_id = params.repository_id.clone();
                suggestion.path_regex = params.path_regex.clone();
                suggestion.glob = params.glob.clone();
                suggestion.path_class = params.path_class.map(|value| value.as_str().to_owned());
            }
            NextActionTarget::ReadFile(params) => {
                suggestion.path = Some(params.path.clone());
                suggestion.repository_id = params.repository_id.clone();
            }
            NextActionTarget::SearchText(params) => {
                suggestion.query = Some(params.query.clone());
                suggestion.repository_id = params.repository_id.clone();
                suggestion.path_regex = params.path_regex.clone();
                suggestion.glob = params.glob.clone();
                suggestion.pattern_type =
                    params.pattern_type.as_ref().map(|pattern| match pattern {
                        super::SearchPatternType::Literal => "literal".to_owned(),
                        super::SearchPatternType::Regex => "regex".to_owned(),
                    });
            }
            NextActionTarget::SearchSymbol(params) => {
                suggestion.query = Some(params.query.clone());
                suggestion.symbol = Some(params.query.clone());
                suggestion.repository_id = params.repository_id.clone();
                suggestion.path_regex = params.path_regex.clone();
                suggestion.path_class = params.path_class.map(|value| value.as_str().to_owned());
            }
            NextActionTarget::FindReferences(params) => {
                suggestion.symbol = params.symbol.clone();
                suggestion.repository_id = params.repository_id.clone();
                suggestion.path = params.path.clone();
            }
            NextActionTarget::GoToDefinition(params) => {
                suggestion.symbol = params.symbol.clone();
                suggestion.repository_id = params.repository_id.clone();
                suggestion.path = params.path.clone();
            }
            NextActionTarget::IncomingCalls(params) => {
                suggestion.symbol = params.symbol.clone();
                suggestion.repository_id = params.repository_id.clone();
                suggestion.path = params.path.clone();
            }
            _ => {}
        }
        suggestion
    }

    fn target_key(&self) -> Option<String> {
        serde_json::to_value(&self.target)
            .ok()
            .map(|value| value.to_string())
    }

    fn valid_owned(&self, ids: &HashMap<String, u16>, id_counts: &HashMap<String, usize>) -> bool {
        self.id.valid()
            && id_counts.get(&self.id.0) == Some(&1)
            && !self.reason.trim().is_empty()
            && self.dependencies.iter().all(|group| {
                !group.action_ids.is_empty()
                    && group.action_ids.iter().collect::<HashSet<_>>().len()
                        == group.action_ids.len()
                    && group.action_ids.iter().all(|id| {
                        id.valid()
                            && id.0 != self.id.0
                            && ids.get(&id.0).is_some_and(|order| *order < self.order)
                    })
            })
    }
}

/// Filter malformed rows, deterministically deduplicate exact serialized targets, and cap output
/// at eight actions. Legacy suggestions are intentionally never promoted into this list.
pub fn normalize_next_actions(actions: impl IntoIterator<Item = NextAction>) -> Vec<NextAction> {
    let mut actions: Vec<_> = actions.into_iter().collect();
    actions.sort_by_key(|action| (action.order, action.role.priority(), action.id.0.clone()));
    let all_ids: HashMap<String, u16> = actions
        .iter()
        .map(|action| (action.id.0.clone(), action.order))
        .collect();
    let id_counts = actions.iter().fold(HashMap::new(), |mut counts, action| {
        *counts.entry(action.id.0.clone()).or_insert(0) += 1;
        counts
    });
    let mut seen_ids = HashSet::new();
    let mut seen_targets = HashSet::new();
    let mut normalized: Vec<_> = actions
        .into_iter()
        .filter(|action| seen_ids.insert(action.id.0.clone()))
        .filter(|action| action.valid_owned(&all_ids, &id_counts))
        .filter(|action| {
            action
                .target_key()
                .is_some_and(|key| seen_targets.insert(key))
        })
        .take(8)
        .collect();

    // Target deduplication and the action cap can remove an otherwise valid prerequisite. Remove
    // every dependent of that missing prerequisite, repeating until the returned graph is closed.
    loop {
        let retained_ids: HashMap<_, _> = normalized
            .iter()
            .map(|action| (action.id.0.clone(), action.order))
            .collect();
        let retained_counts = normalized
            .iter()
            .fold(HashMap::new(), |mut counts, action| {
                *counts.entry(action.id.0.clone()).or_insert(0) += 1;
                counts
            });
        let before = normalized.len();
        normalized.retain(|action| action.valid_owned(&retained_ids, &retained_counts));
        if normalized.len() == before {
            return normalized;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn target(query: &str) -> NextActionTarget {
        let params = SearchTextParams {
            query: query.into(),
            ..SearchTextParams::default()
        };
        NextActionTarget::SearchText(params)
    }

    fn action(id: &str, order: u16, target: NextActionTarget) -> NextAction {
        NextAction {
            id: NextActionId(id.into()),
            role: NextActionRole::VerifyExact,
            order,
            dependencies: vec![],
            target,
            reason: "verify the exact result".into(),
        }
    }

    #[test]
    fn serializes_target_as_tagged_exact_arguments() {
        let value = serde_json::to_value(action("next:1", 0, target("needle"))).unwrap();
        assert_eq!(value["tool"], "search_text");
        assert_eq!(value["arguments"]["query"], "needle");
    }

    #[test]
    fn origin_is_tagged_and_non_recursive() {
        let params = SearchTextParams {
            query: "needle".into(),
            ..SearchTextParams::default()
        };
        let value =
            serde_json::to_value(NextActionOrigin(ReplayOriginTarget::SearchText(params))).unwrap();
        assert_eq!(value["tool"], "search_text");
        assert!(value.get("origin").is_none());
    }

    #[test]
    fn normalizes_invalid_duplicate_and_over_cap_actions() {
        let mut actions = vec![action("bad", 0, target("bad"))];
        actions[0].dependencies.push(NextActionDependency {
            mode: NextActionDependencyMode::All,
            action_ids: vec![NextActionId("missing".into())],
        });
        for number in 0..10 {
            actions.push(action(
                &format!("next:{number}"),
                1,
                target(&format!("needle-{number}")),
            ));
        }
        actions.push(action("duplicate", 0, target("needle-0")));
        let normalized = normalize_next_actions(actions);
        assert_eq!(normalized.len(), 8);
        assert!(normalized.iter().all(|item| item.id.0 != "bad"));
        assert!(normalized.iter().any(|item| item.id.0 == "duplicate"));
        assert!(normalized.iter().all(|item| item.id.0 != "next:0"));
    }

    fn depends_on(id: &str) -> Vec<NextActionDependency> {
        vec![NextActionDependency {
            mode: NextActionDependencyMode::All,
            action_ids: vec![NextActionId(id.into())],
        }]
    }

    #[test]
    fn drops_dependents_of_targets_removed_by_deduplication() {
        let prerequisite = action("first", 0, target("same-target"));
        let duplicate = action("deduped", 1, target("same-target"));
        let mut dependent = action("dependent", 2, target("distinct-target"));
        dependent.dependencies = depends_on("deduped");

        let normalized = normalize_next_actions([prerequisite, duplicate, dependent]);
        assert_eq!(normalized.len(), 1);
        assert_eq!(normalized[0].id.0, "first");
    }

    #[test]
    fn drops_dependents_of_targets_removed_by_the_action_cap() {
        let mut actions: Vec<_> = (0..8)
            .map(|number| {
                action(
                    &format!("kept:{number}"),
                    number,
                    target(&format!("q:{number}")),
                )
            })
            .collect();
        actions.push(action("capped", 8, target("capped-target")));
        let mut dependent = action("dependent", 9, target("dependent-target"));
        dependent.dependencies = depends_on("capped");
        actions.push(dependent);

        let normalized = normalize_next_actions(actions);
        assert_eq!(normalized.len(), 8);
        assert!(normalized.iter().all(|item| item.id.0 != "dependent"));
    }

    #[test]
    fn every_core_target_uses_the_tagged_tool_arguments_wire_shape() {
        let cases = [
            ("workspace", json!({})),
            ("list_files", json!({})),
            ("read_file", json!({"path": "src/lib.rs"})),
            (
                "read_match",
                json!({"result_handle": "result-1", "match_id": "search:m1"}),
            ),
            (
                "explore",
                json!({"path": "src/lib.rs", "operation": "probe"}),
            ),
            ("search_text", json!({"query": "needle"})),
            ("search_hybrid", json!({"query": "needle"})),
            ("search_symbol", json!({"query": "needle"})),
            ("search_batch", json!({"probes": []})),
            ("find_references", json!({})),
            ("go_to_definition", json!({})),
            ("find_declarations", json!({})),
            ("find_implementations", json!({})),
            ("incoming_calls", json!({})),
            ("outgoing_calls", json!({})),
            ("document_symbols", json!({"path": "src/lib.rs"})),
            ("inspect_syntax_tree", json!({"path": "src/lib.rs"})),
            ("search_structural", json!({"query": "(function_item)"})),
            ("impact_bundle", json!({"symbol": "needle"})),
        ];

        for (tool, arguments) in cases {
            let target: NextActionTarget = serde_json::from_value(json!({
                "tool": tool,
                "arguments": arguments,
            }))
            .unwrap_or_else(|error| panic!("{tool} target must deserialize: {error}"));
            let encoded = serde_json::to_value(target).unwrap();
            assert_eq!(encoded["tool"], tool);
            assert!(encoded["arguments"].is_object());
        }
    }

    #[test]
    fn every_origin_target_is_typed_and_excludes_read_match() {
        let cases = [
            (
                "explore",
                json!({"path": "src/lib.rs", "operation": "probe"}),
            ),
            ("search_text", json!({"query": "needle"})),
            ("search_hybrid", json!({"query": "needle"})),
            ("search_symbol", json!({"query": "needle"})),
            ("search_batch", json!({"probes": []})),
            ("find_references", json!({})),
            ("go_to_definition", json!({})),
            ("find_declarations", json!({})),
            ("find_implementations", json!({})),
            ("incoming_calls", json!({})),
            ("outgoing_calls", json!({})),
            ("document_symbols", json!({"path": "src/lib.rs"})),
            ("inspect_syntax_tree", json!({"path": "src/lib.rs"})),
            ("search_structural", json!({"query": "(function_item)"})),
            ("impact_bundle", json!({"symbol": "needle"})),
        ];

        for (tool, arguments) in cases {
            let origin: ReplayOriginTarget = serde_json::from_value(json!({
                "tool": tool,
                "arguments": arguments,
            }))
            .unwrap_or_else(|error| panic!("{tool} origin must deserialize: {error}"));
            assert_eq!(serde_json::to_value(origin).unwrap()["tool"], tool);
        }
        assert!(
            serde_json::from_value::<ReplayOriginTarget>(json!({
                "tool": "read_match",
                "arguments": {"result_handle": "result-1", "match_id": "search:m1"},
            }))
            .is_err()
        );
    }
}
