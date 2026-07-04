//! Shared auto-repair hooks for regenerable storage invariants.
//!
//! Repairs regenerable storage invariants when verification fails; invoked from startup gates and
//! maintenance commands.

use frigg::domain::FriggResult;
use frigg::storage::Storage;

/// Initializes storage and auto-repairs regenerable invariants when verification fails.
pub(crate) fn initialize_storage_with_auto_repair(storage: &Storage) -> FriggResult<Vec<String>> {
    storage.initialize_with_auto_repair()
}

/// Verifies storage invariants and attempts one repair pass before surfacing the original error.
pub(crate) fn verify_storage_with_auto_repair(storage: &Storage) -> FriggResult<Vec<String>> {
    storage.verify_with_auto_repair()
}
