use std::cell::RefCell;

use super::super::super::HybridChannelHit;
use super::super::super::HybridRankedEvidence;
use super::super::super::intent::HybridRankingIntent;
use super::super::super::query_terms::hybrid_query_exact_terms;
use super::super::PolicyQueryContext;
use super::super::trace::PolicyStage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PostSelectionRuleMeta {
    pub(super) id: &'static str,
    pub(super) stage: PolicyStage,
}

pub(crate) struct PostSelectionContext<'a> {
    pub(super) intent: &'a HybridRankingIntent,
    pub(super) query_text: &'a str,
    pub(super) lexical_only_mode: bool,
    pub(super) exact_terms: Vec<String>,
    pub(super) selection_query_context: PolicyQueryContext,
    pub(super) limit: usize,
    pub(super) candidate_pool: &'a [HybridRankedEvidence],
    pub(super) witness_hits: &'a [HybridChannelHit],
    trace: RefCell<Option<PostSelectionTrace>>,
}

impl<'a> PostSelectionContext<'a> {
    #[cfg(test)]
    pub(crate) fn new(
        intent: &'a HybridRankingIntent,
        query_text: &'a str,
        limit: usize,
        candidate_pool: &'a [HybridRankedEvidence],
        witness_hits: &'a [HybridChannelHit],
    ) -> Self {
        Self::new_with_mode(
            intent,
            query_text,
            false,
            limit,
            candidate_pool,
            witness_hits,
        )
    }

    pub(crate) fn new_with_mode(
        intent: &'a HybridRankingIntent,
        query_text: &'a str,
        lexical_only_mode: bool,
        limit: usize,
        candidate_pool: &'a [HybridRankedEvidence],
        witness_hits: &'a [HybridChannelHit],
    ) -> Self {
        Self::with_trace(
            intent,
            query_text,
            lexical_only_mode,
            limit,
            candidate_pool,
            witness_hits,
            false,
        )
    }

    #[cfg(test)]
    pub(crate) fn new_with_trace(
        intent: &'a HybridRankingIntent,
        query_text: &'a str,
        limit: usize,
        candidate_pool: &'a [HybridRankedEvidence],
        witness_hits: &'a [HybridChannelHit],
    ) -> Self {
        Self::new_with_trace_mode(
            intent,
            query_text,
            false,
            limit,
            candidate_pool,
            witness_hits,
        )
    }

    pub(crate) fn new_with_trace_mode(
        intent: &'a HybridRankingIntent,
        query_text: &'a str,
        lexical_only_mode: bool,
        limit: usize,
        candidate_pool: &'a [HybridRankedEvidence],
        witness_hits: &'a [HybridChannelHit],
    ) -> Self {
        Self::with_trace(
            intent,
            query_text,
            lexical_only_mode,
            limit,
            candidate_pool,
            witness_hits,
            true,
        )
    }

    fn with_trace(
        intent: &'a HybridRankingIntent,
        query_text: &'a str,
        lexical_only_mode: bool,
        limit: usize,
        candidate_pool: &'a [HybridRankedEvidence],
        witness_hits: &'a [HybridChannelHit],
        capture_trace: bool,
    ) -> Self {
        Self {
            intent,
            query_text,
            lexical_only_mode,
            exact_terms: hybrid_query_exact_terms(query_text),
            selection_query_context: PolicyQueryContext::new(intent, query_text),
            limit,
            candidate_pool,
            witness_hits,
            trace: RefCell::new(capture_trace.then(PostSelectionTrace::default)),
        }
    }

    pub(super) fn record_repair(
        &self,
        meta: PostSelectionRuleMeta,
        action: PostSelectionRepairAction,
        candidate_path: &str,
        replaced_path: Option<String>,
    ) {
        let mut trace = self.trace.borrow_mut();
        if let Some(trace) = trace.as_mut() {
            trace.events.push(PostSelectionTraceEvent {
                rule_id: meta.id,
                rule_stage: meta.stage,
                action,
                candidate_path: candidate_path.to_owned(),
                replaced_path,
            });
        }
    }

    pub(crate) fn trace_snapshot(&self) -> Option<PostSelectionTrace> {
        self.trace.borrow().clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) enum PostSelectionRepairAction {
    Inserted,
    Replaced,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PostSelectionTraceEvent {
    pub(crate) rule_id: &'static str,
    pub(crate) rule_stage: PolicyStage,
    pub(crate) action: PostSelectionRepairAction,
    pub(crate) candidate_path: String,
    pub(crate) replaced_path: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize)]
pub(crate) struct PostSelectionTrace {
    pub(crate) events: Vec<PostSelectionTraceEvent>,
}
