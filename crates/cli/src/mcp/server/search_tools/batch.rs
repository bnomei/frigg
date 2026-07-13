//! `search_batch` multi-probe merge tool.
//!
//! Runs 2..=8 typed probes (text / symbol / hybrid) **concurrently**, merges and
//! dedupes by path:line:column, and returns probe summaries plus batch recovery.
//! Per-probe match caps bound total index work (`PER_PROBE_MATCH_CAP * n`).
//! Shared-scope multi-query fusion and early-exit saturation are not implemented
//! (still concurrent independent probes).

use super::*;
use crate::mcp::server_cache::ContinuationBinding;
use crate::mcp::types::{
    ContinuationValidationError, LatencyClass, NextActionOrigin, NextActionRole, NextActionTarget,
    ReadFileParams, ReadMatchParams, RecoveryFields, ReplayOriginTarget, ResultCompleteness,
    ResultIncompleteReason, ResultTruncationReason, ResultUnit, SearchBatchEvidence,
    SearchBatchMatch, SearchBatchMatchStrength, SearchBatchMergeStrategy, SearchBatchParams,
    SearchBatchProbe, SearchBatchProbeKind, SearchBatchProbeSummary, SearchBatchProbeTrust,
    SearchBatchResponse, SearchHybridParams, SearchSymbolParams, SearchSymbolPathClass,
    SearchTextParams, ZeroHitReason, ZeroHitScope, canonical_next_action,
};
use crate::path_class::repository_path_class;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const MIN_PROBES: usize = 2;
const MAX_PROBES: usize = 8;
/// Cap per-probe work so a batch cannot explode index cost.
const PER_PROBE_MATCH_CAP: usize = 40;
/// Implicit total work budget: per-probe cap × probe count (≤ `MAX_PROBES`).
const _TOTAL_MATCH_BUDGET: usize = PER_PROBE_MATCH_CAP * MAX_PROBES;
const SEARCH_BATCH_MERGE_ALGORITHM_VERSION: &str = "rrf-v1";
/// Version every continuation binds so a future coordinate-key change cannot reuse an old page.
const SEARCH_BATCH_DEDUPE_KEY_VERSION: &str = "repository-path-line-column-v1";
const SEARCH_BATCH_RRF_K: usize = 60;
const SEARCH_BATCH_STABLE_ORDER_VERSION: &str = "consensus-rrf-strength-coordinate-evidence-v1";

type SearchBatchProbeOutput =
    Result<Vec<(SearchBatchProbeSummary, Vec<SearchBatchMatch>)>, ErrorData>;
type SearchBatchProbeFuture<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = SearchBatchProbeOutput> + Send + 'a>>;

