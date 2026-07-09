//! Optional `impact_bundle` convenience composition.
//!
//! Composes existing symbol / references / callers / implementations tools without replacing them.

use super::*;
use crate::mcp::types::{
    FindImplementationsParams, FindReferencesParams, ImpactBundleParams, ImpactBundleResponse,
    IncomingCallsParams, SearchSymbolParams, SearchSymbolPathClass, SuggestedNext,
};

impl FriggMcpServer {
    pub(in crate::mcp::server) async fn impact_bundle_impl(
        &self,
        params: ImpactBundleParams,
    ) -> Result<Json<ImpactBundleResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("impact_bundle", params.repository_id.clone());
        let symbol = params.symbol.trim().to_owned();
        if symbol.is_empty() {
            let recovery = RecoveryFields {
                error_code: Some("MISSING_SYMBOL".to_owned()),
                message: Some("impact_bundle requires a non-empty symbol.".to_owned()),
                correction_hint: Some(
                    "Pass symbol=<name> (runtime path_class is the default).".to_owned(),
                ),
                related_tools: vec![
                    "search_symbol".to_owned(),
                    "find_references".to_owned(),
                    "incoming_calls".to_owned(),
                ],
                suggested_next: vec![
                    SuggestedNext::tool("search_symbol")
                        .with_path_class("runtime")
                        .with_reason("resolve a symbol name before impact_bundle"),
                ],
                zero_hit_reason: Some(ZeroHitReason::QueryMiss),
                scope: None,
                index: None,
            };
            return Ok(Json(ImpactBundleResponse {
                symbol: String::new(),
                path_class: "runtime".to_owned(),
                symbols: Vec::new(),
                symbols_result_handle: None,
                references: Vec::new(),
                references_result_handle: None,
                references_mode: NavigationMode::UnavailableNoPrecise,
                incoming_calls: Vec::new(),
                incoming_calls_result_handle: None,
                incoming_calls_mode: NavigationMode::UnavailableNoPrecise,
                implementations: Vec::new(),
                implementations_result_handle: None,
                implementations_mode: None,
                implementations_included: false,
                // Single channel: recovery.suggested_next only (flattened).
                recovery,
            }));
        }

        let path_class = params.path_class.unwrap_or(SearchSymbolPathClass::Runtime);
        let path_class_label = match path_class {
            SearchSymbolPathClass::Runtime => "runtime",
            SearchSymbolPathClass::Project => "project",
            SearchSymbolPathClass::Support => "support",
            SearchSymbolPathClass::Any => "any",
        }
        .to_owned();

        let symbols_response = self
            .search_symbol_impl(SearchSymbolParams {
                query: symbol.clone(),
                repository_id: params.repository_id.clone(),
                path_class: Some(path_class),
                path_regex: None,
                limit: None,
                response_mode: params.response_mode,
            })
            .await?
            .0;

        if symbols_response.matches.is_empty() {
            let mut recovery = symbols_response.recovery;
            if recovery.is_empty() {
                recovery = RecoveryFields::for_zero_hit(ZeroHitInput {
                    tool: "impact_bundle",
                    query: Some(symbol.as_str()),
                    pattern_type_is_literal: None,
                    scope: None,
                    index: None,
                    reason_override: None,
                });
            }
            if recovery.suggested_next.is_empty() {
                recovery.suggested_next = vec![
                    SuggestedNext::tool("search_symbol")
                        .with_symbol(symbol.clone())
                        .with_path_class(path_class_label.clone())
                        .with_reason("retry symbol lookup with broader path_class if needed"),
                    SuggestedNext::tool("search_text")
                        .with_query(symbol.clone())
                        .with_reason("textual fallback when symbol index misses"),
                ];
            }
            let provenance_result = self
                .record_provenance_blocking(
                    "impact_bundle",
                    execution_context.repository_hint.as_deref(),
                    json!({
                        "symbol": Self::bounded_text(&symbol),
                        "path_class": path_class_label,
                        "repository_id": execution_context.repository_hint,
                    }),
                    json!({
                        "symbols_count": 0,
                        "references_count": 0,
                        "incoming_calls_count": 0,
                        "implementations_included": false,
                    }),
                    &Ok::<(), ErrorData>(()),
                )
                .await;
            let response = ImpactBundleResponse {
                symbol,
                path_class: path_class_label,
                symbols: Vec::new(),
                symbols_result_handle: symbols_response.result_handle,
                references: Vec::new(),
                references_result_handle: None,
                references_mode: NavigationMode::UnavailableNoPrecise,
                incoming_calls: Vec::new(),
                incoming_calls_result_handle: None,
                incoming_calls_mode: NavigationMode::UnavailableNoPrecise,
                implementations: Vec::new(),
                implementations_result_handle: None,
                implementations_mode: None,
                implementations_included: false,
                recovery,
            };
            return self.finalize_read_only_tool(
                &execution_context,
                Ok(Json(response)),
                provenance_result,
            );
        }

