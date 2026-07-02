//! Shared auto-repair hooks for regenerable storage invariants.

use frigg::domain::{FriggError, FriggResult};
use frigg::storage::Storage;

pub(crate) fn initialize_storage_with_auto_repair(storage: &Storage) -> FriggResult<Vec<String>> {
    match storage.initialize() {
        Ok(()) => verify_storage_with_auto_repair(storage),
        Err(original_err) => repair_then_verify(storage, original_err),
    }
}

pub(crate) fn verify_storage_with_auto_repair(storage: &Storage) -> FriggResult<Vec<String>> {
    match storage.verify() {
        Ok(()) => Ok(Vec::new()),
        Err(original_err) => repair_then_verify(storage, original_err),
    }
}

fn repair_then_verify(storage: &Storage, original_err: FriggError) -> FriggResult<Vec<String>> {
    let repair_summary = storage.repair_storage_invariants()?;
    match storage.verify() {
        Ok(()) => Ok(repair_summary.repaired_categories),
        Err(_) if repair_summary.repaired_categories.is_empty() => Err(original_err),
        Err(err) => Err(err),
    }
}
