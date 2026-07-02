//! Snapshot-scoped projection loaders for the projection service cache.
//!
//! Loads or rebuilds decoded projection families from SQLite storage, keyed by repository and
//! manifest snapshot identity.

#[path = "loaders/common.rs"]
mod common;
#[path = "loaders/families.rs"]
mod families;
