use crate::domain::{FriggError, FriggResult};
use crate::storage::{
    PathAnchorSketchProjection, Storage, db_runtime::i64_to_u64, db_runtime::open_connection,
};

use super::common::normalize_repository_snapshot_ids;

impl Storage {
    pub fn load_path_anchor_sketch_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<PathAnchorSketchProjection>> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT path, anchor_rank, line, anchor_kind, excerpt, terms_json, score_hint
            FROM path_anchor_sketch_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY path ASC, anchor_rank ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare path anchor sketch projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id.as_str(), snapshot_id.as_str()), |row| {
            let terms_json = row.get::<_, String>(5)?;
            Ok(PathAnchorSketchProjection {
                path: row.get(0)?,
                anchor_rank: i64_to_u64(row.get::<_, i64>(1)?, "anchor_rank")? as usize,
                line: i64_to_u64(row.get::<_, i64>(2)?, "line")? as usize,
                anchor_kind: row.get(3)?,
                excerpt: row.get(4)?,
                terms: serde_json::from_str(&terms_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        5,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                score_hint: i64_to_u64(row.get::<_, i64>(6)?, "score_hint")? as usize,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query path anchor sketch projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode path anchor sketch projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }
}
