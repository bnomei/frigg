use crate::domain::{FriggError, FriggResult};
use crate::storage::{PathSurfaceTermProjection, Storage, db_runtime::open_connection};

use super::common::normalize_repository_snapshot_ids;

impl Storage {
    pub fn load_path_surface_term_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<PathSurfaceTermProjection>> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT path, term_weights_json, exact_terms_json
            FROM path_surface_term_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY path ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare path surface-term projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id.as_str(), snapshot_id.as_str()), |row| {
            let term_weights_json = row.get::<_, String>(1)?;
            let exact_terms_json = row.get::<_, String>(2)?;
            Ok(PathSurfaceTermProjection {
                path: row.get(0)?,
                term_weights: serde_json::from_str(&term_weights_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                exact_terms: serde_json::from_str(&exact_terms_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        2,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query path surface-term projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode path surface-term projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }
}
