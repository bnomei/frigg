//! Shared execution-state types for the MCP server: symbol corpora, precise graph cache keys,
//! navigation target resolution envelopes, and per-tool execution result bundles.

use std::collections::{BTreeMap, VecDeque};
use std::mem::{size_of, size_of_val};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::ChannelHealthStatus;
use crate::domain::model::SymbolMatch;
use crate::graph::SymbolGraph;
use crate::indexer::SymbolDefinition;
use crate::languages::{
    BladeSourceEvidence, PhpSourceEvidence, RustImplementationFact, RustSymbolContext,
};
use crate::mcp::types::{
    ExploreLineWindow, ExploreResponse, FindReferencesResponse, ReadFileResponse, RuntimeTaskKind,
    RuntimeTaskStatus, RuntimeTaskSummary, SearchHybridChannelMetadata,
    SearchHybridChannelWeightsParams, SearchHybridResponse, SearchPatternType,
    SearchSymbolResponse, SearchTextResponse,
};
use rmcp::ErrorData;
use rmcp::handler::server::wrapper::Json;
use serde_json::Value;
use tracing::info;

/// Indexed symbol corpus for one repository, including lookup indexes and language evidence.
#[derive(Clone)]
pub(crate) struct RepositorySymbolCorpus {
    pub repository_id: String,
    pub runtime_repository_id: String,
    pub root: PathBuf,
    pub root_signature: String,
    pub source_paths: Vec<PathBuf>,
    pub symbols: Vec<SymbolDefinition>,
    pub container_symbol_index_by_index: Vec<Option<usize>>,
    pub symbols_by_relative_path: BTreeMap<String, Vec<usize>>,
    pub symbol_index_by_stable_id: BTreeMap<String, usize>,
    pub symbol_indices_by_name: BTreeMap<String, Vec<usize>>,
    pub symbol_indices_by_lower_name: BTreeMap<String, Vec<usize>>,
    pub canonical_symbol_name_by_stable_id: BTreeMap<String, String>,
    pub symbol_indices_by_canonical_name: BTreeMap<String, Vec<usize>>,
    pub symbol_indices_by_lower_canonical_name: BTreeMap<String, Vec<usize>>,
    pub rust_symbol_context_by_index: Vec<Option<RustSymbolContext>>,
    pub rust_implementation_facts: Vec<RustImplementationFact>,
    pub php_evidence_by_relative_path: BTreeMap<String, PhpSourceEvidence>,
    pub blade_evidence_by_relative_path: BTreeMap<String, BladeSourceEvidence>,
    pub diagnostics: RepositoryDiagnosticsSummary,
}

impl RepositorySymbolCorpus {
    pub(crate) fn estimated_heap_bytes(&self) -> usize {
        size_of::<Self>()
            + self.repository_id.len()
            + self.runtime_repository_id.len()
            + path_bytes(&self.root)
            + self.root_signature.len()
            + path_vec_bytes(&self.source_paths)
            + symbol_vec_bytes(&self.symbols)
            + self.container_symbol_index_by_index.capacity() * size_of::<Option<usize>>()
            + string_vec_map_bytes(&self.symbols_by_relative_path)
            + string_usize_map_bytes(&self.symbol_index_by_stable_id)
            + string_vec_map_bytes(&self.symbol_indices_by_name)
            + string_vec_map_bytes(&self.symbol_indices_by_lower_name)
            + string_string_map_bytes(&self.canonical_symbol_name_by_stable_id)
            + string_vec_map_bytes(&self.symbol_indices_by_canonical_name)
            + string_vec_map_bytes(&self.symbol_indices_by_lower_canonical_name)
            + self.rust_symbol_context_by_index.capacity() * size_of::<Option<RustSymbolContext>>()
            + rust_implementation_fact_vec_bytes(&self.rust_implementation_facts)
            + php_evidence_map_bytes(&self.php_evidence_by_relative_path)
            + blade_evidence_map_bytes(&self.blade_evidence_by_relative_path)
    }
}

fn path_bytes(path: &Path) -> usize {
    size_of::<PathBuf>() + path.as_os_str().to_string_lossy().len()
}

fn path_vec_bytes(paths: &[PathBuf]) -> usize {
    size_of_val(paths) + paths.iter().map(|path| path_bytes(path)).sum::<usize>()
}

fn symbol_vec_bytes(symbols: &[SymbolDefinition]) -> usize {
    size_of_val(symbols)
        + symbols
            .iter()
            .map(|symbol| symbol.stable_id.len() + symbol.name.len() + path_bytes(&symbol.path))
            .sum::<usize>()
}

fn string_vec_map_bytes(map: &BTreeMap<String, Vec<usize>>) -> usize {
    map.iter()
        .map(|(key, values)| key.len() + values.capacity() * size_of::<usize>())
        .sum()
}

