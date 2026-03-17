use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::domain::{FriggError, FriggResult};
use crate::searcher::{
    HybridRankedEvidence, HybridRankingIntent, SearchFilters, SearchHybridExecutionOutput,
    SearchHybridQuery, SearchStageAttribution, TextSearcher, path_quality_rule_trace,
    path_witness_rule_trace, selection_rule_trace,
};
use crate::text_sanitization::{leading_metadata_comment_bounds, scrub_leading_metadata_comment};
use serde::{Deserialize, Serialize};

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
    pub match_mode: HybridWitnessMatchMode,
    #[serde(default)]
    pub accepted_prefixes: Vec<String>,
    #[serde(default)]
    pub required_when: HybridWitnessRequirement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HybridWitnessMatchMode {
    #[default]
    ExactAny,
    ExactOrPrefix,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
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

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridPlaybookChannelHitSnapshot {
    pub rank: usize,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub score: f32,
    pub excerpt: String,
    pub provenance_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridPlaybookChannelTrace {
    pub channel: String,
    pub health_status: String,
    pub health_reason: Option<String>,
    pub candidate_count: usize,
    pub hit_count: usize,
    pub match_count: usize,
    pub hits: Vec<HybridPlaybookChannelHitSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridPlaybookRankedHitSnapshot {
    pub rank: usize,
    pub repository_id: String,
    pub path: String,
    pub line: usize,
    pub column: usize,
    pub blended_score: f32,
    pub lexical_score: f32,
    pub witness_score: f32,
    pub graph_score: f32,
    pub semantic_score: f32,
    pub excerpt: String,
    pub lexical_sources: Vec<String>,
    pub witness_sources: Vec<String>,
    pub graph_sources: Vec<String>,
    pub semantic_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridPlaybookCandidateTraceSnapshot {
    pub rank: usize,
    pub path: String,
    pub selection_rules: Vec<String>,
    pub path_witness_rules: Vec<String>,
    pub path_quality_rules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridPlaybookTracePacket {
    pub playbook_id: String,
    pub file_name: String,
    pub query: String,
    pub top_k: usize,
    pub semantic_status: String,
    pub semantic_reason: Option<String>,
    pub status_allowed: bool,
    pub duration_ms: u128,
    pub matched_paths: Vec<String>,
    pub required_witness_groups: Vec<HybridPlaybookWitnessOutcome>,
    pub target_witness_groups: Vec<HybridPlaybookWitnessOutcome>,
    pub stage_attribution: Option<SearchStageAttribution>,
    pub channels: Vec<HybridPlaybookChannelTrace>,
    pub ranked_anchors: Vec<HybridPlaybookRankedHitSnapshot>,
    pub coverage_grouped_pool: Vec<HybridPlaybookRankedHitSnapshot>,
    pub final_matches: Vec<HybridPlaybookRankedHitSnapshot>,
    pub candidate_traces: Vec<HybridPlaybookCandidateTraceSnapshot>,
    pub post_selection_repairs: Vec<BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HybridPlaybookWitnessOutcome {
    pub group_id: String,
    pub match_any: Vec<String>,
    pub match_mode: HybridWitnessMatchMode,
    pub accepted_prefixes: Vec<String>,
    pub required_when: HybridWitnessRequirement,
    pub matched_by: HybridWitnessMatchBy,
    pub matched_path: Option<String>,
    pub passed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HybridWitnessMatchBy {
    #[default]
    None,
    Exact,
    Prefix,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HybridPlaybookProbeOutcome {
    pub file_name: String,
    pub playbook_id: String,
    pub semantic_status: String,
    pub semantic_reason: Option<String>,
    pub status_allowed: bool,
    pub duration_ms: u128,
    pub execution_error: Option<String>,
    pub matched_paths: Vec<String>,
    pub trace_path: Option<String>,
    pub required_witness_groups: Vec<HybridPlaybookWitnessOutcome>,
    pub target_witness_groups: Vec<HybridPlaybookWitnessOutcome>,
}

impl HybridPlaybookProbeOutcome {
    pub fn required_missing(&self) -> Vec<String> {
        self.required_witness_groups
            .iter()
            .filter(|group| !group.passed)
            .map(|group| format!("{} -> {:?}", group.group_id, group.match_any))
            .collect()
    }

    pub fn target_missing(&self) -> Vec<String> {
        self.target_witness_groups
            .iter()
            .filter(|group| !group.passed)
            .map(|group| format!("{} -> {:?}", group.group_id, group.match_any))
            .collect()
    }

    pub fn passed_required(&self) -> bool {
        self.execution_error.is_none()
            && self.status_allowed
            && self
                .required_witness_groups
                .iter()
                .all(|group| group.passed)
    }

    pub fn passed_targets(&self) -> bool {
        self.execution_error.is_none()
            && self.status_allowed
            && self.target_witness_groups.iter().all(|group| group.passed)
    }

    pub fn passed_all(&self, enforce_targets: bool) -> bool {
        self.passed_required() && (!enforce_targets || self.passed_targets())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HybridPlaybookRunSummary {
    pub playbooks_root: String,
    pub enforce_targets: bool,
    pub playbook_count: usize,
    pub required_failures: usize,
    pub target_failures: usize,
    pub outcomes: Vec<HybridPlaybookProbeOutcome>,
}

fn default_hybrid_top_k() -> usize {
    8
}

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

fn path_matches_prefix(candidate: &str, prefix: &str) -> bool {
    let normalized_candidate = candidate.trim().trim_matches('/');
    let normalized_prefix = prefix.trim().trim_matches('/');
    if normalized_candidate.is_empty() || normalized_prefix.is_empty() {
        return false;
    }
    normalized_candidate == normalized_prefix
        || normalized_candidate
            .strip_prefix(normalized_prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn witness_group_match(
    group: &HybridWitnessGroup,
    matched_paths: &[String],
) -> (HybridWitnessMatchBy, Option<String>, bool) {
    if let Some(path) = group
        .match_any
        .iter()
        .find_map(|expected| {
            matched_paths
                .iter()
                .find(|candidate| *candidate == expected)
        })
        .cloned()
    {
        return (HybridWitnessMatchBy::Exact, Some(path), true);
    }

    if group.match_mode == HybridWitnessMatchMode::ExactOrPrefix {
        if let Some(path) = group
            .accepted_prefixes
            .iter()
            .find_map(|prefix| {
                matched_paths
                    .iter()
                    .find(|candidate| path_matches_prefix(candidate, prefix))
            })
            .cloned()
        {
            return (HybridWitnessMatchBy::Prefix, Some(path), false);
        }
    }

    (HybridWitnessMatchBy::None, None, false)
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

fn witness_outcomes(
    groups: &[HybridWitnessGroup],
    matched_paths: &[String],
    semantic_status_ok: bool,
    target_only: bool,
) -> Vec<HybridPlaybookWitnessOutcome> {
    groups
        .iter()
        .filter(|group| {
            !target_only
                || matches!(group.required_when, HybridWitnessRequirement::Always)
                || semantic_status_ok
        })
        .map(|group| {
            let required = if target_only {
                true
            } else {
                match group.required_when {
                    HybridWitnessRequirement::Always => true,
                    HybridWitnessRequirement::SemanticOk => semantic_status_ok,
                }
            };
            let (matched_by, matched_path, exact_matched) =
                witness_group_match(group, matched_paths);
            let passed = !required || exact_matched;
            HybridPlaybookWitnessOutcome {
                group_id: group.group_id.clone(),
                match_any: group.match_any.clone(),
                match_mode: group.match_mode,
                accepted_prefixes: group.accepted_prefixes.clone(),
                required_when: group.required_when,
                matched_by,
                matched_path,
                passed,
            }
        })
        .collect()
}

fn semantic_status_allowed(allowed_statuses: &[String], semantic_status: &str) -> bool {
    if allowed_statuses.is_empty() {
        return true;
    }

    let semantic_status = semantic_status.trim().to_ascii_lowercase();
    if allowed_statuses
        .iter()
        .any(|status| status.trim().eq_ignore_ascii_case(&semantic_status))
    {
        return true;
    }

    semantic_status == "unavailable"
        && allowed_statuses
            .iter()
            .any(|status| status.trim().eq_ignore_ascii_case("disabled"))
}

fn sanitize_trace_component(value: &str) -> String {
    let mut sanitized = String::with_capacity(value.len());
    let mut last_was_dash = false;
    for ch in value.chars() {
        let lowered = ch.to_ascii_lowercase();
        if lowered.is_ascii_alphanumeric() {
            sanitized.push(lowered);
            last_was_dash = false;
        } else if !last_was_dash {
            sanitized.push('-');
            last_was_dash = true;
        }
    }
    sanitized.trim_matches('-').to_owned()
}

fn collect_channel_traces(
    output: &SearchHybridExecutionOutput,
    trace_limit: usize,
) -> Vec<HybridPlaybookChannelTrace> {
    output
        .channel_results
        .iter()
        .map(|result| HybridPlaybookChannelTrace {
            channel: result.channel.as_str().to_owned(),
            health_status: result.health.status.as_str().to_owned(),
            health_reason: result.health.reason.clone(),
            candidate_count: result.stats.candidate_count,
            hit_count: result.stats.hit_count,
            match_count: result.stats.match_count,
            hits: result
                .hits
                .iter()
                .take(trace_limit)
                .enumerate()
                .map(|(index, hit)| HybridPlaybookChannelHitSnapshot {
                    rank: index + 1,
                    repository_id: hit.document.repository_id.clone(),
                    path: hit.document.path.clone(),
                    line: hit.document.line,
                    column: hit.document.column,
                    score: hit.raw_score,
                    excerpt: hit.excerpt.clone(),
                    provenance_ids: hit.provenance_ids.clone(),
                })
                .collect(),
        })
        .collect()
}

fn collect_ranked_hit_snapshots(
    hits: &[HybridRankedEvidence],
    trace_limit: usize,
) -> Vec<HybridPlaybookRankedHitSnapshot> {
    hits.iter()
        .take(trace_limit)
        .enumerate()
        .map(|(index, hit)| HybridPlaybookRankedHitSnapshot {
            rank: index + 1,
            repository_id: hit.document.repository_id.clone(),
            path: hit.document.path.clone(),
            line: hit.document.line,
            column: hit.document.column,
            blended_score: hit.blended_score,
            lexical_score: hit.lexical_score,
            witness_score: hit.witness_score,
            graph_score: hit.graph_score,
            semantic_score: hit.semantic_score,
            excerpt: hit.excerpt.clone(),
            lexical_sources: hit.lexical_sources.clone(),
            witness_sources: hit.witness_sources.clone(),
            graph_sources: hit.graph_sources.clone(),
            semantic_sources: hit.semantic_sources.clone(),
        })
        .collect()
}

fn collect_candidate_traces(
    candidates: &[HybridRankedEvidence],
    intent: &HybridRankingIntent,
    query_text: &str,
    trace_limit: usize,
) -> Vec<HybridPlaybookCandidateTraceSnapshot> {
    let mut selected: Vec<HybridRankedEvidence> = Vec::new();
    let mut traces = Vec::new();
    for (index, candidate) in candidates.iter().take(trace_limit).enumerate() {
        traces.push(HybridPlaybookCandidateTraceSnapshot {
            rank: index + 1,
            path: candidate.document.path.clone(),
            selection_rules: selection_rule_trace(candidate.clone(), &selected, intent, query_text),
            path_witness_rules: path_witness_rule_trace(
                &candidate.document.path,
                intent,
                query_text,
            ),
            path_quality_rules: path_quality_rule_trace(&candidate.document.path, intent),
        });
        selected.push(candidate.clone());
    }
    traces
}

fn collect_post_selection_repairs(
    output: &SearchHybridExecutionOutput,
) -> Vec<BTreeMap<String, String>> {
    output
        .post_selection_trace
        .as_ref()
        .map(|trace| {
            trace
                .events
                .iter()
                .map(|event| {
                    let mut entry = BTreeMap::new();
                    entry.insert("rule_id".to_owned(), event.rule_id.to_owned());
                    entry.insert(
                        "rule_stage".to_owned(),
                        format!("{:?}", event.rule_stage).to_ascii_lowercase(),
                    );
                    entry.insert(
                        "action".to_owned(),
                        format!("{:?}", event.action).to_ascii_lowercase(),
                    );
                    entry.insert("candidate_path".to_owned(), event.candidate_path.clone());
                    entry.insert(
                        "replaced_path".to_owned(),
                        event.replaced_path.clone().unwrap_or_default(),
                    );
                    entry
                })
                .collect()
        })
        .unwrap_or_default()
}

fn write_trace_packet(
    trace_root: &Path,
    regression: &LoadedHybridPlaybookRegression,
    output: &SearchHybridExecutionOutput,
    outcome: &HybridPlaybookProbeOutcome,
) -> FriggResult<String> {
    let trace_limit = regression.spec.top_k.max(10);
    let intent = HybridRankingIntent::from_query(&regression.spec.query);
    let file_name = format!(
        "{}.json",
        sanitize_trace_component(&regression.metadata.playbook_id)
    );
    let trace_path = trace_root.join(file_name);
    let packet = HybridPlaybookTracePacket {
        playbook_id: regression.metadata.playbook_id.clone(),
        file_name: regression
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_owned(),
        query: regression.spec.query.clone(),
        top_k: regression.spec.top_k,
        semantic_status: outcome.semantic_status.clone(),
        semantic_reason: outcome.semantic_reason.clone(),
        status_allowed: outcome.status_allowed,
        duration_ms: outcome.duration_ms,
        matched_paths: outcome.matched_paths.clone(),
        required_witness_groups: outcome.required_witness_groups.clone(),
        target_witness_groups: outcome.target_witness_groups.clone(),
        stage_attribution: output.stage_attribution.clone(),
        channels: collect_channel_traces(output, trace_limit),
        ranked_anchors: collect_ranked_hit_snapshots(&output.ranked_anchors, trace_limit),
        coverage_grouped_pool: collect_ranked_hit_snapshots(
            &output.coverage_grouped_pool,
            trace_limit,
        ),
        final_matches: collect_ranked_hit_snapshots(&output.matches, trace_limit),
        candidate_traces: collect_candidate_traces(
            &output.coverage_grouped_pool,
            &intent,
            &regression.spec.query,
            trace_limit,
        ),
        post_selection_repairs: collect_post_selection_repairs(output),
    };
    let payload = serde_json::to_string_pretty(&packet)
        .map_err(|error| FriggError::Internal(error.to_string()))?;
    if let Some(parent) = trace_path.parent() {
        fs::create_dir_all(parent).map_err(FriggError::Io)?;
    }
    fs::write(&trace_path, payload).map_err(FriggError::Io)?;
    Ok(trace_path.display().to_string())
}

pub fn run_hybrid_playbook_regression(
    searcher: &TextSearcher,
    regression: &LoadedHybridPlaybookRegression,
    trace_root: Option<&Path>,
) -> HybridPlaybookProbeOutcome {
    let started = Instant::now();
    let query = SearchHybridQuery {
        query: regression.spec.query.clone(),
        limit: regression.spec.top_k,
        weights: Default::default(),
        semantic: Some(true),
    };
    let result = if trace_root.is_some() {
        searcher.search_hybrid_with_filters_with_trace(query, SearchFilters::default())
    } else {
        searcher.search_hybrid_with_filters(query, SearchFilters::default())
    };

    match result {
        Ok(output) => {
            let semantic_status = output.note.semantic_status.as_str().to_owned();
            let allowed_statuses = regression
                .spec
                .allowed_semantic_statuses
                .iter()
                .map(|status| status.trim().to_ascii_lowercase())
                .collect::<Vec<_>>();
            let status_allowed = semantic_status_allowed(&allowed_statuses, &semantic_status);
            let matched_paths = output
                .matches
                .iter()
                .map(|entry| entry.document.path.clone())
                .collect::<Vec<_>>();
            let semantic_status_ok = output.note.semantic_status.as_str() == "ok";
            let required_witness_groups = witness_outcomes(
                &regression.spec.witness_groups,
                &matched_paths,
                semantic_status_ok,
                false,
            );
            let target_witness_groups = witness_outcomes(
                &regression.spec.target_witness_groups,
                &matched_paths,
                semantic_status_ok,
                true,
            );
            let mut outcome = HybridPlaybookProbeOutcome {
                file_name: regression
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                playbook_id: regression.metadata.playbook_id.clone(),
                semantic_status,
                semantic_reason: output.note.semantic_reason.clone(),
                status_allowed,
                duration_ms: started.elapsed().as_millis(),
                execution_error: None,
                matched_paths,
                trace_path: None,
                required_witness_groups,
                target_witness_groups,
            };
            if let Some(trace_root) = trace_root {
                if let Ok(trace_path) =
                    write_trace_packet(trace_root, regression, &output, &outcome)
                {
                    outcome.trace_path = Some(trace_path);
                }
            }
            outcome
        }
        Err(err) => HybridPlaybookProbeOutcome {
            file_name: regression
                .path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default()
                .to_owned(),
            playbook_id: regression.metadata.playbook_id.clone(),
            semantic_status: "error".to_owned(),
            semantic_reason: None,
            status_allowed: false,
            duration_ms: started.elapsed().as_millis(),
            execution_error: Some(err.to_string()),
            matched_paths: Vec::new(),
            trace_path: None,
            required_witness_groups: regression
                .spec
                .witness_groups
                .iter()
                .map(|group| HybridPlaybookWitnessOutcome {
                    group_id: group.group_id.clone(),
                    match_any: group.match_any.clone(),
                    match_mode: group.match_mode,
                    accepted_prefixes: group.accepted_prefixes.clone(),
                    required_when: group.required_when,
                    matched_by: HybridWitnessMatchBy::None,
                    matched_path: None,
                    passed: false,
                })
                .collect(),
            target_witness_groups: regression
                .spec
                .target_witness_groups
                .iter()
                .map(|group| HybridPlaybookWitnessOutcome {
                    group_id: group.group_id.clone(),
                    match_any: group.match_any.clone(),
                    match_mode: group.match_mode,
                    accepted_prefixes: group.accepted_prefixes.clone(),
                    required_when: group.required_when,
                    matched_by: HybridWitnessMatchBy::None,
                    matched_path: None,
                    passed: false,
                })
                .collect(),
        },
    }
}

pub fn run_hybrid_playbook_regressions(
    searcher: &TextSearcher,
    playbooks_root: &Path,
    enforce_targets: bool,
    trace_root: Option<&Path>,
) -> FriggResult<HybridPlaybookRunSummary> {
    let regressions = load_hybrid_playbook_regressions(playbooks_root)?;
    let outcomes = regressions
        .iter()
        .map(|regression| run_hybrid_playbook_regression(searcher, regression, trace_root))
        .collect::<Vec<_>>();
    let required_failures = outcomes
        .iter()
        .filter(|outcome| !outcome.passed_required())
        .count();
    let target_failures = if enforce_targets {
        outcomes
            .iter()
            .filter(|outcome| !outcome.passed_targets())
            .count()
    } else {
        0
    };
    Ok(HybridPlaybookRunSummary {
        playbooks_root: playbooks_root.display().to_string(),
        enforce_targets,
        playbook_count: outcomes.len(),
        required_failures,
        target_failures,
        outcomes,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        HybridPlaybookProbeOutcome, HybridPlaybookWitnessOutcome, HybridWitnessGroup,
        HybridWitnessMatchBy, HybridWitnessMatchMode, HybridWitnessRequirement, PlaybookDocument,
        load_hybrid_playbook_regressions, parse_playbook_document, scrub_playbook_metadata_header,
        semantic_status_allowed, witness_outcomes,
    };
    use crate::domain::FriggResult;
    use std::env;
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn mk_group(
        group_id: &str,
        match_any: Vec<&str>,
        required_when: HybridWitnessRequirement,
    ) -> HybridWitnessGroup {
        HybridWitnessGroup {
            group_id: group_id.to_owned(),
            match_any: match_any.into_iter().map(str::to_owned).collect(),
            match_mode: HybridWitnessMatchMode::ExactAny,
            accepted_prefixes: Vec::new(),
            required_when,
        }
    }

    fn mk_outcome(
        group_id: &str,
        match_any: Vec<&str>,
        required_when: HybridWitnessRequirement,
        passed: bool,
    ) -> HybridPlaybookWitnessOutcome {
        HybridPlaybookWitnessOutcome {
            group_id: group_id.to_owned(),
            match_any: match_any.into_iter().map(str::to_owned).collect(),
            match_mode: HybridWitnessMatchMode::ExactAny,
            accepted_prefixes: Vec::new(),
            required_when,
            matched_by: HybridWitnessMatchBy::None,
            matched_path: None,
            passed,
        }
    }

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
    fn parse_playbook_document_normalizes_nested_hybrid_defaults_and_witness_groups()
    -> FriggResult<()> {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "nested-hybrid-defaults",
  "hybrid_regression": {
    "query": "trace hybrid witness defaults",
    "allowed_semantic_statuses": ["ok"],
    "witness_groups": [
      {
        "group_id": "runtime",
        "match_any": ["src/runtime.rs"]
      }
    ],
    "target_witness_groups": [
      {
        "name": "docs",
        "paths": ["docs/runtime.md"]
      }
    ],
    "target_paths": ["contracts/runtime.md"]
  }
}
-->
"#;

        let parsed = parse_playbook_document(raw)?;
        let spec = parsed
            .metadata
            .hybrid_regression
            .expect("hybrid regression metadata must be present");
        assert_eq!(spec.query, "trace hybrid witness defaults");
        assert_eq!(spec.top_k, 8);
        assert_eq!(spec.allowed_semantic_statuses, vec!["ok"]);
        assert_eq!(spec.witness_groups.len(), 1);
        assert_eq!(spec.witness_groups[0].group_id, "runtime");
        assert_eq!(spec.witness_groups[0].match_any, vec!["src/runtime.rs"]);
        assert_eq!(
            spec.witness_groups[0].required_when,
            HybridWitnessRequirement::Always
        );
        assert_eq!(spec.target_witness_groups.len(), 2);
        assert_eq!(spec.target_witness_groups[0].group_id, "docs");
        assert_eq!(
            spec.target_witness_groups[0].match_any,
            vec!["docs/runtime.md"]
        );
        assert_eq!(
            spec.target_witness_groups[0].required_when,
            HybridWitnessRequirement::Always
        );
        assert_eq!(
            spec.target_witness_groups[1].group_id,
            "contracts/runtime.md"
        );
        assert_eq!(
            spec.target_witness_groups[1].match_any,
            vec!["contracts/runtime.md"]
        );
        assert_eq!(
            spec.target_witness_groups[1].required_when,
            HybridWitnessRequirement::SemanticOk
        );
        Ok(())
    }

    #[test]
    fn witness_outcomes_evaluates_required_and_optional_groups() {
        let groups = vec![
            mk_group(
                "always",
                vec!["src/lib.rs"],
                HybridWitnessRequirement::Always,
            ),
            mk_group(
                "ok-only",
                vec!["docs/ok.md"],
                HybridWitnessRequirement::SemanticOk,
            ),
            mk_group(
                "empty",
                vec!["missing"],
                HybridWitnessRequirement::SemanticOk,
            ),
        ];

        let all_required = witness_outcomes(&groups, &["src/lib.rs".to_owned()], true, false);
        assert_eq!(all_required.len(), 3);
        assert_eq!(all_required[0].passed, true);
        assert_eq!(all_required[0].matched_by, HybridWitnessMatchBy::Exact);
        assert_eq!(all_required[1].passed, false);
        assert_eq!(all_required[1].matched_by, HybridWitnessMatchBy::None);
        assert_eq!(all_required[2].passed, false);
        assert_eq!(all_required[2].matched_by, HybridWitnessMatchBy::None);

        let all_required = witness_outcomes(&groups, &["src/ignored".to_owned()], false, true);
        assert_eq!(all_required.len(), 1);
        assert_eq!(all_required[0].passed, false);
        assert_eq!(all_required[0].matched_by, HybridWitnessMatchBy::None);
        let all_required = witness_outcomes(&groups, &["docs/ok.md".to_owned()], true, false);
        assert_eq!(all_required.len(), 3);
        assert_eq!(all_required[0].passed, false);
        assert_eq!(all_required[1].passed, true);
        assert_eq!(all_required[1].matched_by, HybridWitnessMatchBy::Exact);
        assert_eq!(all_required[2].passed, false);
    }

    #[test]
    fn witness_outcomes_records_prefix_hits_without_flipping_exact_gate() {
        let groups = vec![HybridWitnessGroup {
            group_id: "tests".to_owned(),
            match_any: vec!["apps/server/tests/unit/foo_test.py".to_owned()],
            match_mode: HybridWitnessMatchMode::ExactOrPrefix,
            accepted_prefixes: vec!["apps/server/tests".to_owned()],
            required_when: HybridWitnessRequirement::Always,
        }];

        let outcomes = witness_outcomes(
            &groups,
            &["apps/server/tests/integration/bar_test.py".to_owned()],
            true,
            false,
        );
        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].matched_by, HybridWitnessMatchBy::Prefix);
        assert_eq!(
            outcomes[0].matched_path,
            Some("apps/server/tests/integration/bar_test.py".to_owned())
        );
        assert!(!outcomes[0].passed);
    }

    #[test]
    fn semantic_status_allowed_respects_empty_allowlist_as_open() {
        assert!(semantic_status_allowed(&[], "OK"));
        assert!(semantic_status_allowed(&[], "random"));
    }

    #[test]
    fn hybrid_probe_outcome_helpers_cover_required_and_target_modes() {
        let outcome = HybridPlaybookProbeOutcome {
            file_name: "pb.md".to_owned(),
            playbook_id: "pb".to_owned(),
            semantic_status: "ok".to_owned(),
            semantic_reason: None,
            status_allowed: true,
            duration_ms: 1,
            execution_error: None,
            matched_paths: vec!["src/lib.rs".to_owned()],
            trace_path: None,
            required_witness_groups: vec![mk_outcome(
                "runtime",
                vec!["src/lib.rs"],
                HybridWitnessRequirement::Always,
                true,
            )],
            target_witness_groups: vec![mk_outcome(
                "docs",
                vec!["README.md"],
                HybridWitnessRequirement::SemanticOk,
                false,
            )],
        };

        assert!(outcome.passed_required());
        assert!(!outcome.passed_targets());
        assert!(!outcome.passed_all(true));
        assert!(outcome.passed_all(false));
        assert_eq!(outcome.required_missing(), Vec::<String>::new());
        assert_eq!(
            outcome.target_missing(),
            vec!["docs -> [\"README.md\"]".to_owned()]
        );

        let errored = HybridPlaybookProbeOutcome {
            execution_error: Some("boom".to_owned()),
            status_allowed: false,
            ..outcome
        };
        assert!(!errored.passed_required());
    }

    #[test]
    fn hybrid_probe_outcome_target_only_blocks_disabled_semantic() {
        let groups = vec![mk_group(
            "docs",
            vec!["docs/readme.md"],
            HybridWitnessRequirement::Always,
        )];
        let targets = witness_outcomes(&groups, &["docs/readme.md".to_owned()], false, true);
        assert_eq!(targets.len(), 1);
        assert!(targets[0].passed);
    }

    #[test]
    fn parse_playbook_document_requires_query_for_legacy_hybrid_metadata() {
        let raw = r#"<!-- frigg-playbook
{
  "schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "missing-query"
}
-->
"#;

        let error =
            parse_playbook_document(raw).expect_err("hybrid playbooks without a query should fail");
        assert!(
            error
                .to_string()
                .contains("hybrid playbook metadata must include a query"),
            "unexpected missing query error: {error}"
        );
    }

    #[test]
    fn parse_playbook_document_rejects_witness_groups_without_identity() {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "missing-group-id",
  "hybrid_regression": {
    "query": "trace witness identity validation",
    "witness_groups": [
      {
        "paths": ["src/lib.rs"]
      }
    ]
  }
}
-->
"#;

        let error = parse_playbook_document(raw)
            .expect_err("witness groups without group_id or name should fail");
        assert!(
            error
                .to_string()
                .contains("hybrid witness group must include group_id or name"),
            "unexpected missing witness group identity error: {error}"
        );
    }

    #[test]
    fn parse_playbook_document_rejects_witness_groups_without_paths() {
        let raw = r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.hybrid.v1",
  "playbook_id": "missing-group-paths",
  "hybrid_regression": {
    "query": "trace witness path validation",
    "target_witness_groups": [
      {
        "name": "docs"
      }
    ]
  }
}
-->
"#;

        let error = parse_playbook_document(raw)
            .expect_err("witness groups without match_any or paths should fail");
        assert!(
            error
                .to_string()
                .contains("hybrid witness group 'docs' must include at least one path"),
            "unexpected missing witness group paths error: {error}"
        );
    }

    #[test]
    fn semantic_status_allowed_treats_unavailable_like_disabled_fallback() {
        let allowed = vec![
            "ok".to_owned(),
            "degraded".to_owned(),
            "disabled".to_owned(),
        ];

        assert!(super::semantic_status_allowed(&allowed, "disabled"));
        assert!(super::semantic_status_allowed(&allowed, "unavailable"));
        assert!(!super::semantic_status_allowed(
            &["ok".to_owned()],
            "unavailable"
        ));
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
    fn load_hybrid_playbook_regressions_requires_hybrid_regression_metadata() -> FriggResult<()> {
        let root = temp_playbook_root("missing-hybrid-regression");
        fs::create_dir_all(&root).map_err(crate::domain::FriggError::Io)?;
        fs::write(
            root.join("alpha.md"),
            r#"<!-- frigg-playbook
{
  "playbook_schema": "frigg.playbook.v1",
  "playbook_id": "docs-only"
}
-->
# Alpha
"#,
        )
        .map_err(crate::domain::FriggError::Io)?;

        let error = load_hybrid_playbook_regressions(&root)
            .expect_err("non-hybrid playbooks should fail executable regression loading");
        assert!(
            error
                .to_string()
                .contains("missing hybrid_regression metadata"),
            "unexpected missing hybrid regression error: {error}"
        );

        cleanup_root(&root);
        Ok(())
    }

    #[test]
    fn load_hybrid_playbook_regressions_rejects_empty_playbook_roots() -> FriggResult<()> {
        let root = temp_playbook_root("empty-root");
        fs::create_dir_all(&root).map_err(crate::domain::FriggError::Io)?;

        let error =
            load_hybrid_playbook_regressions(&root).expect_err("empty playbook roots should fail");
        assert!(
            error
                .to_string()
                .contains("no executable hybrid playbooks found under"),
            "unexpected empty playbook root error: {error}"
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