impl FriggMcpServer {
    pub(crate) async fn search_batch_impl(
        &self,
        params: SearchBatchParams,
    ) -> Result<Json<SearchBatchResponse>, ErrorData> {
        ContinuationValidationError::reject_mixed_cursor_forms(
            params.resume_from.is_some(),
            params.continuation.is_some(),
        )
        .map_err(|error| self.search_batch_continuation_error(&params, error))?;
        let probe_count = params.probes.len();
        if !(MIN_PROBES..=MAX_PROBES).contains(&probe_count) {
            return Err(Self::invalid_params(
                format!(
                    "search_batch requires between {MIN_PROBES} and {MAX_PROBES} probes (got {probe_count})"
                ),
                Some(serde_json::json!({
                    "probe_count": probe_count,
                    "min_probes": MIN_PROBES,
                    "max_probes": MAX_PROBES,
                })),
            ));
        }

        let mut seen_ids = std::collections::BTreeSet::new();
        for probe in &params.probes {
            let id = probe.id.trim();
            if id.is_empty() {
                return Err(Self::invalid_params(
                    "each probe must have a non-empty id",
                    None,
                ));
            }
            if !seen_ids.insert(id.to_owned()) {
                return Err(Self::invalid_params(
                    format!("duplicate probe id {id:?}"),
                    Some(serde_json::json!({ "probe_id": id })),
                ));
            }
            if probe.query.trim().is_empty() {
                return Err(Self::invalid_params(
                    format!("probe {id:?} query must not be empty"),
                    Some(serde_json::json!({ "probe_id": id })),
                ));
            }
        }

        let limit = params
            .limit
            .unwrap_or(self.config.max_search_results.min(30))
            .min(self.config.max_search_results.max(1))
            .max(1);
        let (resume_from, continuation_binding) =
            self.search_batch_continuation_context(&params)?;
        let response_mode = params.response_mode;
        let compatibility_note = params.merge.map(|_| {
            "Deprecated merge=rank_by_probe_hit_strength was normalized to fixed reciprocal_rank_fusion. This compatibility input will be removed after two minor releases.".to_owned()
        });

        let probe_outcomes = self
            .search_batch_run_probes_concurrent(
                &params.probes,
                params.repository_id.as_deref(),
                response_mode,
            )
            .await?;

        let mut probe_summaries = Vec::with_capacity(probe_outcomes.len());
        let merged = merge_batch_rows(probe_outcomes, &mut probe_summaries);

        let total_merged = merged.len();
        let page: Vec<SearchBatchMatch> =
            merged.into_iter().skip(resume_from).take(limit).collect();
        let returned = page.len();
        let merge_page_omitted = resume_from + returned < total_merged;
        let child_truncated = probe_summaries
            .iter()
            .any(|summary| summary.completeness.truncated);
        let child_incomplete = probe_summaries
            .iter()
            .any(|summary| !summary.completeness.complete);
        let exact_merged_total = (!child_incomplete).then_some(total_merged);
        let truncated = child_truncated || merge_page_omitted;
        // A capped or diagnostic child makes coverage incomplete, but it does not make the
        // already merged rows unstable. Continue that known, snapshot-bound merged page rather
        // than forcing callers to lose rows solely because the underlying query was bounded.
        let continuation = (merge_page_omitted && continuation_binding.is_some())
            .then(|| {
                continuation_binding.clone().map(|mut binding| {
                    binding.next_position = resume_from.saturating_add(returned);
                    self.store_session_continuation(binding)
                })
            })
            .flatten();
        let mut truncation_reasons = Vec::new();
        if child_truncated {
            truncation_reasons.push(ResultTruncationReason::ChildLimit);
        }
        if merge_page_omitted {
            truncation_reasons.push(ResultTruncationReason::MergePageLimit);
        }
        let mut incomplete_reasons = Vec::new();
        for summary in &probe_summaries {
            if !summary.completeness.complete {
                incomplete_reasons.push(ResultIncompleteReason::ChildIncomplete);
                incomplete_reasons.extend(summary.completeness.incomplete_reasons.iter().copied());
            }
        }
        let completeness = ResultCompleteness::try_new(
            ResultUnit::BatchProbe,
            returned,
            exact_merged_total,
            !child_incomplete && !merge_page_omitted,
            truncated,
            truncation_reasons,
            incomplete_reasons,
            continuation,
        )
        .expect("batch completeness must preserve child completeness invariants");
        let next_resume = merge_page_omitted.then_some(resume_from.saturating_add(returned));

        let mut response = SearchBatchResponse {
            matches: page,
            probe_summary: probe_summaries,
            merge_strategy: SearchBatchMergeStrategy::ReciprocalRankFusion,
            merge_algorithm_version: SEARCH_BATCH_MERGE_ALGORITHM_VERSION.to_owned(),
            compatibility_note,
            completeness,
            returned,
            truncated,
            resume_from: next_resume,
            result_handle: None,
            handle_scope: None,
            handle_expires: None,
            latency_class: Some(if params.probes.len() <= 3 {
                LatencyClass::Warm
            } else {
                LatencyClass::Cold
            }),
            recovery: RecoveryFields::default(),
        };

        response.result_handle =
            self.assign_result_handle_for_batch_matches("search_batch", &mut response.matches);
        if response.result_handle.is_some() {
            response.handle_scope = Some("batch".to_owned());
            response.handle_expires = Some("session".to_owned());
        }
        for summary in &mut response.probe_summary {
            if summary.hits != 0 {
                // Successful child responses are rebound into one aggregate batch handle below.
                // Their child proof/target actions address private child handles and must not be
                // copied into the public probe summary. The aggregate recovery is built from the
                // final top-level row after rebinding.
                summary.set_next_actions(Vec::new());
            }
        }

        let all_zero = response
            .probe_summary
            .iter()
            .all(|summary| summary.hits == 0);
        if all_zero {
            let strongest_reason = strongest_batch_zero_reason(&response.probe_summary);
            let batch_actions = response
                .probe_summary
                .iter()
                .flat_map(|summary| summary.next_actions.iter().cloned())
                .take(6)
                .collect::<Vec<_>>();
            response.recovery =
                RecoveryFields::batch_all_zero_actions(strongest_reason, batch_actions);
        } else if let Some(top) = response.matches.first() {
            let mut actions = Vec::new();
            if let (Some(result_handle), Some(match_id)) =
                (response.result_handle.as_ref(), top.match_id.as_ref())
            {
                actions.push(canonical_next_action(
                    "batch-proof",
                    NextActionRole::ProofRead,
                    0,
                    NextActionTarget::ReadMatch(ReadMatchParams {
                        result_handle: result_handle.clone(),
                        match_id: match_id.clone(),
                        before: None,
                        after: None,
                        presentation_mode: None,
                        include_context_efficiency: None,
                        origin: Some(NextActionOrigin(ReplayOriginTarget::SearchBatch(
                            params.clone(),
                        ))),
                    }),
                    "proof-read the strongest merged batch hit",
                ));
            }
            actions.push(canonical_next_action(
                "batch-file",
                NextActionRole::Inspect,
                1,
                NextActionTarget::ReadFile(ReadFileParams {
                    path: top.path.clone(),
                    repository_id: Some(top.repository_id.clone()),
                    max_bytes: None,
                    start_line: None,
                    end_line: None,
                    line_count: None,
                    presentation_mode: None,
                    include_context_efficiency: None,
                }),
                "bounded proof read of the strongest merged hit",
            ));
            let next_order = actions
                .iter()
                .map(|action| action.order)
                .max()
                .map_or(0, |order| order.saturating_add(1));
            let mut target_actions = top
                .target_ref
                .as_ref()
                .filter(|target| self.target_is_semantically_resolvable(target))
                .map_or_else(Vec::new, |target| {
                    Self::target_follow_up_actions(Some(target))
                });
            for (offset, action) in target_actions.iter_mut().enumerate() {
                action.order = next_order.saturating_add(offset as u16);
            }
            actions.extend(target_actions);
            response.recovery.set_next_actions(actions);
        }
        self.validate_recovery_actions(&mut response.recovery);

        Ok(Json(response))
    }

