use std::collections::VecDeque;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ScheduledRefresh {
    pub root_idx: usize,
    pub class: WatchRefreshClass,
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
pub(super) struct RootWatchState {
    last_event_at: Option<Instant>,
    active_class: Option<WatchRefreshClass>,
    manifest_fast: RefreshQueueState,
    semantic_followup: RefreshQueueState,
    recent_paths: VecDeque<PathBuf>,
}

impl RootWatchState {
    fn push_sample(&mut self, path: PathBuf) {
        if self.recent_paths.len() == MAX_RECENT_PATH_SAMPLES {
            self.recent_paths.pop_front();
        }
        self.recent_paths.push_back(path);
    }

    fn record_event(&mut self, path: PathBuf, now: Instant, debounce: Duration) {
        self.last_event_at = Some(now);
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
            WatchRefreshClass::ManifestFast => {
                self.last_event_at = Some(now);
                self.manifest_fast.enqueue(now);
            }
            WatchRefreshClass::SemanticFollowup => {
                self.semantic_followup.enqueue(now);
            }
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

#[derive(Debug, Clone)]
pub(super) struct WatchSchedulerState {
    roots: Vec<RootWatchState>,
    in_flight_manifest_fast: usize,
    in_flight_semantic_followup: usize,
}

impl WatchSchedulerState {
    pub(super) fn new(root_count: usize) -> Self {
        Self {
            roots: vec![
                RootWatchState {
                    last_event_at: None,
                    active_class: None,
                    manifest_fast: RefreshQueueState::default(),
                    semantic_followup: RefreshQueueState::default(),
                    recent_paths: VecDeque::new(),
                };
                root_count
            ],
            in_flight_manifest_fast: 0,
            in_flight_semantic_followup: 0,
        }
    }

    pub(super) fn enqueue_initial_sync(
        &mut self,
        root_idx: usize,
        class: WatchRefreshClass,
        now: Instant,
    ) {
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.enqueue_initial_sync(class, now);
        }
    }

    pub(super) fn enqueue_semantic_followup(&mut self, root_idx: usize, now: Instant) {
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.enqueue_semantic_followup(now);
        }
    }

    pub(super) fn record_path_change(
        &mut self,
        root_idx: usize,
        path: PathBuf,
        now: Instant,
        debounce: Duration,
    ) {
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.record_event(path, now, debounce);
        }
    }

    pub(super) fn next_ready_refresh(&self, now: Instant) -> Option<ScheduledRefresh> {
        if self.in_flight_manifest_fast == 0 {
            if let Some(root_idx) =
                self.next_ready_root_for_class(now, WatchRefreshClass::ManifestFast)
            {
                return Some(ScheduledRefresh {
                    root_idx,
                    class: WatchRefreshClass::ManifestFast,
                });
            }
        }

        if self.in_flight_semantic_followup == 0 {
            if let Some(root_idx) =
                self.next_ready_root_for_class(now, WatchRefreshClass::SemanticFollowup)
            {
                return Some(ScheduledRefresh {
                    root_idx,
                    class: WatchRefreshClass::SemanticFollowup,
                });
            }
        }

        None
    }

    fn next_ready_root_for_class(&self, now: Instant, class: WatchRefreshClass) -> Option<usize> {
        self.roots
            .iter()
            .enumerate()
            .filter_map(|(idx, state)| state.ready_at(class).map(|ready_at| (idx, ready_at)))
            .filter(|(_, ready_at)| *ready_at <= now)
            .min_by_key(|(_, ready_at)| *ready_at)
            .map(|(idx, _)| idx)
    }

    pub(super) fn mark_started(
        &mut self,
        root_idx: usize,
        class: WatchRefreshClass,
    ) -> Vec<PathBuf> {
        *self.in_flight_count_mut(class) += 1;
        self.roots
            .get_mut(root_idx)
            .map(|state| state.mark_started(class))
            .unwrap_or_default()
    }

    pub(super) fn mark_succeeded(
        &mut self,
        root_idx: usize,
        class: WatchRefreshClass,
        now: Instant,
    ) {
        let in_flight = self.in_flight_count_mut(class);
        *in_flight = in_flight.saturating_sub(1);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_succeeded(class, now);
        }
    }

    pub(super) fn mark_failed(
        &mut self,
        root_idx: usize,
        class: WatchRefreshClass,
        now: Instant,
        retry: Duration,
    ) {
        let in_flight = self.in_flight_count_mut(class);
        *in_flight = in_flight.saturating_sub(1);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_failed(class, now, retry);
        }
    }

    fn in_flight_count_mut(&mut self, class: WatchRefreshClass) -> &mut usize {
        match class {
            WatchRefreshClass::ManifestFast => &mut self.in_flight_manifest_fast,
            WatchRefreshClass::SemanticFollowup => &mut self.in_flight_semantic_followup,
        }
    }

    #[cfg(test)]
    pub(super) fn root_pending(&self, root_idx: usize, class: WatchRefreshClass) -> bool {
        self.roots.get(root_idx).is_some_and(|state| match class {
            WatchRefreshClass::ManifestFast => state.manifest_fast.pending,
            WatchRefreshClass::SemanticFollowup => state.semantic_followup.pending,
        })
    }

    #[cfg(test)]
    pub(super) fn root_rerun_requested(&self, root_idx: usize, class: WatchRefreshClass) -> bool {
        self.roots.get(root_idx).is_some_and(|state| match class {
            WatchRefreshClass::ManifestFast => state.manifest_fast.rerun_requested,
            WatchRefreshClass::SemanticFollowup => state.semantic_followup.rerun_requested,
        })
    }
}