fn string_usize_map_bytes(map: &BTreeMap<String, usize>) -> usize {
    map.keys().map(|key| key.len() + size_of::<usize>()).sum()
}

fn string_string_map_bytes(map: &BTreeMap<String, String>) -> usize {
    map.iter().map(|(key, value)| key.len() + value.len()).sum()
}

fn optional_string_bytes(value: &Option<String>) -> usize {
    value.as_ref().map(|value| value.len()).unwrap_or_default()
}

fn string_vec_bytes(values: &[String]) -> usize {
    size_of_val(values) + values.iter().map(String::len).sum::<usize>()
}

fn rust_implementation_fact_vec_bytes(facts: &[RustImplementationFact]) -> usize {
    size_of_val(facts)
        + facts
            .iter()
            .map(|fact| optional_string_bytes(&fact.trait_name) + fact.self_type.len())
            .sum::<usize>()
}

fn php_evidence_map_bytes(map: &BTreeMap<String, PhpSourceEvidence>) -> usize {
    map.iter()
        .map(|(path, evidence)| path.len() + php_evidence_bytes(evidence))
        .sum()
}

fn php_evidence_bytes(evidence: &PhpSourceEvidence) -> usize {
    string_string_map_bytes(&evidence.canonical_names_by_stable_id)
        + evidence.type_evidence.capacity() * size_of::<crate::languages::PhpTypeEvidence>()
        + evidence
            .type_evidence
            .iter()
            .map(|item| {
                optional_string_bytes(&item.owner_symbol_id) + item.target_canonical_name.len()
            })
            .sum::<usize>()
        + evidence.target_evidence.capacity() * size_of::<crate::languages::PhpTargetEvidence>()
        + evidence
            .target_evidence
            .iter()
            .map(|item| {
                optional_string_bytes(&item.owner_symbol_id)
                    + item.target_canonical_name.len()
                    + optional_string_bytes(&item.target_member_name)
            })
            .sum::<usize>()
        + evidence.literal_evidence.capacity() * size_of::<crate::languages::PhpLiteralEvidence>()
        + evidence
            .literal_evidence
            .iter()
            .map(|item| {
                optional_string_bytes(&item.owner_symbol_id)
                    + string_vec_bytes(&item.array_keys)
                    + string_vec_bytes(&item.named_arguments)
            })
            .sum::<usize>()
}

fn blade_evidence_map_bytes(map: &BTreeMap<String, BladeSourceEvidence>) -> usize {
    map.iter()
        .map(|(path, evidence)| path.len() + blade_evidence_bytes(evidence))
        .sum()
}

fn blade_evidence_bytes(evidence: &BladeSourceEvidence) -> usize {
    evidence.relations.capacity() * size_of::<crate::languages::BladeRelationEvidence>()
        + evidence
            .relations
            .iter()
            .map(|item| optional_string_bytes(&item.owner_symbol_id) + item.target_name.len())
            .sum::<usize>()
        + string_vec_bytes(&evidence.livewire_components)
        + string_vec_bytes(&evidence.wire_directives)
        + string_vec_bytes(&evidence.flux_components)
        + evidence
            .flux_hints
            .iter()
            .map(|(key, hint)| {
                key.len()
                    + string_vec_bytes(&hint.props)
                    + string_vec_bytes(&hint.slots)
                    + string_vec_bytes(&hint.variant_values)
                    + string_vec_bytes(&hint.size_values)
            })
            .sum::<usize>()
}

/// Aggregate diagnostic counts collected while building one repository symbol corpus.
#[derive(Debug, Clone, Default)]
pub(crate) struct RepositoryDiagnosticsSummary {
    pub manifest_walk_count: usize,
    pub manifest_read_count: usize,
    pub symbol_extraction_count: usize,
}

/// Cache key for one repository symbol corpus snapshot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SymbolCorpusCacheKey {
    pub repository_id: String,
    pub manifest_token: String,
}

/// Ranked symbol candidate considered during navigation target resolution.
#[derive(Clone)]
pub(crate) struct SymbolCandidate {
    pub rank: u8,
    pub path_class_rank: u8,
    pub path_class: &'static str,
    pub repository_id: String,
    pub root: PathBuf,
    pub symbol: SymbolDefinition,
}

/// Unambiguous navigation target selected from one symbol candidate.
#[derive(Clone)]
pub(crate) struct ResolvedSymbolTarget {
    pub candidate: SymbolCandidate,
    pub corpus: Arc<RepositorySymbolCorpus>,
    pub candidate_count: usize,
    pub selected_rank_candidate_count: usize,
}

