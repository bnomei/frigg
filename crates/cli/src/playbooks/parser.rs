use std::borrow::Cow;

use crate::domain::{FriggError, FriggResult};
use crate::text_sanitization::{leading_metadata_comment_bounds, scrub_leading_metadata_comment};
use serde::Deserialize;

use super::model::default_hybrid_top_k;
use super::{
    HybridPlaybookRegression, HybridWitnessGroup, HybridWitnessMatchMode, HybridWitnessRequirement,
    PlaybookDocument, PlaybookMetadata,
};

const PLAYBOOK_METADATA_MARKER: &str = "<!-- frigg-playbook";

pub fn scrub_playbook_metadata_header(raw: &str) -> Cow<'_, str> {
    scrub_leading_metadata_comment(raw, PLAYBOOK_METADATA_MARKER)
}

pub fn parse_playbook_document(raw: &str) -> FriggResult<PlaybookDocument> {
    let raw = raw.trim_start_matches('\u{feff}');
    let Some((header_start, header_end)) =
        leading_metadata_comment_bounds(raw, PLAYBOOK_METADATA_MARKER)
    else {
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
    match_mode: HybridWitnessMatchMode,
    #[serde(default)]
    accepted_prefixes: Vec<String>,
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
            match_mode: HybridWitnessMatchMode::ExactAny,
            accepted_prefixes: Vec::new(),
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
    let accepted_prefixes = raw
        .accepted_prefixes
        .into_iter()
        .map(|prefix| prefix.trim().trim_matches('/').to_owned())
        .filter(|prefix| !prefix.is_empty())
        .fold(Vec::<String>::new(), |mut acc, prefix| {
            if !acc.iter().any(|existing| existing == &prefix) {
                acc.push(prefix);
            }
            acc
        });

    Ok(HybridWitnessGroup {
        group_id,
        match_any,
        match_mode: raw.match_mode,
        accepted_prefixes,
        required_when: raw.required_when,
    })
}
