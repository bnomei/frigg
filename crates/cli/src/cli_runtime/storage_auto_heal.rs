//! Shared auto-repair hooks for regenerable storage invariants.
//!
//! Repairs regenerable storage invariants when verification fails; invoked from startup gates and
//! maintenance commands.

use frigg::domain::FriggResult;
use frigg::storage::Storage;

/// Initializes storage and repairs only a missing or incompatible sqlite-vec table.
///
/// Full embedding membership validation is intentionally opt-in through
/// `frigg index --validate-embeddings`.
pub(crate) fn initialize_storage_with_auto_repair(storage: &Storage) -> FriggResult<Vec<String>> {
    storage.initialize_with_auto_repair()
}
