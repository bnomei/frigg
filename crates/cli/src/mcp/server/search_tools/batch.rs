//! `search_batch` multi-probe merge tool (`FUT-008`).
//!
//! Runs 2..=8 typed probes (text / symbol / hybrid) **concurrently**, merges and
//! dedupes by path:line:column, and returns probe summaries plus batch recovery.
//! Per-probe match caps bound total index work (`PER_PROBE_MATCH_CAP * n`).
//! Shared-scope multi-query fusion and early-exit saturation are not implemented
//! (still concurrent independent probes).

use super::*;
use crate::mcp::types::{
    LatencyClass, RecoveryFields, SearchBatchMatch, SearchBatchMergeMode, SearchBatchParams,
    SearchBatchProbe, SearchBatchProbeKind, SearchBatchProbeSummary, SearchBatchResponse,
    SearchHybridParams, SearchSymbolParams, SearchSymbolPathClass, SearchTextParams, SuggestedNext,
    ZeroHitReason, ZeroHitScope,
};
use crate::path_class::repository_path_class;

const MIN_PROBES: usize = 2;
const MAX_PROBES: usize = 8;
/// Cap per-probe work so a batch cannot explode index cost.
const PER_PROBE_MATCH_CAP: usize = 40;
/// Implicit total work budget: per-probe cap × probe count (≤ `MAX_PROBES`).
const _TOTAL_MATCH_BUDGET: usize = PER_PROBE_MATCH_CAP * MAX_PROBES;

impl FriggMcpServer {
    pub(crate) async fn search_batch_impl(
        &self,
        params: SearchBatchParams,
    ) -> Result<Json<SearchBatchResponse>, ErrorData> {
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
        let resume_from = params.resume_from.unwrap_or(0);
        let response_mode = params.response_mode;
        // Only one merge mode today; accept and apply explicitly so the field is live.
        let _merge = params.merge.unwrap_or(SearchBatchMergeMode::RankByProbeHitStrength);

        let probe_outcomes = self
            .search_batch_run_probes_concurrent(
                &params.probes,
                params.repository_id.as_deref(),
                response_mode,
            )
            .await?;

        let mut probe_summaries = Vec::with_capacity(probe_outcomes.len());
        let mut merged: Vec<SearchBatchMatch> = Vec::new();
        let mut dedupe_index: std::collections::HashMap<(String, String, usize, usize), usize> =
            std::collections::HashMap::new();

        for (summary, rows) in probe_outcomes {
            for mut row in rows {
                let key = (
                    row.repository_id.clone(),
                    row.path.clone(),
                    row.line,
                    row.column.unwrap_or(0),
                );
                if let Some(&idx) = dedupe_index.get(&key) {
                    let existing = &mut merged[idx];
                    for probe_id in row.probe_ids.drain(..) {
                        if !existing.probe_ids.contains(&probe_id) {
                            existing.probe_ids.push(probe_id);
                        }
                    }
                    if row.score > existing.score
                        || (row.score == existing.score
                            && kind_rank(row.kind) < kind_rank(existing.kind))
                    {
                        existing.score = row.score;
                        existing.kind = row.kind;
                        if existing.excerpt.is_none() {
                            existing.excerpt = row.excerpt.take();
                        }
                        if existing.symbol.is_none() {
                            existing.symbol = row.symbol.take();
                        }
                        if existing.path_class.is_none() {
                            existing.path_class = row.path_class.take();
                        }
                    }
                } else {
                    let idx = merged.len();
                    dedupe_index.insert(key, idx);
                    merged.push(row);
                }
            }
            probe_summaries.push(summary);
        }

        // RankByProbeHitStrength: score desc, then kind (symbol > text > hybrid), path, line.
        let _ = _merge;
        merged.sort_by(|left, right| {
            right
                .score
                .partial_cmp(&left.score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| kind_rank(left.kind).cmp(&kind_rank(right.kind)))
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.line.cmp(&right.line))
        });

        let total_merged = merged.len();
        let page: Vec<SearchBatchMatch> = merged
            .into_iter()
            .skip(resume_from)
            .take(limit)
            .collect();
        let returned = page.len();
        let truncated = resume_from + returned < total_merged;
        let next_resume = truncated.then_some(resume_from + returned);