/// Ambiguous navigation target requiring caller disambiguation.
#[derive(Clone)]
pub(crate) struct DisambiguationRequiredSymbolTarget {
    pub candidates: Vec<SymbolCandidate>,
    pub candidate_count: usize,
    pub selected_rank_candidate_count: usize,
}

/// Result of resolving one navigation symbol query to a target or candidate set.
#[derive(Clone)]
pub(crate) enum NavigationTargetSelection {
    Resolved(ResolvedSymbolTarget),
    DisambiguationRequired(DisambiguationRequiredSymbolTarget),
}

/// Navigation target resolution envelope shared by read-only navigation tools.
#[derive(Clone)]
pub(crate) struct ResolvedNavigationTarget {
    pub symbol_query: String,
    pub selection: NavigationTargetSelection,
    pub resolution_source: &'static str,
}

/// Result bundle for `read_file`, including best-effort provenance attribution.
pub(crate) struct ReadFileExecution {
    pub result: Result<ReadFileResponse, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
}

/// Execution summary for `search_text` used to emit diagnostics and tool-call telemetry.
#[allow(dead_code)]
pub(crate) struct SearchTextExecution {
    pub result: Result<Json<SearchTextResponse>, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub total_matches: usize,
    pub effective_limit: Option<usize>,
    pub effective_pattern_type: Option<SearchPatternType>,
    pub diagnostics_count: usize,
    pub walk_diagnostics_count: usize,
    pub read_diagnostics_count: usize,
}

/// Execution summary for `explore`, including resolved scope and pagination metadata.
pub(crate) struct ExploreExecution {
    pub result: Result<ExploreResponse, ErrorData>,
    pub resolved_repository_id: Option<String>,
    pub resolved_path: Option<String>,
    pub resolved_absolute_path: Option<String>,
    pub effective_context_lines: Option<usize>,
    pub effective_max_matches: Option<usize>,
    pub scan_scope: Option<ExploreLineWindow>,
    pub total_matches: usize,
    pub truncated: bool,
}

/// Execution summary for `search_hybrid`, including channel health and optional result anchors.
#[allow(dead_code)]
pub(crate) struct SearchHybridExecution {
    pub result: Result<Json<SearchHybridResponse>, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub effective_limit: Option<usize>,
    pub effective_weights: Option<SearchHybridChannelWeightsParams>,
    pub diagnostics_count: usize,
    pub walk_diagnostics_count: usize,
    pub read_diagnostics_count: usize,
    pub semantic_requested: Option<bool>,
    pub semantic_enabled: Option<bool>,
    pub semantic_status: Option<ChannelHealthStatus>,
    pub semantic_reason: Option<String>,
    pub semantic_candidate_count: Option<usize>,
    pub semantic_hit_count: Option<usize>,
    pub semantic_match_count: Option<usize>,
    pub warning: Option<String>,
    pub channel_metadata: Option<BTreeMap<String, SearchHybridChannelMetadata>>,
    pub match_anchors: Option<Value>,
}

const RUNTIME_TASK_RECENT_LIMIT: usize = 16;

/// Tracks in-flight and recently completed runtime work so long-lived servers can coordinate
/// background activity without duplicating the same repository task.
#[derive(Debug, Default)]
pub struct RuntimeTaskRegistry {
    next_sequence: u64,
    active: BTreeMap<String, RuntimeTaskSummary>,
    recent: VecDeque<RuntimeTaskSummary>,
}

impl RuntimeTaskRegistry {
    /// Creates an empty runtime task registry with no active or recent work.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one repository-scoped runtime task and returns its stable task id.
    pub fn start_task(
        &mut self,
        kind: RuntimeTaskKind,
        repository_id: impl Into<String>,
        phase: impl Into<String>,
        detail: Option<String>,
    ) -> String {
        self.next_sequence = self.next_sequence.saturating_add(1);
        let repository_id = repository_id.into();
        let phase = phase.into();
        let task_id = format!(
            "{}:{}:{:04}",
            runtime_task_kind_name(kind),
            repository_id,
            self.next_sequence
        );
        let summary = RuntimeTaskSummary {
            task_id: task_id.clone(),
            kind,
            status: RuntimeTaskStatus::Running,
            repository_id,
            phase,
            created_at_ms: now_unix_ms(),
            finished_at_ms: None,
            detail,
        };
        info!(
            task_id = %summary.task_id,
            task_kind = runtime_task_kind_name(summary.kind),
            repository_id = %summary.repository_id,
            phase = %summary.phase,
            detail = summary.detail.as_deref().unwrap_or(""),
            "runtime task started"
        );
        self.active.insert(task_id.clone(), summary);
        task_id
    }