    /// Build one continuation identity across every effective child scope. We recompute child
    /// rows on each page, so the token retains only this identity and the merged offset.
    fn search_batch_continuation_context(
        &self,
        params: &SearchBatchParams,
    ) -> Result<(usize, Option<ContinuationBinding>), ErrorData> {
        let mut repository_hints = std::collections::BTreeSet::new();
        for probe in &params.probes {
            repository_hints.insert(
                probe
                    .repository_id
                    .clone()
                    .or_else(|| params.repository_id.clone()),
            );
        }
        let mut repository_ids = Vec::new();
        let mut snapshot_fingerprints = Vec::new();
        for repository_hint in repository_hints {
            let scoped = self.scoped_read_only_tool_execution_context(
                "search_batch",
                repository_hint,
                RepositoryResponseCacheFreshnessMode::ManifestOnly,
            )?;
            repository_ids.extend(scoped.scoped_repository_ids);
            snapshot_fingerprints.extend(
                scoped
                    .cache_freshness
                    .scopes
                    .as_ref()
                    .into_iter()
                    .flatten()
                    .map(|scope| format!("{}:{}", scope.repository_id, scope.snapshot_id)),
            );
        }
        repository_ids.sort();
        repository_ids.dedup();
        snapshot_fingerprints.sort();
        snapshot_fingerprints.dedup();
        let request_digest = Self::search_batch_continuation_digest(params, &repository_ids);
        let resume_from = match params.continuation.as_deref() {
            Some(token) => self
                .session_continuation_lookup(
                    token,
                    "search_batch",
                    &request_digest,
                    &repository_ids,
                    &snapshot_fingerprints,
                    ResultUnit::BatchProbe,
                )
                .map(|binding| binding.next_position)
                .map_err(|error| self.search_batch_continuation_error(params, error))?,
            None => params.resume_from.unwrap_or(0),
        };
        let binding = (!snapshot_fingerprints.is_empty()).then_some(ContinuationBinding {
            tool: "search_batch",
            request_digest,
            repository_ids,
            snapshot_fingerprints,
            unit: ResultUnit::BatchProbe,
            next_position: 0,
        });
        Ok((resume_from, binding))
    }

    fn search_batch_continuation_digest(
        params: &SearchBatchParams,
        repository_ids: &[String],
    ) -> String {
        let mut request = serde_json::to_value(params)
            .expect("search batch parameters must serialize for continuation binding");
        if let Some(object) = request.as_object_mut() {
            object.remove("resume_from");
            object.remove("continuation");
        }
        let normalized = serde_json::json!({
            "tool": "search_batch",
            "request": request,
            "repository_ids": repository_ids,
            "rrf_k": SEARCH_BATCH_RRF_K,
            "merge_algorithm_version": SEARCH_BATCH_MERGE_ALGORITHM_VERSION,
            "dedupe_key_version": SEARCH_BATCH_DEDUPE_KEY_VERSION,
            "stable_order_version": SEARCH_BATCH_STABLE_ORDER_VERSION,
        });
        let mut hasher = DefaultHasher::new();
        normalized.to_string().hash(&mut hasher);
        format!("batch-v2:{:016x}", hasher.finish())
    }

    /// Continuation rejection must offer the exact producer request, never a stale cursor or
    /// batch match id. A fresh execution is the only safe way to rebuild its merged ordering.
    fn search_batch_continuation_error(
        &self,
        params: &SearchBatchParams,
        error: ContinuationValidationError,
    ) -> ErrorData {
        let mut retry = params.clone();
        retry.resume_from = None;
        retry.continuation = None;
        let action = canonical_next_action(
            "batch-continuation-retry",
            NextActionRole::Retry,
            0,
            NextActionTarget::SearchBatch(retry),
            "re-run the original batch request to obtain a fresh snapshot-bound continuation",
        );
        Self::invalid_params(
            error.message.clone(),
            Some(serde_json::json!({
                "continuation": error,
                "next_actions": [action],
            })),
        )
    }

    /// Run probes concurrently (binary join tree) so multi-probe batches improve
    /// wall-clock vs sequential MCP fan-out. Order of `probe_summary` matches request order.
    async fn search_batch_run_probes_concurrent(
        &self,
        probes: &[SearchBatchProbe],
        batch_repository_id: Option<&str>,
        response_mode: Option<crate::mcp::types::ResponseMode>,
    ) -> Result<Vec<(SearchBatchProbeSummary, Vec<SearchBatchMatch>)>, ErrorData> {
        self.search_batch_run_probes_concurrent_boxed(probes, batch_repository_id, response_mode)
            .await
    }

