//! Normalized policy facts shared across path quality, path witness, and selection stages.
//! [`SharedIntentFacts`] and [`SharedPathFacts`] keep predicates consistent; stage structs add
//! query-match and coverage fields.

mod path_quality;
mod path_witness;
mod selection;
mod shared;

pub(crate) use path_quality::PathQualityFacts;
pub(crate) use path_witness::PathWitnessFacts;
pub(crate) use selection::{SelectionCandidate, SelectionFacts, SelectionState};
pub(crate) use shared::{PolicyQueryContext, SharedIntentFacts, SharedPathFacts};
