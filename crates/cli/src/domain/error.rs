use thiserror::Error;

pub type FriggResult<T> = Result<T, FriggError>;

#[derive(Debug, Error)]
pub enum FriggError {
    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("access denied: {0}")]
    AccessDenied(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("semantic_status=strict_failure: {reason}")]
    StrictSemanticFailure {
        reason: String,
    },

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
    }

    #[test]
    fn frigg_error_from_io_error() {
        let io_err = IoError::new(ErrorKind::PermissionDenied, "no permission");
        let frigg_err: FriggError = io_err.into();

        assert_eq!(frigg_err.to_string(), "io error: no permission");
    }
}
