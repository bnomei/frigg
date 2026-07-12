//! Optional `impact_bundle` convenience composition.
//!
//! Composes existing symbol / references / callers / implementations tools without replacing them.

use super::*;
use crate::mcp::types::{
    FindImplementationsParams, FindReferencesParams, ImpactBundleParams, ImpactBundleResponse,
    IncomingCallsParams, NextActionOrigin, NextActionRole, NextActionTarget, ReadFileParams,
    ReadMatchParams, ReplayOriginTarget, ResultCompleteness, ResultIncompleteReason, ResultUnit,
    SearchSymbolParams, SearchSymbolPathClass, SearchTextParams, WorkspaceParams,
    canonical_next_action,
};

fn unavailable_section(unit: ResultUnit) -> ResultCompleteness {
    ResultCompleteness::try_new(
        unit,
        0,
        None,
        false,
        false,
        Vec::new(),
        vec![ResultIncompleteReason::NavigationUnavailable],
        None,
    )
    .expect("unavailable impact section completeness is valid")
}

impl FriggMcpServer {
    pub(in crate::mcp::server) async fn impact_bundle_impl(
        &self,
        params: ImpactBundleParams,
    ) -> Result<Json<ImpactBundleResponse>, ErrorData> {
        let execution_context =
            self.read_only_tool_execution_context("impact_bundle", params.repository_id.clone());
        let symbol = params.symbol.trim().to_owned();
        if symbol.is_empty() {
            let mut recovery = RecoveryFields::default();
            recovery.error_code = Some("MISSING_SYMBOL".to_owned());
            recovery.message = Some("impact_bundle requires a non-empty symbol.".to_owned());
            recovery.correction_hint =
                Some("Pass symbol=<name> (runtime path_class is the default).".to_owned());
            recovery.related_tools = vec![
                "search_symbol".to_owned(),
                "find_references".to_owned(),
                "incoming_calls".to_owned(),
            ];
            recovery.zero_hit_reason = Some(ZeroHitReason::QueryMiss);
            // No exact symbol is known, so use only the argument-free workspace inspection
            // target rather than fabricating a query-bearing symbol retry.
            recovery.set_next_actions([canonical_next_action(
                "impact-workspace",
                NextActionRole::Diagnose,
                0,
                NextActionTarget::Workspace(WorkspaceParams {
                    path: None,
                    repository_id: params.repository_id.clone(),
                    set_default: None,
                    resolve_mode: None,
                }),
                "inspect the active workspace before supplying an exact impact symbol",
            )]);
            self.validate_recovery_actions(&mut recovery);
            return Ok(Json(
                ImpactBundleResponse {
                    symbol: String::new(),
                    path_class: "runtime".to_owned(),
                    summary: ImpactBundleResponse::compute_summary(
                        &[],
                        &[],
                        &[],
                        &[],
                        NavigationMode::UnavailableNoPrecise,
                        NavigationMode::UnavailableNoPrecise,
                        None,
                        false,
                    ),
                    symbols: Vec::new(),
                    symbols_completeness: unavailable_section(ResultUnit::Symbol),
                    symbols_result_handle: None,
                    references: Vec::new(),
                    references_completeness: unavailable_section(ResultUnit::Reference),
                    references_result_handle: None,
                    references_mode: NavigationMode::UnavailableNoPrecise,
                    incoming_calls: Vec::new(),
                    incoming_calls_completeness: unavailable_section(ResultUnit::IncomingCall),
                    incoming_calls_result_handle: None,
                    incoming_calls_mode: NavigationMode::UnavailableNoPrecise,
                    implementations: Vec::new(),
                    implementations_completeness: None,
                    implementations_result_handle: None,
                    implementations_mode: None,
                    implementations_included: false,
                    completeness: unavailable_section(ResultUnit::ImpactSection),
                    recovery,
                }
                .with_computed_summary(),
            ));
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
                continuation: None,
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
            if recovery.next_actions.is_empty() {
                recovery.set_next_actions([
                    canonical_next_action(
                        "impact-symbol-retry",
                        NextActionRole::Retry,
                        0,
                        NextActionTarget::SearchSymbol(SearchSymbolParams {
                            query: symbol.clone(),
                            repository_id: params.repository_id.clone(),
                            path_class: Some(path_class),
                            path_regex: None,
                            limit: None,
                            continuation: None,
                            response_mode: params.response_mode,
                        }),
                        "retry exact symbol lookup with the requested impact scope",
                    ),
                    canonical_next_action(
                        "impact-text-fallback",
                        NextActionRole::VerifyExact,
                        1,
                        NextActionTarget::SearchText(SearchTextParams {
                            query: symbol.clone(),
                            repository_id: params.repository_id.clone(),
                            ..SearchTextParams::default()
                        }),
                        "textual fallback when the symbol index has no matching target",
                    ),
                ]);
            }
            self.validate_recovery_actions(&mut recovery);
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
            let references_completeness = unavailable_section(ResultUnit::Reference);
            let incoming_calls_completeness = unavailable_section(ResultUnit::IncomingCall);
            let completeness = ImpactBundleResponse::aggregate_completeness(&[
                &symbols_response.completeness,
                &references_completeness,
                &incoming_calls_completeness,
            ]);
            let response = ImpactBundleResponse {
                symbol,
                path_class: path_class_label,
                summary: ImpactBundleResponse::compute_summary(
                    &[],
                    &[],
                    &[],
                    &[],
                    NavigationMode::UnavailableNoPrecise,
                    NavigationMode::UnavailableNoPrecise,
                    None,
                    false,
                ),
                symbols: Vec::new(),
                symbols_completeness: symbols_response.completeness.clone(),
                symbols_result_handle: symbols_response.result_handle,
                references: Vec::new(),
                references_completeness,
                references_result_handle: None,
                references_mode: NavigationMode::UnavailableNoPrecise,
                incoming_calls: Vec::new(),
                incoming_calls_completeness,
                incoming_calls_result_handle: None,
                incoming_calls_mode: NavigationMode::UnavailableNoPrecise,
                implementations: Vec::new(),
                implementations_completeness: None,
                implementations_result_handle: None,
                implementations_mode: None,
                implementations_included: false,
                completeness,
                recovery,
            }
            .with_computed_summary();
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
                target: None,
                symbol: None,
                repository_id: selected_repository_id.clone(),
                path: selected_path.clone(),
                line: selected_line,
                column: selected_column,
                include_definition: Some(false),
                include_follow_up_structural: None,
                limit: None,
                continuation: None,
                response_mode: params.response_mode,
            })
            .await?
            .0;

