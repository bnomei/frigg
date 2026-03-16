use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;
use std::time::Duration;

use tokio::time::Instant;

const MAX_RECENT_PATH_SAMPLES: usize = 4;

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
    debounce_deadline: Option<Instant>,
    retry_deadline: Option<Instant>,
    rerun_requested: bool,
}

impl RefreshQueueState {
    fn enqueue(&mut self, now: Instant) {
        self.pending = true;
        self.debounce_deadline = Some(now);
        self.retry_deadline = None;
    }

    fn mark_started(&mut self) {
        self.pending = false;
        self.debounce_deadline = None;
        self.retry_deadline = None;
    }

    fn mark_succeeded(&mut self, now: Instant) {
        self.retry_deadline = None;
        if self.rerun_requested {
            self.pending = true;
            self.rerun_requested = false;
            if self.debounce_deadline.is_none() {
                self.debounce_deadline = Some(now);
            }
        } else {
            self.pending = false;
            self.debounce_deadline = None;
        }
    }

    fn mark_failed(&mut self, now: Instant, retry: Duration) {
        self.pending = true;
        self.rerun_requested = false;
        self.debounce_deadline = None;
        self.retry_deadline = Some(now + retry);
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

#[derive(Debug, Clone)]
pub(super) struct RepositoryWatchState {
    active_class: Option<WatchRefreshClass>,
    manifest_fast: RefreshQueueState,
    semantic_followup: RefreshQueueState,
    recent_paths: VecDeque<PathBuf>,
}

impl RepositoryWatchState {
    fn push_sample(&mut self, path: PathBuf) {
        if self.recent_paths.len() == MAX_RECENT_PATH_SAMPLES {
            self.recent_paths.pop_front();
        }
        self.recent_paths.push_back(path);
    }

    fn record_event(&mut self, path: PathBuf, now: Instant, debounce: Duration) {
        self.push_sample(path);
        self.manifest_fast.pending = true;
        if self.manifest_fast.retry_deadline.is_some()
            && self.active_class != Some(WatchRefreshClass::ManifestFast)
        {
            return;
        }
        self.manifest_fast.debounce_deadline = Some(now + debounce);
        if self.active_class == Some(WatchRefreshClass::ManifestFast) {
            self.manifest_fast.rerun_requested = true;
        }
        if self.active_class != Some(WatchRefreshClass::SemanticFollowup) {
            self.semantic_followup = RefreshQueueState::default();
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
                self.recent_paths.drain(..).collect()
            }
            WatchRefreshClass::SemanticFollowup => {
                self.semantic_followup.mark_started();
                Vec::new()
            }
        }
    }

    fn mark_succeeded(&mut self, class: WatchRefreshClass, now: Instant) {
        self.active_class = self.active_class.filter(|active| *active != class);
        match class {
            WatchRefreshClass::ManifestFast => self.manifest_fast.mark_succeeded(now),
            WatchRefreshClass::SemanticFollowup => self.semantic_followup.mark_succeeded(now),
        }
    }

    fn mark_failed(&mut self, class: WatchRefreshClass, now: Instant, retry: Duration) {
        self.active_class = self.active_class.filter(|active| *active != class);
        match class {
            WatchRefreshClass::ManifestFast => self.manifest_fast.mark_failed(now, retry),
            WatchRefreshClass::SemanticFollowup => self.semantic_followup.mark_failed(now, retry),
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
}

impl WatchSchedulerState {
    pub(super) fn new(root_count: usize) -> Self {
        let mut scheduler = Self::default();
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
        if self.in_flight_manifest_fast.is_empty()
            && let Some(repository_id) =
                self.next_ready_repository_for_class(now, WatchRefreshClass::ManifestFast)
        {
            return Some(ScheduledRefresh {
                root_idx: self.repository_index(&repository_id).unwrap_or(usize::MAX),
                repository_id,
                class: WatchRefreshClass::ManifestFast,
            });
        }

        if self.in_flight_semantic_followup.is_empty()
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
    ) {
        let Some(repository_id) = self.resolve_repository_id(repository_id.into()) else {
            return;
        };
        self.in_flight_set_mut(class).remove(&repository_id);
        if let Some(state) = self.repositories.get_mut(&repository_id) {
            state.mark_failed(class, now, retry);
        }
    }

    #[cfg(test)]
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