    /// Starts a task only when none of the supplied repository aliases already has conflicting work.
    pub fn start_task_if_no_active_for_any_repository(
        &mut self,
        conflict_kinds: &[RuntimeTaskKind],
        repository_ids: &[&str],
        kind: RuntimeTaskKind,
        repository_id: impl Into<String>,
        phase: impl Into<String>,
        detail: Option<String>,
    ) -> Result<String, Vec<RuntimeTaskSummary>> {
        let active_tasks =
            self.active_tasks_for_kinds_and_any_repository(conflict_kinds, repository_ids);
        if !active_tasks.is_empty() {
            return Err(active_tasks);
        }
        Ok(self.start_task(kind, repository_id, phase, detail))
    }

    /// Moves an active task into the bounded recent-task history with its final status.
    pub fn finish_task(
        &mut self,
        task_id: &str,
        status: RuntimeTaskStatus,
        detail: Option<String>,
    ) {
        let Some(mut summary) = self.active.remove(task_id) else {
            return;
        };
        summary.status = status;
        summary.finished_at_ms = Some(now_unix_ms());
        if detail.is_some() {
            summary.detail = detail;
        }
        let duration_ms = summary
            .finished_at_ms
            .unwrap_or(summary.created_at_ms)
            .saturating_sub(summary.created_at_ms);
        info!(
            task_id = %summary.task_id,
            task_kind = runtime_task_kind_name(summary.kind),
            repository_id = %summary.repository_id,
            phase = %summary.phase,
            status = runtime_task_status_name(summary.status),
            duration_ms,
            detail = summary.detail.as_deref().unwrap_or(""),
            "runtime task finished"
        );
        self.push_recent(summary);
    }

    /// Returns active tasks in deterministic creation order for status responses.
    pub fn active_tasks(&self) -> Vec<RuntimeTaskSummary> {
        let mut tasks = self.active.values().cloned().collect::<Vec<_>>();
        tasks.sort_by(|left, right| {
            left.created_at_ms
                .cmp(&right.created_at_ms)
                .then(left.task_id.cmp(&right.task_id))
        });
        tasks
    }

    /// Checks for exact task-kind, repository-id, and phase activity.
    pub fn has_active_task(&self, kind: RuntimeTaskKind, repository_id: &str, phase: &str) -> bool {
        self.active.values().any(|task| {
            task.kind == kind && task.repository_id == repository_id && task.phase == phase
        })
    }

    /// Checks whether one repository already has active work of the requested kind.
    pub fn has_active_task_for_repository(
        &self,
        kind: RuntimeTaskKind,
        repository_id: &str,
    ) -> bool {
        self.active
            .values()
            .any(|task| task.kind == kind && task.repository_id == repository_id)
    }

    /// Checks active work across stable and runtime repository-id aliases.
    pub fn has_active_task_for_any_repository(
        &self,
        kind: RuntimeTaskKind,
        repository_ids: &[&str],
    ) -> bool {
        self.active
            .values()
            .any(|task| task.kind == kind && repository_ids.contains(&task.repository_id.as_str()))
    }

    /// Returns conflicting active tasks for a set of kinds and repository aliases.
    pub fn active_tasks_for_kinds_and_any_repository(
        &self,
        kinds: &[RuntimeTaskKind],
        repository_ids: &[&str],
    ) -> Vec<RuntimeTaskSummary> {
        let mut tasks = self
            .active
            .values()
            .filter(|task| {
                kinds.contains(&task.kind) && repository_ids.contains(&task.repository_id.as_str())
            })
            .cloned()
            .collect::<Vec<_>>();
        tasks.sort_by(|left, right| {
            left.created_at_ms
                .cmp(&right.created_at_ms)
                .then(left.task_id.cmp(&right.task_id))
        });
        tasks
    }

    /// Returns recently completed tasks newest-first for runtime status payloads.
    pub fn recent_tasks(&self) -> Vec<RuntimeTaskSummary> {
        self.recent.iter().rev().cloned().collect::<Vec<_>>()
    }

    /// Updates task detail in active or recent history when late error context becomes available.
    pub fn update_task_detail(&mut self, task_id: &str, detail: Option<String>) -> bool {
        if let Some(task) = self.active.get_mut(task_id) {
            task.detail = detail;
            return true;
        }
        if let Some(task) = self.recent.iter_mut().find(|task| task.task_id == task_id) {
            task.detail = detail;
            return true;
        }
        false
    }

    fn push_recent(&mut self, summary: RuntimeTaskSummary) {
        self.recent.push_back(summary);
        while self.recent.len() > RUNTIME_TASK_RECENT_LIMIT {
            self.recent.pop_front();
        }
    }
}

/// Finishes a runtime task on normal completion or marks it failed if unwinding skips the owner.
pub struct RuntimeTaskGuard {
    registry: Arc<RwLock<RuntimeTaskRegistry>>,
    task_id: Option<String>,
}

