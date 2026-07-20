//! Debounced watch scheduler that coalesces filesystem events into manifest-fast and
//! semantic-followup index work without letting concurrent refresh classes collide.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use tokio::time::Instant;

const MAX_RECENT_PATH_SAMPLES: usize = 4;
pub(super) const MAX_DEBOUNCE_DELAY_MULTIPLIER: u32 = 5;
pub(super) const MAX_RETRY_BACKOFF_EXPONENT: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WatchRefreshClass {
    ManifestFast,
    SemanticFollowup,
}

impl WatchRefreshClass {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::ManifestFast => "manifest_fast",
            Self::SemanticFollowup => "semantic_followup",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ScheduledRefresh {
    pub root_idx: usize,
    pub repository_id: String,
    pub class: WatchRefreshClass,
}

/// Agent/operator-facing dual-class queue snapshot for one repository (EXP-hotpath-queue D).
///
/// Depth counts pending + in-flight units across the two refresh classes only — there is no
/// third `agent_hot` class (EXP-hotpath-queue A).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WatchRepositoryQueueSnapshot {
    /// Manifest-fast refresh is debounced/queued but not yet running.
    pub manifest_fast_pending: bool,
    /// Semantic-followup refresh is queued after a successful manifest-fast pass.
    pub semantic_followup_pending: bool,
    /// Manifest-fast worker currently holds this repository.
    pub manifest_fast_in_flight: bool,
    /// Semantic-followup worker currently holds this repository.
    pub semantic_followup_in_flight: bool,
    /// Sampled dirty path count retained for status hints (not a full change set).
    pub dirty_path_hint_count: usize,
    /// Oldest `first_pending_at` across pending classes (tokio Instant).
    pub(crate) oldest_first_pending_at: Option<Instant>,
}

impl WatchRepositoryQueueSnapshot {
    /// Pending + in-flight units for dual-class watch (0..=4).
    pub fn refresh_queue_depth(&self) -> usize {
        usize::from(self.manifest_fast_pending)
            + usize::from(self.semantic_followup_pending)
            + usize::from(self.manifest_fast_in_flight)
            + usize::from(self.semantic_followup_in_flight)
    }

    /// Best-effort age of oldest pending debounce/retry signal (not a hard ETA).
    pub fn oldest_pending_age_ms(&self, now: Instant) -> Option<u64> {
        self.oldest_first_pending_at
            .map(|at| now.saturating_duration_since(at).as_millis() as u64)
    }

    /// Whether any dual-class unit is queued or actively refreshing this repository.
    pub fn has_pending_or_in_flight(&self) -> bool {
        self.refresh_queue_depth() > 0
    }
}

pub(super) enum RepositorySelector {
    Index(usize),
    Id(String),
}

impl From<usize> for RepositorySelector {
    fn from(value: usize) -> Self {
        Self::Index(value)
    }
}

impl From<&str> for RepositorySelector {
    fn from(value: &str) -> Self {
        Self::Id(value.to_owned())
    }
}

impl From<&String> for RepositorySelector {
    fn from(value: &String) -> Self {
        Self::Id(value.clone())
    }
}

impl From<String> for RepositorySelector {
    fn from(value: String) -> Self {
        Self::Id(value)
    }
}

#[derive(Debug, Clone, Default)]
struct RefreshQueueState {
    pending: bool,
    first_pending_at: Option<Instant>,
    debounce_deadline: Option<Instant>,
    retry_deadline: Option<Instant>,
    retry_attempts: u32,
    rerun_requested: bool,
}

impl RefreshQueueState {
    fn enqueue(&mut self, now: Instant) {
        self.pending = true;
        self.first_pending_at = Some(now);
        self.debounce_deadline = Some(now);
        self.retry_deadline = None;
        self.retry_attempts = 0;
    }

    fn mark_started(&mut self) {
        self.pending = false;
        self.first_pending_at = None;
        self.debounce_deadline = None;
        self.retry_deadline = None;
    }

    fn mark_succeeded(&mut self, now: Instant) {
        self.retry_deadline = None;
        self.retry_attempts = 0;
        if self.rerun_requested {
            self.pending = true;
            self.rerun_requested = false;
            if self.first_pending_at.is_none() {
                self.first_pending_at = Some(now);
            }
            if self.debounce_deadline.is_none() {
                self.debounce_deadline = Some(now);
            }
        } else {
            self.pending = false;
            self.first_pending_at = None;
            self.debounce_deadline = None;
        }
    }

