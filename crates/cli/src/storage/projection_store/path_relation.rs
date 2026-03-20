use crate::domain::{FriggError, FriggResult};
use crate::storage::{
    PathRelationProjection, Storage, db_runtime::i64_to_u64, db_runtime::open_connection,
};

use super::common::normalize_repository_snapshot_ids;

impl Storage {
    pub fn load_path_relation_projections_for_repository_snapshot(
        &self,
        repository_id: &str,
        snapshot_id: &str,
    ) -> FriggResult<Vec<PathRelationProjection>> {
        let (repository_id, snapshot_id) =
            normalize_repository_snapshot_ids(repository_id, snapshot_id)?;

        let conn = open_connection(&self.db_path)?;
        let mut stmt = conn.prepare(
            r#"
            SELECT src_path, dst_path, relation_kind, evidence_source, src_symbol_id, dst_symbol_id,
                   src_family_bits, dst_family_bits, shared_terms_json, score_hint
            FROM path_relation_projection
            WHERE repository_id = ?1 AND snapshot_id = ?2
            ORDER BY src_path ASC, dst_path ASC, relation_kind ASC
            "#,
        ).map_err(|err| {
            FriggError::Internal(format!(
                "failed to prepare path relation projection load query for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?;
        stmt.query_map((repository_id.as_str(), snapshot_id.as_str()), |row| {
            let shared_terms_json = row.get::<_, String>(8)?;
            Ok(PathRelationProjection {
                src_path: row.get(0)?,
                dst_path: row.get(1)?,
                relation_kind: row.get(2)?,
                evidence_source: row.get(3)?,
                src_symbol_id: row.get(4)?,
                dst_symbol_id: row.get(5)?,
                src_family_bits: i64_to_u64(row.get::<_, i64>(6)?, "src_family_bits")?,
                dst_family_bits: i64_to_u64(row.get::<_, i64>(7)?, "dst_family_bits")?,
                shared_terms: serde_json::from_str(&shared_terms_json).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?,
                score_hint: i64_to_u64(row.get::<_, i64>(9)?, "score_hint")? as usize,
            })
        }).map_err(|err| {
            FriggError::Internal(format!(
                "failed to query path relation projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode path relation projections for repository '{repository_id}' snapshot '{snapshot_id}': {err}"
            ))
        })
    }
}