impl RuntimeTaskGuard {
    /// Starts a registry task and ensures unfinished work is marked failed when the guard drops.
    pub fn start(
        registry: Arc<RwLock<RuntimeTaskRegistry>>,
        kind: RuntimeTaskKind,
        repository_id: impl Into<String>,
        phase: impl Into<String>,
        detail: Option<String>,
    ) -> Self {
        let task_id = registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .start_task(kind, repository_id, phase, detail);
        Self {
            registry,
            task_id: Some(task_id),
        }
    }

    /// Starts a guarded task only when no conflicting task exists for any repository alias.
    pub fn try_start_if_no_active_for_any_repository(
        registry: Arc<RwLock<RuntimeTaskRegistry>>,
        conflict_kinds: &[RuntimeTaskKind],
        repository_ids: &[&str],
        kind: RuntimeTaskKind,
        repository_id: impl Into<String>,
        phase: impl Into<String>,
        detail: Option<String>,
    ) -> Result<Self, Vec<RuntimeTaskSummary>> {
        let task_id = registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .start_task_if_no_active_for_any_repository(
                conflict_kinds,
                repository_ids,
                kind,
                repository_id,
                phase,
                detail,
            )?;
        Ok(Self {
            registry,
            task_id: Some(task_id),
        })
    }

    /// Returns the registry task id while the guard still owns the active task.
    pub fn task_id(&self) -> &str {
        self.task_id
            .as_deref()
            .expect("runtime task guard should hold an active task id")
    }

    /// Completes the guarded task exactly once and removes the drop-time failure fallback.
    pub fn finish(&mut self, status: RuntimeTaskStatus, detail: Option<String>) {
        self.finish_inner(status, detail);
    }

    fn finish_inner(&mut self, status: RuntimeTaskStatus, detail: Option<String>) {
        let Some(task_id) = self.task_id.take() else {
            return;
        };
        self.registry
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .finish_task(&task_id, status, detail);
    }
}

impl Drop for RuntimeTaskGuard {
    fn drop(&mut self) {
        if self.task_id.is_some() {
            self.finish_inner(
                RuntimeTaskStatus::Failed,
                Some("runtime task owner dropped before reporting completion".to_owned()),
            );
        }
    }
}

fn runtime_task_kind_name(kind: RuntimeTaskKind) -> &'static str {
    match kind {
        RuntimeTaskKind::ChangedIndex => "changed_index",
        RuntimeTaskKind::SemanticRefresh => "semantic_refresh",
        RuntimeTaskKind::PrecisePrewarm => "precise_prewarm",
        RuntimeTaskKind::PreciseGenerate => "precise_generate",
        RuntimeTaskKind::WorkspacePrepare => "workspace_prepare",
        RuntimeTaskKind::WorkspaceIndex => "workspace_index",
    }
}