    fn mark_failed(&mut self, now: Instant, retry: Duration) -> Duration {
        self.retry_attempts = self.retry_attempts.saturating_add(1);
        let retry_delay = retry_backoff_delay(retry, self.retry_attempts);
        self.pending = true;
        self.first_pending_at = Some(now);
        self.rerun_requested = false;
        self.debounce_deadline = None;
        self.retry_deadline = Some(now + retry_delay);
        retry_delay
    }

    fn mark_blocked(&mut self) {
        self.pending = false;
        self.first_pending_at = None;
        self.rerun_requested = false;
        self.debounce_deadline = None;
        self.retry_deadline = None;
        self.retry_attempts = 0;
    }

    fn ready_at(&self) -> Option<Instant> {
        if !self.pending {
            return None;
        }

        match (self.debounce_deadline, self.retry_deadline) {
            (Some(debounce), Some(retry)) => Some(std::cmp::max(debounce, retry)),
            (Some(debounce), None) => Some(debounce),
            (None, Some(retry)) => Some(retry),
            (None, None) => Some(Instant::now()),
        }
    }
}

fn retry_backoff_delay(base: Duration, attempts_after_failure: u32) -> Duration {
    let exponent = attempts_after_failure
        .saturating_sub(1)
        .min(MAX_RETRY_BACKOFF_EXPONENT);
    base.checked_mul(1_u32 << exponent).unwrap_or(Duration::MAX)
}

#[derive(Debug, Clone)]
pub(super) struct RepositoryWatchState {
    active_class: Option<WatchRefreshClass>,
    manifest_fast: RefreshQueueState,
    semantic_followup: RefreshQueueState,
    recent_paths: VecDeque<PathBuf>,
    dirty_path_hints: BTreeSet<PathBuf>,
    manifest_fast_inflight_paths: BTreeSet<PathBuf>,
    semantic_followup_paths: BTreeSet<PathBuf>,
}

impl RepositoryWatchState {
    fn push_sample(&mut self, path: PathBuf) {
        if self.recent_paths.len() == MAX_RECENT_PATH_SAMPLES {
            self.recent_paths.pop_front();
        }
        self.recent_paths.push_back(path);
    }

    fn record_event(&mut self, path: PathBuf, now: Instant, debounce: Duration) {
        self.push_sample(path.clone());
        self.dirty_path_hints.insert(path.clone());
        if self.semantic_followup.pending {
            self.semantic_followup_paths.insert(path);
        }
        self.manifest_fast.pending = true;
        let first_pending_at = *self.manifest_fast.first_pending_at.get_or_insert(now);
        if self.manifest_fast.retry_deadline.is_some()
            && self.active_class != Some(WatchRefreshClass::ManifestFast)
        {
            return;
        }
        let max_delay = debounce
            .checked_mul(MAX_DEBOUNCE_DELAY_MULTIPLIER)
            .unwrap_or(Duration::MAX);
        self.manifest_fast.debounce_deadline =
            Some(std::cmp::min(now + debounce, first_pending_at + max_delay));
        if self.active_class == Some(WatchRefreshClass::ManifestFast) {
            self.manifest_fast.rerun_requested = true;
        }
        if !self.semantic_followup.pending
            && self.active_class != Some(WatchRefreshClass::SemanticFollowup)
            && self.semantic_followup.retry_deadline.is_none()
        {
            self.semantic_followup = RefreshQueueState::default();
            self.semantic_followup_paths.clear();
        }
    }

    fn enqueue_initial_sync(&mut self, class: WatchRefreshClass, now: Instant) {
        match class {
            WatchRefreshClass::ManifestFast => self.manifest_fast.enqueue(now),
            WatchRefreshClass::SemanticFollowup => self.semantic_followup.enqueue(now),
        }
    }

    fn enqueue_semantic_followup(&mut self, now: Instant) {
        if self.active_class == Some(WatchRefreshClass::SemanticFollowup)
            || self.semantic_followup.pending
        {
            return;
        }
        self.semantic_followup.enqueue(now);
    }

