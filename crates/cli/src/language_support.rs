//! Stable re-export surface for language registry types used outside the `languages` module.
//!
//! Keeps indexer, searcher, and path-classification call sites on one import path while language
//! adapters and grammars remain encapsulated under `languages/`.

#![allow(unused_imports)]

pub(crate) use crate::languages::*;