fn runtime_task_status_name(status: RuntimeTaskStatus) -> &'static str {
    match status {
        RuntimeTaskStatus::Running => "running",
        RuntimeTaskStatus::Succeeded => "succeeded",
        RuntimeTaskStatus::Failed => "failed",
    }
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_task_registry_tracks_active_and_recent_tasks() {
        let mut registry = RuntimeTaskRegistry::new();
        let first = registry.start_task(
            RuntimeTaskKind::ChangedIndex,
            "repo-001",
            "changed_only_index",
            Some("watch root /tmp/repo-001".to_owned()),
        );
        let second = registry.start_task(
            RuntimeTaskKind::SemanticRefresh,
            "repo-001",
            "semantic_attach_refresh",
            None,
        );

        let active = registry.active_tasks();
        assert_eq!(active.len(), 2);
        assert_eq!(active[0].task_id, first);
        assert_eq!(active[1].task_id, second);

        registry.finish_task(
            &first,
            RuntimeTaskStatus::Succeeded,
            Some("index complete".to_owned()),
        );
        registry.finish_task(
            &second,
            RuntimeTaskStatus::Failed,
            Some("startup validation failed".to_owned()),
        );

        assert!(registry.active_tasks().is_empty());
        let recent = registry.recent_tasks();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].task_id, second);
        assert_eq!(recent[0].status, RuntimeTaskStatus::Failed);
        assert_eq!(recent[1].task_id, first);
        assert_eq!(recent[1].status, RuntimeTaskStatus::Succeeded);
    }

    #[test]
    fn has_active_task_for_any_repository_treats_stable_and_runtime_ids_as_aliases() {
        let mut registry = RuntimeTaskRegistry::new();
        let task = registry.start_task(
            RuntimeTaskKind::SemanticRefresh,
            "repo-001",
            "watch_semantic_followup",
            None,
        );

        assert!(registry.has_active_task_for_any_repository(
            RuntimeTaskKind::SemanticRefresh,
            &["myrepo-abc123def456", "repo-001"],
        ));
        assert!(!registry.has_active_task_for_repository(
            RuntimeTaskKind::SemanticRefresh,
            "myrepo-abc123def456",
        ));
        assert!(!registry.has_active_task_for_any_repository(
            RuntimeTaskKind::WorkspaceIndex,
            &["myrepo-abc123def456", "repo-001"],
        ));

        registry.finish_task(&task, RuntimeTaskStatus::Succeeded, None);
        assert!(!registry.has_active_task_for_any_repository(
            RuntimeTaskKind::SemanticRefresh,
            &["myrepo-abc123def456", "repo-001"],
        ));
    }

    #[test]
    fn runtime_task_registry_atomic_start_rejects_conflicting_alias_without_insert() {
        let mut registry = RuntimeTaskRegistry::new();
        let active = registry.start_task(
            RuntimeTaskKind::WorkspaceIndex,
            "stable-repo",
            "workspace_index",
            None,
        );

        let rejected = registry
            .start_task_if_no_active_for_any_repository(
                &[
                    RuntimeTaskKind::WorkspaceIndex,
                    RuntimeTaskKind::ChangedIndex,
                ],
                &["repo-001", "stable-repo"],
                RuntimeTaskKind::ChangedIndex,
                "repo-001",
                "watch_manifest_fast",
                None,
            )
            .expect_err("conflicting active alias should reject atomic task start");

        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].task_id, active);
        assert_eq!(registry.active_tasks().len(), 1);

        registry.finish_task(&active, RuntimeTaskStatus::Succeeded, None);
        let started = registry
            .start_task_if_no_active_for_any_repository(
                &[
                    RuntimeTaskKind::WorkspaceIndex,
                    RuntimeTaskKind::ChangedIndex,
                ],
                &["repo-001", "stable-repo"],
                RuntimeTaskKind::ChangedIndex,
                "repo-001",
                "watch_manifest_fast",
                None,
            )
            .expect("task should start once conflicting alias finishes");
        let active_tasks = registry.active_tasks();
        assert_eq!(active_tasks.len(), 1);
        assert_eq!(active_tasks[0].task_id, started);
        assert_eq!(active_tasks[0].repository_id, "repo-001");
    }

    #[test]
    fn runtime_task_guard_finishes_explicit_status_once() {
        let registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
        let mut guard = RuntimeTaskGuard::start(
            Arc::clone(&registry),
            RuntimeTaskKind::WorkspaceIndex,
            "repo-001",
            "workspace_index",
            None,
        );
        let task_id = guard.task_id().to_owned();

        guard.finish(RuntimeTaskStatus::Succeeded, Some("done".to_owned()));

        let registry = registry.read().expect("registry lock should be available");
        assert!(registry.active_tasks().is_empty());
        let recent = registry.recent_tasks();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].task_id, task_id);
        assert_eq!(recent[0].status, RuntimeTaskStatus::Succeeded);
        assert_eq!(recent[0].detail.as_deref(), Some("done"));
    }

    #[test]
    fn runtime_task_guard_drop_marks_task_failed() {
        let registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
        let task_id = {
            let guard = RuntimeTaskGuard::start(
                Arc::clone(&registry),
                RuntimeTaskKind::SemanticRefresh,
                "repo-001",
                "semantic_attach_refresh",
                None,
            );
            guard.task_id().to_owned()
        };

        let registry = registry.read().expect("registry lock should be available");
        assert!(registry.active_tasks().is_empty());
        let recent = registry.recent_tasks();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].task_id, task_id);
        assert_eq!(recent[0].status, RuntimeTaskStatus::Failed);
        assert_eq!(
            recent[0].detail.as_deref(),
            Some("runtime task owner dropped before reporting completion")
        );
    }

    #[test]
    #[allow(clippy::panic)]
    fn runtime_task_guard_drop_marks_task_failed_during_unwind() {
        let registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
        let result = std::panic::catch_unwind({
            let registry = Arc::clone(&registry);
            move || {
                let _guard = RuntimeTaskGuard::start(
                    registry,
                    RuntimeTaskKind::PreciseGenerate,
                    "repo-001",
                    "precise_generation",
                    None,
                );
                std::panic::panic_any("simulate runtime task panic");
            }
        });

        assert!(result.is_err());
        let registry = registry.read().expect("registry lock should be available");
        assert!(registry.active_tasks().is_empty());
        let recent = registry.recent_tasks();
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].status, RuntimeTaskStatus::Failed);
        assert_eq!(
            recent[0].detail.as_deref(),
            Some("runtime task owner dropped before reporting completion")
        );
    }

    #[test]
    fn runtime_task_guard_atomic_start_returns_conflicts_or_guard() {
        let registry = Arc::new(RwLock::new(RuntimeTaskRegistry::new()));
        let active = registry
            .write()
            .expect("registry lock should be available")
            .start_task(
                RuntimeTaskKind::WorkspacePrepare,
                "repo-001",
                "workspace_prepare",
                None,
            );

        let rejected = RuntimeTaskGuard::try_start_if_no_active_for_any_repository(
            Arc::clone(&registry),
            &[RuntimeTaskKind::WorkspacePrepare],
            &["repo-001"],
            RuntimeTaskKind::WorkspaceIndex,
            "repo-001",
            "workspace_index",
            None,
        );
        assert!(
            rejected.is_err(),
            "guard start should return active conflicts"
        );
        let rejected = rejected.err().expect("active conflicts should be returned");

        assert_eq!(rejected.len(), 1);
        assert_eq!(rejected[0].task_id, active);
        assert_eq!(
            registry
                .read()
                .expect("registry lock should be available")
                .active_tasks()
                .len(),
            1
        );

        registry
            .write()
            .expect("registry lock should be available")
            .finish_task(&active, RuntimeTaskStatus::Succeeded, None);
        let mut guard = RuntimeTaskGuard::try_start_if_no_active_for_any_repository(
            Arc::clone(&registry),
            &[RuntimeTaskKind::WorkspacePrepare],
            &["repo-001"],
            RuntimeTaskKind::WorkspaceIndex,
            "repo-001",
            "workspace_index",
            None,
        )
        .expect("guard should start when no conflicts remain");
        let task_id = guard.task_id().to_owned();
        guard.finish(RuntimeTaskStatus::Succeeded, None);

        let recent = registry
            .read()
            .expect("registry lock should be available")
            .recent_tasks();
        assert!(recent.iter().any(|task| task.task_id == task_id));
    }

    #[test]
    fn runtime_task_registry_updates_recent_task_detail() {
        let mut registry = RuntimeTaskRegistry::new();
        let task_id = registry.start_task(
            RuntimeTaskKind::PrecisePrewarm,
            "repo-001",
            "precise_attach_prewarm",
            None,
        );
        registry.finish_task(
            &task_id,
            RuntimeTaskStatus::Failed,
            Some("generic".to_owned()),
        );

        assert!(registry.update_task_detail(
            &task_id,
            Some("failed to spawn precise prewarm thread: unavailable".to_owned()),
        ));

        let recent = registry.recent_tasks();
        assert_eq!(
            recent[0].detail.as_deref(),
            Some("failed to spawn precise prewarm thread: unavailable")
        );
    }
}

