use crate::domain::{FriggError, FriggResult};
use crate::storage::{
    Storage, SubtreeCoverageProjection, db_runtime::i64_to_u64, db_runtime::open_connection,
};

use super::common::normalize_repository_snapshot_ids;

impl Storage {
    pub fn load_subtree_coverage_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<SubtreeCoverageProjection>> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT subtree_root, family, path_count, exemplar_path, exemplar_score_hint
            FROM subtree_coverage_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY subtree_root ASC, family ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare subtree coverage projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id.as_str(), snapshot_id.as_str()), |row| {
            Ok(SubtreeCoverageProjection {
                subtree_root: row.get(0)?,
                family: row.get(1)?,
                path_count: i64_to_u64(row.get::<_, i64>(2)?, "path_count")? as usize,
                exemplar_path: row.get(3)?,
                exemplar_score_hint: i64_to_u64(row.get::<_, i64>(4)?, "exemplar_score_hint")?
                    as usize,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query subtree coverage projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode subtree coverage projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }
}
