//! Shared context-efficiency helpers for optional response metadata and local summaries.
//!
//! Computes affected-file and returned-source byte estimates for MCP responses and optional
//! JSONL operator logs controlled by `FRIGG_CONTEXT_EFFICIENCY_LOG`.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

use chrono::{DateTime, Duration, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::storage::RepositoryManifestMetadataSnapshot;

/// Environment variable that enables optional JSONL context-efficiency logging.
pub(crate) const CONTEXT_EFFICIENCY_LOG_ENV: &str = "FRIGG_CONTEXT_EFFICIENCY_LOG";
const MANIFEST_METADATA_CACHE_LIMIT: usize = 64;
const JSONL_SUMMARY_CACHE_LIMIT: usize = 32;

/// Cache key for manifest metadata summaries derived from storage identity and snapshot id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ManifestMetadataCacheKey {
    storage_identity: String,
    repository_id: String,
    snapshot_id: String,
}

impl ManifestMetadataCacheKey {
    pub(crate) fn new(
        storage_identity: impl Into<String>,
        repository_id: impl Into<String>,
        snapshot_id: impl Into<String>,
    ) -> Self {
        Self {
            storage_identity: storage_identity.into(),
            repository_id: repository_id.into(),
            snapshot_id: snapshot_id.into(),
        }
    }

    pub(crate) fn from_storage_path(
        storage_path: &Path,
        repository_id: &str,
        snapshot_id: &str,
    ) -> Self {
        Self::new(
            storage_path.to_string_lossy().into_owned(),
            repository_id.to_owned(),
            snapshot_id.to_owned(),
        )
    }
}

/// Indexed corpus byte totals and per-path sizes for context-efficiency estimates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestMetadataSummary {
    pub(crate) repository_id: String,
    pub(crate) snapshot_id: String,
    pub(crate) indexed_readable_files: usize,
    pub(crate) indexed_readable_bytes: u64,
    pub(crate) min_mtime_ns: Option<u64>,
    pub(crate) max_mtime_ns: Option<u64>,
    pub(crate) path_size_bytes: BTreeMap<String, u64>,
}

impl ManifestMetadataSummary {
    pub(crate) fn from_snapshot(snapshot: &RepositoryManifestMetadataSnapshot) -> Self {
        let mut indexed_readable_bytes = 0_u64;
        let mut min_mtime_ns = None;
        let mut max_mtime_ns = None;
        let mut path_size_bytes = BTreeMap::new();

        for entry in &snapshot.entries {
            indexed_readable_bytes = indexed_readable_bytes.saturating_add(entry.size_bytes);
            if let Some(mtime_ns) = entry.mtime_ns {
                min_mtime_ns =
                    Some(min_mtime_ns.map_or(mtime_ns, |current: u64| current.min(mtime_ns)));
                max_mtime_ns =
                    Some(max_mtime_ns.map_or(mtime_ns, |current: u64| current.max(mtime_ns)));
            }
            path_size_bytes.insert(entry.path.clone(), entry.size_bytes);
        }

        Self {
            repository_id: snapshot.repository_id.clone(),
            snapshot_id: snapshot.snapshot_id.clone(),
            indexed_readable_files: snapshot.entries.len(),
            indexed_readable_bytes,
            min_mtime_ns,
            max_mtime_ns,
            path_size_bytes,
        }
    }

    pub(crate) fn returned_unique_file_bytes<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> u64 {
        let unique_paths = paths.into_iter().collect::<BTreeSet<_>>();
        unique_paths
            .into_iter()
            .filter_map(|path| self.path_size_bytes.get(path))
            .copied()
            .fold(0_u64, u64::saturating_add)
    }

    /// Sums indexed byte sizes for returned unique paths when every path is in the manifest snapshot.
    pub(crate) fn returned_unique_file_bytes_if_known<'a>(
        &self,
        paths: impl IntoIterator<Item = &'a str>,
    ) -> Option<u64> {
        let unique_paths = paths.into_iter().collect::<BTreeSet<_>>();
        let mut total = 0_u64;
        for path in unique_paths {
            total = total.saturating_add(*self.path_size_bytes.get(path)?);
        }
        Some(total)
    }

    /// Returns the indexed byte size for either repository-relative or absolute path forms.
    pub(crate) fn file_size_bytes_for_path(
        &self,
        workspace_root: Option<&Path>,
        path: &str,
    ) -> Option<u64> {
        if let Some(size_bytes) = self.path_size_bytes.get(path).copied() {
            return Some(size_bytes);
        }

        let workspace_root = workspace_root?;
        let requested_path = Path::new(path);
        if requested_path.is_relative() {
            let absolute_path = workspace_root.join(requested_path);
            return self
                .path_size_bytes
                .get(&absolute_path.to_string_lossy().into_owned())
                .copied();
        }

        let relative_path = requested_path.strip_prefix(workspace_root).ok()?;
        self.path_size_bytes
            .get(&relative_path.to_string_lossy().into_owned())
            .copied()
    }
}

#[derive(Debug, Default)]
struct ManifestMetadataCache {
    insertion_order: VecDeque<ManifestMetadataCacheKey>,
    entries: BTreeMap<ManifestMetadataCacheKey, ManifestMetadataSummary>,
}