        let mut response = SearchBatchResponse {
            matches: page,
            probe_summary: probe_summaries,
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

        let all_zero = response.probe_summary.iter().all(|summary| summary.hits == 0);
        if all_zero {
            let probes: Vec<&str> = params
                .probes
                .iter()
                .map(|probe| probe.query.as_str())
                .collect();
            response.recovery = RecoveryFields::multi_hypothesis(&probes);
            response.recovery.error_code = Some("BATCH_ALL_ZERO".to_owned());
            response.recovery.message = Some(
                "All search_batch probes returned zero hits; inspect probe_summary for per-probe diagnostics."
                    .to_owned(),
            );
            let mut batch_next = response
                .probe_summary
                .iter()
                .flat_map(|summary| summary.suggested_next.iter().cloned())
                .take(6)
                .collect::<Vec<_>>();
            if batch_next.is_empty() {
                batch_next = response.recovery.suggested_next.clone();
            }
            response.recovery.suggested_next = batch_next;
        } else if let Some(top) = response.matches.first() {
            response.recovery.suggested_next = vec![
                SuggestedNext::tool("read_match")
                    .with_result_handle(response.result_handle.clone().unwrap_or_default())
                    .with_reason("proof-read top batch hit")
                    .with_path(top.path.clone()),
                SuggestedNext::tool("read_file")
                    .with_path(top.path.clone())
                    .with_reason("bounded proof read of strongest merged hit"),
            ];
        }

        Ok(Json(response))
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
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<
                    Output = Result<
                        Vec<(SearchBatchProbeSummary, Vec<SearchBatchMatch>)>,
                        ErrorData,
                    >,
                > + Send
                + 'a,
        >,
    > {
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
                // Nested full search tools mint session handles the batch response does not
                // return; drop them so they cannot thrash the session handle budget.
                if let Some(handle) = body.result_handle.as_deref() {
                    self.drop_session_result_handle(handle);
                }
                let hits = body.total_matches;
                let rows = body
                    .matches
                    .into_iter()
                    .map(|matched| SearchBatchMatch {
                        match_id: None,
                        probe_ids: vec![probe.id.clone()],
                        kind: SearchBatchProbeKind::Text,
                        repository_id: matched.repository_id,
                        path: matched.path.clone(),
                        line: matched.line,
                        column: Some(matched.column),
                        excerpt: Some(matched.excerpt),
                        path_class: Some(repository_path_class(&matched.path).to_owned()),
                        score: text_score(hits.max(1)),
                        symbol: None,
                    })
                    .collect::<Vec<_>>();
                let summary = SearchBatchProbeSummary {
                    id: probe.id.clone(),
                    kind: SearchBatchProbeKind::Text,
                    hits,
                    zero_hit_reason: body.recovery.zero_hit_reason,
                    correction_hint: body.recovery.correction_hint,
                    suggested_next: body.recovery.suggested_next,
                    scope: body.recovery.scope.or_else(|| probe_scope(probe)),
                };
                Ok((summary, rows))
            }
            SearchBatchProbeKind::Symbol => {
                let symbol_params = SearchSymbolParams {
                    query: probe.query.clone(),
                    repository_id,
                    path_class: Some(
                        probe
                            .path_class
                            .unwrap_or(SearchSymbolPathClass::Runtime),
                    ),
                    path_regex: probe.path_regex.clone(),
                    limit: per_limit,
                    response_mode,
                };
                let result = self.search_symbol_impl(symbol_params).await?;
                let body = result.0;
                if let Some(handle) = body.result_handle.as_deref() {
                    self.drop_session_result_handle(handle);
                }
                let hits = body.matches.len();
                let rows = body
                    .matches
                    .into_iter()
                    .map(|matched| SearchBatchMatch {
                        match_id: None,
                        probe_ids: vec![probe.id.clone()],
                        kind: SearchBatchProbeKind::Symbol,
                        repository_id: matched.repository_id,
                        path: matched.path.clone(),
                        line: matched.line,
                        column: matched.column,
                        excerpt: matched.excerpt.or(matched.signature.clone()),
                        path_class: matched
                            .path_class
                            .or_else(|| Some(repository_path_class(&matched.path).to_owned())),
                        score: symbol_score(hits.max(1)),
                        symbol: Some(matched.symbol),
                    })
                    .collect::<Vec<_>>();
                let summary = SearchBatchProbeSummary {
                    id: probe.id.clone(),
                    kind: SearchBatchProbeKind::Symbol,
                    hits,
                    zero_hit_reason: body.recovery.zero_hit_reason,
                    correction_hint: body.recovery.correction_hint,
                    suggested_next: body.recovery.suggested_next,
                    scope: body.recovery.scope.or_else(|| probe_scope(probe)),
                };
                Ok((summary, rows))
            }
            SearchBatchProbeKind::Hybrid => {
                let hybrid_params = SearchHybridParams {
                    query: probe.query.clone(),
                    repository_id,
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
                    .map(|matched| SearchBatchMatch {
                        match_id: None,
                        probe_ids: vec![probe.id.clone()],
                        kind: SearchBatchProbeKind::Hybrid,
                        repository_id: matched.repository_id,
                        path: matched.path.clone(),
                        line: matched.line,
                        column: Some(matched.column),
                        excerpt: Some(matched.excerpt),
                        path_class: matched
                            .path_class
                            .map(|class| class.as_str().to_owned())
                            .or_else(|| Some(repository_path_class(&matched.path).to_owned())),
                        score: hybrid_score(matched.blended_score),
                        symbol: None,
                    })
                    .collect::<Vec<_>>();
                let mut summary = SearchBatchProbeSummary {
                    id: probe.id.clone(),
                    kind: SearchBatchProbeKind::Hybrid,
                    hits,
                    zero_hit_reason: body.recovery.zero_hit_reason.or_else(|| {
                        (hits == 0).then_some(ZeroHitReason::IndexedSearchComplete)
                    }),
                    correction_hint: body.recovery.correction_hint.or_else(|| {
                        (hits == 0).then(|| {
                            "Hybrid is discovery-only; pivot to search_text/search_symbol.".to_owned()
                        })
                    }),
                    suggested_next: body.recovery.suggested_next,
                    scope: body.recovery.scope.or_else(|| probe_scope(probe)),
                };
                if hits == 0 && summary.suggested_next.is_empty() {
                    summary.suggested_next = vec![
                        SuggestedNext::tool("search_symbol")
                            .with_query(probe.query.clone())
                            .with_path_class("runtime")
                            .with_reason("exact symbol pivot after hybrid probe zero"),
                        SuggestedNext::tool("search_text")
                            .with_query(probe.query.clone())
                            .with_reason("exact text pivot after hybrid probe zero"),
                    ];
                }
                Ok((summary, rows))
            }
        }
    }
}

