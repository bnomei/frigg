//! Canonical cardinality and paging truth for bounded MCP result collections.
//!
//! `ResultCompleteness` deliberately separates an exact total from pagination. A missing total
//! is never a lower bound: it means coverage could not prove the cardinality.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Unit represented by the bounded collection returned from a tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResultUnit {
    Occurrence,
    File,
    Symbol,
    Reference,
    Definition,
    Declaration,
    Implementation,
    IncomingCall,
    OutgoingCall,
    DocumentSymbol,
    SyntaxNode,
    StructuralMatch,
    BatchProbe,
    ImpactSection,
}

/// Why a deliberately bounded collection omitted otherwise qualifying rows.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ResultTruncationReason {
    PageLimit,
    PerFileLimit,
    TopKLimit,
    ChildLimit,
    MergePageLimit,
    AncestorLimit,
}

/// Why Frigg cannot claim exhaustive coverage for a collection.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum ResultIncompleteReason {
    RankedDiscovery,
    DiagnosticCoverage,
    UnreadableCandidate,
    WalkFailure,
    BackendFailure,
    BackendSemanticsUnsupported,
    NavigationPartialCoverage,
    NavigationHeuristicCoverage,
    NavigationUnavailable,
    ChildIncomplete,
}

/// Invariants rejected while constructing a public completeness envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum CompletenessInvariantError {
    #[error("returned rows cannot exceed an exact total")]
    ReturnedExceedsTotal,
    #[error("complete results require an exact total")]
    CompleteWithoutTotal,
    #[error("complete results cannot be truncated, incomplete, or continued")]
    CompleteHasOmissions,
    #[error("a truncation flag requires at least one truncation reason")]
    TruncatedWithoutReason,
    #[error("truncation reasons require truncated=true")]
    ReasonWithoutTruncation,
    #[error("incomplete results require at least one incomplete reason or truncation")]
    IncompleteWithoutReason,
    #[error("a continuation can only represent an intentionally truncated page")]
    ContinuationWithoutTruncation,
}

/// Public completeness envelope shared by every bounded result collection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ResultCompleteness {
    pub unit: ResultUnit,
    pub returned: usize,
    /// Exact cardinality when and only when coverage proves it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<usize>,
    pub complete: bool,
    pub truncated: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub truncation_reasons: Vec<ResultTruncationReason>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub incomplete_reasons: Vec<ResultIncompleteReason>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation: Option<String>,
}

impl ResultCompleteness {
    /// Builds the envelope only when its public truth statements agree.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        unit: ResultUnit,
        returned: usize,
        total: Option<usize>,
        complete: bool,
        truncated: bool,
        mut truncation_reasons: Vec<ResultTruncationReason>,
        mut incomplete_reasons: Vec<ResultIncompleteReason>,
        continuation: Option<String>,
    ) -> Result<Self, CompletenessInvariantError> {
        truncation_reasons.sort();
        truncation_reasons.dedup();
        incomplete_reasons.sort();
        incomplete_reasons.dedup();
        if total.is_some_and(|total| returned > total) {
            return Err(CompletenessInvariantError::ReturnedExceedsTotal);
        }
        if complete && total.is_none() {
            return Err(CompletenessInvariantError::CompleteWithoutTotal);
        }
        if complete
            && (truncated
                || !truncation_reasons.is_empty()
                || !incomplete_reasons.is_empty()
                || continuation.is_some())
        {
            return Err(CompletenessInvariantError::CompleteHasOmissions);
        }
        if truncated && truncation_reasons.is_empty() {
            return Err(CompletenessInvariantError::TruncatedWithoutReason);
        }
        if !truncated && !truncation_reasons.is_empty() {
            return Err(CompletenessInvariantError::ReasonWithoutTruncation);
        }
        if continuation.is_some() && !truncated {
            return Err(CompletenessInvariantError::ContinuationWithoutTruncation);
        }
        if !complete && !truncated && incomplete_reasons.is_empty() {
            return Err(CompletenessInvariantError::IncompleteWithoutReason);
        }
        Ok(Self {
            unit,
            returned,
            total,
            complete,
            truncated,
            truncation_reasons,
            incomplete_reasons,
            continuation,
        })
    }

    /// A page that exhausts an exact row collection.
    ///
    /// `returned` is page-local while `total` is cardinality for the normalized request, so a
    /// final continuation page may be complete even when it contains only the remaining suffix.
    pub fn complete(
        unit: ResultUnit,
        returned: usize,
        total: usize,
    ) -> Result<Self, CompletenessInvariantError> {
        Self::try_new(
            unit,
            returned,
            Some(total),
            true,
            false,
            vec![],
            vec![],
            None,
        )
    }

    /// An exhaustive count-only response that intentionally serializes no collection rows.
    pub fn complete_count_only(
        unit: ResultUnit,
        total: usize,
    ) -> Result<Self, CompletenessInvariantError> {
        Ok(Self {
            unit,
            returned: 0,
            total: Some(total),
            complete: true,
            truncated: false,
            truncation_reasons: vec![],
            incomplete_reasons: vec![],
            continuation: None,
        })
    }
}

