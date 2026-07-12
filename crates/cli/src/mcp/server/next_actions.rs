#![allow(dead_code)] // T004 wires these finalizers into every response and structured-error path.

//! Active-surface validation for canonical next actions.
//!
//! This module only validates advisory follow-ups before they leave the server. It deliberately
//! does not dispatch tools, replay origins, or mutate the registered router.

use std::collections::HashSet;

use rmcp::handler::server::router::tool::ToolRouter;
use tracing::warn;

use super::FriggMcpServer;
use crate::mcp::types::{
    ExploreOperation, NextAction, NextActionTarget, RecoveryFields, normalize_next_actions,
};

impl FriggMcpServer {
    /// Filter canonical actions against this server's live, profile-filtered router.
    ///
    /// Invalid advisory rows are suppressed rather than turning an otherwise useful primary
    /// response into an error. This is intentionally separate from action production so every
    /// response and structured-error producer can use the same final gate.
    pub(super) fn validate_next_actions(
        &self,
        actions: impl IntoIterator<Item = NextAction>,
    ) -> Vec<NextAction> {
        validate_next_actions_for_router(&self.tool_router, actions)
    }

    /// Finalize a recovery payload after its producer has assembled canonical actions.
    ///
    /// `set_next_actions` regenerates the deprecated projection from the retained canonical rows,
    /// preventing server emission from ever exposing disagreeing canonical and legacy lists.
    pub(super) fn validate_recovery_actions(&self, recovery: &mut RecoveryFields) {
        let actions = std::mem::take(&mut recovery.next_actions);
        recovery.set_next_actions(self.validate_next_actions(actions));
    }
}

/// Filter canonical actions against a specific live router. Kept narrow for server response and
/// structured-error producers, and testable with a filtered router without constructing a server.
pub(super) fn validate_next_actions_for_router(
    router: &ToolRouter<FriggMcpServer>,
    actions: impl IntoIterator<Item = NextAction>,
) -> Vec<NextAction> {
    let actions = actions.into_iter().collect::<Vec<_>>();
    let input_count = actions.len();
    let retained = actions
        .into_iter()
        .filter(|action| {
            router.get(action.target.tool_name()).is_some()
                && target_has_required_fields(&action.target)
        })
        .collect::<Vec<_>>();
    let normalized = normalize_next_actions(retained);
    let suppressed = input_count.saturating_sub(normalized.len());
    if suppressed != 0 {
        // Do not include action reason/arguments here: they can contain user queries or source
        // snippets. The count is bounded to avoid unbounded diagnostic cardinality.
        warn!(
            suppressed_actions = suppressed.min(8),
            "suppressed invalid or unavailable canonical next actions"
        );
    }
    normalized
}

fn target_has_required_fields(target: &NextActionTarget) -> bool {
    match target {
        NextActionTarget::Workspace(_) | NextActionTarget::ListFiles(_) => true,
        NextActionTarget::ReadFile(params) => non_empty(&params.path),
        NextActionTarget::ReadMatch(params) => {
            non_empty(&params.result_handle) && non_empty(&params.match_id)
        }
        NextActionTarget::Explore(params) => {
            non_empty(&params.path)
                && match params.operation {
                    ExploreOperation::Probe => non_empty_optional(params.query.as_deref()),
                    ExploreOperation::Zoom => valid_explore_anchor(params.anchor.as_ref()),
                    ExploreOperation::Refine => {
                        valid_explore_anchor(params.anchor.as_ref())
                            && non_empty_optional(params.query.as_deref())
                    }
                }
        }
        NextActionTarget::SearchText(params) => non_empty(&params.query),
        NextActionTarget::SearchHybrid(params) => non_empty(&params.query),
        NextActionTarget::SearchSymbol(params) => non_empty(&params.query),
        NextActionTarget::SearchBatch(params) => {
            (2..=8).contains(&params.probes.len())
                && params
                    .probes
                    .iter()
                    .all(|probe| non_empty(&probe.id) && non_empty(&probe.query))
                && params
                    .probes
                    .iter()
                    .map(|probe| probe.id.as_str())
                    .collect::<HashSet<_>>()
                    .len()
                    == params.probes.len()
        }
        NextActionTarget::FindReferences(params) => valid_navigation_target(
            params.target.as_ref(),
            params.symbol.as_deref(),
            params.path.as_deref(),
            params.line,
            params.column,
        ),
        NextActionTarget::GoToDefinition(params) => valid_navigation_target(
            params.target.as_ref(),
            params.symbol.as_deref(),
            params.path.as_deref(),
            params.line,
            params.column,
        ),
        NextActionTarget::FindDeclarations(params) => valid_navigation_target(
            params.target.as_ref(),
            params.symbol.as_deref(),
            params.path.as_deref(),
            params.line,
            params.column,
        ),
        NextActionTarget::FindImplementations(params) => valid_navigation_target(
            params.target.as_ref(),
            params.symbol.as_deref(),
            params.path.as_deref(),
            params.line,
            params.column,
        ),
        NextActionTarget::IncomingCalls(params) => valid_navigation_target(
            params.target.as_ref(),
            params.symbol.as_deref(),
            params.path.as_deref(),
            params.line,
            params.column,
        ),
        NextActionTarget::OutgoingCalls(params) => valid_navigation_target(
            params.target.as_ref(),
            params.symbol.as_deref(),
            params.path.as_deref(),
            params.line,
            params.column,
        ),
        NextActionTarget::DocumentSymbols(params) => non_empty(&params.path),
        NextActionTarget::InspectSyntaxTree(params) => non_empty(&params.path),
        NextActionTarget::SearchStructural(params) => non_empty(&params.query),
        NextActionTarget::ImpactBundle(params) => {
            params.target.is_some() || non_empty(&params.symbol)
        }
    }
}

