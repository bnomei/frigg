//! Shared error vocabulary for CLI, storage, indexing, and MCP boundaries.
//!
//! Centralizes `FriggError` variants and the `FriggResult` alias so every layer maps failures into
//! consistent CLI exits and MCP error codes without duplicating transport-specific wrappers.

use std::path::PathBuf;

use thiserror::Error;

/// Result alias used across Frigg crates.
pub type FriggResult<T> = Result<T, FriggError>;

/// Top-level failure modes surfaced to callers and MCP error envelopes.
#[derive(Debug, Error)]
pub enum FriggError {
    /// Caller-supplied arguments or policy inputs failed validation.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// Requested entity or path is absent from workspace/storage.
    #[error("not found: {0}")]
    NotFound(String),

    /// Authorization or workspace isolation blocked the operation.
    #[error("access denied: {0}")]
    AccessDenied(String),

    /// Underlying filesystem or IO failure mapped into the domain error surface.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// Strict semantic mode failed; recovery must not silently fall back.
    #[error("semantic_status=strict_failure: {reason}")]
    StrictSemanticFailure { reason: String },

    /// Local index schema epoch mismatch; rebuild required (no auto-migration).
    #[error(
        "internal error: storage schema is incompatible (found {found_version}, expected {expected_version}); automatic schema migrations are disabled for Frigg's regenerable local index; delete '{}' and run `frigg index` to rebuild it",
        db_path.display()
    )]
    StorageSchemaIncompatible {
        found_version: i64,
        expected_version: i64,
        db_path: PathBuf,
    },

    /// Unexpected invariant or programming failure not covered by other variants.
    #[error("internal error: {0}")]
    Internal(String),
}

#[cfg(test)]
mod tests {
    use std::io::{Error as IoError, ErrorKind};

    use super::FriggError;

    #[test]
    fn frigg_error_displays_expected_messages() {
        assert_eq!(
            FriggError::InvalidInput("bad".to_string()).to_string(),
            "invalid input: bad"
        );
        assert_eq!(
            FriggError::NotFound("missing".to_string()).to_string(),
            "not found: missing"
        );
        assert_eq!(
            FriggError::AccessDenied("denied".to_string()).to_string(),
            "access denied: denied"
        );
        assert_eq!(
            FriggError::Internal("oops".to_string()).to_string(),
            "internal error: oops"
        );
        assert_eq!(
            FriggError::StrictSemanticFailure {
                reason: "provider outage".to_owned()
            }
            .to_string(),
            "semantic_status=strict_failure: provider outage"
        );
        assert_eq!(
            FriggError::StorageSchemaIncompatible {
                found_version: 10,
                expected_version: 11,
                db_path: "/tmp/frigg.db".into(),
            }
            .to_string(),
            "internal error: storage schema is incompatible (found 10, expected 11); automatic schema migrations are disabled for Frigg's regenerable local index; delete '/tmp/frigg.db' and run `frigg index` to rebuild it"
        );
    }

    #[test]
    fn frigg_error_from_io_error() {
        let io_err = IoError::new(ErrorKind::PermissionDenied, "no permission");
        let frigg_err: FriggError = io_err.into();

        assert_eq!(frigg_err.to_string(), "io error: no permission");
    }
}
