use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::{FriggError, FriggResult};
use serde::Deserialize;

const PLAYBOOK_METADATA_MARKER: &str = "<!-- frigg-playbook";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlaybookMetadata {
    pub playbook_schema: String,
    pub playbook_id: String,
    #[serde(default)]
    pub hybrid_regression: Option<HybridPlaybookRegression>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HybridPlaybookRegression {
    pub query: String,
    #[serde(default = "default_hybrid_top_k")]
    pub top_k: usize,
    #[serde(default)]
    pub allowed_semantic_statuses: Vec<String>,
    #[serde(default)]
    pub witness_groups: Vec<HybridWitnessGroup>,
    #[serde(default)]
    pub target_witness_groups: Vec<HybridWitnessGroup>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct HybridWitnessGroup {
    pub group_id: String,
    pub match_any: Vec<String>,
    #[serde(default)]
    pub required_when: HybridWitnessRequirement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HybridWitnessRequirement {
    #[default]
    Always,
    SemanticOk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookDocument {
    pub metadata: PlaybookMetadata,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedHybridPlaybookRegression {
    pub path: PathBuf,
    pub metadata: PlaybookMetadata,
    pub spec: HybridPlaybookRegression,
}

fn default_hybrid_top_k() -> usize {
    8
}

fn playbook_metadata_header_bounds(raw: &str) -> Option<(usize, usize)> {
    let raw = raw.trim_start_matches('\u{feff}');
    let start = raw.find(PLAYBOOK_METADATA_MARKER)?;
    let after_marker = &raw[start + PLAYBOOK_METADATA_MARKER.len()..];
    let close_index = after_marker.find("-->")?;
    Some((
        start,
        start + PLAYBOOK_METADATA_MARKER.len() + close_index + 3,
    ))
}

pub fn scrub_playbook_metadata_header(raw: &str) -> Cow<'_, str> {
    let Some((start, end)) = playbook_metadata_header_bounds(raw) else {
        return Cow::Borrowed(raw);
    };

    let mut scrubbed = String::with_capacity(raw.len());
    scrubbed.extend(raw[..start].chars());
    scrubbed.extend(raw[start..end].chars().map(|ch| match ch {
        '\n' | '\r' => ch,
        _ => ' ',
    }));
    scrubbed.push_str(&raw[end..]);
    Cow::Owned(scrubbed)
}

pub fn parse_playbook_document(raw: &str) -> FriggResult<PlaybookDocument> {
    let raw = raw.trim_start_matches('\u{feff}');
    let Some((header_start, header_end)) = playbook_metadata_header_bounds(raw) else {
        return Err(FriggError::InvalidInput(
            "playbook metadata header must include '<!-- frigg-playbook'".to_owned(),
        ));
    };
    let after_marker = &raw[header_start + PLAYBOOK_METADATA_MARKER.len()..header_end - 3];
    let metadata_block = after_marker.trim();
    let metadata = normalize_playbook_metadata(
        serde_json::from_str::<RawPlaybookMetadata>(metadata_block).map_err(|err| {
            FriggError::InvalidInput(format!("failed to parse playbook metadata header: {err}"))
        })?,
    )?;
    let mut body = String::with_capacity(raw.len().saturating_sub(header_end - header_start));
    body.push_str(&raw[..header_start]);
    body.push_str(&raw[header_end..]);
    let body = body.trim_start_matches(['\r', '\n']).to_owned();

    Ok(PlaybookDocument { metadata, body })
}

#[derive(Debug, Clone, Deserialize)]
struct RawPlaybookMetadata {
    #[serde(default)]
    playbook_schema: Option<String>,
    #[serde(default)]
    schema: Option<String>,
    playbook_id: String,
    #[serde(default)]
    hybrid_regression: Option<RawHybridPlaybookRegression>,
    #[serde(default)]
    query: Option<String>,
    #[serde(default)]
    top_k: Option<usize>,
    #[serde(default)]
    allowed_semantic_statuses: Vec<String>,
    #[serde(default)]
    required_witness_groups: Vec<RawHybridWitnessGroup>,
    #[serde(default)]
    target_witness_groups: Vec<RawHybridWitnessGroup>,
    #[serde(default)]
    target_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawHybridPlaybookRegression {
    query: String,
    #[serde(default = "default_hybrid_top_k")]
    top_k: usize,
    #[serde(default)]
    allowed_semantic_statuses: Vec<String>,
    #[serde(default)]
    witness_groups: Vec<RawHybridWitnessGroup>,
    #[serde(default)]
    target_witness_groups: Vec<RawHybridWitnessGroup>,
    #[serde(default)]
    target_paths: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawHybridWitnessGroup {
    #[serde(default)]
    group_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    match_any: Vec<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    required_when: HybridWitnessRequirement,
}

fn normalize_playbook_metadata(raw: RawPlaybookMetadata) -> FriggResult<PlaybookMetadata> {
    let playbook_schema = raw.playbook_schema.or(raw.schema).ok_or_else(|| {
        FriggError::InvalidInput("playbook metadata must include a schema".to_owned())
    })?;
    let hybrid_regression = match raw.hybrid_regression {
        Some(spec) => Some(normalize_hybrid_regression(spec)?),
        None if playbook_schema == "frigg.playbook.hybrid.v1" => {
            Some(normalize_hybrid_regression(RawHybridPlaybookRegression {
                query: raw.query.ok_or_else(|| {
                    FriggError::InvalidInput(
                        "hybrid playbook metadata must include a query".to_owned(),
                    )
                })?,
                top_k: raw.top_k.unwrap_or_else(default_hybrid_top_k),
                allowed_semantic_statuses: raw.allowed_semantic_statuses,
                witness_groups: raw.required_witness_groups,
                target_witness_groups: raw.target_witness_groups,
                target_paths: raw.target_paths,
            })?)
        }
        None => None,
    };

    Ok(PlaybookMetadata {
        playbook_schema,
        playbook_id: raw.playbook_id,
        hybrid_regression,
    })
}

fn normalize_hybrid_regression(
    raw: RawHybridPlaybookRegression,
) -> FriggResult<HybridPlaybookRegression> {
    let mut target_witness_groups = raw
        .target_witness_groups
        .into_iter()
        .map(normalize_hybrid_witness_group)
        .collect::<FriggResult<Vec<_>>>()?;
    for path in raw.target_paths {
        target_witness_groups.push(HybridWitnessGroup {
            group_id: path.clone(),
            match_any: vec![path],
            required_when: HybridWitnessRequirement::SemanticOk,
        });
    }

    Ok(HybridPlaybookRegression {
        query: raw.query,
        top_k: raw.top_k,
        allowed_semantic_statuses: raw.allowed_semantic_statuses,
        witness_groups: raw
            .witness_groups
            .into_iter()
            .map(normalize_hybrid_witness_group)
            .collect::<FriggResult<Vec<_>>>()?,
        target_witness_groups,
    })
}

fn normalize_hybrid_witness_group(raw: RawHybridWitnessGroup) -> FriggResult<HybridWitnessGroup> {
    let group_id = raw.group_id.or(raw.name).ok_or_else(|| {
        FriggError::InvalidInput("hybrid witness group must include group_id or name".to_owned())
    })?;
    let match_any = if raw.match_any.is_empty() {
        raw.paths
    } else {
        raw.match_any
    };
    if match_any.is_empty() {
        return Err(FriggError::InvalidInput(format!(
            "hybrid witness group '{group_id}' must include at least one path"
        )));
    }

    Ok(HybridWitnessGroup {
        group_id,
        match_any,
        required_when: raw.required_when,
    })
}

pub fn load_playbook_document(path: &Path) -> FriggResult<PlaybookDocument> {
    let raw = fs::read_to_string(path).map_err(FriggError::Io)?;
    parse_playbook_document(&raw).map_err(|err| {
        FriggError::InvalidInput(format!(
            "failed to load playbook metadata from '{}': {err}",
            path.display()
        ))
    })
}

pub fn load_hybrid_playbook_regressions(
    playbooks_root: &Path,
) -> FriggResult<Vec<LoadedHybridPlaybookRegression>> {
    let mut paths = fs::read_dir(playbooks_root)
        .map_err(FriggError::Io)?
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.extension().and_then(|extension| extension.to_str()) == Some("md")
                && path.file_name().and_then(|name| name.to_str()) != Some("README.md")
        })
        .collect::<Vec<_>>();
    paths.sort();

    let mut regressions = Vec::new();
    for path in paths {
        let document = load_playbook_document(&path)?;
        let spec = document.metadata.hybrid_regression.clone().ok_or_else(|| {
            FriggError::InvalidInput(format!(
                "playbook '{}' is missing hybrid_regression metadata",
                path.display()
            ))
        })?;
        regressions.push(LoadedHybridPlaybookRegression {
            path,
            metadata: document.metadata,
            spec,
        });
    }

    if regressions.is_empty() {
        return Err(FriggError::InvalidInput(format!(
            "no executable hybrid playbooks found under '{}'",
            playbooks_root.display()
        )));
    }

    Ok(regressions)
}

#[cfg(test)]
mod tests {
    use super::{
        HybridWitnessRequirement, PlaybookDocument, load_hybrid_playbook_regressions,
        parse_playbook_document, scrub_playbook_metadata_header,
    };
    use crate::domain::FriggResult;
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn parse_playbook_document_extracts_metadata_and_body() -> FriggResult<()> {
        let raw = r#"# Example

<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "hybrid-search-context-retrieval",
  "query": "semantic runtime strict failure note metadata",
  "top_k": 8,
  "allowed_semantic_statuses": ["ok", "degraded", "disabled"],
  "required_witness_groups": [
    {
      "name": "docs",
      "paths": ["contracts/errors.md"],
      "required_when": "semantic_ok"
    }
  ],
  "target_witness_groups": [
    {
      "name": "docs",
      "paths": ["contracts/errors.md"]
    }
  ]
}
-->
Body text.
"#;

        let parsed = parse_playbook_document(raw)?;
        assert_eq!(
            parsed.metadata.playbook_schema,
            "frigg.playbook.hybrid.v1".to_owned()
        );
        assert_eq!(
            parsed.metadata.playbook_id,
            "hybrid-search-context-retrieval".to_owned()
        );
        let spec = parsed
            .metadata
            .hybrid_regression
            .clone()
            .expect("hybrid regression metadata must be present");
        assert_eq!(spec.query, "semantic runtime strict failure note metadata");
        assert_eq!(spec.top_k, 8);
        assert_eq!(
            spec.allowed_semantic_statuses,
            vec!["ok", "degraded", "disabled"]
        );
        assert_eq!(spec.witness_groups.len(), 1);
        assert_eq!(
            spec.witness_groups[0].required_when,
            HybridWitnessRequirement::SemanticOk
        );
        assert_eq!(spec.target_witness_groups.len(), 1);
        assert_eq!(
            spec.target_witness_groups[0].match_any,
            vec!["contracts/errors.md"]
        );
        assert_eq!(
            parsed,
            PlaybookDocument {
                metadata: parsed.metadata.clone(),
                body: "# Example\n\n\nBody text.\n".to_owned(),
            }
        );
        Ok(())
    }

    #[test]
    fn load_hybrid_playbook_regressions_requires_metadata_for_markdown_playbooks() -> FriggResult<()>
    {
        let root = temp_playbook_root("missing-metadata");
        fs::create_dir_all(&root).map_err(crate::domain::FriggError::Io)?;
        fs::write(root.join("README.md"), "# Playbooks\n")
            .map_err(crate::domain::FriggError::Io)?;
        fs::write(root.join("alpha.md"), "# Alpha\n").map_err(crate::domain::FriggError::Io)?;

        let error = load_hybrid_playbook_regressions(&root)
            .expect_err("markdown playbooks without metadata should fail");
        assert!(
            error
                .to_string()
                .contains("failed to load playbook metadata"),
            "unexpected playbook metadata error: {error}"
        );

        cleanup_root(&root);
        Ok(())
    }

    #[test]
    fn scrub_playbook_metadata_header_preserves_line_numbers_but_hides_query_text() {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.v1",
  "playbook_id": "http-auth-entrypoint-trace",
  "hybrid_regression": {
    "query": "where is the optional HTTP MCP auth token declared enforced and documented"
  }
}
-->
# HTTP Auth
"#;

        let scrubbed = scrub_playbook_metadata_header(raw);
        assert_eq!(raw.lines().count(), scrubbed.lines().count());
        assert!(
            !scrubbed.contains("where is the optional HTTP MCP auth token"),
            "scrubbed playbook text should not expose executable query strings"
        );
        assert!(scrubbed.contains("# HTTP Auth"));
    }

    fn temp_playbook_root(test_name: &str) -> std::path::PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        env::temp_dir().join(format!(
            "frigg-playbooks-{test_name}-{nonce}-{}",
            std::process::id()
        ))
    }

    fn cleanup_root(root: &Path) {
        let _ = fs::remove_dir_all(root);
    }
}