/// Execution summary for `search_symbol`, including corpus diagnostics and effective limit.
pub(crate) struct SearchSymbolExecution {
    pub result: Result<Json<SearchSymbolResponse>, ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub diagnostics_count: usize,
    pub manifest_walk_diagnostics_count: usize,
    pub manifest_read_diagnostics_count: usize,
    pub symbol_extraction_diagnostics_count: usize,
    pub effective_limit: Option<usize>,
}

/// Symbol match after navigation-aware rank, path-class rank, and context rank are assigned.
#[derive(Debug, Clone)]
pub(crate) struct RankedSymbolMatch {
    pub rank: u8,
    pub path_class_rank: u8,
    pub context_rank: u8,
    pub matched: SymbolMatch,
}

/// Execution summary for `find_references`, including precise and source fallback budgets.
#[allow(dead_code)]
pub(crate) struct FindReferencesExecution {
    pub result: Result<Json<FindReferencesResponse>, ErrorData>,
    pub provenance_result: Result<(), ErrorData>,
    pub scoped_repository_ids: Vec<String>,
    pub total_matches: usize,
    pub selected_symbol_id: Option<String>,
    pub selected_precise_symbol: Option<String>,
    pub resolution_precision: Option<String>,
    pub resolution_source: Option<String>,
    pub diagnostics_count: usize,
    pub manifest_walk_diagnostics_count: usize,
    pub manifest_read_diagnostics_count: usize,
    pub symbol_extraction_diagnostics_count: usize,
    pub source_read_diagnostics_count: usize,
    pub precise_artifacts_discovered: usize,
    pub precise_artifacts_discovered_bytes: u64,
    pub precise_artifacts_ingested: usize,
    pub precise_artifacts_ingested_bytes: u64,
    pub precise_artifacts_failed: usize,
    pub precise_artifacts_failed_bytes: u64,
    pub precise_reference_count: usize,
    pub source_files_discovered: usize,
    pub source_files_loaded: usize,
    pub source_bytes_loaded: u64,
    pub effective_limit: Option<usize>,
}