    fn mark_started(&mut self, class: WatchRefreshClass) -> Vec<PathBuf> {
        self.active_class = Some(class);
        match class {
            WatchRefreshClass::ManifestFast => {
                self.manifest_fast.mark_started();
                let started_paths = self.dirty_path_hints.iter().cloned().collect::<Vec<_>>();
                self.dirty_path_hints.clear();
                self.manifest_fast_inflight_paths = started_paths.iter().cloned().collect();
                self.recent_paths.clear();
                if !started_paths.is_empty() {
                    self.semantic_followup_paths
                        .extend(started_paths.iter().cloned());
                }
                started_paths
            }
            WatchRefreshClass::SemanticFollowup => {
                self.semantic_followup.mark_started();
                self.semantic_followup_paths.iter().cloned().collect()
            }
        }
    }

    fn mark_succeeded(&mut self, class: WatchRefreshClass, now: Instant) {
        self.active_class = self.active_class.filter(|active| *active != class);
        match class {
            WatchRefreshClass::ManifestFast => {
                self.manifest_fast_inflight_paths.clear();
                self.manifest_fast.mark_succeeded(now);
            }
            WatchRefreshClass::SemanticFollowup => {
                self.semantic_followup.mark_succeeded(now);
                if !self.semantic_followup.pending {
                    self.semantic_followup_paths.clear();
                }
            }
        }
    }

    fn mark_failed(&mut self, class: WatchRefreshClass, now: Instant, retry: Duration) -> Duration {
        self.active_class = self.active_class.filter(|active| *active != class);
        match class {
            WatchRefreshClass::ManifestFast => {
                self.dirty_path_hints
                    .extend(self.manifest_fast_inflight_paths.iter().cloned());
                self.manifest_fast_inflight_paths.clear();
                self.manifest_fast.mark_failed(now, retry)
            }
            WatchRefreshClass::SemanticFollowup => self.semantic_followup.mark_failed(now, retry),
        }
    }

    fn mark_blocked(&mut self, class: WatchRefreshClass) {
        self.active_class = self.active_class.filter(|active| *active != class);
        match class {
            WatchRefreshClass::ManifestFast => {
                self.manifest_fast_inflight_paths.clear();
                self.manifest_fast.mark_blocked();
            }
            WatchRefreshClass::SemanticFollowup => {
                self.semantic_followup.mark_blocked();
                self.semantic_followup_paths.clear();
            }
        }
    }

