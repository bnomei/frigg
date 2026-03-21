use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum LexicalBackendMode {
    #[default]
    Auto,
    Native,
    Ripgrep,
}

impl LexicalBackendMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Native => "native",
            Self::Ripgrep => "ripgrep",
        }
    }
}

impl std::fmt::Display for LexicalBackendMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LexicalBackendMode {
    type Err = String;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        let normalized = raw.trim().to_ascii_lowercase();
        match normalized.as_str() {
            "auto" => Ok(Self::Auto),
            "native" => Ok(Self::Native),
            "ripgrep" | "rg" => Ok(Self::Ripgrep),
            _ => Err(format!(
                "lexical backend mode must be one of: auto, native, ripgrep (received: {normalized})"
            )),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct LexicalRuntimeConfig {
    pub backend: LexicalBackendMode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ripgrep_executable: Option<PathBuf>,
}
