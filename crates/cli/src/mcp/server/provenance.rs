use super::*;

impl FriggMcpServer {
    pub(super) fn bounded_text(value: &str) -> String {
        if value.chars().count() <= Self::PROVENANCE_MAX_TEXT_CHARS {
            return value.to_owned();
        }
        let mut bounded = value
            .chars()
            .take(Self::PROVENANCE_MAX_TEXT_CHARS)
            .collect::<String>();
        bounded.push_str("...");
        bounded
    }

    fn default_provenance_target(&self) -> Option<(String, PathBuf)> {
        self.current_workspace()
            .into_iter()
            .chain(self.attached_workspaces())
            .map(|workspace| (workspace.repository_id, workspace.root))
            .min_by(|left, right| left.0.cmp(&right.0))
    }

    fn provenance_target_for_repository(
        &self,
        repository_id: Option<&str>,
    ) -> Option<(String, PathBuf)> {
        match repository_id {
            Some(repository_id) => self
                .attached_workspaces()
                .into_iter()
                .find(|workspace| workspace.repository_id == repository_id)
                .map(|workspace| (workspace.repository_id, workspace.root)),
            None => self.default_provenance_target(),
        }
    }

    fn provenance_error_code(error: &ErrorData) -> String {
        error
            .data
            .as_ref()
            .and_then(|value| value.get("error_code"))
            .and_then(|value| value.as_str())
            .unwrap_or("missing_error_code")
            .to_owned()
    }

    pub(super) fn provenance_outcome<T>(result: &Result<Json<T>, ErrorData>) -> Value {
        match result {
            Ok(_) => json!({
                "status": "ok",
            }),
            Err(error) => json!({
                "status": "error",
                "error_code": Self::provenance_error_code(error),
                "mcp_error_code": error.code,
            }),
        }
    }

    fn provenance_storage_for_target(
        &self,
        tool_name: &str,
        target_repository_id: &str,
        db_path: &Path,
    ) -> Result<Arc<Storage>, ErrorData> {
        let cache_key = ProvenanceStorageCacheKey {
            repository_id: target_repository_id.to_owned(),
            db_path: db_path.to_path_buf(),
        };
        if let Some(storage) = self
            .provenance_storage_cache
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(&cache_key)
            .cloned()
        {
            return Ok(storage);
        }

        let storage = Arc::new(Storage::new(db_path));
        if let Err(err) = storage.initialize() {
            return Err(Self::provenance_persistence_error(
                ProvenancePersistenceStage::InitializeStorage,
                tool_name,
                Some(target_repository_id),
                Some(db_path),
                err,
            ));
        }

        let mut cache = self
            .provenance_storage_cache
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(storage) = cache.get(&cache_key).cloned() {
            return Ok(storage);
        }
        cache.insert(cache_key, storage.clone());
        Ok(storage)
    }

    pub(super) fn record_provenance_with_outcome(
        &self,
        tool_name: &str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        outcome: Value,
    ) -> Result<(), ErrorData> {
        if !self.provenance_enabled {
            return Ok(());
        }
        let Some((target_repository_id, target_root)) =
            self.provenance_target_for_repository(repository_hint)
        else {
            return Ok(());
        };

        let db_path = match ensure_provenance_db_parent_dir(&target_root) {
            Ok(path) => path,
            Err(err) => {
                return Err(Self::provenance_persistence_error(
                    ProvenancePersistenceStage::ResolveStoragePath,
                    tool_name,
                    Some(&target_repository_id),
                    None,
                    err,
                ));
            }
        };

        let storage =
            self.provenance_storage_for_target(tool_name, &target_repository_id, &db_path)?;

        let payload = json!({
            "tool_name": tool_name,
            "params": params,
            "source_refs": source_refs,
            "outcome": outcome,
            "target_repository_id": target_repository_id,
        });
        let trace_id = Storage::new_provenance_trace_id(tool_name);
        if let Err(err) = storage.append_provenance_event(&trace_id, tool_name, &payload) {
            return Err(Self::provenance_persistence_error(
                ProvenancePersistenceStage::AppendEvent,
                tool_name,
                Some(&target_repository_id),
                Some(&db_path),
                err,
            ));
        }

        Ok(())
    }

    pub(super) async fn record_provenance_blocking<T>(
        &self,
        tool_name: &'static str,
        repository_hint: Option<&str>,
        params: Value,
        source_refs: Value,
        result: &Result<Json<T>, ErrorData>,
    ) -> Result<(), ErrorData> {
        if !self.provenance_enabled {
            return Ok(());
        }
        let server = self.clone();
        let repository_hint = repository_hint.map(str::to_owned);
        let outcome = Self::provenance_outcome(result);
        Self::run_blocking_task("record_provenance", move || {
            server.record_provenance_with_outcome(
                tool_name,
                repository_hint.as_deref(),
                params,
                source_refs,
                outcome,
            )
        })
        .await?
    }

    pub(super) fn finalize_with_provenance<T>(
        &self,
        tool_name: &str,
        result: Result<Json<T>, ErrorData>,
        provenance_result: Result<(), ErrorData>,
    ) -> Result<Json<T>, ErrorData> {
        match provenance_result {
            Ok(_) => result,
            Err(provenance_error) if self.provenance_best_effort => {
                warn!(
                    tool_name,
                    error = %provenance_error.message,
                    "provenance persistence failed in best-effort mode"
                );
                result
            }
            Err(provenance_error) => match result {
                Ok(_) => Err(provenance_error),
                Err(original_error) => {
                    warn!(
                        tool_name,
                        original_error_code = ?original_error.code,
                        provenance_error_code = ?provenance_error.code,
                        "provenance persistence failed but original request already returned typed error"
                    );
                    Err(original_error)
                }
            },
        }
    }

    pub(super) fn with_provenance_enabled(&self, provenance_enabled: bool) -> Self {
        let mut cloned = self.clone();
        cloned.provenance_enabled = provenance_enabled;
        cloned
    }
}