/// One failed precise-artifact candidate retained for user-facing ingest diagnostics.
#[derive(Debug, Clone)]
pub(crate) struct PreciseArtifactFailureSample {
    pub artifact_label: String,
    pub stage: String,
    pub detail: String,
}

/// Precise-artifact discovery and ingest counters collected during graph build.
#[derive(Debug, Clone, Default)]
pub(crate) struct PreciseIngestStats {
    pub candidate_directories: Vec<String>,
    pub discovered_artifacts: Vec<String>,
    pub artifacts_discovered: usize,
    pub artifacts_discovered_bytes: u64,
    pub artifacts_ingested: usize,
    pub artifacts_ingested_bytes: u64,
    pub artifacts_failed: usize,
    pub artifacts_failed_bytes: u64,
    pub failed_artifacts: Vec<PreciseArtifactFailureSample>,
}

/// Resource ceilings that keep reference search bounded across SCIP artifacts and source fallback.
#[derive(Debug, Clone, Copy)]
pub(crate) struct FindReferencesResourceBudgets {
    pub scip_max_artifacts: usize,
    pub scip_max_artifact_bytes: usize,
    pub scip_max_total_bytes: usize,
    pub scip_max_documents_per_artifact: usize,
    pub scip_max_elapsed_ms: u64,
    pub source_max_files: usize,
    pub source_max_file_bytes: usize,
    pub source_max_total_bytes: usize,
    pub source_max_elapsed_ms: u64,
}

/// Cache key for one ingested precise symbol graph.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct PreciseGraphCacheKey {
    pub repository_id: String,
    pub scip_signature: String,
    pub corpus_signature: String,
}

/// Cached precise graph with ingest stats and artifact discovery metadata.
#[derive(Debug, Clone)]
pub(crate) struct CachedPreciseGraph {
    pub graph: Arc<SymbolGraph>,
    pub ingest_stats: PreciseIngestStats,
    pub corpus_signature: String,
    pub discovery: ScipArtifactDiscovery,
    pub coverage_mode: PreciseCoverageMode,
}

/// How completely precise artifacts cover navigation for one repository.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PreciseCoverageMode {
    Full,
    Partial,
    None,
}

impl PreciseCoverageMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Partial => "partial",
            Self::None => "none",
        }
    }
}

/// Stable file metadata digest for a discovered SCIP artifact candidate.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScipArtifactDigest {
    pub path: PathBuf,
    pub format: ScipArtifactFormat,
    pub size_bytes: u64,
    pub mtime_ns: Option<u64>,
}

/// Discovered SCIP artifact and candidate-directory digests for precise ingest.
#[derive(Debug, Clone, Default)]
pub(crate) struct ScipArtifactDiscovery {
    pub candidate_directories: Vec<String>,
    pub candidate_directory_digests: Vec<ScipCandidateDirectoryDigest>,
    pub artifact_digests: Vec<ScipArtifactDigest>,
}

/// Stable existence and freshness digest for a directory probed during SCIP discovery.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ScipCandidateDirectoryDigest {
    pub path: PathBuf,
    pub exists: bool,
    pub mtime_ns: Option<u64>,
}

/// On-disk encoding supported for SCIP precise-artifact ingest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum ScipArtifactFormat {
    Json,
    Protobuf,
}

impl ScipArtifactFormat {
    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("json") => Some(Self::Json),
            Some("scip") => Some(Self::Protobuf),
            _ => None,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Json => "json",
            Self::Protobuf => "protobuf",
        }
    }
}

/// Small deterministic FNV-style hasher used for cache signatures that must not vary by process.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DeterministicSignatureHasher {
    state: u64,
}

impl DeterministicSignatureHasher {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

    pub(crate) fn new() -> Self {
        Self {
            state: Self::OFFSET_BASIS,
        }
    }

    pub(crate) fn write_bytes(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.state ^= u64::from(*byte);
            self.state = self.state.wrapping_mul(Self::FNV_PRIME);
        }
    }

    pub(crate) fn write_separator(&mut self) {
        self.write_bytes(&[0xff]);
    }

    pub(crate) fn write_str(&mut self, value: &str) {
        self.write_bytes(value.as_bytes());
        self.write_separator();
    }

    pub(crate) fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
        self.write_separator();
    }

    pub(crate) fn write_optional_u64(&mut self, value: Option<u64>) {
        match value {
            Some(value) => {
                self.write_bytes(&[1]);
                self.write_u64(value);
            }
            None => {
                self.write_bytes(&[0]);
                self.write_separator();
            }
        }
    }

    pub(crate) fn finish_hex(self) -> String {
        format!("{:016x}", self.state)
    }
}