        let selected_symbol = symbols_response
            .matches
            .first()
            .expect("non-empty symbols response should have a first match")
            .clone();
        let selected_repository_id = Some(selected_symbol.repository_id.clone());
        let selected_path = Some(selected_symbol.path.clone());
        let selected_line = Some(selected_symbol.line);
        let selected_column = selected_symbol.column;

        let references_response = self
            .find_references_impl(FindReferencesParams {
                symbol: None,
                repository_id: selected_repository_id.clone(),
                path: selected_path.clone(),
                line: selected_line,
                column: selected_column,
                include_definition: Some(false),
                include_follow_up_structural: None,
                limit: None,
                response_mode: params.response_mode,
            })
            .await?
            .0;

        let incoming_response = self
            .incoming_calls_impl(IncomingCallsParams {
                symbol: None,
                repository_id: selected_repository_id.clone(),
                path: selected_path.clone(),
                line: selected_line,
                column: selected_column,
                include_follow_up_structural: None,
                limit: None,
                response_mode: params.response_mode,
            })
            .await?
            .0;

        let primary_kind = selected_symbol.kind.to_ascii_lowercase();
        let kind_wants_impls = primary_kind.contains("trait")
            || primary_kind.contains("interface")
            || primary_kind == "protocol";
        let include_implementations =
            params.include_implementations.unwrap_or(false) || kind_wants_impls;

        let (implementations, implementations_result_handle, implementations_mode) =
            if include_implementations {
                let impl_response = self
                    .find_implementations_impl(FindImplementationsParams {
                        symbol: None,
                        repository_id: selected_repository_id.clone(),
                        path: selected_path.clone(),
                        line: selected_line,
                        column: selected_column,
                        include_follow_up_structural: None,
                        limit: None,
                        response_mode: params.response_mode,
                    })
                    .await?
                    .0;
                (
                    impl_response.matches,
                    impl_response.result_handle,
                    Some(impl_response.mode),
                )
            } else {
                (Vec::new(), None, None)
            };

        // Success path: still one channel — next steps live in recovery.suggested_next only.
        let mut suggested_next = vec![
            SuggestedNext::tool("search_text")
                .with_query(symbol.clone())
                .with_path_regex("^tests/")
                .with_reason("optional tests textual pass for impact"),
            SuggestedNext::tool("read_match").with_reason(
                "proof-read strongest reference/caller clusters via handles from this bundle",
            ),
            SuggestedNext::tool("read_file")
                .with_reason("body proof for outgoing callees or ambiguous clusters"),
        ];
        if !include_implementations {
            suggested_next.insert(
                0,
                SuggestedNext::tool("find_implementations")
                    .with_symbol(symbol.clone())
                    .with_reason("include trait/interface implementations when relevant"),
            );
        }
        let recovery = RecoveryFields {
            suggested_next,
            ..RecoveryFields::default()
        };

        let response = ImpactBundleResponse {
            symbol: symbol.clone(),
            path_class: path_class_label,
            symbols: symbols_response.matches,
            symbols_result_handle: symbols_response.result_handle,
            references: references_response.matches,
            references_result_handle: references_response.result_handle,
            references_mode: references_response.mode,
            incoming_calls: incoming_response.matches,
            incoming_calls_result_handle: incoming_response.result_handle,
            incoming_calls_mode: incoming_response.mode,
            implementations,
            implementations_result_handle,
            implementations_mode,
            implementations_included: include_implementations,
            recovery,
        };

        let provenance_result = self
            .record_provenance_blocking(
                "impact_bundle",
                execution_context.repository_hint.as_deref(),
                json!({
                    "symbol": Self::bounded_text(&symbol),
                    "path_class": response.path_class,
                    "repository_id": execution_context.repository_hint,
                    "include_implementations": include_implementations,
                    "selected_repository_id": selected_symbol.repository_id,
                    "selected_path": selected_symbol.path,
                    "selected_line": selected_symbol.line,
                    "selected_column": selected_symbol.column,
                }),
                json!({
                    "symbols_count": response.symbols.len(),
                    "references_count": response.references.len(),
                    "incoming_calls_count": response.incoming_calls.len(),
                    "implementations_count": response.implementations.len(),
                    "implementations_included": response.implementations_included,
                }),
                &Ok::<(), ErrorData>(()),
            )
            .await;
        self.finalize_read_only_tool(&execution_context, Ok(Json(response)), provenance_result)
    }
}