    fn search_batch_run_probes_concurrent_boxed<'a>(
        &'a self,
        probes: &'a [SearchBatchProbe],
        batch_repository_id: Option<&'a str>,
        response_mode: Option<crate::mcp::types::ResponseMode>,
    ) -> SearchBatchProbeFuture<'a> {
        Box::pin(async move {
            match probes {
                [] => Ok(Vec::new()),
                [only] => {
                    let one = self
                        .search_batch_run_probe(only, batch_repository_id, response_mode)
                        .await?;
                    Ok(vec![one])
                }
                _ => {
                    let mid = probes.len() / 2;
                    let (left, right) = probes.split_at(mid);
                    let (left_out, right_out) = tokio::join!(
                        self.search_batch_run_probes_concurrent_boxed(
                            left,
                            batch_repository_id,
                            response_mode
                        ),
                        self.search_batch_run_probes_concurrent_boxed(
                            right,
                            batch_repository_id,
                            response_mode
                        ),
                    );
                    let mut out = left_out?;
                    out.extend(right_out?);
                    Ok(out)
                }
            }
        })
    }

    /// Run one batch probe. Nested search tools mint session handles that the batch response does
    /// not return — drop them immediately so they cannot thrash the session handle budget.
    async fn search_batch_run_probe(
        &self,
        probe: &SearchBatchProbe,
        batch_repository_id: Option<&str>,
        response_mode: Option<crate::mcp::types::ResponseMode>,
    ) -> Result<(SearchBatchProbeSummary, Vec<SearchBatchMatch>), ErrorData> {
        let repository_id = probe
            .repository_id
            .clone()
            .or_else(|| batch_repository_id.map(ToOwned::to_owned));
        let per_limit = Some(PER_PROBE_MATCH_CAP);

        match probe.kind {
            SearchBatchProbeKind::Text => {
                let text_params = SearchTextParams {
                    query: probe.query.clone(),
                    pattern_type: probe.pattern_type.clone(),
                    repository_id,
                    path_regex: probe.path_regex.clone(),
                    limit: per_limit,
                    glob: probe.glob.clone(),
                    response_mode,
                    ..Default::default()
                };
                let result = self.search_text_impl(text_params).await?;
                let body = result.0;
                if let Some(handle) = body.result_handle.as_deref() {
                    self.drop_session_result_handle(handle);
                }
                let hits = body.total_matches;
                let rows = body
                    .matches
                    .into_iter()
                    .enumerate()
                    .map(|(index, matched)| SearchBatchMatch {
                        match_id: None,
                        target_ref: None,
                        stable_symbol_id: None,
                        probe_ids: vec![probe.id.clone()],
                        kind: SearchBatchProbeKind::Text,
                        evidence: vec![batch_evidence(probe, index + 1)],
                        consensus_count: 1,
                        rrf_score: reciprocal_rank_fusion(&[index + 1]),
                        match_strength: SearchBatchMatchStrength::ExactLiteral,
                        repository_id: matched.repository_id,
                        path: matched.path.clone(),
                        line: matched.line,
                        column: Some(matched.column),
                        excerpt: Some(matched.excerpt),
                        path_class: Some(repository_path_class(&matched.path).to_owned()),
                        symbol: None,
                    })
                    .collect::<Vec<_>>();
                let mut summary = SearchBatchProbeSummary::canonical(
                    probe.id.clone(),
                    SearchBatchProbeKind::Text,
                    hits,
                    body.completeness,
                    body.recovery.zero_hit_reason,
                    body.recovery.correction_hint,
                    body.recovery.scope.or_else(|| probe_scope(probe)),
                );
                summary.set_next_actions(body.recovery.next_actions);
                Ok((summary, rows))
            }
            SearchBatchProbeKind::Symbol => {
                let symbol_params = SearchSymbolParams {
                    query: probe.query.clone(),
                    repository_id,
                    path_class: Some(probe.path_class.unwrap_or(SearchSymbolPathClass::Runtime)),
                    path_regex: probe.path_regex.clone(),
                    limit: per_limit,
                    continuation: None,
                    response_mode,
                };
                let result = self.search_symbol_impl(symbol_params).await?;
                let body = result.0;
                if let Some(handle) = body.result_handle.as_deref() {
                    self.drop_session_result_handle(handle);
                }
                let hits = body.completeness.total.unwrap_or(body.matches.len());
                let rows = body
                    .matches
                    .into_iter()
                    .enumerate()
                    .map(|(index, matched)| SearchBatchMatch {
                        match_id: None,
                        target_ref: None,
                        stable_symbol_id: matched.stable_symbol_id,
                        probe_ids: vec![probe.id.clone()],
                        kind: SearchBatchProbeKind::Symbol,
                        evidence: vec![batch_evidence(probe, index + 1)],
                        consensus_count: 1,
                        rrf_score: reciprocal_rank_fusion(&[index + 1]),
                        match_strength: SearchBatchMatchStrength::IndexedSymbol,
                        repository_id: matched.repository_id,
                        path: matched.path.clone(),
                        line: matched.line,
                        column: matched.column,
                        excerpt: matched.excerpt.or(matched.signature.clone()),
                        path_class: matched
                            .path_class
                            .or_else(|| Some(repository_path_class(&matched.path).to_owned())),
                        symbol: Some(matched.symbol),
                    })
                    .collect::<Vec<_>>();
                let mut summary = SearchBatchProbeSummary::canonical(
                    probe.id.clone(),
                    SearchBatchProbeKind::Symbol,
                    hits,
                    body.completeness,
                    body.recovery.zero_hit_reason,
                    body.recovery.correction_hint,
                    body.recovery.scope.or_else(|| probe_scope(probe)),
                );
                summary.set_next_actions(body.recovery.next_actions);
                Ok((summary, rows))
            }
            SearchBatchProbeKind::Hybrid => {
                let hybrid_params = SearchHybridParams {
                    query: probe.query.clone(),
                    repository_id: repository_id.clone(),
                    language: None,
                    limit: per_limit,
                    weights: None,
                    semantic: None,
                    response_mode,
                    include_context_efficiency: None,
                };
                let result = self.search_hybrid_impl(hybrid_params).await?;
                let body = result.0;
                if let Some(handle) = body.result_handle.as_deref() {
                    self.drop_session_result_handle(handle);
                }
                let hits = body.matches.len();
                let rows = body
                    .matches
                    .into_iter()
                    .enumerate()
                    .map(|(index, matched)| SearchBatchMatch {
                        match_id: None,
                        target_ref: None,
                        stable_symbol_id: None,
                        probe_ids: vec![probe.id.clone()],
                        kind: SearchBatchProbeKind::Hybrid,
                        evidence: vec![batch_evidence(probe, index + 1)],
                        consensus_count: 1,
                        rrf_score: reciprocal_rank_fusion(&[index + 1]),
                        match_strength: SearchBatchMatchStrength::RankedHybrid,
                        repository_id: matched.repository_id,
                        path: matched.path.clone(),
                        line: matched.line,
                        column: Some(matched.column),
                        excerpt: Some(matched.excerpt),
                        path_class: matched
                            .path_class
                            .map(|class| class.as_str().to_owned())
                            .or_else(|| Some(repository_path_class(&matched.path).to_owned())),
                        symbol: None,
                    })
                    .collect::<Vec<_>>();
                let mut summary = SearchBatchProbeSummary::canonical(
                    probe.id.clone(),
                    SearchBatchProbeKind::Hybrid,
                    hits,
                    body.completeness,
                    body.recovery
                        .zero_hit_reason
                        .or_else(|| (hits == 0).then_some(ZeroHitReason::IndexedSearchComplete)),
                    body.recovery.correction_hint.or_else(|| {
                        (hits == 0).then(|| {
                            "Hybrid is discovery-only; pivot to search_text/search_symbol."
                                .to_owned()
                        })
                    }),
                    body.recovery.scope.or_else(|| probe_scope(probe)),
                );
                summary.set_next_actions(body.recovery.next_actions);
                if hits == 0 && summary.next_actions.is_empty() {
                    summary.set_next_actions([
                        canonical_next_action(
                            "batch-hybrid-symbol",
                            NextActionRole::VerifyExact,
                            0,
                            NextActionTarget::SearchSymbol(SearchSymbolParams {
                                query: probe.query.clone(),
                                repository_id: repository_id.clone(),
                                path_class: Some(SearchSymbolPathClass::Runtime),
                                path_regex: probe.path_regex.clone(),
                                limit: None,
                                continuation: None,
                                response_mode,
                            }),
                            "exact symbol pivot after hybrid probe zero",
                        ),
                        canonical_next_action(
                            "batch-hybrid-text",
                            NextActionRole::VerifyExact,
                            1,
                            NextActionTarget::SearchText(SearchTextParams {
                                query: probe.query.clone(),
                                repository_id: repository_id.clone(),
                                path_regex: probe.path_regex.clone(),
                                glob: probe.glob.clone(),
                                pattern_type: probe.pattern_type.clone(),
                                ..SearchTextParams::default()
                            }),
                            "exact text pivot after hybrid probe zero",
                        ),
                    ]);
                }
                Ok((summary, rows))
            }
        }
    }
}