        let incoming_response = self
            .incoming_calls_impl(IncomingCallsParams {
                target: None,
                symbol: None,
                repository_id: selected_repository_id.clone(),
                path: selected_path.clone(),
                line: selected_line,
                column: selected_column,
                include_follow_up_structural: None,
                limit: None,
                continuation: None,
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

        let (
            implementations,
            implementations_completeness,
            implementations_result_handle,
            implementations_mode,
        ) = if include_implementations {
            let impl_response = self
                .find_implementations_impl(FindImplementationsParams {
                    target: None,
                    symbol: None,
                    repository_id: selected_repository_id.clone(),
                    path: selected_path.clone(),
                    line: selected_line,
                    column: selected_column,
                    include_follow_up_structural: None,
                    limit: None,
                    continuation: None,
                    response_mode: params.response_mode,
                })
                .await?
                .0;
            (
                impl_response.matches,
                Some(impl_response.completeness),
                impl_response.result_handle,
                Some(impl_response.mode),
            )
        } else {
            (Vec::new(), None, None, None)
        };

        let mut actions = vec![canonical_next_action(
            "impact-tests",
            NextActionRole::VerifyExact,
            0,
            NextActionTarget::SearchText(SearchTextParams {
                query: symbol.clone(),
                repository_id: params.repository_id.clone(),
                path_regex: Some("^tests/".to_owned()),
                ..SearchTextParams::default()
            }),
            "optional exact tests pass for the selected impact symbol",
        )];
        if !include_implementations {
            actions.insert(
                0,
                canonical_next_action(
                    "impact-implementations",
                    NextActionRole::ResolveTarget,
                    0,
                    NextActionTarget::FindImplementations(FindImplementationsParams {
                        target: None,
                        symbol: Some(symbol.clone()),
                        repository_id: selected_repository_id.clone(),
                        path: selected_path.clone(),
                        line: selected_line,
                        column: selected_column,
                        include_follow_up_structural: None,
                        limit: None,
                        continuation: None,
                        response_mode: params.response_mode,
                    }),
                    "include implementations for the exact selected impact target",
                ),
            );
        }
        let proof = references_response
            .matches
            .first()
            .and_then(|matched| {
                references_response
                    .result_handle
                    .as_ref()
                    .zip(matched.match_id.as_ref())
                    .map(|(result_handle, match_id)| {
                        (
                            result_handle.clone(),
                            match_id.clone(),
                            matched.path.clone(),
                            matched.repository_id.clone(),
                            matched.line,
                        )
                    })
            })
            .or_else(|| {
                incoming_response.matches.first().and_then(|matched| {
                    incoming_response
                        .result_handle
                        .as_ref()
                        .zip(matched.match_id.as_ref())
                        .map(|(result_handle, match_id)| {
                            (
                                result_handle.clone(),
                                match_id.clone(),
                                matched.path.clone(),
                                matched.repository_id.clone(),
                                matched.line,
                            )
                        })
                })
            });
        if let Some((result_handle, match_id, path, repository_id, line)) = proof {
            actions.push(canonical_next_action(
                "impact-proof",
                NextActionRole::ProofRead,
                2,
                NextActionTarget::ReadMatch(ReadMatchParams {
                    result_handle,
                    match_id,
                    before: None,
                    after: None,
                    presentation_mode: None,
                    include_context_efficiency: None,
                    origin: Some(NextActionOrigin(ReplayOriginTarget::ImpactBundle(
                        params.clone(),
                    ))),
                }),
                "proof-read a concrete reference or incoming-call row from this bundle",
            ));
            actions.push(canonical_next_action(
                "impact-file",
                NextActionRole::Inspect,
                3,
                NextActionTarget::ReadFile(ReadFileParams {
                    path,
                    repository_id: Some(repository_id),
                    max_bytes: None,
                    start_line: Some(line),
                    end_line: None,
                    line_count: None,
                    presentation_mode: None,
                    include_context_efficiency: None,
                }),
                "read the concrete source row selected for impact proof",
            ));
        }
        let mut recovery = RecoveryFields::default();
        recovery.set_next_actions(actions);
        self.validate_recovery_actions(&mut recovery);

        let mut sections = vec![
            &symbols_response.completeness,
            &references_response.completeness,
            &incoming_response.completeness,
        ];
        if let Some(implementations_completeness) = implementations_completeness.as_ref() {
            sections.push(implementations_completeness);
        }
        let symbols_completeness = symbols_response.completeness.clone();
        let references_completeness = references_response.completeness.clone();
        let incoming_calls_completeness = incoming_response.completeness.clone();
        let completeness = ImpactBundleResponse::aggregate_completeness(&sections);
        let response = ImpactBundleResponse {
            symbol: symbol.clone(),
            path_class: path_class_label,
            summary: ImpactBundleResponse::compute_summary(
                &symbols_response.matches,
                &references_response.matches,
                &incoming_response.matches,
                &implementations,
                references_response.mode,
                incoming_response.mode,
                implementations_mode,
                include_implementations,
            ),
            symbols: symbols_response.matches,
            symbols_completeness,
            symbols_result_handle: symbols_response.result_handle,
            references: references_response.matches,
            references_completeness,
            references_result_handle: references_response.result_handle,
            references_mode: references_response.mode,
            incoming_calls: incoming_response.matches,
            incoming_calls_completeness,
            incoming_calls_result_handle: incoming_response.result_handle,
            incoming_calls_mode: incoming_response.mode,
            implementations,
            implementations_completeness,
            implementations_result_handle,
            implementations_mode,
            implementations_included: include_implementations,
            completeness,
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
                    "symbols_count": response.summary.symbols_count,
                    "references_count": response.summary.references_count,
                    "incoming_calls_count": response.summary.incoming_calls_count,
                    "implementations_count": response.summary.implementations_count,
                    "implementations_included": response.summary.implementations_included,
                }),
                &Ok::<(), ErrorData>(()),
            )
            .await;
        self.finalize_read_only_tool(&execution_context, Ok(Json(response)), provenance_result)
    }
}
