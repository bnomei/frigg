use crate::domain::{FriggError, FriggResult};

use super::vector_store::{
    initialize_vector_store_on_connection, semantic_chunk_embedding_record_order,
    validate_semantic_chunk_embedding_record,
};
use super::{
    DEFAULT_VECTOR_DIMENSIONS, SNAPSHOT_KIND_MANIFEST, SemanticChunkEmbeddingRecord,
    SemanticStorageHealth, Storage, VECTOR_TABLE_NAME,
    load_semantic_head_snapshot_ids_for_repository, load_snapshot_ids_for_repository_and_kind,
    open_connection,
};

#[path = "semantic_store_read.rs"]
mod semantic_store_read;
#[path = "semantic_store_support.rs"]
mod semantic_store_support;
use semantic_store_support::{
    clear_live_semantic_corpus_for_repository_model, count_manifest_snapshots_for_repository,
    count_semantic_chunk_rows_for_repository_model,
    count_semantic_embedding_rows_for_repository_model,
    count_semantic_vector_rows_for_repository_model, delete_live_semantic_rows_for_paths,
    delete_vector_rows_for_chunk_ids, insert_semantic_embeddings_for_records,
    load_live_semantic_chunk_ids_for_paths,
    load_ready_semantic_head_for_repository_snapshot_model_on_connection,
    load_semantic_head_for_repository_model_on_connection,
    normalize_embedding_for_vector_projection, rebuild_semantic_vector_rows,
    sync_vector_partition_replace, sync_vector_rows_insert, upsert_semantic_head,
    validate_semantic_target,
};