fn non_empty(value: &str) -> bool {
    !value.trim().is_empty()
}

fn non_empty_optional(value: Option<&str>) -> bool {
    value.is_some_and(non_empty)
}

fn valid_explore_anchor(anchor: Option<&crate::mcp::types::ExploreAnchor>) -> bool {
    let Some(anchor) = anchor else {
        return false;
    };
    anchor.start_line > 0
        && anchor.start_column > 0
        && anchor.end_line >= anchor.start_line
        && anchor.end_column > 0
}

fn valid_navigation_target(
    target: Option<&crate::mcp::types::TargetRef>,
    symbol: Option<&str>,
    path: Option<&str>,
    line: Option<usize>,
    column: Option<usize>,
) -> bool {
    if target.is_some() {
        return symbol.is_none() && path.is_none() && line.is_none() && column.is_none();
    }
    match symbol {
        Some(symbol) => non_empty(symbol),
        None => path.is_some_and(non_empty) && line.is_some_and(|line| line > 0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::tool_surface::ToolSurfaceProfile;
    use crate::mcp::types::{
        ExploreAnchor, GoToDefinitionParams, ImpactBundleParams, NextActionDependency,
        NextActionDependencyMode, NextActionId, NextActionRole, SearchTextParams, TargetRef,
    };
    use serde_json::json;

    fn action(id: &str, target: NextActionTarget) -> NextAction {
        NextAction {
            id: NextActionId(id.to_owned()),
            role: NextActionRole::VerifyExact,
            order: 0,
            dependencies: Vec::new(),
            target,
            reason: "continue with an exact request".to_owned(),
        }
    }

    fn text_target(query: &str) -> NextActionTarget {
        NextActionTarget::SearchText(SearchTextParams {
            query: query.to_owned(),
            ..SearchTextParams::default()
        })
    }

    #[test]
    fn active_filtered_router_accepts_core_targets_and_drops_unavailable_targets() {
        let mut router = FriggMcpServer::filtered_tool_router(ToolSurfaceProfile::Core);
        let accepted =
            validate_next_actions_for_router(&router, [action("text", text_target("needle"))]);
        assert_eq!(accepted.len(), 1);

        router.remove_route("search_text");
        let suppressed =
            validate_next_actions_for_router(&router, [action("text", text_target("needle"))]);
        assert!(suppressed.is_empty());
    }

    #[test]
    fn target_bearing_navigation_actions_validate_without_reconstructed_inputs() {
        let router = FriggMcpServer::filtered_tool_router(ToolSurfaceProfile::Core);
        let target = TargetRef::result_match(
            "result-000001".to_owned(),
            "search:m1".to_owned(),
            "session-scope".to_owned(),
        )
        .expect("non-empty target");
        let retained = validate_next_actions_for_router(
            &router,
            [
                action(
                    "definition",
                    NextActionTarget::GoToDefinition(GoToDefinitionParams {
                        target: Some(target.clone()),
                        ..GoToDefinitionParams::default()
                    }),
                ),
                action(
                    "impact",
                    NextActionTarget::ImpactBundle(ImpactBundleParams {
                        target: Some(target),
                        ..ImpactBundleParams::default()
                    }),
                ),
            ],
        );
        assert_eq!(retained.len(), 2);
    }

    #[test]
    fn target_bearing_navigation_actions_reject_legacy_columns() {
        let router = FriggMcpServer::filtered_tool_router(ToolSurfaceProfile::Core);
        let target = TargetRef::result_match(
            "result-000001".to_owned(),
            "search:m1".to_owned(),
            "session-scope".to_owned(),
        )
        .expect("non-empty target");
        let retained = validate_next_actions_for_router(
            &router,
            [action(
                "definition",
                NextActionTarget::GoToDefinition(GoToDefinitionParams {
                    target: Some(target),
                    column: Some(1),
                    ..GoToDefinitionParams::default()
                }),
            )],
        );
        assert!(retained.is_empty());
    }

    #[test]
    fn every_core_target_variant_is_accepted_on_core_and_extended_routers() {
        let targets = [
            json!({"tool": "workspace", "arguments": {}}),
            json!({"tool": "list_files", "arguments": {}}),
            json!({"tool": "read_file", "arguments": {"path": "src/lib.rs"}}),
            json!({"tool": "read_match", "arguments": {"result_handle": "result-1", "match_id": "search:m1"}}),
            json!({"tool": "explore", "arguments": {"path": "src/lib.rs", "operation": "probe", "query": "needle"}}),
            json!({"tool": "search_text", "arguments": {"query": "needle"}}),
            json!({"tool": "search_hybrid", "arguments": {"query": "needle"}}),
            json!({"tool": "search_symbol", "arguments": {"query": "needle"}}),
            json!({"tool": "search_batch", "arguments": {"probes": [
                {"id": "one", "kind": "text", "query": "needle"},
                {"id": "two", "kind": "symbol", "query": "Needle"}
            ]}}),
            json!({"tool": "find_references", "arguments": {"symbol": "needle"}}),
            json!({"tool": "go_to_definition", "arguments": {"symbol": "needle"}}),
            json!({"tool": "find_declarations", "arguments": {"symbol": "needle"}}),
            json!({"tool": "find_implementations", "arguments": {"symbol": "needle"}}),
            json!({"tool": "incoming_calls", "arguments": {"symbol": "needle"}}),
            json!({"tool": "outgoing_calls", "arguments": {"symbol": "needle"}}),
            json!({"tool": "document_symbols", "arguments": {"path": "src/lib.rs"}}),
            json!({"tool": "inspect_syntax_tree", "arguments": {"path": "src/lib.rs"}}),
            json!({"tool": "search_structural", "arguments": {"query": "(function_item)"}}),
            json!({"tool": "impact_bundle", "arguments": {"symbol": "needle"}}),
        ];

        for profile in [ToolSurfaceProfile::Core, ToolSurfaceProfile::Extended] {
            let router = FriggMcpServer::filtered_tool_router(profile);
            for (index, value) in targets.iter().cloned().enumerate() {
                let target = serde_json::from_value::<NextActionTarget>(value)
                    .expect("typed core target must deserialize");
                let retained = validate_next_actions_for_router(
                    &router,
                    [action(&format!("target:{index}"), target)],
                );
                assert_eq!(
                    retained.len(),
                    1,
                    "{profile:?} must retain every core target variant"
                );
            }
        }
    }

    #[test]
    fn invalid_required_target_fields_and_dependents_are_suppressed() {
        let router = FriggMcpServer::filtered_tool_router(ToolSurfaceProfile::Core);
        let invalid = action("invalid", text_target(" "));
        let mut dependent = action("dependent", text_target("surviving query"));
        dependent.order = 1;
        dependent.dependencies = vec![NextActionDependency {
            mode: NextActionDependencyMode::All,
            action_ids: vec![NextActionId("invalid".to_owned())],
        }];

        assert!(validate_next_actions_for_router(&router, [invalid, dependent]).is_empty());
    }

    #[test]
    fn recovery_projection_is_regenerated_after_active_surface_filtering() {
        let server = FriggMcpServer::new_with_runtime_options(
            crate::settings::FriggConfig::default(),
            false,
        );
        let mut recovery = RecoveryFields::default();
        recovery.next_actions = vec![action("valid", text_target("needle"))];
        let RecoveryFields { suggested_next, .. } = &mut recovery;
        suggested_next.push(crate::mcp::types::SuggestedNext {
            tool: "workspace".to_owned(),
            ..crate::mcp::types::SuggestedNext::default()
        });

        server.validate_recovery_actions(&mut recovery);
        let RecoveryFields {
            next_actions,
            suggested_next,
            ..
        } = &recovery;
        assert_eq!(next_actions.len(), 1);
        assert_eq!(suggested_next.len(), 1);
        assert_eq!(suggested_next[0].tool, "search_text");
    }

    #[test]
    fn explore_and_navigation_required_fields_are_checked_without_execution() {
        let router = FriggMcpServer::filtered_tool_router(ToolSurfaceProfile::Core);
        let invalid_explore = NextActionTarget::Explore(crate::mcp::types::ExploreParams {
            path: "src/lib.rs".to_owned(),
            repository_id: None,
            operation: ExploreOperation::Zoom,
            query: None,
            pattern_type: None,
            anchor: None,
            context_lines: None,
            max_matches: None,
            resume_from: None,
            continuation: None,
            presentation_mode: None,
            include_context_efficiency: None,
        });
        let valid_explore = NextActionTarget::Explore(crate::mcp::types::ExploreParams {
            anchor: Some(ExploreAnchor {
                start_line: 1,
                start_column: 1,
                end_line: 1,
                end_column: 1,
            }),
            ..match invalid_explore {
                NextActionTarget::Explore(ref params) => params.clone(),
                _ => unreachable!(),
            }
        });
        let invalid_navigation =
            NextActionTarget::FindReferences(crate::mcp::types::FindReferencesParams::default());

        let retained = validate_next_actions_for_router(
            &router,
            [
                action("invalid-explore", invalid_explore),
                action("valid-explore", valid_explore),
                action("invalid-navigation", invalid_navigation),
            ],
        );
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].id.0, "valid-explore");
    }
}