impl ManifestMetadataCache {
    fn get(&self, key: &ManifestMetadataCacheKey) -> Option<ManifestMetadataSummary> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: ManifestMetadataCacheKey, value: ManifestMetadataSummary) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key, value);
            return;
        }

        self.insertion_order.push_back(key.clone());
        self.entries.insert(key, value);
        while self.entries.len() > MANIFEST_METADATA_CACHE_LIMIT {
            let Some(oldest_key) = self.insertion_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest_key);
        }
    }
}

static MANIFEST_METADATA_CACHE: OnceLock<Mutex<ManifestMetadataCache>> = OnceLock::new();

fn manifest_metadata_cache() -> &'static Mutex<ManifestMetadataCache> {
    MANIFEST_METADATA_CACHE.get_or_init(|| Mutex::new(ManifestMetadataCache::default()))
}

pub(crate) fn cached_manifest_metadata_summary(
    key: &ManifestMetadataCacheKey,
) -> Option<ManifestMetadataSummary> {
    manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned")
        .get(key)
}

pub(crate) fn store_manifest_metadata_summary(
    key: ManifestMetadataCacheKey,
    value: ManifestMetadataSummary,
) {
    manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned")
        .insert(key, value);
}

pub(crate) fn is_truthy_env_value(value: &str) -> bool {
    let normalized = value.trim().to_ascii_lowercase();
    !matches!(normalized.as_str(), "" | "0" | "false" | "no" | "off")
}

pub(crate) fn context_efficiency_log_enabled() -> bool {
    std::env::var(CONTEXT_EFFICIENCY_LOG_ENV)
        .map(|value| is_truthy_env_value(&value))
        .unwrap_or(false)
}

pub(crate) fn need_context_efficiency(include_context_efficiency: Option<bool>) -> bool {
    need_context_efficiency_with_log_state(
        include_context_efficiency,
        context_efficiency_log_enabled(),
    )
}

pub(crate) fn need_context_efficiency_with_log_state(
    include_context_efficiency: Option<bool>,
    context_efficiency_log_enabled: bool,
) -> bool {
    include_context_efficiency == Some(true) || context_efficiency_log_enabled
}

/// Compact context-efficiency metrics written to JSONL rows.
///
/// Saved byte and percent estimates are scoped to the affected files represented by the returned
/// result, not to the full indexed corpus. Full-corpus byte totals remain available as inventory
/// counters through `indexed_readable_*`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContextEfficiencyLogMetrics {
    pub(crate) indexed_readable_files: usize,
    pub(crate) indexed_readable_bytes: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) indexed_min_mtime_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) indexed_max_mtime_ns: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) candidate_input_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) candidate_output_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) returned_match_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) returned_unique_paths: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) returned_unique_file_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) returned_source_bytes_estimate: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) matched_file_context_saved_bytes_estimate: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) matched_file_context_saved_percent_estimate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) query_duration_ms: Option<u64>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_optional_u64_from_number"
    )]
    pub(crate) narrowing_ratio_estimate: Option<u64>,
}

fn deserialize_optional_u64_from_number<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let Some(value) = Option::<serde_json::Value>::deserialize(deserializer)? else {
        return Ok(None);
    };
    match value {
        serde_json::Value::Number(number) => {
            if let Some(value) = number.as_u64() {
                return Ok(Some(value));
            }
            let value = number.as_f64().ok_or_else(|| {
                serde::de::Error::custom("expected finite non-negative numeric ratio")
            })?;
            if !value.is_finite() || value < 0.0 {
                return Err(serde::de::Error::custom(
                    "expected finite non-negative numeric ratio",
                ));
            }
            Ok(Some(value.round() as u64))
        }
        other => Err(serde::de::Error::custom(format!(
            "expected numeric ratio, got {other}"
        ))),
    }
}

/// Single JSONL row written to `.frigg/context.jsonl` when logging is enabled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct ContextEfficiencyLogRow {
    pub(crate) timestamp: String,
    pub(crate) tool: String,
    pub(crate) repository_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) snapshot_id: Option<String>,
    #[serde(flatten)]
    pub(crate) metrics: ContextEfficiencyLogMetrics,
}

/// Inclusive time window used when aggregating context-efficiency JSONL logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSummaryWindow {
    pub date_since: DateTime<Utc>,
    pub date_until: DateTime<Utc>,
}

