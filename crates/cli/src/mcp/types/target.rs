//! Stable, opaque identities used to address result rows across MCP calls.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Closed target identity; unknown variants, fields, and empty identities are rejected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TargetRef {
    ResultMatch {
        #[schemars(length(min = 1))]
        result_handle: String,
        #[schemars(length(min = 1))]
        match_id: String,
        #[schemars(length(min = 1))]
        target_scope: String,
    },
    StableSymbol {
        #[schemars(length(min = 1))]
        repository_id: String,
        #[schemars(length(min = 1))]
        stable_symbol_id: String,
        #[schemars(length(min = 1))]
        snapshot_token: String,
    },
}

impl<'de> Deserialize<'de> for TargetRef {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum Raw {
            ResultMatch {
                result_handle: String,
                match_id: String,
                target_scope: String,
            },
            StableSymbol {
                repository_id: String,
                stable_symbol_id: String,
                snapshot_token: String,
            },
        }
        let value = match Raw::deserialize(deserializer)? {
            Raw::ResultMatch {
                result_handle,
                match_id,
                target_scope,
            } => Self::ResultMatch {
                result_handle,
                match_id,
                target_scope,
            },
            Raw::StableSymbol {
                repository_id,
                stable_symbol_id,
                snapshot_token,
            } => Self::StableSymbol {
                repository_id,
                stable_symbol_id,
                snapshot_token,
            },
        };
        value
            .validate()
            .map_err(serde::de::Error::custom)
            .map(|_| value)
    }
}

impl TargetRef {
    /// Builds a session-bound result-match target when all identity fields are non-empty.
    pub fn result_match(
        result_handle: String,
        match_id: String,
        target_scope: String,
    ) -> Option<Self> {
        (!result_handle.is_empty() && !match_id.is_empty() && !target_scope.is_empty()).then_some(
            Self::ResultMatch {
                result_handle,
                match_id,
                target_scope,
            },
        )
    }

    /// Session correlation scope for `ResultMatch`; `None` for snapshot-bound stable symbols.
    pub fn target_scope(&self) -> Option<&str> {
        match self {
            Self::ResultMatch { target_scope, .. } => Some(target_scope),
            Self::StableSymbol { .. } => None,
        }
    }

    /// Rejects empty identity fields; deserialization also enforces this fail-closed.
    pub fn validate(&self) -> Result<(), &'static str> {
        let fields = match self {
            Self::ResultMatch {
                result_handle,
                match_id,
                target_scope,
            } => [result_handle, match_id, target_scope],
            Self::StableSymbol {
                repository_id,
                stable_symbol_id,
                snapshot_token,
            } => [repository_id, stable_symbol_id, snapshot_token],
        };
        fields
            .iter()
            .all(|value| !value.is_empty())
            .then_some(())
            .ok_or("target identity fields must not be empty")
    }
}

/// Generate one opaque UUIDv7 correlation scope for an MCP session.
pub fn new_target_scope() -> String {
    uuid::Uuid::now_v7().simple().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_variants_round_trip_and_reject_unknown_variant() {
        let target = TargetRef::result_match("h".into(), "m".into(), "s".into())
            .expect("non-empty fixture target should validate");
        let value = serde_json::to_value(&target).expect("fixture target should serialize");
        assert_eq!(value["kind"], "result_match");
        assert!(
            serde_json::from_value::<TargetRef>(serde_json::json!({"kind":"other","x":"y"}))
                .is_err()
        );
        assert!(serde_json::from_value::<TargetRef>(serde_json::json!({"kind":"result_match","result_handle":"h","match_id":"m","target_scope":"s","extra":1})).is_err());
    }

    #[test]
    fn empty_identity_is_invalid() {
        let target = TargetRef::ResultMatch {
            result_handle: String::new(),
            match_id: "m".into(),
            target_scope: "s".into(),
        };
        assert!(target.validate().is_err());
        assert!(serde_json::from_value::<TargetRef>(serde_json::json!({"kind":"result_match","result_handle":"","match_id":"m","target_scope":"s"})).is_err());
        assert!(serde_json::from_value::<TargetRef>(serde_json::json!({"kind":"stable_symbol","repository_id":"repo","stable_symbol_id":"","snapshot_token":"snapshot"})).is_err());
    }
}
