use super::*;

#[allow(clippy::enum_variant_names)]
enum FriggErrorTransportCode {
    InvalidParams,
    ResourceNotFound,
    InvalidRequest,
    Internal,
}

pub(super) struct FriggErrorTranslation {
    transport_code: FriggErrorTransportCode,
    message: String,
    error_code: &'static str,
    retryable: bool,
    detail: Option<Value>,
}

impl FriggMcpServer {
    pub(super) fn filtered_tool_router(profile: ToolSurfaceProfile) -> ToolRouter<Self> {
        let mut router = Self::tool_router();
        let allowed_tools = manifest_for_tool_surface_profile(profile)
            .tool_names
            .into_iter()
            .collect::<BTreeSet<_>>();
        for tool_name in router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.into_owned())
            .collect::<Vec<_>>()
        {
            if !allowed_tools.contains(&tool_name) {
                router.remove_route(&tool_name);
            }
        }
        router
    }

    pub(super) fn with_error_metadata(
        error_code: &str,
        retryable: bool,
        detail: Option<Value>,
    ) -> Value {
        let mut payload = serde_json::Map::new();
        payload.insert(
            "error_code".to_owned(),
            Value::String(error_code.to_owned()),
        );
        payload.insert("retryable".to_owned(), Value::Bool(retryable));

        if let Some(detail) = detail {
            match detail {
                Value::Object(detail_map) => {
                    for (key, value) in detail_map {
                        payload.insert(key, value);
                    }
                }
                other => {
                    payload.insert("detail".to_owned(), other);
                }
            }
        }

        Value::Object(payload)
    }

    pub(super) fn invalid_params(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        ErrorData::invalid_params(
            message.into(),
            Some(Self::with_error_metadata("invalid_params", false, detail)),
        )
    }

    pub(super) fn resource_not_found(
        message: impl Into<String>,
        detail: Option<Value>,
    ) -> ErrorData {
        ErrorData::resource_not_found(
            message.into(),
            Some(Self::with_error_metadata(
                "resource_not_found",
                false,
                detail,
            )),
        )
    }

    pub(super) fn access_denied(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        ErrorData::invalid_request(
            message.into(),
            Some(Self::with_error_metadata("access_denied", false, detail)),
        )
    }

    pub(super) fn internal_with_code(
        message: impl Into<String>,
        error_code: &str,
        retryable: bool,
        detail: Option<Value>,
    ) -> ErrorData {
        ErrorData::internal_error(
            message.into(),
            Some(Self::with_error_metadata(error_code, retryable, detail)),
        )
    }

    pub(super) fn internal(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        Self::internal_with_code(message, "internal", false, detail)
    }

    pub(super) fn confirmation_required(tool_name: &'static str) -> ErrorData {
        Self::internal_with_code(
            format!("{tool_name} requires explicit {WRITE_CONFIRM_PARAM}=true before side effects"),
            WRITE_CONFIRMATION_REQUIRED_ERROR_CODE,
            false,
            Some(json!({
                "tool_name": tool_name,
                "confirm_param": WRITE_CONFIRM_PARAM,
            })),
        )
    }

    pub(super) fn require_confirm(
        tool_name: &'static str,
        confirm: Option<bool>,
    ) -> Result<(), ErrorData> {
        if confirm == Some(true) {
            return Ok(());
        }
        Err(Self::confirmation_required(tool_name))
    }

    pub(super) fn build_frigg_error_data(translation: FriggErrorTranslation) -> ErrorData {
        match translation.transport_code {
            FriggErrorTransportCode::InvalidParams => {
                Self::invalid_params(translation.message, translation.detail)
            }
            FriggErrorTransportCode::ResourceNotFound => {
                Self::resource_not_found(translation.message, translation.detail)
            }
            FriggErrorTransportCode::InvalidRequest => {
                Self::access_denied(translation.message, translation.detail)
            }
            FriggErrorTransportCode::Internal => Self::internal_with_code(
                translation.message,
                translation.error_code,
                translation.retryable,
                translation.detail,
            ),
        }
    }

    pub(super) fn translate_frigg_error(err: FriggError) -> FriggErrorTranslation {
        match err {
            FriggError::InvalidInput(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::InvalidParams,
                message,
                error_code: "invalid_params",
                retryable: false,
                detail: None,
            },
            FriggError::NotFound(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::ResourceNotFound,
                message,
                error_code: "resource_not_found",
                retryable: false,
                detail: None,
            },
            FriggError::AccessDenied(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::InvalidRequest,
                message,
                error_code: "access_denied",
                retryable: false,
                detail: None,
            },
            FriggError::Io(err) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::Internal,
                message: "IO failure".to_string(),
                error_code: "internal",
                retryable: false,
                detail: Some(json!({
                    "error_class": "io",
                    "io_error": Self::bounded_text(&err.to_string()),
                })),
            },
            FriggError::StrictSemanticFailure { reason } => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::Internal,
                message: format!("semantic channel strict failure: {reason}"),
                error_code: "unavailable",
                retryable: true,
                detail: Some(json!({
                    "error_class": "semantic",
                    "semantic_status": "strict_failure",
                    "semantic_reason": Self::bounded_text(&reason),
                })),
            },
            FriggError::Internal(message) => FriggErrorTranslation {
                transport_code: FriggErrorTransportCode::Internal,
                message,
                error_code: "internal",
                retryable: false,
                detail: None,
            },
        }
    }

    pub(super) fn timeout(message: impl Into<String>, detail: Option<Value>) -> ErrorData {
        Self::internal_with_code(message, "timeout", true, detail)
    }

    pub(super) fn usize_to_u64(value: usize) -> u64 {
        u64::try_from(value).unwrap_or(u64::MAX)
    }

    pub(super) fn find_references_resource_budgets(&self) -> FindReferencesResourceBudgets {
        let source_max_file_bytes = self
            .config
            .max_file_bytes
            .saturating_mul(Self::FIND_REFERENCES_SOURCE_FILE_BYTES_MULTIPLIER)
            .max(self.config.max_file_bytes);
        if self.config.full_scip_ingest {
            let source_max_total_bytes = source_max_file_bytes
                .saturating_mul(Self::FIND_REFERENCES_TOTAL_BYTES_MULTIPLIER)
                .max(source_max_file_bytes);
            return FindReferencesResourceBudgets {
                scip_max_artifacts: usize::MAX,
                scip_max_artifact_bytes: usize::MAX,
                scip_max_total_bytes: usize::MAX,
                scip_max_documents_per_artifact: usize::MAX,
                scip_max_elapsed_ms: u64::MAX,
                source_max_files: Self::FIND_REFERENCES_MAX_SOURCE_FILES,
                source_max_file_bytes,
                source_max_total_bytes,
                source_max_elapsed_ms: Self::FIND_REFERENCES_SOURCE_MAX_ELAPSED_MS,
            };
        }
        let scip_max_artifact_bytes = self
            .config
            .max_file_bytes
            .saturating_mul(Self::FIND_REFERENCES_SCIP_ARTIFACT_BYTES_MULTIPLIER)
            .max(self.config.max_file_bytes);
        let source_max_total_bytes = source_max_file_bytes
            .saturating_mul(Self::FIND_REFERENCES_TOTAL_BYTES_MULTIPLIER)
            .max(source_max_file_bytes);
        let scip_max_total_bytes = scip_max_artifact_bytes
            .saturating_mul(Self::FIND_REFERENCES_TOTAL_BYTES_MULTIPLIER)
            .max(scip_max_artifact_bytes);
        let scip_max_documents_per_artifact = self
            .config
            .max_search_results
            .saturating_mul(Self::FIND_REFERENCES_DOCUMENT_BUDGET_MULTIPLIER)
            .max(Self::FIND_REFERENCES_MIN_SCIP_DOCUMENT_BUDGET);

        FindReferencesResourceBudgets {
            scip_max_artifacts: Self::FIND_REFERENCES_MAX_SCIP_ARTIFACTS,
            scip_max_artifact_bytes,
            scip_max_total_bytes,
            scip_max_documents_per_artifact,
            scip_max_elapsed_ms: Self::FIND_REFERENCES_SCIP_MAX_ELAPSED_MS,
            source_max_files: Self::FIND_REFERENCES_MAX_SOURCE_FILES,
            source_max_file_bytes,
            source_max_total_bytes,
            source_max_elapsed_ms: Self::FIND_REFERENCES_SOURCE_MAX_ELAPSED_MS,
        }
    }

    pub(super) fn find_references_budget_metadata(budgets: FindReferencesResourceBudgets) -> Value {
        json!({
            "scip": {
                "max_artifacts": budgets.scip_max_artifacts,
                "max_artifact_bytes": budgets.scip_max_artifact_bytes,
                "max_total_bytes": budgets.scip_max_total_bytes,
                "max_documents_per_artifact": budgets.scip_max_documents_per_artifact,
                "max_elapsed_ms": budgets.scip_max_elapsed_ms,
            },
            "source": {
                "max_files": budgets.source_max_files,
                "max_file_bytes": budgets.source_max_file_bytes,
                "max_total_bytes": budgets.source_max_total_bytes,
                "max_elapsed_ms": budgets.source_max_elapsed_ms,
            },
        })
    }

    pub(super) fn find_references_resource_budget_error(
        budget_scope: &str,
        budget_code: &str,
        message: impl Into<String>,
        detail: Value,
    ) -> ErrorData {
        let mut detail = match detail {
            Value::Object(object) => object,
            other => {
                let mut object = serde_json::Map::new();
                object.insert("detail".to_owned(), other);
                object
            }
        };
        detail.insert(
            "tool_name".to_owned(),
            Value::String("find_references".to_owned()),
        );
        detail.insert(
            "budget_scope".to_owned(),
            Value::String(budget_scope.to_owned()),
        );
        detail.insert(
            "budget_code".to_owned(),
            Value::String(budget_code.to_owned()),
        );

        Self::timeout(message, Some(Value::Object(detail)))
    }

    pub(super) fn provenance_persistence_error(
        stage: ProvenancePersistenceStage,
        tool_name: &str,
        repository_id: Option<&str>,
        db_path: Option<&Path>,
        err: impl std::fmt::Display,
    ) -> ErrorData {
        let mut detail = serde_json::Map::new();
        detail.insert(
            "provenance_stage".to_owned(),
            Value::String(stage.as_str().to_owned()),
        );
        detail.insert("tool_name".to_owned(), Value::String(tool_name.to_owned()));
        if let Some(repository_id) = repository_id {
            detail.insert(
                "repository_id".to_owned(),
                Value::String(repository_id.to_owned()),
            );
        }
        if let Some(db_path) = db_path {
            detail.insert(
                "db_path".to_owned(),
                Value::String(db_path.display().to_string()),
            );
        }

        let raw_message = err.to_string();
        detail.insert(
            "provenance_error".to_owned(),
            Value::String(Self::bounded_text(&raw_message)),
        );

        Self::internal_with_code(
            format!("failed to persist provenance for tool {tool_name}"),
            "provenance_persistence_failed",
            stage.retryable(),
            Some(Value::Object(detail)),
        )
    }

    pub(super) fn parse_env_flag(raw: &str) -> bool {
        let normalized = raw.trim().to_ascii_lowercase();
        matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
    }

    pub(super) fn provenance_best_effort_from_env() -> bool {
        std::env::var(Self::PROVENANCE_BEST_EFFORT_ENV)
            .map(|raw| Self::parse_env_flag(&raw))
            .unwrap_or(false)
    }

    pub(super) fn map_frigg_error(err: FriggError) -> ErrorData {
        Self::build_frigg_error_data(Self::translate_frigg_error(err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_with_metadata_publish_object_schemas() {
        let router = FriggMcpServer::filtered_tool_router(ToolSurfaceProfile::Extended);
        let metadata_tools = [
            "document_symbols",
            "find_declarations",
            "find_implementations",
            "find_references",
            "go_to_definition",
            "incoming_calls",
            "inspect_syntax_tree",
            "outgoing_calls",
            "search_structural",
            "search_symbol",
        ];

        for tool_name in metadata_tools {
            let tool = router
                .get(tool_name)
                .expect("expected tool to be registered");
            let output_schema = tool
                .output_schema
                .as_ref()
                .expect("expected tool to publish output schema");
            let properties = output_schema
                .get("properties")
                .and_then(Value::as_object)
                .expect("expected tool output schema properties");
            let metadata_schema = properties
                .get("metadata")
                .expect("expected tool metadata schema");
            assert!(
                metadata_schema.is_object(),
                "tool `{tool_name}` published non-object metadata schema: {metadata_schema}",
            );
            assert_eq!(
                metadata_schema.get("type"),
                Some(&Value::String("object".to_owned())),
                "tool `{tool_name}` should publish metadata as an explicit object schema",
            );
        }
    }
}