impl ContextSummaryWindow {
    pub fn resolve(
        since: Option<&str>,
        until: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<Self, ContextSummaryError> {
        let parsed_since = since
            .map(parse_context_summary_datetime)
            .transpose()
            .map_err(ContextSummaryError::InvalidDate)?;
        let parsed_until = until
            .map(parse_context_summary_datetime)
            .transpose()
            .map_err(ContextSummaryError::InvalidDate)?;

        let (date_since, date_until) = match (parsed_since, parsed_until) {
            (Some(date_since), Some(date_until)) => (date_since, date_until),
            (Some(date_since), None) => (date_since, now),
            (None, Some(date_until)) => (date_until - Duration::days(30), date_until),
            (None, None) => (now - Duration::days(30), now),
        };

        if date_since > date_until {
            return Err(ContextSummaryError::InvalidDate(format!(
                "date_since must be before or equal to date_until: {} > {}",
                date_since.to_rfc3339(),
                date_until.to_rfc3339()
            )));
        }

        Ok(Self {
            date_since,
            date_until,
        })
    }

    fn since_key(&self) -> String {
        self.date_since.to_rfc3339()
    }

    fn until_key(&self) -> String {
        self.date_until.to_rfc3339()
    }
}

/// Failure modes when resolving summary windows or reading context logs.
#[derive(Debug)]
pub enum ContextSummaryError {
    InvalidDate(String),
    Io(io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ContextSummaryError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidDate(message) => formatter.write_str(message),
            Self::Io(error) => error.fmt(formatter),
            Self::Json(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for ContextSummaryError {}

impl From<io::Error> for ContextSummaryError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ContextSummaryError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

/// Rolled-up context-efficiency counters across one or more workspace roots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ContextLogAggregate {
    pub events: usize,
    pub indexed_readable_files_max: usize,
    pub indexed_readable_bytes_max: u64,
    pub query_duration_ms_total: u64,
    pub query_duration_ms_max: u64,
    pub candidate_input_count: u64,
    pub candidate_output_count: u64,
    pub returned_match_count: u64,
    pub returned_unique_paths: u64,
    pub returned_unique_file_bytes: u64,
    pub returned_source_bytes_estimate: u64,
    pub matched_file_context_saved_bytes_estimate: i64,
}

impl ContextLogAggregate {
    fn add_row(&mut self, row: &ContextEfficiencyLogRow) {
        self.events = self.events.saturating_add(1);
        self.indexed_readable_files_max = self
            .indexed_readable_files_max
            .max(row.metrics.indexed_readable_files);
        self.indexed_readable_bytes_max = self
            .indexed_readable_bytes_max
            .max(row.metrics.indexed_readable_bytes);
        self.query_duration_ms_total = self
            .query_duration_ms_total
            .saturating_add(row.metrics.query_duration_ms.unwrap_or_default());
        self.query_duration_ms_max = self
            .query_duration_ms_max
            .max(row.metrics.query_duration_ms.unwrap_or_default());
        self.candidate_input_count = self
            .candidate_input_count
            .saturating_add(row.metrics.candidate_input_count.unwrap_or_default() as u64);
        self.candidate_output_count = self
            .candidate_output_count
            .saturating_add(row.metrics.candidate_output_count.unwrap_or_default() as u64);
        self.returned_match_count = self
            .returned_match_count
            .saturating_add(row.metrics.returned_match_count.unwrap_or_default() as u64);
        self.returned_unique_paths = self
            .returned_unique_paths
            .saturating_add(row.metrics.returned_unique_paths.unwrap_or_default() as u64);
        self.returned_unique_file_bytes = self
            .returned_unique_file_bytes
            .saturating_add(row.metrics.returned_unique_file_bytes.unwrap_or_default());
        self.returned_source_bytes_estimate = self.returned_source_bytes_estimate.saturating_add(
            row.metrics
                .returned_source_bytes_estimate
                .unwrap_or_default(),
        );
        self.matched_file_context_saved_bytes_estimate = self
            .matched_file_context_saved_bytes_estimate
            .saturating_add(
                row.metrics
                    .matched_file_context_saved_bytes_estimate
                    .unwrap_or_default(),
            );
    }

    fn merge(&mut self, other: &Self) {
        self.events = self.events.saturating_add(other.events);
        self.indexed_readable_files_max = self
            .indexed_readable_files_max
            .max(other.indexed_readable_files_max);
        self.indexed_readable_bytes_max = self
            .indexed_readable_bytes_max
            .max(other.indexed_readable_bytes_max);
        self.query_duration_ms_total = self
            .query_duration_ms_total
            .saturating_add(other.query_duration_ms_total);
        self.query_duration_ms_max = self.query_duration_ms_max.max(other.query_duration_ms_max);
        self.candidate_input_count = self
            .candidate_input_count
            .saturating_add(other.candidate_input_count);
        self.candidate_output_count = self
            .candidate_output_count
            .saturating_add(other.candidate_output_count);
        self.returned_match_count = self
            .returned_match_count
            .saturating_add(other.returned_match_count);
        self.returned_unique_paths = self
            .returned_unique_paths
            .saturating_add(other.returned_unique_paths);
        self.returned_unique_file_bytes = self
            .returned_unique_file_bytes
            .saturating_add(other.returned_unique_file_bytes);
        self.returned_source_bytes_estimate = self
            .returned_source_bytes_estimate
            .saturating_add(other.returned_source_bytes_estimate);
        self.matched_file_context_saved_bytes_estimate = self
            .matched_file_context_saved_bytes_estimate
            .saturating_add(other.matched_file_context_saved_bytes_estimate);
    }
}

/// Per-workspace-root summary of context-efficiency JSONL activity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextLogRootSummary {
    pub root: String,
    pub context_log: String,
    pub exists: bool,
    pub repositories: Vec<String>,
    pub tools: BTreeMap<String, usize>,
    #[serde(flatten)]
    pub aggregate: ContextLogAggregate,
}

/// Multi-root context-efficiency summary for operator-facing reporting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextLogSummary {
    pub date_since: String,
    pub date_until: String,
    pub roots: Vec<ContextLogRootSummary>,
    pub totals: ContextLogAggregate,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextLogFileSummary {
    exists: bool,
    repositories: BTreeSet<String>,
    tools: BTreeMap<String, usize>,
    aggregate: ContextLogAggregate,
}

impl Default for ContextLogFileSummary {
    fn default() -> Self {
        Self {
            exists: false,
            repositories: BTreeSet::new(),
            tools: BTreeMap::new(),
            aggregate: ContextLogAggregate::default(),
        }
    }
}

impl ContextLogFileSummary {
    fn into_root_summary(self, root: &Path, context_log: &Path) -> ContextLogRootSummary {
        ContextLogRootSummary {
            root: root.display().to_string(),
            context_log: context_log.display().to_string(),
            exists: self.exists,
            repositories: self.repositories.into_iter().collect(),
            tools: self.tools,
            aggregate: self.aggregate,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct JsonlSummaryCacheKey {
    context_log: PathBuf,
    size_bytes: u64,
    modified_ns: u128,
    date_since: String,
    date_until: String,
}

#[derive(Debug, Default)]
struct JsonlSummaryCache {
    insertion_order: VecDeque<JsonlSummaryCacheKey>,
    entries: BTreeMap<JsonlSummaryCacheKey, ContextLogFileSummary>,
}

impl JsonlSummaryCache {
    fn get(&self, key: &JsonlSummaryCacheKey) -> Option<ContextLogFileSummary> {
        self.entries.get(key).cloned()
    }

    fn insert(&mut self, key: JsonlSummaryCacheKey, value: ContextLogFileSummary) {
        if self.entries.contains_key(&key) {
            self.entries.insert(key, value);
            return;
        }

        self.insertion_order.push_back(key.clone());
        self.entries.insert(key, value);
        while self.entries.len() > JSONL_SUMMARY_CACHE_LIMIT {
            let Some(oldest_key) = self.insertion_order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest_key);
        }
    }
}

static JSONL_SUMMARY_CACHE: OnceLock<Mutex<JsonlSummaryCache>> = OnceLock::new();

fn jsonl_summary_cache() -> &'static Mutex<JsonlSummaryCache> {
    JSONL_SUMMARY_CACHE.get_or_init(|| Mutex::new(JsonlSummaryCache::default()))
}

impl ContextEfficiencyLogRow {
    pub(crate) fn new(
        tool: impl Into<String>,
        repository_id: impl Into<String>,
        snapshot_id: Option<String>,
        metrics: ContextEfficiencyLogMetrics,
    ) -> Self {
        Self {
            timestamp: Utc::now().to_rfc3339(),
            tool: tool.into(),
            repository_id: repository_id.into(),
            snapshot_id,
            metrics,
        }
    }
}

pub(crate) fn append_context_efficiency_log_row_if_enabled(
    workspace_root: &Path,
    log_enabled: bool,
    row: &ContextEfficiencyLogRow,
) -> io::Result<()> {
    if !log_enabled {
        return Ok(());
    }
    append_context_efficiency_log_row(workspace_root, row)
}

pub(crate) fn append_context_efficiency_log_row(
    workspace_root: &Path,
    row: &ContextEfficiencyLogRow,
) -> io::Result<()> {
    let frigg_dir = workspace_root.join(".frigg");
    std::fs::create_dir_all(&frigg_dir)?;
    let log_path = frigg_dir.join("context.jsonl");
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)?;
    serde_json::to_writer(&mut file, row)?;
    file.write_all(b"\n")?;
    Ok(())
}

pub fn summarize_context_logs_for_roots(
    roots: &[PathBuf],
    window: &ContextSummaryWindow,
) -> Result<ContextLogSummary, ContextSummaryError> {
    let mut root_summaries = Vec::new();
    let mut totals = ContextLogAggregate::default();

    for root in roots {
        let context_log = root.join(".frigg/context.jsonl");
        let file_summary = summarize_context_log_file(&context_log, window)?;
        totals.merge(&file_summary.aggregate);
        root_summaries.push(file_summary.into_root_summary(root, &context_log));
    }

    Ok(ContextLogSummary {
        date_since: window.since_key(),
        date_until: window.until_key(),
        roots: root_summaries,
        totals,
    })
}

fn summarize_context_log_file(
    context_log: &Path,
    window: &ContextSummaryWindow,
) -> Result<ContextLogFileSummary, ContextSummaryError> {
    let metadata = match std::fs::metadata(context_log) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(ContextLogFileSummary::default());
        }
        Err(error) => return Err(ContextSummaryError::Io(error)),
    };