impl Storage {
    pub fn replace_semantic_embeddings_for_repository(
        &self,
        repository_id: &str,
        snapshot_id: &str,
        provider: &str,
        model: &str,
        records: &[SemanticChunkEmbeddingRecord],
    ) -> FriggResult<()> {
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
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        for record in records {
            validate_semantic_chunk_embedding_record(record, repository_id, snapshot_id)?;
            validate_semantic_target(record, provider, model)?;
        }

        let mut conn = open_connection(&self.db_path)?;
        let _ = initialize_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start semantic embedding replace transaction for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;

        clear_live_semantic_corpus_for_repository_model(&tx, repository_id, provider, model)?;

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by(semantic_chunk_embedding_record_order);
        let live_chunk_count = insert_semantic_embeddings_for_records(
            &tx,
            repository_id,
            snapshot_id,
            provider,
            model,
            &ordered_records,
        )?;
        upsert_semantic_head(
            &tx,
            repository_id,
            provider,
            model,
            snapshot_id,
            live_chunk_count,
            Some("replace_full"),
        )?;
        sync_vector_partition_replace(&tx, repository_id, provider, model, &ordered_records)?;

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit semantic embedding replace for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn advance_semantic_embeddings_for_repository(
        &self,
        repository_id: &str,
        previous_snapshot_id: Option<&str>,
        snapshot_id: &str,
        provider: &str,
        model: &str,
        changed_paths: &[String],
        deleted_paths: &[String],
        records: &[SemanticChunkEmbeddingRecord],
    ) -> FriggResult<()> {
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
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }
        let previous_snapshot_id = previous_snapshot_id
            .map(str::trim)
            .filter(|value| !value.is_empty());

        for record in records {
            validate_semantic_chunk_embedding_record(record, repository_id, snapshot_id)?;
            validate_semantic_target(record, provider, model)?;
        }

        let mut conn = open_connection(&self.db_path)?;
        let _ = initialize_vector_store_on_connection(&conn, DEFAULT_VECTOR_DIMENSIONS)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start semantic embedding advance transaction for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;

        let head = load_semantic_head_for_repository_model_on_connection(
            &tx,
            repository_id,
            provider,
            model,
        )?;
        let current_covered_snapshot_id = head
            .as_ref()
            .map(|record| record.covered_snapshot_id.as_str());
        if current_covered_snapshot_id != previous_snapshot_id {
            let found = current_covered_snapshot_id.unwrap_or("-");
            let expected = previous_snapshot_id.unwrap_or("-");
            return Err(FriggError::Internal(format!(
                "semantic advance requires live corpus covered snapshot '{expected}' for repository '{repository_id}' provider '{provider}' model '{model}', found '{found}'; run a full semantic rebuild instead"
            )));
        }

        let mut removed_paths = changed_paths
            .iter()
            .chain(deleted_paths.iter())
            .map(|path| path.trim())
            .filter(|path| !path.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        removed_paths.sort();
        removed_paths.dedup();
        let removed_chunk_ids = load_live_semantic_chunk_ids_for_paths(
            &tx,
            repository_id,
            provider,
            model,
            &removed_paths,
        )?;
        delete_vector_rows_for_chunk_ids(&tx, repository_id, provider, model, &removed_chunk_ids)?;
        delete_live_semantic_rows_for_paths(&tx, repository_id, provider, model, &removed_paths)?;

        let mut ordered_records = records.to_vec();
        ordered_records.sort_by(semantic_chunk_embedding_record_order);
        insert_semantic_embeddings_for_records(
            &tx,
            repository_id,
            snapshot_id,
            provider,
            model,
            &ordered_records,
        )?;
        sync_vector_rows_insert(&tx, repository_id, provider, model, &ordered_records)?;
        let live_chunk_count =
            count_semantic_chunk_rows_for_repository_model(&tx, repository_id, provider, model)?;
        upsert_semantic_head(
            &tx,
            repository_id,
            provider,
            model,
            snapshot_id,
            live_chunk_count,
            Some("advance_delta"),
        )?;

        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit semantic embedding advance for repository '{repository_id}' provider '{provider}' model '{model}': {err}"
            ))
        })?;
        Ok(())
    }

    pub fn collect_semantic_storage_health_for_repository_model(
        &self,
        repository_id: &str,
        provider: &str,
        model: &str,
    ) -> FriggResult<SemanticStorageHealth> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        let provider = provider.trim();
        if provider.is_empty() {
            return Err(FriggError::InvalidInput(
                "provider must not be empty".to_owned(),
            ));
        }
        let model = model.trim();
        if model.is_empty() {
            return Err(FriggError::InvalidInput(
                "model must not be empty".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let head = load_semantic_head_for_repository_model_on_connection(
            &conn,
            repository_id,
            provider,
            model,
        )?;
        let live_chunk_rows =
            count_semantic_chunk_rows_for_repository_model(&conn, repository_id, provider, model)?;
        let live_embedding_rows = count_semantic_embedding_rows_for_repository_model(
            &conn,
            repository_id,
            provider,
            model,
        )?;
        let live_vector_rows =
            count_semantic_vector_rows_for_repository_model(&conn, repository_id, provider, model)?;
        let retained_manifest_snapshots =
            count_manifest_snapshots_for_repository(&conn, repository_id)?;

        Ok(SemanticStorageHealth {
            repository_id: repository_id.to_owned(),
            provider: provider.to_owned(),
            model: model.to_owned(),
            covered_snapshot_id: head
                .as_ref()
                .map(|record| record.covered_snapshot_id.clone()),
            live_chunk_rows,
            live_embedding_rows,
            live_vector_rows,
            retained_manifest_snapshots,
            vector_consistent: live_embedding_rows == live_vector_rows,
        })
    }

    pub fn repair_semantic_vector_store(&self) -> FriggResult<()> {
        let mut conn = open_connection(&self.db_path)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start semantic vector repair transaction: {err}"
            ))
        })?;
        tx.execute_batch(&format!("DROP TABLE IF EXISTS {VECTOR_TABLE_NAME}"))
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to drop semantic vector table during repair: {err}"
                ))
            })?;
        let _ = initialize_vector_store_on_connection(&tx, DEFAULT_VECTOR_DIMENSIONS)?;
        rebuild_semantic_vector_rows(&tx)?;
        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit semantic vector repair transaction: {err}"
            ))
        })?;

        Ok(())
    }

    pub fn prune_repository_snapshots(
        &self,
        repository_id: &str,
        keep_latest: usize,
    ) -> FriggResult<usize> {
        let repository_id = repository_id.trim();
        if repository_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "repository_id must not be empty".to_owned(),
            ));
        }
        if keep_latest == 0 {
            return Err(FriggError::InvalidInput(
                "keep_latest must be greater than zero".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let protected_snapshot_ids =
            load_semantic_head_snapshot_ids_for_repository(&conn, repository_id)?;
        let snapshot_ids = load_snapshot_ids_for_repository_and_kind(
            &conn,
            repository_id,
            SNAPSHOT_KIND_MANIFEST,
        )?;

        let mut deleted = 0usize;
        for snapshot_id in snapshot_ids.into_iter().skip(keep_latest) {
            if protected_snapshot_ids.contains(&snapshot_id) {
                continue;
            }
            self.delete_snapshot(&snapshot_id)?;
            deleted = deleted.saturating_add(1);
        }

        Ok(deleted)
    }
}
