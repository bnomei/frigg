use super::*;

impl Storage {
    pub fn append_provenance_event(
        &self,
        trace_id: &str,
        tool_name: &str,
        payload_json: &Value,
    ) -> FriggResult<()> {
        let trace_id = trace_id.trim();
        if trace_id.is_empty() {
            return Err(FriggError::InvalidInput(
                "trace_id must not be empty".to_owned(),
            ));
        }

        let tool_name = tool_name.trim();
        if tool_name.is_empty() {
            return Err(FriggError::InvalidInput(
                "tool_name must not be empty".to_owned(),
            ));
        }

        let payload_raw = serde_json::to_string(payload_json).map_err(|err| {
            FriggError::Internal(format!(
                "failed to serialize provenance payload for tool '{tool_name}': {err}"
            ))
        })?;

        let conn = if let Some(conn) = self.provenance_write_connection.get() {
            conn
        } else {
            let connection = Mutex::new(open_connection(&self.db_path)?);
            let _ = self.provenance_write_connection.set(connection);
            self.provenance_write_connection
                .get()
                .expect("provenance write connection should be initialized")
        };
        let conn = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut attempt_ms = 0i64;
        loop {
            let insert_result = conn.execute(
                r#"
                INSERT INTO provenance_event (trace_id, tool_name, payload_json, created_at)
                VALUES (
                    ?1,
                    ?2,
                    ?3,
                    printf(
                        '%s-%03d',
                        STRFTIME('%Y-%m-%dT%H:%M:%fZ', 'now'),
                        ?4
                    )
                )
                "#,
                (trace_id, tool_name, &payload_raw, attempt_ms),
            );

            match insert_result {
                Ok(_) => {
                    prune_provenance_events_on_connection(
                        &conn,
                        DEFAULT_RETAINED_PROVENANCE_EVENTS,
                    )?;
                    return Ok(());
                }
                Err(rusqlite::Error::SqliteFailure(err, _))
                    if err.code == ErrorCode::ConstraintViolation
                        && attempt_ms < PROVENANCE_CREATED_AT_MAX_RETRY_MS =>
                {
                    attempt_ms += 1;
                }
                Err(err) => {
                    return Err(FriggError::Internal(format!(
                        "failed to persist provenance event for tool '{tool_name}': {err}"
                    )));
                }
            }
        }
    }

    pub fn load_provenance_events_for_tool(
        &self,
        tool_name: &str,
        limit: usize,
    ) -> FriggResult<Vec<ProvenanceEventRow>> {
        let tool_name = tool_name.trim();
        if tool_name.is_empty() {
            return Err(FriggError::InvalidInput(
                "tool_name must not be empty".to_owned(),
            ));
        }
        if limit == 0 {
            return Err(FriggError::InvalidInput(
                "limit must be greater than zero".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT trace_id, tool_name, payload_json, created_at
                FROM provenance_event
                WHERE tool_name = ?1
                ORDER BY created_at DESC, rowid DESC
                LIMIT ?2
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to prepare provenance query for tool '{tool_name}': {err}"
                ))
            })?;

        let rows = statement
            .query_map((tool_name, usize_to_i64(limit, "limit")?), |row| {
                Ok(ProvenanceEventRow {
                    trace_id: row.get(0)?,
                    tool_name: row.get(1)?,
                    payload_json: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to query provenance events for tool '{tool_name}': {err}"
                ))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!(
                "failed to decode provenance events for tool '{tool_name}': {err}"
            ))
        })
    }

    pub fn load_recent_provenance_events(
        &self,
        limit: usize,
    ) -> FriggResult<Vec<ProvenanceEventRow>> {
        if limit == 0 {
            return Err(FriggError::InvalidInput(
                "limit must be greater than zero".to_owned(),
            ));
        }

        let conn = open_connection(&self.db_path)?;
        let mut statement = conn
            .prepare(
                r#"
                SELECT trace_id, tool_name, payload_json, created_at
                FROM provenance_event
                ORDER BY created_at DESC, rowid DESC
                LIMIT ?1
                "#,
            )
            .map_err(|err| {
                FriggError::Internal(format!("failed to prepare recent provenance query: {err}"))
            })?;

        let rows = statement
            .query_map((usize_to_i64(limit, "limit")?,), |row| {
                Ok(ProvenanceEventRow {
                    trace_id: row.get(0)?,
                    tool_name: row.get(1)?,
                    payload_json: row.get(2)?,
                    created_at: row.get(3)?,
                })
            })
            .map_err(|err| {
                FriggError::Internal(format!("failed to query recent provenance events: {err}"))
            })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
            FriggError::Internal(format!("failed to decode recent provenance events: {err}"))
        })
    }

    pub fn prune_provenance_events(&self, keep_latest: usize) -> FriggResult<usize> {
        if keep_latest == 0 {
            return Err(FriggError::InvalidInput(
                "keep_latest must be greater than zero".to_owned(),
            ));
        }

        let mut conn = open_connection(&self.db_path)?;
        let before = count_provenance_events(&conn)?;
        let tx = conn.transaction().map_err(|err| {
            FriggError::Internal(format!(
                "failed to start provenance prune transaction: {err}"
            ))
        })?;
        tx.execute(
            r#"
            DELETE FROM provenance_event
            WHERE rowid NOT IN (
              SELECT rowid
              FROM provenance_event
              ORDER BY created_at DESC, rowid DESC
              LIMIT ?1
            )
            "#,
            [usize_to_i64(keep_latest, "keep_latest")?],
        )
        .map_err(|err| FriggError::Internal(format!("failed to prune provenance events: {err}")))?;
        tx.commit().map_err(|err| {
            FriggError::Internal(format!(
                "failed to commit provenance prune transaction: {err}"
            ))
        })?;
        let conn = open_connection(&self.db_path)?;
        let after = count_provenance_events(&conn)?;

        Ok(before.saturating_sub(after))
    }
}