    let key = JsonlSummaryCacheKey {
        context_log: canonical_context_log_path(context_log),
        size_bytes: metadata.len(),
        modified_ns: metadata_modified_ns(&metadata),
        date_since: window.since_key(),
        date_until: window.until_key(),
    };
    if let Some(summary) = jsonl_summary_cache()
        .lock()
        .expect("context-efficiency jsonl summary cache mutex poisoned")
        .get(&key)
    {
        return Ok(summary);
    }

    let summary = read_context_log_file_summary(context_log, window)?;
    jsonl_summary_cache()
        .lock()
        .expect("context-efficiency jsonl summary cache mutex poisoned")
        .insert(key, summary.clone());
    Ok(summary)
}

fn read_context_log_file_summary(
    context_log: &Path,
    window: &ContextSummaryWindow,
) -> Result<ContextLogFileSummary, ContextSummaryError> {
    let file = File::open(context_log)?;
    let mut summary = ContextLogFileSummary {
        exists: true,
        ..ContextLogFileSummary::default()
    };

    for line in BufReader::new(file).lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let row: ContextEfficiencyLogRow = serde_json::from_str(&line)?;
        let Ok(timestamp) = DateTime::parse_from_rfc3339(&row.timestamp) else {
            continue;
        };
        let timestamp = timestamp.with_timezone(&Utc);
        if timestamp < window.date_since || timestamp > window.date_until {
            continue;
        }

        summary.repositories.insert(row.repository_id.clone());
        *summary.tools.entry(row.tool.clone()).or_default() += 1;
        summary.aggregate.add_row(&row);
    }

    Ok(summary)
}