    fn ready_at(&self, class: WatchRefreshClass) -> Option<Instant> {
        if self.active_class.is_some() {
            return None;
        }

        match class {
            WatchRefreshClass::ManifestFast => self.manifest_fast.ready_at(),
            WatchRefreshClass::SemanticFollowup => self.semantic_followup.ready_at(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub(super) struct WatchSchedulerState {
    repositories: BTreeMap<String, RepositoryWatchState>,
    repository_ids_by_index: Vec<String>,
    in_flight_manifest_fast: BTreeSet<String>,
    in_flight_semantic_followup: BTreeSet<String>,
    manifest_fast_concurrency_limit: usize,
    semantic_followup_concurrency_limit: usize,
}

impl WatchSchedulerState {
    #[cfg(test)]
    pub(super) fn new(root_count: usize) -> Self {
        Self::with_concurrency_limits(root_count, 1, 1)
    }

    pub(super) fn with_concurrency_limits(
        root_count: usize,
        manifest_fast_concurrency_limit: usize,
        semantic_followup_concurrency_limit: usize,
    ) -> Self {
        assert!(
            manifest_fast_concurrency_limit > 0,
            "manifest-fast watch concurrency limit must be greater than zero"
        );
        assert!(
            semantic_followup_concurrency_limit > 0,
            "semantic-followup watch concurrency limit must be greater than zero"
        );
        let mut scheduler = Self {
            manifest_fast_concurrency_limit,
            semantic_followup_concurrency_limit,
            ..Self::default()
        };
        for index in 0..root_count {
            let repository_id = format!("repo-{index:03}");
            scheduler.add_repository(&repository_id);
        }
        scheduler
    }

    pub(super) fn add_repository(&mut self, repository_id: &str) {
        if !self
            .repository_ids_by_index
            .iter()
            .any(|id| id == repository_id)
        {
            self.repository_ids_by_index.push(repository_id.to_owned());
        }
        self.repositories
            .entry(repository_id.to_owned())
            .or_insert_with(|| RepositoryWatchState {
                active_class: None,
                manifest_fast: RefreshQueueState::default(),
                semantic_followup: RefreshQueueState::default(),
                recent_paths: VecDeque::new(),
                dirty_path_hints: BTreeSet::new(),
                manifest_fast_inflight_paths: BTreeSet::new(),
                semantic_followup_paths: BTreeSet::new(),
            });
    }

    pub(super) fn remove_repository(&mut self, repository_id: &str) {
        self.repositories.remove(repository_id);
        self.in_flight_manifest_fast.remove(repository_id);
        self.in_flight_semantic_followup.remove(repository_id);
    }

    pub(super) fn enqueue_initial_sync(
        &mut self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
        now: Instant,
    ) {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return;
        };
        if let Some(state) = self.repositories.get_mut(&repository_id) {
            state.enqueue_initial_sync(class, now);
        }
    }

    pub(super) fn enqueue_semantic_followup(
        &mut self,
        repository_id: impl Into<RepositorySelector>,
        now: Instant,
    ) {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return;
        };
        if let Some(state) = self.repositories.get_mut(&repository_id) {
            state.enqueue_semantic_followup(now);
        }
    }

    pub(super) fn record_path_change(
        &mut self,
        repository_id: impl Into<RepositorySelector>,
        path: PathBuf,
        now: Instant,
        debounce: Duration,
    ) {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return;
        };
        if let Some(state) = self.repositories.get_mut(&repository_id) {
            state.record_event(path, now, debounce);
        }
    }

    pub(super) fn next_ready_refresh(&self, now: Instant) -> Option<ScheduledRefresh> {
        if self.in_flight_manifest_fast.len() < self.manifest_fast_concurrency_limit
            && let Some(repository_id) =
                self.next_ready_repository_for_class(now, WatchRefreshClass::ManifestFast)
        {
            return Some(ScheduledRefresh {
                root_idx: self.repository_index(&repository_id).unwrap_or(usize::MAX),
                repository_id,
                class: WatchRefreshClass::ManifestFast,
            });
        }

        if self.in_flight_semantic_followup.len() < self.semantic_followup_concurrency_limit
            && let Some(repository_id) =
                self.next_ready_repository_for_class(now, WatchRefreshClass::SemanticFollowup)
        {
            return Some(ScheduledRefresh {
                root_idx: self.repository_index(&repository_id).unwrap_or(usize::MAX),
                repository_id,
                class: WatchRefreshClass::SemanticFollowup,
            });
        }

        None
    }

    fn next_ready_repository_for_class(
        &self,
        now: Instant,
        class: WatchRefreshClass,
    ) -> Option<String> {
        self.repositories
            .iter()
            .filter_map(|(repository_id, state)| {
                state
                    .ready_at(class)
                    .map(|ready_at| (repository_id.clone(), ready_at))
            })
            .filter(|(_, ready_at)| *ready_at <= now)
            .min_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)))
            .map(|(repository_id, _)| repository_id)
    }

    pub(super) fn mark_started(
        &mut self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
    ) -> Vec<PathBuf> {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return Vec::new();
        };
        self.in_flight_set_mut(class).insert(repository_id.clone());
        self.repositories
            .get_mut(&repository_id)
            .map(|state| state.mark_started(class))
            .unwrap_or_default()
    }

    pub(super) fn mark_succeeded(
        &mut self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
        now: Instant,
    ) {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return;
        };
        self.in_flight_set_mut(class).remove(&repository_id);
        if let Some(state) = self.repositories.get_mut(&repository_id) {
            state.mark_succeeded(class, now);
        }
    }

    pub(super) fn mark_failed(
        &mut self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
        now: Instant,
        retry: Duration,
    ) -> Option<Duration> {
        let repository_id = self.resolve_repository_id(repository_id.into())?;
        self.in_flight_set_mut(class).remove(&repository_id);
        self.repositories
            .get_mut(&repository_id)
            .map(|state| state.mark_failed(class, now, retry))
    }

    pub(super) fn mark_blocked(
        &mut self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
    ) {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return;
        };
        self.in_flight_set_mut(class).remove(&repository_id);
        if let Some(state) = self.repositories.get_mut(&repository_id) {
            state.mark_blocked(class);
        }
    }

    pub(super) fn repository_pending(
        &self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
    ) -> bool {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return false;
        };
        self.repositories
            .get(&repository_id)
            .map(|state| match class {
                WatchRefreshClass::ManifestFast => state.manifest_fast.pending,
                WatchRefreshClass::SemanticFollowup => state.semantic_followup.pending,
            })
            .unwrap_or(false)
    }

    /// Dual-class queue snapshot for MCP/agent observability (no third hot-path class).
    pub(super) fn repository_queue_snapshot(
        &self,
        repository_id: &str,
    ) -> Option<WatchRepositoryQueueSnapshot> {
        let state = self.repositories.get(repository_id)?;
        let mut oldest: Option<Instant> = None;
        for first in [
            state.manifest_fast.first_pending_at,
            state.semantic_followup.first_pending_at,
        ]
        .into_iter()
        .flatten()
        {
            oldest = Some(match oldest {
                Some(prev) => std::cmp::min(prev, first),
                None => first,
            });
        }
        let mut unique_dirty = BTreeSet::new();
        unique_dirty.extend(state.dirty_path_hints.iter().cloned());
        unique_dirty.extend(state.manifest_fast_inflight_paths.iter().cloned());
        unique_dirty.extend(state.semantic_followup_paths.iter().cloned());
        Some(WatchRepositoryQueueSnapshot {
            manifest_fast_pending: state.manifest_fast.pending,
            semantic_followup_pending: state.semantic_followup.pending,
            manifest_fast_in_flight: self.in_flight_manifest_fast.contains(repository_id)
                || state.active_class == Some(WatchRefreshClass::ManifestFast),
            semantic_followup_in_flight: self.in_flight_semantic_followup.contains(repository_id)
                || state.active_class == Some(WatchRefreshClass::SemanticFollowup),
            dirty_path_hint_count: unique_dirty.len(),
            oldest_first_pending_at: oldest,
        })
    }

    /// Publish dual-class queue snapshots for all known repositories (EXP-hotpath-queue D).
    pub(super) fn publish_queue_snapshots(
        &self,
        out: &mut BTreeMap<String, WatchRepositoryQueueSnapshot>,
    ) {
        out.clear();
        for repository_id in self.repositories.keys() {
            if let Some(snapshot) = self.repository_queue_snapshot(repository_id) {
                out.insert(repository_id.clone(), snapshot);
            }
        }
    }

    /// Age in ms of the oldest pending signal for a class, if pending (hotpath measure A).
    pub(super) fn pending_age_ms(
        &self,
        repository_id: &str,
        class: WatchRefreshClass,
        now: Instant,
    ) -> Option<u64> {
        let state = self.repositories.get(repository_id)?;
        let first = match class {
            WatchRefreshClass::ManifestFast => state.manifest_fast.first_pending_at,
            WatchRefreshClass::SemanticFollowup => state.semantic_followup.first_pending_at,
        }?;
        Some(now.saturating_duration_since(first).as_millis() as u64)
    }

    #[cfg(test)]
    pub(super) fn repository_rerun_requested(
        &self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
    ) -> bool {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return false;
        };
        self.repositories
            .get(&repository_id)
            .map(|state| match class {
                WatchRefreshClass::ManifestFast => state.manifest_fast.rerun_requested,
                WatchRefreshClass::SemanticFollowup => state.semantic_followup.rerun_requested,
            })
            .unwrap_or(false)
    }

    #[cfg(test)]
    pub(super) fn root_pending(
        &self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
    ) -> bool {
        self.repository_pending(repository_id, class)
    }

    #[cfg(test)]
    pub(super) fn root_rerun_requested(
        &self,
        repository_id: impl Into<RepositorySelector>,
        class: WatchRefreshClass,
    ) -> bool {
        self.repository_rerun_requested(repository_id, class)
    }

    fn repository_index(&self, repository_id: &str) -> Option<usize> {
        self.repository_ids_by_index
            .iter()
            .position(|candidate| candidate == repository_id)
    }

    fn resolve_repository_id(&self, selector: RepositorySelector) -> Option<String> {
        match selector {
            RepositorySelector::Index(index) => self.repository_ids_by_index.get(index).cloned(),
            RepositorySelector::Id(repository_id) => self
                .repositories
                .contains_key(&repository_id)
                .then_some(repository_id),
        }
    }

    fn in_flight_set_mut(&mut self, class: WatchRefreshClass) -> &mut BTreeSet<String> {
        match class {
            WatchRefreshClass::ManifestFast => &mut self.in_flight_manifest_fast,
            WatchRefreshClass::SemanticFollowup => &mut self.in_flight_semantic_followup,
        }
    }
}
