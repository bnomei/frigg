use super::*;

#[derive(Debug, Clone)]
pub(super) struct RootWatchState {
    pending: bool,
    last_event_at: Option<Instant>,
    debounce_deadline: Option<Instant>,
    retry_deadline: Option<Instant>,
    in_flight: bool,
    rerun_requested: bool,
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
        self.pending = true;
        if self.retry_deadline.is_some() && !self.in_flight {
            return;
        }
        self.debounce_deadline = Some(now + debounce);
        if self.in_flight {
            self.rerun_requested = true;
        }
    }

    fn enqueue_initial_sync(&mut self, now: Instant) {
        self.pending = true;
        self.last_event_at = Some(now);
        self.debounce_deadline = Some(now);
        self.retry_deadline = None;
    }

    fn mark_started(&mut self) {
        self.pending = false;
        self.in_flight = true;
        self.debounce_deadline = None;
        self.retry_deadline = None;
    }

    fn mark_succeeded(&mut self, now: Instant) {
        self.in_flight = false;
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
        self.in_flight = false;
        self.rerun_requested = false;
        self.debounce_deadline = None;
        self.retry_deadline = Some(now + retry);
    }

    fn ready_at(&self) -> Option<Instant> {
        if self.in_flight || !self.pending {
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
pub(super) struct WatchSchedulerState {
    roots: Vec<RootWatchState>,
    active_root: Option<usize>,
}

impl WatchSchedulerState {
    pub(super) fn new(root_count: usize) -> Self {
        Self {
            roots: vec![
                RootWatchState {
                    pending: false,
                    last_event_at: None,
                    debounce_deadline: None,
                    retry_deadline: None,
                    in_flight: false,
                    rerun_requested: false,
                    recent_paths: VecDeque::new(),
                };
                root_count
            ],
            active_root: None,
        }
    }

    pub(super) fn enqueue_initial_sync(&mut self, root_idx: usize, now: Instant) {
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.enqueue_initial_sync(now);
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

    pub(super) fn next_ready_root(&self, now: Instant) -> Option<usize> {
        if self.active_root.is_some() {
            return None;
        }

        self.roots
            .iter()
            .enumerate()
            .filter_map(|(idx, state)| state.ready_at().map(|ready_at| (idx, ready_at)))
            .filter(|(_, ready_at)| *ready_at <= now)
            .min_by_key(|(_, ready_at)| *ready_at)
            .map(|(idx, _)| idx)
    }

    pub(super) fn mark_started(&mut self, root_idx: usize) {
        self.active_root = Some(root_idx);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_started();
        }
    }

    pub(super) fn mark_succeeded(&mut self, root_idx: usize, now: Instant) {
        self.active_root = self
            .active_root
            .filter(|active_root| *active_root != root_idx);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_succeeded(now);
        }
    }

    pub(super) fn mark_failed(&mut self, root_idx: usize, now: Instant, retry: Duration) {
        self.active_root = self
            .active_root
            .filter(|active_root| *active_root != root_idx);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_failed(now, retry);
        }
    }

    #[cfg(test)]
    pub(super) fn root_pending(&self, root_idx: usize) -> bool {
        self.roots.get(root_idx).is_some_and(|state| state.pending)
    }

    #[cfg(test)]
    pub(super) fn root_rerun_requested(&self, root_idx: usize) -> bool {
        self.roots
            .get(root_idx)
            .is_some_and(|state| state.rerun_requested)
    }
}