fn parse_context_summary_datetime(input: &str) -> Result<DateTime<Utc>, String> {
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(input) {
        return Ok(timestamp.with_timezone(&Utc));
    }

    let date = NaiveDate::parse_from_str(input, "%Y-%m-%d")
        .map_err(|_| format!("expected RFC3339 timestamp or YYYY-MM-DD date, got {input:?}"))?;
    let datetime = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| format!("invalid date: {input:?}"))?;
    Ok(DateTime::from_naive_utc_and_offset(datetime, Utc))
}

fn canonical_context_log_path(context_log: &Path) -> PathBuf {
    context_log
        .canonicalize()
        .unwrap_or_else(|_| context_log.to_path_buf())
}

fn metadata_modified_ns(metadata: &std::fs::Metadata) -> u128 {
    metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_nanos())
}

#[cfg(test)]
pub(crate) fn clear_manifest_metadata_cache_for_tests() {
    let mut cache = manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned");
    *cache = ManifestMetadataCache::default();
}

#[cfg(test)]
pub(crate) fn manifest_metadata_cache_entry_count_for_tests() -> usize {
    manifest_metadata_cache()
        .lock()
        .expect("context-efficiency manifest cache mutex poisoned")
        .entries
        .len()
}

#[cfg(test)]
pub(crate) fn clear_jsonl_summary_cache_for_tests() {
    let mut cache = jsonl_summary_cache()
        .lock()
        .expect("context-efficiency jsonl summary cache mutex poisoned");
    *cache = JsonlSummaryCache::default();
}

