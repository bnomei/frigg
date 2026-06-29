//! Hybrid playbook model types: metadata contracts, witness groups, probe outcomes, and trace snapshots.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::searcher::SearchStageAttribution;
use serde::{Deserialize, Serialize};

pub(crate) fn default_hybrid_top_k() -> usize {
    8
}

/// Parsed playbook header metadata, including optional hybrid regression specification.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PlaybookMetadata {
    pub playbook_schema: String,
    pub playbook_id: String,
    #[serde(default)]
    pub hybrid_regression: Option<HybridPlaybookRegression>,
}

/// Executable hybrid search regression contract embedded in a playbook header.
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

/// Witness group that must or may match ranked retrieval paths for a playbook probe.
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

/// Path matching mode for a witness group during playbook evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HybridWitnessMatchMode {
    #[default]
    ExactAny,
    ExactOrPrefix,
}

/// Gating rule that decides when a witness group is required for a passing probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HybridWitnessRequirement {
    #[default]
    Always,
    SemanticOk,
}

/// Parsed markdown playbook with scrubbable metadata header and retained body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaybookDocument {
    pub metadata: PlaybookMetadata,
    pub body: String,
}

/// Filesystem-backed hybrid playbook ready for regression execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedHybridPlaybookRegression {
    pub path: PathBuf,
    pub metadata: PlaybookMetadata,
    pub spec: HybridPlaybookRegression,
}

/// Single channel hit captured in a playbook trace packet.
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

/// Per-channel retrieval trace for a hybrid playbook probe.
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

/// Ranked hybrid hit snapshot used in playbook trace packets.
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

/// Selection and witness rule trace for one ranked candidate.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HybridPlaybookCandidateTraceSnapshot {
    pub rank: usize,
    pub path: String,
    pub selection_rules: Vec<String>,
    pub path_witness_rules: Vec<String>,
    pub path_quality_rules: Vec<String>,
}

/// Serialized trace packet for one hybrid playbook regression run.
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

/// Witness evaluation outcome for one playbook group.
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

/// How a witness group matched, if at all, against ranked paths.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HybridWitnessMatchBy {
    #[default]
    None,
    Exact,
    Prefix,
}

/// Outcome of executing one hybrid playbook regression probe.
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
    /// Returns human-readable descriptions of required witness groups that failed.
    pub fn required_missing(&self) -> Vec<String> {
        self.required_witness_groups
            .iter()
            .filter(|group| !group.passed)
            .map(|group| format!("{} -> {:?}", group.group_id, group.match_any))
            .collect()
    }

    /// Returns human-readable descriptions of target witness groups that failed.
    pub fn target_missing(&self) -> Vec<String> {
        self.target_witness_groups
            .iter()
            .filter(|group| !group.passed)
            .map(|group| format!("{} -> {:?}", group.group_id, group.match_any))
            .collect()
    }

    /// Whether required witnesses and semantic status checks passed.
    pub fn passed_required(&self) -> bool {
        self.execution_error.is_none()
            && self.status_allowed
            && self
                .required_witness_groups
                .iter()
                .all(|group| group.passed)
    }

    /// Whether target witness groups passed when semantic status allows evaluation.
    pub fn passed_targets(&self) -> bool {
        self.execution_error.is_none()
            && self.status_allowed
            && self.target_witness_groups.iter().all(|group| group.passed)
    }

    /// Whether the probe passed required checks and, when requested, target checks.
    pub fn passed_all(&self, enforce_targets: bool) -> bool {
        self.passed_required() && (!enforce_targets || self.passed_targets())
    }
}

/// Aggregated results for a batch of hybrid playbook regressions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HybridPlaybookRunSummary {
    pub playbooks_root: String,
    pub enforce_targets: bool,
    pub playbook_count: usize,
    pub required_failures: usize,
    pub target_failures: usize,
    pub outcomes: Vec<HybridPlaybookProbeOutcome>,
}
