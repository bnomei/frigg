//! `go_to_definition` and `find_declarations` location-oriented navigation tools.
//!
//! Location-oriented navigation entrypoints for `go_to_definition` and `find_declarations` with
//! shared resolution plumbing.

use super::*;

#[path = "location/go_to_definition.rs"]
mod go_to_definition;
