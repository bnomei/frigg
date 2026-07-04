//! Score-rule pipelines for path quality, path witness recall, and top-k selection.
//!
//! Each pipeline applies score rules in a declared stage order; predicates gate rules so disabled
//! intents never mutate rankings.

pub(crate) mod path_quality;
pub(crate) mod path_witness;
pub(crate) mod selection;
