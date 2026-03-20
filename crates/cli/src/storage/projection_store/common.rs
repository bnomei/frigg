use crate::domain::{FriggError, FriggResult};

pub(super) fn normalize_repository_snapshot_ids(
    repository_id: &str,
    snapshot_id: &str,
) -> FriggResult<(String, String)> {
    let repository_id = repository_id.trim();
    if repository_id.is_empty() {
        return Err(FriggError::InvalidInput(
            "repository_id must not be empty".to_owned(),
        ));
    }

    let snapshot_id = snapshot_id.trim();
    if snapshot_id.is_empty() {
        return Err(FriggError::InvalidInput(
            "snapshot_id must not be empty".to_owned(),
        ));
    }

    Ok((repository_id.to_owned(), snapshot_id.to_owned()))
}

pub(super) fn normalize_repository_snapshot_family_ids(
    repository_id: &str,
    snapshot_id: &str,
    family: &str,
) -> FriggResult<(String, String, String)> {
    let (repository_id, snapshot_id) =
        normalize_repository_snapshot_ids(repository_id, snapshot_id)?;
    let family = family.trim();
    if family.is_empty() {
        return Err(FriggError::InvalidInput(
            "family must not be empty".to_owned(),
        ));
    }

    Ok((repository_id, snapshot_id, family.to_owned()))
}