fn batch_evidence(probe: &SearchBatchProbe, rank_one_based: usize) -> SearchBatchEvidence {
    let trust = match probe.kind {
        SearchBatchProbeKind::Text => SearchBatchProbeTrust::LexicalText,
        SearchBatchProbeKind::Symbol => SearchBatchProbeTrust::IndexedSymbol,
        SearchBatchProbeKind::Hybrid => SearchBatchProbeTrust::RankedHybrid,
    };
    SearchBatchEvidence {
        probe_id: probe.id.clone(),
        kind: probe.kind,
        rank_one_based,
        trust,
    }
}

fn reciprocal_rank_fusion(ranks: &[usize]) -> f64 {
    ranks
        .iter()
        .map(|rank| 1.0 / (SEARCH_BATCH_RRF_K + rank) as f64)
        .sum()
}

fn merge_batch_rows(
    probe_outcomes: Vec<(SearchBatchProbeSummary, Vec<SearchBatchMatch>)>,
    probe_summaries: &mut Vec<SearchBatchProbeSummary>,
) -> Vec<SearchBatchMatch> {
    let mut merged = Vec::new();
    let mut dedupe_index = std::collections::HashMap::new();
    // Outcomes are returned in request order. Retain that order independently of
    // probe ids: ids are caller-chosen labels, not a ranking signal.
    let probe_request_order: std::collections::HashMap<String, usize> = probe_outcomes
        .iter()
        .enumerate()
        .map(|(request_order, (summary, _))| (summary.id.clone(), request_order))
        .collect();

    for (summary, rows) in probe_outcomes {
        for mut row in rows {
            let key = (
                row.repository_id.clone(),
                row.path.clone(),
                row.line,
                row.column.unwrap_or(0),
            );
            if let Some(&index) = dedupe_index.get(&key) {
                let existing: &mut SearchBatchMatch = &mut merged[index];
                for evidence in row.evidence.drain(..) {
                    if !existing
                        .evidence
                        .iter()
                        .any(|retained| retained.probe_id == evidence.probe_id)
                    {
                        existing.evidence.push(evidence);
                    }
                }
                // The first row is the earliest request-order contributor. It owns the
                // representative, while later contributors only fill absent optional fields.
                if existing.excerpt.is_none() {
                    existing.excerpt = row.excerpt.take();
                }
                if existing.symbol.is_none() {
                    existing.symbol = row.symbol.take();
                }
                if existing.path_class.is_none() {
                    existing.path_class = row.path_class.take();
                }
                if existing.stable_symbol_id.is_none() {
                    existing.stable_symbol_id = row.stable_symbol_id.take();
                }
            } else {
                let index = merged.len();
                dedupe_index.insert(key, index);
                merged.push(row);
            }
        }
        probe_summaries.push(summary);
    }

    for row in &mut merged {
        row.evidence.sort_by_key(|evidence| {
            *probe_request_order
                .get(&evidence.probe_id)
                .expect("merged evidence always belongs to a requested probe")
        });
        row.probe_ids = row
            .evidence
            .iter()
            .map(|evidence| evidence.probe_id.clone())
            .collect();
        row.consensus_count = row.probe_ids.len();
        row.rrf_score = reciprocal_rank_fusion(
            &row.evidence
                .iter()
                .map(|evidence| evidence.rank_one_based)
                .collect::<Vec<_>>(),
        );
        row.match_strength = row
            .evidence
            .iter()
            .map(|evidence| SearchBatchMatchStrength::from_kind(evidence.kind))
            .max()
            .expect("merged rows always retain evidence");
    }
    merged.sort_by(|left, right| {
        right
            .consensus_count
            .cmp(&left.consensus_count)
            .then_with(|| right.rrf_score.total_cmp(&left.rrf_score))
            .then_with(|| right.match_strength.cmp(&left.match_strength))
            .then_with(|| left.repository_id.cmp(&right.repository_id))
            .then_with(|| left.path.cmp(&right.path))
            .then_with(|| left.line.cmp(&right.line))
            .then_with(|| left.column.cmp(&right.column))
            .then_with(|| left.evidence.cmp(&right.evidence))
    });
    merged
}