/// Structured failures when a v2 continuation is no longer valid for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ContinuationErrorKind {
    Stale,
    ScopeMismatch,
    MixedCursorForms,
}

/// Stable recovery payload fields for continuation validation errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ContinuationValidationError {
    pub kind: ContinuationErrorKind,
    pub code: String,
    pub message: String,
    pub correction_hint: String,
}

impl ContinuationValidationError {
    /// Continuation token no longer matches the repository snapshot (stale proof handle).
    pub fn stale() -> Self {
        Self {
            kind: ContinuationErrorKind::Stale,
            code: "STALE_CONTINUATION".to_owned(),
            message: "Continuation is no longer valid for the current repository snapshot."
                .to_owned(),
            correction_hint: "Re-run the original tool to obtain a fresh continuation.".to_owned(),
        }
    }

    /// Continuation token does not bind this tool, request shape, repository scope, or session.
    pub fn scope_mismatch() -> Self {
        Self {
            kind: ContinuationErrorKind::ScopeMismatch,
            code: "CONTINUATION_SCOPE_MISMATCH".to_owned(),
            message:
                "Continuation does not match this tool, request, repository scope, or session."
                    .to_owned(),
            correction_hint: "Repeat the original request exactly or start a fresh search."
                .to_owned(),
        }
    }

    /// Rejects combining legacy `resume_from` with a v2 `continuation` on the same request.
    pub fn reject_mixed_cursor_forms(
        legacy_present: bool,
        continuation_present: bool,
    ) -> Result<(), Self> {
        if legacy_present && continuation_present {
            return Err(Self {
                kind: ContinuationErrorKind::MixedCursorForms,
                code: "MIXED_CONTINUATION_FORMS".to_owned(),
                message: "Use either legacy resume_from or v2 continuation, not both.".to_owned(),
                correction_hint: "Prefer the v2 continuation returned by the previous response."
                    .to_owned(),
            });
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_reject_contradictory_states() {
        assert_eq!(
            ResultCompleteness::try_new(
                ResultUnit::File,
                1,
                None,
                true,
                false,
                vec![],
                vec![],
                None
            ),
            Err(CompletenessInvariantError::CompleteWithoutTotal)
        );
        assert_eq!(
            ResultCompleteness::try_new(
                ResultUnit::File,
                1,
                Some(1),
                false,
                false,
                vec![],
                vec![],
                None
            ),
            Err(CompletenessInvariantError::IncompleteWithoutReason)
        );
        assert_eq!(
            ResultCompleteness::try_new(
                ResultUnit::File,
                1,
                Some(2),
                false,
                false,
                vec![],
                vec![],
                Some("c".into())
            ),
            Err(CompletenessInvariantError::ContinuationWithoutTruncation)
        );
        assert_eq!(
            ResultCompleteness::complete(ResultUnit::File, 1, 2),
            Ok(ResultCompleteness {
                unit: ResultUnit::File,
                returned: 1,
                total: Some(2),
                complete: true,
                truncated: false,
                truncation_reasons: vec![],
                incomplete_reasons: vec![],
                continuation: None,
            })
        );
        assert_eq!(
            ResultCompleteness::try_new(
                ResultUnit::File,
                1,
                Some(2),
                true,
                true,
                vec![],
                vec![],
                None
            ),
            Err(CompletenessInvariantError::CompleteHasOmissions)
        );
        assert_eq!(
            ResultCompleteness::try_new(
                ResultUnit::File,
                1,
                Some(2),
                true,
                true,
                vec![ResultTruncationReason::PageLimit],
                vec![],
                None
            ),
            Err(CompletenessInvariantError::CompleteHasOmissions)
        );
        assert_eq!(
            ResultCompleteness::try_new(
                ResultUnit::File,
                1,
                Some(2),
                true,
                false,
                vec![],
                vec![ResultIncompleteReason::DiagnosticCoverage],
                None
            ),
            Err(CompletenessInvariantError::CompleteHasOmissions)
        );
        assert_eq!(
            ResultCompleteness::try_new(
                ResultUnit::File,
                1,
                Some(2),
                true,
                false,
                vec![],
                vec![],
                Some("continuation-000001".to_owned())
            ),
            Err(CompletenessInvariantError::CompleteHasOmissions)
        );
        assert_eq!(
            ResultCompleteness::complete_count_only(ResultUnit::Occurrence, 2),
            Ok(ResultCompleteness {
                unit: ResultUnit::Occurrence,
                returned: 0,
                total: Some(2),
                complete: true,
                truncated: false,
                truncation_reasons: vec![],
                incomplete_reasons: vec![],
                continuation: None,
            })
        );
    }

    #[test]
    fn compact_serialization_keeps_completeness_truth() {
        let value = serde_json::to_value(
            ResultCompleteness::try_new(
                ResultUnit::Occurrence,
                10,
                Some(15),
                false,
                true,
                vec![ResultTruncationReason::PageLimit],
                vec![],
                Some("continuation-000001".to_owned()),
            )
            .expect("page-limited completeness fixture must be valid"),
        )
        .expect("completeness fixture must serialize");
        assert_eq!(value["total"], 15);
        assert_eq!(value["truncated"], true);
        assert_eq!(value["continuation"], "continuation-000001");
    }
}