#[cfg(test)]
pub(crate) fn jsonl_summary_cache_entry_count_for_tests() -> usize {
    jsonl_summary_cache()
        .lock()
        .expect("context-efficiency jsonl summary cache mutex poisoned")
        .entries
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{ManifestMetadataEntry, RepositoryManifestMetadataSnapshot};
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn context_efficiency_test_lock() -> MutexGuard<'static, ()> {
        static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        TEST_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("context-efficiency test mutex poisoned")
    }

    #[test]
    fn context_efficiency_guard_requires_response_flag_or_env_log() {
        assert!(!need_context_efficiency_with_log_state(None, false));
        assert!(!need_context_efficiency_with_log_state(Some(false), false));
        assert!(need_context_efficiency_with_log_state(Some(true), false));
        assert!(need_context_efficiency_with_log_state(None, true));
        assert!(need_context_efficiency_with_log_state(Some(false), true));
    }

    #[test]
    fn context_efficiency_env_truthiness_is_explicitly_false_for_common_false_values() {
        for value in ["", "0", "false", "False", " no ", "OFF"] {
            assert!(!is_truthy_env_value(value), "{value:?} should be false");
        }
        for value in ["1", "true", "yes", "on", "anything-else"] {
            assert!(is_truthy_env_value(value), "{value:?} should be true");
        }
    }

    #[test]
    fn context_efficiency_manifest_summary_derives_totals_and_path_sizes() {
        let snapshot = RepositoryManifestMetadataSnapshot {
            repository_id: "repo-1".to_owned(),
            snapshot_id: "snapshot-1".to_owned(),
            entries: vec![
                ManifestMetadataEntry {
                    path: "src/a.rs".to_owned(),
                    size_bytes: 10,
                    mtime_ns: Some(300),
                },
                ManifestMetadataEntry {
                    path: "src/b.rs".to_owned(),
                    size_bytes: 25,
                    mtime_ns: Some(100),
                },
                ManifestMetadataEntry {
                    path: "README.md".to_owned(),
                    size_bytes: 5,
                    mtime_ns: None,
                },
            ],
        };

        let summary = ManifestMetadataSummary::from_snapshot(&snapshot);
        assert_eq!(summary.indexed_readable_files, 3);
        assert_eq!(summary.indexed_readable_bytes, 40);
        assert_eq!(summary.min_mtime_ns, Some(100));
        assert_eq!(summary.max_mtime_ns, Some(300));
        assert_eq!(
            summary.returned_unique_file_bytes(["src/a.rs", "src/a.rs", "README.md"]),
            15
        );
    }

    #[test]
    fn context_efficiency_manifest_cache_is_bounded_fifo() {
        let mut cache = ManifestMetadataCache::default();
        for index in 0..(MANIFEST_METADATA_CACHE_LIMIT + 3) {
            let key = ManifestMetadataCacheKey::new("storage", "repo", format!("snapshot-{index}"));
            let summary = ManifestMetadataSummary {
                repository_id: "repo".to_owned(),
                snapshot_id: format!("snapshot-{index}"),
                indexed_readable_files: index,
                indexed_readable_bytes: index as u64,
                min_mtime_ns: None,
                max_mtime_ns: None,
                path_size_bytes: BTreeMap::new(),
            };
            cache.insert(key, summary);
        }

        assert_eq!(cache.entries.len(), MANIFEST_METADATA_CACHE_LIMIT);
        assert!(
            cache
                .get(&ManifestMetadataCacheKey::new(
                    "storage",
                    "repo",
                    "snapshot-0"
                ))
                .is_none()
        );
        assert!(
            cache
                .get(&ManifestMetadataCacheKey::new(
                    "storage",
                    "repo",
                    format!("snapshot-{}", MANIFEST_METADATA_CACHE_LIMIT + 2)
                ))
                .is_some()
        );
    }

    #[test]
    fn context_efficiency_jsonl_append_is_env_gated_by_caller() {
        let root = std::env::temp_dir().join(format!(
            "frigg-context-efficiency-log-disabled-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let row = ContextEfficiencyLogRow::new(
            "search_text",
            "repo-1",
            Some("snapshot-1".to_owned()),
            ContextEfficiencyLogMetrics {
                indexed_readable_files: 3,
                indexed_readable_bytes: 90,
                indexed_min_mtime_ns: None,
                indexed_max_mtime_ns: None,
                candidate_input_count: None,
                candidate_output_count: None,
                returned_match_count: Some(2),
                returned_unique_paths: Some(1),
                returned_unique_file_bytes: Some(20),
                returned_source_bytes_estimate: Some(5),
                matched_file_context_saved_bytes_estimate: Some(15),
                matched_file_context_saved_percent_estimate: Some(75.0),
                query_duration_ms: Some(4),
                narrowing_ratio_estimate: Some(4),
            },
        );

        append_context_efficiency_log_row_if_enabled(&root, false, &row)
            .expect("disabled logging should be a no-op");

        assert!(!root.join(".frigg/context.jsonl").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn context_efficiency_jsonl_append_is_compact_and_bounded() {
        let root = std::env::temp_dir().join(format!(
            "frigg-context-efficiency-log-enabled-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let row = ContextEfficiencyLogRow::new(
            "read_file",
            "repo-1",
            Some("snapshot-1".to_owned()),
            ContextEfficiencyLogMetrics {
                indexed_readable_files: 2,
                indexed_readable_bytes: 140,
                indexed_min_mtime_ns: Some(10),
                indexed_max_mtime_ns: Some(20),
                candidate_input_count: None,
                candidate_output_count: None,
                returned_match_count: None,
                returned_unique_paths: Some(1),
                returned_unique_file_bytes: Some(100),
                returned_source_bytes_estimate: Some(9),
                matched_file_context_saved_bytes_estimate: Some(91),
                matched_file_context_saved_percent_estimate: Some(91.0),
                query_duration_ms: Some(6),
                narrowing_ratio_estimate: Some(11),
            },
        );

        append_context_efficiency_log_row_if_enabled(&root, true, &row)
            .expect("enabled logging should append");

        let logged = std::fs::read_to_string(root.join(".frigg/context.jsonl"))
            .expect("context log should be readable");
        assert_eq!(logged.lines().count(), 1);
        let value: serde_json::Value =
            serde_json::from_str(logged.trim()).expect("context row should be json");
        assert_eq!(value["tool"], "read_file");
        assert_eq!(value["repository_id"], "repo-1");
        assert_eq!(value["snapshot_id"], "snapshot-1");
        assert_eq!(value["query_duration_ms"], 6);
        assert_eq!(value["matched_file_context_saved_bytes_estimate"], 91);
        assert_eq!(value["matched_file_context_saved_percent_estimate"], 91.0);
        assert_eq!(value["narrowing_ratio_estimate"], 11);
        assert!(value.get("corpus_context_saved_bytes_estimate").is_none());
        assert!(value.get("corpus_context_saved_percent_estimate").is_none());
        assert!(value.get("corpus_narrowing_ratio_estimate").is_none());
        assert!(value.get("query").is_none());
        assert!(value.get("content").is_none());
        assert!(value.get("excerpt").is_none());

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn context_efficiency_summary_window_defaults_to_last_30_days() {
        let now = DateTime::parse_from_rfc3339("2026-07-02T12:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);

        let window =
            ContextSummaryWindow::resolve(None, None, now).expect("default window should resolve");

        assert_eq!(window.date_since.to_rfc3339(), "2026-06-02T12:00:00+00:00");
        assert_eq!(window.date_until.to_rfc3339(), "2026-07-02T12:00:00+00:00");
    }

    #[test]
    fn context_efficiency_summary_window_accepts_date_and_rfc3339_bounds() {
        let now = DateTime::parse_from_rfc3339("2026-07-02T12:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);

        let until_only = ContextSummaryWindow::resolve(None, Some("2026-07-01T12:00:00Z"), now)
            .expect("until-only window should resolve");
        assert_eq!(
            until_only.date_since.to_rfc3339(),
            "2026-06-01T12:00:00+00:00"
        );
        assert_eq!(
            until_only.date_until.to_rfc3339(),
            "2026-07-01T12:00:00+00:00"
        );

        let since_only = ContextSummaryWindow::resolve(Some("2026-06-01"), None, now)
            .expect("since-only window should resolve");
        assert_eq!(
            since_only.date_since.to_rfc3339(),
            "2026-06-01T00:00:00+00:00"
        );
        assert_eq!(
            since_only.date_until.to_rfc3339(),
            "2026-07-02T12:00:00+00:00"
        );
    }

    #[test]
    fn context_efficiency_summary_missing_log_returns_empty_summary() {
        let _guard = context_efficiency_test_lock();
        clear_jsonl_summary_cache_for_tests();
        let root = std::env::temp_dir().join(format!(
            "frigg-context-efficiency-summary-missing-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("root should be created");
        let now = DateTime::parse_from_rfc3339("2026-07-02T12:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let window = ContextSummaryWindow::resolve(Some("2026-06-01"), Some("2026-07-01"), now)
            .expect("window should resolve");

        let summary = summarize_context_logs_for_roots(&[root.clone()], &window)
            .expect("missing context log should summarize");

        assert_eq!(summary.date_since, "2026-06-01T00:00:00+00:00");
        assert_eq!(summary.date_until, "2026-07-01T00:00:00+00:00");
        assert_eq!(summary.roots.len(), 1);
        assert!(!summary.roots[0].exists);
        assert_eq!(summary.totals.events, 0);
        assert_eq!(jsonl_summary_cache_entry_count_for_tests(), 0);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn context_efficiency_summary_aggregates_filtered_jsonl_without_raw_rows() {
        let _guard = context_efficiency_test_lock();
        clear_jsonl_summary_cache_for_tests();
        let root = std::env::temp_dir().join(format!(
            "frigg-context-efficiency-summary-filtered-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        let frigg_dir = root.join(".frigg");
        std::fs::create_dir_all(&frigg_dir).expect("frigg dir should be created");
        let log_path = frigg_dir.join("context.jsonl");

        let rows = [
            ContextEfficiencyLogRow {
                timestamp: "2026-05-31T23:59:59Z".to_owned(),
                tool: "search_text".to_owned(),
                repository_id: "repo-1".to_owned(),
                snapshot_id: Some("snapshot-1".to_owned()),
                metrics: ContextEfficiencyLogMetrics {
                    indexed_readable_files: 100,
                    indexed_readable_bytes: 1000,
                    indexed_min_mtime_ns: None,
                    indexed_max_mtime_ns: None,
                    candidate_input_count: Some(100),
                    candidate_output_count: Some(10),
                    returned_match_count: Some(10),
                    returned_unique_paths: Some(4),
                    returned_unique_file_bytes: Some(400),
                    returned_source_bytes_estimate: Some(40),
                    matched_file_context_saved_bytes_estimate: Some(360),
                    matched_file_context_saved_percent_estimate: Some(90.0),
                    query_duration_ms: Some(5),
                    narrowing_ratio_estimate: Some(10),
                },
            },
            ContextEfficiencyLogRow {
                timestamp: "2026-06-10T00:00:00Z".to_owned(),
                tool: "search_text".to_owned(),
                repository_id: "repo-1".to_owned(),
                snapshot_id: Some("snapshot-2".to_owned()),
                metrics: ContextEfficiencyLogMetrics {
                    indexed_readable_files: 120,
                    indexed_readable_bytes: 1200,
                    indexed_min_mtime_ns: None,
                    indexed_max_mtime_ns: None,
                    candidate_input_count: Some(50),
                    candidate_output_count: Some(5),
                    returned_match_count: Some(5),
                    returned_unique_paths: Some(2),
                    returned_unique_file_bytes: Some(200),
                    returned_source_bytes_estimate: Some(20),
                    matched_file_context_saved_bytes_estimate: Some(180),
                    matched_file_context_saved_percent_estimate: Some(90.0),
                    query_duration_ms: Some(7),
                    narrowing_ratio_estimate: Some(10),
                },
            },
            ContextEfficiencyLogRow {
                timestamp: "2026-06-11T00:00:00Z".to_owned(),
                tool: "read_file".to_owned(),
                repository_id: "repo-1".to_owned(),
                snapshot_id: Some("snapshot-2".to_owned()),
                metrics: ContextEfficiencyLogMetrics {
                    indexed_readable_files: 120,
                    indexed_readable_bytes: 1200,
                    indexed_min_mtime_ns: None,
                    indexed_max_mtime_ns: None,
                    candidate_input_count: None,
                    candidate_output_count: None,
                    returned_match_count: None,
                    returned_unique_paths: Some(1),
                    returned_unique_file_bytes: Some(80),
                    returned_source_bytes_estimate: Some(12),
                    matched_file_context_saved_bytes_estimate: Some(68),
                    matched_file_context_saved_percent_estimate: Some(85.0),
                    query_duration_ms: Some(3),
                    narrowing_ratio_estimate: Some(7),
                },
            },
        ];
        let mut encoded = String::new();
        for row in rows {
            encoded.push_str(&serde_json::to_string(&row).expect("row should serialize"));
            encoded.push('\n');
        }
        std::fs::write(&log_path, encoded).expect("log should be written");

        let now = DateTime::parse_from_rfc3339("2026-07-02T12:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let window = ContextSummaryWindow::resolve(Some("2026-06-01"), Some("2026-07-01"), now)
            .expect("window should resolve");
        let summary = summarize_context_logs_for_roots(&[root.clone()], &window)
            .expect("context log should summarize");

        assert_eq!(summary.totals.events, 2);
        assert_eq!(summary.totals.indexed_readable_files_max, 120);
        assert_eq!(summary.totals.indexed_readable_bytes_max, 1200);
        assert_eq!(summary.totals.query_duration_ms_total, 10);
        assert_eq!(summary.totals.query_duration_ms_max, 7);
        assert_eq!(summary.totals.candidate_input_count, 50);
        assert_eq!(summary.totals.returned_unique_file_bytes, 280);
        assert_eq!(
            summary.totals.matched_file_context_saved_bytes_estimate,
            248
        );
        assert_eq!(summary.roots[0].repositories, vec!["repo-1".to_owned()]);
        assert_eq!(summary.roots[0].tools["search_text"], 1);
        assert_eq!(summary.roots[0].tools["read_file"], 1);

        let value = serde_json::to_value(&summary).expect("summary should serialize");
        assert!(value.get("window").is_none());
        let serialized = serde_json::to_string(&value).expect("summary should serialize");
        assert!(!serialized.contains("2026-06-10T00:00:00Z"));
        assert!(!serialized.contains("snapshot-2"));
        assert!(!serialized.contains("narrowing_ratio_estimate"));

        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn context_efficiency_jsonl_summary_cache_keys_file_metadata_and_window() {
        let _guard = context_efficiency_test_lock();
        clear_jsonl_summary_cache_for_tests();
        let root = std::env::temp_dir().join(format!(
            "frigg-context-efficiency-summary-cache-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".frigg")).expect("frigg dir should be created");
        let log_path = root.join(".frigg/context.jsonl");
        let row = ContextEfficiencyLogRow {
            timestamp: "2026-06-10T00:00:00Z".to_owned(),
            tool: "search_text".to_owned(),
            repository_id: "repo-1".to_owned(),
            snapshot_id: Some("snapshot-1".to_owned()),
            metrics: ContextEfficiencyLogMetrics {
                indexed_readable_files: 1,
                indexed_readable_bytes: 10,
                indexed_min_mtime_ns: None,
                indexed_max_mtime_ns: None,
                candidate_input_count: Some(1),
                candidate_output_count: Some(1),
                returned_match_count: Some(1),
                returned_unique_paths: Some(1),
                returned_unique_file_bytes: Some(10),
                returned_source_bytes_estimate: Some(5),
                matched_file_context_saved_bytes_estimate: Some(5),
                matched_file_context_saved_percent_estimate: Some(50.0),
                query_duration_ms: Some(1),
                narrowing_ratio_estimate: Some(2),
            },
        };
        std::fs::write(
            &log_path,
            format!(
                "{}\n",
                serde_json::to_string(&row).expect("row should serialize")
            ),
        )
        .expect("log should be written");

        let now = DateTime::parse_from_rfc3339("2026-07-02T12:00:00Z")
            .expect("timestamp")
            .with_timezone(&Utc);
        let first_window =
            ContextSummaryWindow::resolve(Some("2026-06-01"), Some("2026-07-01"), now)
                .expect("first window should resolve");
        summarize_context_logs_for_roots(&[root.clone()], &first_window)
            .expect("first summary should load");
        summarize_context_logs_for_roots(&[root.clone()], &first_window)
            .expect("second summary should use cache");
        assert_eq!(jsonl_summary_cache_entry_count_for_tests(), 1);

        let second_window =
            ContextSummaryWindow::resolve(Some("2026-06-02"), Some("2026-07-01"), now)
                .expect("second window should resolve");
        summarize_context_logs_for_roots(&[root.clone()], &second_window)
            .expect("changed window should load");
        assert_eq!(jsonl_summary_cache_entry_count_for_tests(), 2);

        std::fs::write(&log_path, "").expect("log should be truncated");
        summarize_context_logs_for_roots(&[root.clone()], &first_window)
            .expect("changed file size should load");
        assert_eq!(jsonl_summary_cache_entry_count_for_tests(), 3);

        let _ = std::fs::remove_dir_all(root);
    }
}