fn kind_rank(kind: SearchBatchProbeKind) -> u8 {
    match kind {
        SearchBatchProbeKind::Symbol => 0,
        SearchBatchProbeKind::Text => 1,
        SearchBatchProbeKind::Hybrid => 2,
    }
}

fn text_score(total_hits: usize) -> f32 {
    10.0 + (total_hits.min(20) as f32) * 0.1
}

fn symbol_score(total_hits: usize) -> f32 {
    20.0 + (total_hits.min(20) as f32) * 0.1
}

fn hybrid_score(blended: f32) -> f32 {
    5.0 + blended.max(0.0)
}

/// Echo only filters actually applied by the probe kind (not raw request fields).
fn probe_scope(probe: &SearchBatchProbe) -> Option<ZeroHitScope> {
    let mut scope = ZeroHitScope::default();
    match probe.kind {
        SearchBatchProbeKind::Text => {
            // Text maps path_regex + glob; path_class is not a SearchTextParams field.
            if let Some(path_regex) = probe.path_regex.as_ref() {
                scope = scope.with_path_regex(path_regex.clone());
            }
            if let Some(glob) = probe.glob.as_ref() {
                scope = scope.with_glob(glob.clone());
            }
        }
        SearchBatchProbeKind::Symbol => {
            // Symbol maps path_regex + path_class (default runtime); glob is unused.
            if let Some(path_regex) = probe.path_regex.as_ref() {
                scope = scope.with_path_regex(path_regex.clone());
            }
            let path_class = probe
                .path_class
                .unwrap_or(SearchSymbolPathClass::Runtime);
            scope = scope.with_path_class(path_class.as_str());
        }
        SearchBatchProbeKind::Hybrid => {
            // Hybrid has no path_regex/glob/path_class params; only repository scope applies.
        }
    }
    if let Some(repository_id) = probe.repository_id.as_ref() {
        scope = scope.with_repository_id(repository_id.clone());
    }
    if scope.is_empty() {
        None
    } else {
        Some(scope)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::types::SearchBatchMergeMode;

    #[test]
    fn probe_count_bounds_are_documented() {
        assert_eq!(MIN_PROBES, 2);
        assert_eq!(MAX_PROBES, 8);
    }

    #[test]
    fn kind_rank_prefers_symbol() {
        assert!(kind_rank(SearchBatchProbeKind::Symbol) < kind_rank(SearchBatchProbeKind::Text));
        assert!(kind_rank(SearchBatchProbeKind::Text) < kind_rank(SearchBatchProbeKind::Hybrid));
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

    #[test]
    fn merge_mode_default_is_rank_by_probe_hit_strength() {
        assert_eq!(
            SearchBatchMergeMode::default(),
            SearchBatchMergeMode::RankByProbeHitStrength
        );
    }
}