/// Echo only filters actually applied by the probe kind (not raw request fields).
fn strongest_batch_zero_reason(summaries: &[SearchBatchProbeSummary]) -> Option<ZeroHitReason> {
    const PRIORITY: &[ZeroHitReason] = &[
        ZeroHitReason::IndexStalePossible,
        ZeroHitReason::NoIndexCoverage,
        ZeroHitReason::ScopeExcludedAllCandidates,
        ZeroHitReason::QueryLooksLikeRegex,
        ZeroHitReason::WrongRepositoryPossible,
        ZeroHitReason::PathClassNotIndexed,
        ZeroHitReason::PreciseGraphUnavailable,
        ZeroHitReason::IndexedSearchComplete,
        ZeroHitReason::QueryMiss,
        ZeroHitReason::ToolUnavailable,
    ];
    for reason in PRIORITY {
        if summaries
            .iter()
            .any(|summary| summary.zero_hit_reason == Some(*reason))
        {
            return Some(*reason);
        }
    }
    summaries.iter().find_map(|summary| summary.zero_hit_reason)
}

fn probe_scope(probe: &SearchBatchProbe) -> Option<ZeroHitScope> {
    let mut scope = ZeroHitScope::default();
    match probe.kind {
        SearchBatchProbeKind::Text => {
            if let Some(path_regex) = probe.path_regex.as_ref() {
                scope = scope.with_path_regex(path_regex.clone());
            }
            if let Some(glob) = probe.glob.as_ref() {
                scope = scope.with_glob(glob.clone());
            }
        }
        SearchBatchProbeKind::Symbol => {
            if let Some(path_regex) = probe.path_regex.as_ref() {
                scope = scope.with_path_regex(path_regex.clone());
            }
            let path_class = probe.path_class.unwrap_or(SearchSymbolPathClass::Runtime);
            scope = scope.with_path_class(path_class.as_str());
        }
        SearchBatchProbeKind::Hybrid => {}
    }
    if let Some(repository_id) = probe.repository_id.as_ref() {
        scope = scope.with_repository_id(repository_id.clone());
    }
    if scope.is_empty() { None } else { Some(scope) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_count_bounds_are_documented() {
        assert_eq!(MIN_PROBES, 2);
        assert_eq!(MAX_PROBES, 8);
    }

    #[test]
    fn search_batch_rrf_consensus_strength_and_evidence_are_deterministic() {
        let complete = ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0)
            .expect("empty probe fixture is complete");
        let row = |probe_id: &str,
                   kind: SearchBatchProbeKind,
                   rank: usize,
                   path: &str|
         -> SearchBatchMatch {
            SearchBatchMatch {
                match_id: None,
                target_ref: None,
                stable_symbol_id: None,
                probe_ids: vec![probe_id.to_owned()],
                kind,
                evidence: vec![SearchBatchEvidence {
                    probe_id: probe_id.to_owned(),
                    kind,
                    rank_one_based: rank,
                    trust: match kind {
                        SearchBatchProbeKind::Text => SearchBatchProbeTrust::LexicalText,
                        SearchBatchProbeKind::Symbol => SearchBatchProbeTrust::IndexedSymbol,
                        SearchBatchProbeKind::Hybrid => SearchBatchProbeTrust::RankedHybrid,
                    },
                }],
                consensus_count: 1,
                rrf_score: 0.0,
                match_strength: SearchBatchMatchStrength::RankedHybrid,
                repository_id: "repo".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: Some(1),
                excerpt: None,
                path_class: None,
                symbol: None,
            }
        };
        let mut summaries = Vec::new();
        let merged = merge_batch_rows(
            vec![
                (
                    SearchBatchProbeSummary::canonical(
                        "text".to_owned(),
                        SearchBatchProbeKind::Text,
                        2,
                        complete.clone(),
                        None,
                        None,
                        None,
                    ),
                    vec![
                        row("text", SearchBatchProbeKind::Text, 1, "shared.rs"),
                        row("text", SearchBatchProbeKind::Text, 2, "text.rs"),
                    ],
                ),
                (
                    SearchBatchProbeSummary::canonical(
                        "symbol".to_owned(),
                        SearchBatchProbeKind::Symbol,
                        1,
                        complete,
                        None,
                        None,
                        None,
                    ),
                    vec![row("symbol", SearchBatchProbeKind::Symbol, 1, "shared.rs")],
                ),
            ],
            &mut summaries,
        );
        assert_eq!(
            merged[0].path, "shared.rs",
            "consensus sorts before strength"
        );
        assert_eq!(merged[0].consensus_count, 2);
        assert_eq!(merged[0].evidence.len(), 2);
        assert_eq!(
            merged[0].match_strength,
            SearchBatchMatchStrength::ExactLiteral
        );
        assert!((merged[0].rrf_score - (1.0 / 61.0 + 1.0 / 61.0)).abs() < 1e-12);
        assert_eq!(
            merged[0]
                .evidence
                .iter()
                .map(|evidence| evidence.probe_id.as_str())
                .collect::<Vec<_>>(),
            vec!["text", "symbol"],
            "evidence retains request order rather than lexical probe-id order"
        );
        assert_eq!(merged[1].path, "text.rs");
        assert_eq!(
            merged[1].match_strength,
            SearchBatchMatchStrength::ExactLiteral
        );
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn search_batch_rrf_uses_strength_then_canonical_ties_and_dedupes_probe_evidence() {
        let complete = ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0)
            .expect("empty probe fixture is complete");
        let row = |probe_id: &str,
                   kind: SearchBatchProbeKind,
                   rank: usize,
                   path: &str|
         -> SearchBatchMatch {
            SearchBatchMatch {
                match_id: None,
                target_ref: None,
                stable_symbol_id: None,
                probe_ids: vec![probe_id.to_owned()],
                kind,
                evidence: vec![SearchBatchEvidence {
                    probe_id: probe_id.to_owned(),
                    kind,
                    rank_one_based: rank,
                    trust: match kind {
                        SearchBatchProbeKind::Text => SearchBatchProbeTrust::LexicalText,
                        SearchBatchProbeKind::Symbol => SearchBatchProbeTrust::IndexedSymbol,
                        SearchBatchProbeKind::Hybrid => SearchBatchProbeTrust::RankedHybrid,
                    },
                }],
                consensus_count: 1,
                rrf_score: 0.0,
                match_strength: SearchBatchMatchStrength::RankedHybrid,
                repository_id: "repo".to_owned(),
                path: path.to_owned(),
                line: 1,
                column: Some(1),
                excerpt: None,
                path_class: None,
                symbol: None,
            }
        };
        let mut summaries = Vec::new();
        let merged = merge_batch_rows(
            vec![
                (
                    SearchBatchProbeSummary::canonical(
                        "text".to_owned(),
                        SearchBatchProbeKind::Text,
                        5,
                        complete.clone(),
                        None,
                        None,
                        None,
                    ),
                    vec![
                        row("text", SearchBatchProbeKind::Text, 1, "exact.rs"),
                        row("text", SearchBatchProbeKind::Text, 1, "b.rs"),
                        row("text", SearchBatchProbeKind::Text, 1, "a.rs"),
                        row("text", SearchBatchProbeKind::Text, 1, "duplicate.rs"),
                        row("text", SearchBatchProbeKind::Text, 2, "duplicate.rs"),
                    ],
                ),
                (
                    SearchBatchProbeSummary::canonical(
                        "symbol".to_owned(),
                        SearchBatchProbeKind::Symbol,
                        1,
                        complete,
                        None,
                        None,
                        None,
                    ),
                    vec![row("symbol", SearchBatchProbeKind::Symbol, 1, "symbol.rs")],
                ),
            ],
            &mut summaries,
        );

        let paths = merged
            .iter()
            .map(|row| row.path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            paths,
            vec!["a.rs", "b.rs", "duplicate.rs", "exact.rs", "symbol.rs"],
            "equal consensus and RRF rows use derived strength before canonical coordinates"
        );
        assert_eq!(
            merged.last().expect("symbol row retained").match_strength,
            SearchBatchMatchStrength::IndexedSymbol,
            "exact literal strength wins the equal-RRF tie against indexed symbols"
        );
        let duplicate = merged
            .iter()
            .find(|row| row.path == "duplicate.rs")
            .expect("duplicate coordinate retained once");
        assert_eq!(duplicate.consensus_count, 1);
        assert_eq!(duplicate.probe_ids, vec!["text"]);
        assert_eq!(duplicate.evidence.len(), 1);
        assert_eq!(duplicate.evidence[0].rank_one_based, 1);
        assert!((duplicate.rrf_score - 1.0 / 61.0).abs() < 1e-12);
        assert_eq!(summaries.len(), 2);
    }

    #[test]
    fn strongest_batch_zero_reason_prefers_actionable_codes() {
        let summaries = vec![
            SearchBatchProbeSummary::canonical(
                "a".to_owned(),
                SearchBatchProbeKind::Text,
                0,
                ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0)
                    .expect("empty text probe is complete"),
                Some(ZeroHitReason::QueryMiss),
                None,
                None,
            ),
            SearchBatchProbeSummary::canonical(
                "b".to_owned(),
                SearchBatchProbeKind::Text,
                0,
                ResultCompleteness::complete(ResultUnit::Occurrence, 0, 0)
                    .expect("empty text probe is complete"),
                Some(ZeroHitReason::ScopeExcludedAllCandidates),
                None,
                None,
            ),
        ];
        assert_eq!(
            strongest_batch_zero_reason(&summaries),
            Some(ZeroHitReason::ScopeExcludedAllCandidates)
        );
    }

    #[test]
    fn batch_all_zero_recovery_is_not_multi_hypothesis() {
        let recovery = RecoveryFields::batch_all_zero(
            Some(ZeroHitReason::ScopeExcludedAllCandidates),
            Vec::new(),
        );
        assert_eq!(recovery.error_code.as_deref(), Some("BATCH_ALL_ZERO"));
        assert_eq!(
            recovery.zero_hit_reason,
            Some(ZeroHitReason::ScopeExcludedAllCandidates)
        );
        assert!(
            recovery
                .correction_hint
                .as_ref()
                .is_some_and(|hint| !hint.contains("Prefer search_batch")),
            "post-batch recovery must not recommend re-entering multi_hypothesis routing"
        );
    }

    #[test]
    fn probe_scope_echoes_only_applied_filters_per_kind() {
        let hybrid = SearchBatchProbe {
            id: "h".to_owned(),
            kind: SearchBatchProbeKind::Hybrid,
            query: "foo".to_owned(),
            path_regex: Some("^src/".to_owned()),
            glob: Some("**/*.rs".to_owned()),
            path_class: Some(SearchSymbolPathClass::Runtime),
            repository_id: Some("repo-1".to_owned()),
            pattern_type: None,
        };
        let hybrid_scope = probe_scope(&hybrid).expect("repo scope");
        assert_eq!(hybrid_scope.repository_id.as_deref(), Some("repo-1"));
        assert!(hybrid_scope.path_regex.is_none());
        assert!(hybrid_scope.glob.is_none());
        assert!(hybrid_scope.path_class.is_none());

        let text = SearchBatchProbe {
            id: "t".to_owned(),
            kind: SearchBatchProbeKind::Text,
            query: "foo".to_owned(),
            path_regex: Some("^src/".to_owned()),
            glob: Some("**/*.rs".to_owned()),
            path_class: Some(SearchSymbolPathClass::Support),
            repository_id: None,
            pattern_type: None,
        };
        let text_scope = probe_scope(&text).expect("text filters");
        assert_eq!(text_scope.path_regex.as_deref(), Some("^src/"));
        assert_eq!(text_scope.glob.as_deref(), Some("**/*.rs"));
        assert!(
            text_scope.path_class.is_none(),
            "text probes do not apply path_class"
        );

        let symbol = SearchBatchProbe {
            id: "s".to_owned(),
            kind: SearchBatchProbeKind::Symbol,
            query: "Foo".to_owned(),
            path_regex: Some("^src/".to_owned()),
            glob: Some("**/*.rs".to_owned()),
            path_class: None,
            repository_id: None,
            pattern_type: None,
        };
        let symbol_scope = probe_scope(&symbol).expect("symbol filters");
        assert_eq!(symbol_scope.path_regex.as_deref(), Some("^src/"));
        assert!(symbol_scope.glob.is_none());
        assert_eq!(symbol_scope.path_class.as_deref(), Some("runtime"));
    }
}
