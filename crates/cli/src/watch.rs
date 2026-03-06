use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::{
    FileMetadataDigest, ReindexMode, reindex_repository_with_runtime_config,
    semantic_chunk_language_for_path,
};
use crate::manifest_validation::validate_manifest_digests_for_root;
use crate::settings::{
    FriggConfig, RuntimeTransportKind, SemanticRuntimeConfig, SemanticRuntimeCredentials,
};
use crate::storage::{Storage, resolve_provenance_db_path};

const WATCH_TICK_MS: u64 = 50;
const MAX_RECENT_PATH_SAMPLES: usize = 4;

#[derive(Debug, Clone)]
struct WatchedRepository {
    repository_id: String,
    root: PathBuf,
    canonical_root: Option<PathBuf>,
    db_path: PathBuf,
}

#[derive(Debug, Clone)]
struct RootWatchState {
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
struct WatchSchedulerState {
    roots: Vec<RootWatchState>,
    active_root: Option<usize>,
}

impl WatchSchedulerState {
    fn new(root_count: usize) -> Self {
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

    fn enqueue_initial_sync(&mut self, root_idx: usize, now: Instant) {
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.enqueue_initial_sync(now);
        }
    }

    fn record_path_change(
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

    fn next_ready_root(&self, now: Instant) -> Option<usize> {
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

    fn mark_started(&mut self, root_idx: usize) {
        self.active_root = Some(root_idx);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_started();
        }
    }

    fn mark_succeeded(&mut self, root_idx: usize, now: Instant) {
        self.active_root = self.active_root.filter(|active_root| *active_root != root_idx);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_succeeded(now);
        }
    }

    fn mark_failed(&mut self, root_idx: usize, now: Instant, retry: Duration) {
        self.active_root = self.active_root.filter(|active_root| *active_root != root_idx);
        if let Some(state) = self.roots.get_mut(root_idx) {
            state.mark_failed(now, retry);
        }
    }
}

enum SupervisorCommand {
    Event(Event),
    InitialSync(usize),
    ReindexCompleted {
        root_idx: usize,
        result: Result<crate::indexer::ReindexSummary, String>,
    },
}

pub struct WatchRuntime {
    _watcher: RecommendedWatcher,
    supervisor_handle: JoinHandle<()>,
    _command_tx: mpsc::UnboundedSender<SupervisorCommand>,
}

impl Drop for WatchRuntime {
    fn drop(&mut self) {
        self.supervisor_handle.abort();
    }
}

pub fn maybe_start_watch_runtime(
    config: &FriggConfig,
    transport: RuntimeTransportKind,
) -> FriggResult<Option<WatchRuntime>> {
    if !config.watch.enabled_for_transport(transport) {
        info!(
            watch_mode = %config.watch.mode.as_str(),
            transport = ?transport,
            "built-in watch mode disabled for resolved transport"
        );
        return Ok(None);
    }

    let repositories = build_watched_repositories(config)?;
    let (command_tx, command_rx) = mpsc::unbounded_channel();
    let callback_tx = command_tx.clone();
    let mut watcher = notify::recommended_watcher(move |result| match result {
        Ok(event) => {
            let _ = callback_tx.send(SupervisorCommand::Event(event));
        }
        Err(error) => {
            warn!(error = %error, "built-in watch mode dropped notify event");
        }
    })
    .map_err(|err| FriggError::Internal(format!("failed to create filesystem watcher: {err}")))?;

    for repository in &repositories {
        watcher
            .watch(&repository.root, RecursiveMode::Recursive)
            .map_err(|err| {
                FriggError::Internal(format!(
                    "failed to register watcher for root {}: {err}",
                    repository.root.display()
                ))
            })?;
        info!(
            repository_id = %repository.repository_id,
            root = %repository.root.display(),
            "built-in watch mode registered workspace root"
        );
    }

    let watch_config = config.watch.clone();
    let semantic_runtime = config.semantic_runtime.clone();
    let semantic_credentials = SemanticRuntimeCredentials::from_process_env();
    let repositories = Arc::new(repositories);
    let supervisor_handle = tokio::spawn(run_supervisor(
        repositories.clone(),
        watch_config.clone(),
        semantic_runtime.clone(),
        semantic_credentials.clone(),
        command_rx,
        command_tx.clone(),
    ));

    for (root_idx, repository) in repositories.iter().enumerate() {
        let startup_status = startup_refresh_status(
            repository,
            &semantic_runtime,
            &semantic_credentials,
        )?;
        if !startup_status.should_refresh {
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                snapshot_id = startup_status.snapshot_id.as_deref().unwrap_or("-"),
                "built-in watch mode found refreshable startup state already satisfied"
            );
            continue;
        }

        info!(
            repository_id = %repository.repository_id,
            root = %repository.root.display(),
            startup_reason = %startup_status.reason,
            snapshot_id = startup_status.snapshot_id.as_deref().unwrap_or("-"),
            debounce_ms = watch_config.debounce_ms,
            "built-in watch mode queued initial changed reindex"
        );
        let _ = command_tx.send(SupervisorCommand::InitialSync(root_idx));
    }

    Ok(Some(WatchRuntime {
        _watcher: watcher,
        supervisor_handle,
        _command_tx: command_tx,
    }))
}

async fn run_supervisor(
    repositories: Arc<Vec<WatchedRepository>>,
    watch_config: crate::settings::WatchConfig,
    semantic_runtime: SemanticRuntimeConfig,
    semantic_credentials: SemanticRuntimeCredentials,
    mut command_rx: mpsc::UnboundedReceiver<SupervisorCommand>,
    command_tx: mpsc::UnboundedSender<SupervisorCommand>,
) {
    let debounce = Duration::from_millis(watch_config.debounce_ms);
    let retry = Duration::from_millis(watch_config.retry_ms);
    let mut scheduler = WatchSchedulerState::new(repositories.len());
    let mut ticker = tokio::time::interval(Duration::from_millis(WATCH_TICK_MS));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe_command = command_rx.recv() => {
                let Some(command) = maybe_command else {
                    break;
                };
                let now = Instant::now();
                match command {
                    SupervisorCommand::Event(event) => handle_notify_event(&repositories, &mut scheduler, event, now, debounce),
                    SupervisorCommand::InitialSync(root_idx) => scheduler.enqueue_initial_sync(root_idx, now),
                    SupervisorCommand::ReindexCompleted { root_idx, result } => {
                        handle_reindex_completed(
                            &repositories,
                            &mut scheduler,
                            root_idx,
                            result,
                            now,
                            retry,
                        );
                    }
                }
            }
            _ = ticker.tick() => {}
        }

        let now = Instant::now();
        if let Some(root_idx) = scheduler.next_ready_root(now) {
            scheduler.mark_started(root_idx);
            let Some(repository) = repositories.get(root_idx).cloned() else {
                warn!(root_idx, "built-in watch mode resolved invalid root index");
                continue;
            };
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                debounce_ms = watch_config.debounce_ms,
                "built-in watch mode starting changed reindex"
            );
            let completion_tx = command_tx.clone();
            let semantic_runtime = semantic_runtime.clone();
            let semantic_credentials = semantic_credentials.clone();
            tokio::task::spawn_blocking(move || {
                let result = reindex_repository_with_runtime_config(
                    &repository.repository_id,
                    &repository.root,
                    &repository.db_path,
                    ReindexMode::ChangedOnly,
                    &semantic_runtime,
                    &semantic_credentials,
                )
                .map_err(|err| err.to_string());
                let _ = completion_tx.send(SupervisorCommand::ReindexCompleted { root_idx, result });
            });
        }
    }
}

fn handle_notify_event(
    repositories: &[WatchedRepository],
    scheduler: &mut WatchSchedulerState,
    event: Event,
    now: Instant,
    debounce: Duration,
) {
    if !event_kind_is_relevant(&event.kind) {
        return;
    }

    for path in event.paths {
        let Some(root_idx) = repository_index_for_path(repositories, &path) else {
            continue;
        };
        if should_ignore_watch_path(&repositories[root_idx], &path) {
            info!(
                repository_id = %repositories[root_idx].repository_id,
                path = %path.display(),
                "built-in watch mode ignored internal path change"
            );
            continue;
        }

        scheduler.record_path_change(root_idx, path.clone(), now, debounce);
        info!(
            repository_id = %repositories[root_idx].repository_id,
            path = %path.display(),
            "built-in watch mode accepted path change"
        );
    }
}

fn handle_reindex_completed(
    repositories: &[WatchedRepository],
    scheduler: &mut WatchSchedulerState,
    root_idx: usize,
    result: Result<crate::indexer::ReindexSummary, String>,
    now: Instant,
    retry: Duration,
) {
    let Some(repository) = repositories.get(root_idx) else {
        warn!(root_idx, "built-in watch mode completed for unknown root index");
        return;
    };

    match result {
        Ok(summary) => {
            scheduler.mark_succeeded(root_idx, now);
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                snapshot_id = %summary.snapshot_id,
                files_scanned = summary.files_scanned,
                files_changed = summary.files_changed,
                files_deleted = summary.files_deleted,
                duration_ms = summary.duration_ms,
                "built-in watch mode changed reindex succeeded"
            );
        }
        Err(error) => {
            scheduler.mark_failed(root_idx, now, retry);
            warn!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                retry_ms = retry.as_millis(),
                error = %error,
                "built-in watch mode changed reindex failed; retry scheduled"
            );
        }
    }
}

fn build_watched_repositories(config: &FriggConfig) -> FriggResult<Vec<WatchedRepository>> {
    config
        .repositories()
        .into_iter()
        .map(|repository| {
            let root = PathBuf::from(&repository.root_path);
            let db_path = resolve_provenance_db_path(&root)?;
            Ok(WatchedRepository {
                repository_id: repository.repository_id.0,
                canonical_root: root.canonicalize().ok(),
                root,
                db_path,
            })
        })
        .collect()
}

#[cfg(test)]
fn latest_manifest_is_valid(repository: &WatchedRepository) -> FriggResult<bool> {
    let storage = Storage::new(&repository.db_path);
    let latest = storage.load_latest_manifest_for_repository(&repository.repository_id)?;
    let Some(snapshot) = latest else {
        return Ok(false);
    };
    let digests = snapshot
        .entries
        .iter()
        .map(|entry| FileMetadataDigest {
            path: PathBuf::from(&entry.path),
            size_bytes: entry.size_bytes,
            mtime_ns: entry.mtime_ns,
        })
        .collect::<Vec<_>>();
    Ok(validate_manifest_digests_for_root(&repository.root, &digests).is_some())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StartupRefreshStatus {
    should_refresh: bool,
    reason: &'static str,
    snapshot_id: Option<String>,
}

fn startup_refresh_status(
    repository: &WatchedRepository,
    semantic_runtime: &SemanticRuntimeConfig,
    credentials: &SemanticRuntimeCredentials,
) -> FriggResult<StartupRefreshStatus> {
    let storage = Storage::new(&repository.db_path);
    let latest = storage.load_latest_manifest_for_repository(&repository.repository_id)?;
    let Some(snapshot) = latest else {
        return Ok(StartupRefreshStatus {
            should_refresh: true,
            reason: "missing_manifest_snapshot",
            snapshot_id: None,
        });
    };
    let snapshot_id = snapshot.snapshot_id.clone();

    let digests = snapshot
        .entries
        .iter()
        .map(|entry| FileMetadataDigest {
            path: PathBuf::from(&entry.path),
            size_bytes: entry.size_bytes,
            mtime_ns: entry.mtime_ns,
        })
        .collect::<Vec<_>>();
    if validate_manifest_digests_for_root(&repository.root, &digests).is_none() {
        return Ok(StartupRefreshStatus {
            should_refresh: true,
            reason: "stale_manifest_snapshot",
            snapshot_id: Some(snapshot_id),
        });
    }

    if !semantic_runtime.enabled {
        return Ok(StartupRefreshStatus {
            should_refresh: false,
            reason: "manifest_valid",
            snapshot_id: Some(snapshot_id),
        });
    }

    semantic_runtime
        .validate_startup(credentials)
        .map_err(|err| FriggError::InvalidInput(format!("{err}")))?;
    let provider = semantic_runtime.provider.ok_or_else(|| {
        FriggError::Internal("semantic runtime provider missing after validation".to_owned())
    })?;
    let model = semantic_runtime.normalized_model().ok_or_else(|| {
        FriggError::Internal("semantic runtime model missing after validation".to_owned())
    })?;

    let has_semantic_eligible_entries = snapshot.entries.iter().any(|entry| {
        let path = PathBuf::from(&entry.path);
        !should_ignore_watch_path(repository, &path)
            && semantic_chunk_language_for_path(Path::new(&entry.path)).is_some()
    });
    if !has_semantic_eligible_entries {
        return Ok(StartupRefreshStatus {
            should_refresh: false,
            reason: "manifest_valid_no_semantic_eligible_entries",
            snapshot_id: Some(snapshot_id),
        });
    }

    let has_rows = storage.has_semantic_embeddings_for_repository_snapshot_model(
        &repository.repository_id,
        &snapshot.snapshot_id,
        provider.as_str(),
        model,
    )?;
    Ok(StartupRefreshStatus {
        should_refresh: !has_rows,
        reason: if has_rows {
            "manifest_and_semantic_snapshot_valid"
        } else {
            "semantic_snapshot_missing_for_active_model"
        },
        snapshot_id: Some(snapshot.snapshot_id),
    })
}

fn event_kind_is_relevant(kind: &EventKind) -> bool {
    !matches!(kind, EventKind::Access(_))
}

fn repository_index_for_path(repositories: &[WatchedRepository], path: &Path) -> Option<usize> {
    repositories
        .iter()
        .enumerate()
        .filter(|(_, repository)| repository_relative_watch_path(repository, path).is_some())
        .max_by_key(|(_, repository)| repository.root.components().count())
        .map(|(idx, _)| idx)
}

fn repository_relative_watch_path<'a>(
    repository: &'a WatchedRepository,
    path: &'a Path,
) -> Option<PathBuf> {
    if !path.is_absolute() {
        return Some(path.to_path_buf());
    }

    if let Ok(relative) = path.strip_prefix(&repository.root) {
        return Some(relative.to_path_buf());
    }

    if let Some(canonical_root) = repository.canonical_root.as_deref() {
        if let Ok(relative) = path.strip_prefix(canonical_root) {
            return Some(relative.to_path_buf());
        }
        if let Ok(canonical_path) = path.canonicalize() {
            if let Ok(relative) = canonical_path.strip_prefix(canonical_root) {
                return Some(relative.to_path_buf());
            }
        }
    }

    None
}

fn should_ignore_watch_path(repository: &WatchedRepository, path: &Path) -> bool {
    let Some(relative) = repository_relative_watch_path(repository, path) else {
        return true;
    };
    let Some(component) = relative.components().next() else {
        return false;
    };
    let component = component.as_os_str().to_string_lossy();
    matches!(component.as_ref(), ".frigg" | ".git" | "target")
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;
    use crate::indexer::{ManifestStore, ReindexMode, reindex_repository};
    use crate::searcher::{SearchFilters, SearchTextQuery, TextSearcher};
    use crate::settings::{FriggConfig, WatchConfig, WatchMode};
    use crate::storage::ensure_provenance_db_parent_dir;

    fn temp_workspace_root(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("frigg-watch-{test_name}-{unique}"))
    }

    fn cleanup_workspace(path: &Path) {
        if path.exists() {
            fs::remove_dir_all(path).expect("temp watch workspace should be removable");
        }
    }

    fn init_storage(workspace_root: &Path) -> PathBuf {
        let db_path =
            ensure_provenance_db_parent_dir(workspace_root).expect("db path should be creatable");
        Storage::new(&db_path)
            .initialize()
            .expect("storage should initialize");
        db_path
    }

    async fn wait_for_snapshot_id(
        db_path: &Path,
        repository_id: &str,
        timeout: Duration,
    ) -> Option<String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(snapshot) = Storage::new(db_path)
                .load_latest_manifest_for_repository(repository_id)
                .expect("latest manifest query should succeed")
            {
                return Some(snapshot.snapshot_id);
            }

            if tokio::time::Instant::now() >= deadline {
                return None;
            }

            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn wait_for_snapshot_id_change(
        db_path: &Path,
        repository_id: &str,
        previous_snapshot_id: &str,
        timeout: Duration,
    ) -> Option<String> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if let Some(snapshot) = Storage::new(db_path)
                .load_latest_manifest_for_repository(repository_id)
                .expect("latest manifest query should succeed")
            {
                if snapshot.snapshot_id != previous_snapshot_id {
                    return Some(snapshot.snapshot_id);
                }
            }

            if tokio::time::Instant::now() >= deadline {
                return None;
            }

            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[test]
    fn scheduler_debounces_roots_and_serializes_execution() {
        let mut scheduler = WatchSchedulerState::new(2);
        let now = Instant::now();
        let debounce = Duration::from_millis(750);

        scheduler.record_path_change(0, PathBuf::from("one.rs"), now, debounce);
        scheduler.record_path_change(1, PathBuf::from("two.rs"), now, debounce);
        assert_eq!(scheduler.next_ready_root(now + Duration::from_millis(749)), None);
        assert_eq!(scheduler.next_ready_root(now + Duration::from_millis(750)), Some(0));

        scheduler.mark_started(0);
        assert_eq!(scheduler.next_ready_root(now + Duration::from_millis(750)), None);

        scheduler.mark_succeeded(0, now + Duration::from_millis(760));
        assert_eq!(scheduler.next_ready_root(now + Duration::from_millis(760)), Some(1));
    }

    #[test]
    fn scheduler_coalesces_rerun_when_event_arrives_in_flight() {
        let mut scheduler = WatchSchedulerState::new(1);
        let now = Instant::now();
        let debounce = Duration::from_millis(750);

        scheduler.record_path_change(0, PathBuf::from("one.rs"), now, debounce);
        scheduler.mark_started(0);
        scheduler.record_path_change(
            0,
            PathBuf::from("one.rs"),
            now + Duration::from_millis(100),
            debounce,
        );
        assert!(scheduler.roots[0].rerun_requested);

        scheduler.mark_succeeded(0, now + Duration::from_millis(200));
        assert!(scheduler.roots[0].pending);
        assert_eq!(
            scheduler.next_ready_root(now + Duration::from_millis(849)),
            None
        );
        assert_eq!(
            scheduler.next_ready_root(now + Duration::from_millis(850)),
            Some(0)
        );
    }

    #[test]
    fn scheduler_failure_schedules_retry_without_parallel_restart() {
        let mut scheduler = WatchSchedulerState::new(1);
        let now = Instant::now();
        let retry = Duration::from_millis(5_000);

        scheduler.enqueue_initial_sync(0, now);
        scheduler.mark_started(0);
        scheduler.mark_failed(0, now, retry);
        scheduler.record_path_change(
            0,
            PathBuf::from("retry.rs"),
            now + Duration::from_millis(100),
            Duration::from_millis(750),
        );

        assert_eq!(
            scheduler.next_ready_root(now + Duration::from_millis(4_999)),
            None
        );
        assert_eq!(
            scheduler.next_ready_root(now + Duration::from_millis(5_000)),
            Some(0)
        );
    }

    #[test]
    fn watch_path_filter_ignores_internal_roots_only() {
        let root = PathBuf::from("/tmp/frigg-root");
        let repository = WatchedRepository {
            repository_id: "repo-001".to_owned(),
            canonical_root: Some(root.clone()),
            root: root.clone(),
            db_path: root.join(".frigg/storage.sqlite3"),
        };
        assert!(should_ignore_watch_path(
            &repository,
            &root.join(".frigg/storage.sqlite3")
        ));
        assert!(should_ignore_watch_path(&repository, &root.join(".git/index")));
        assert!(should_ignore_watch_path(
            &repository,
            &root.join("target/debug/app")
        ));
        assert!(!should_ignore_watch_path(
            &repository,
            &root.join("docs/contracts/errors.md")
        ));
        assert!(!should_ignore_watch_path(
            &repository,
            &root.join("crates/cli/src/main.rs")
        ));
    }

    #[test]
    fn repository_relative_watch_path_accepts_canonical_root_prefix() {
        let repository = WatchedRepository {
            repository_id: "repo-001".to_owned(),
            root: PathBuf::from("/var/folders/example/frigg-root"),
            canonical_root: Some(PathBuf::from("/private/var/folders/example/frigg-root")),
            db_path: PathBuf::from("/var/folders/example/frigg-root/.frigg/storage.sqlite3"),
        };

        let relative = repository_relative_watch_path(
            &repository,
            Path::new("/private/var/folders/example/frigg-root/src/lib.rs"),
        )
        .expect("canonical-root event path should map back to the repository");
        assert_eq!(relative, PathBuf::from("src/lib.rs"));
    }

    #[test]
    fn latest_manifest_validation_requires_present_fresh_snapshot() {
        let workspace_root = temp_workspace_root("manifest-validity");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        let source_path = workspace_root.join("src.rs");
        fs::write(&source_path, "fn alpha() {}\n").expect("source file should be writable");

        let db_path = crate::storage::ensure_provenance_db_parent_dir(&workspace_root)
            .expect("db path should be creatable");
        let store = ManifestStore::new(&db_path);
        store.initialize().expect("manifest store should initialize");

        let entries = vec![crate::indexer::FileDigest {
            path: source_path.clone(),
            size_bytes: fs::metadata(&source_path)
                .expect("source metadata should resolve")
                .len(),
            mtime_ns: fs::metadata(&source_path)
                .expect("source metadata should resolve")
                .modified()
                .ok()
                .and_then(crate::manifest_validation::system_time_to_unix_nanos),
            hash_blake3_hex: "abc".to_owned(),
        }];
        store
            .persist_snapshot_manifest("repo-001", "snapshot-test", &entries)
            .expect("snapshot should persist");

        let repository = WatchedRepository {
            repository_id: "repo-001".to_owned(),
            canonical_root: workspace_root.canonicalize().ok(),
            root: workspace_root.clone(),
            db_path: db_path.clone(),
        };
        assert!(latest_manifest_is_valid(&repository).expect("fresh snapshot should validate"));

        fs::write(&source_path, "fn beta() {}\n").expect("source file should be writable");
        assert!(
            !latest_manifest_is_valid(&repository).expect("modified file should invalidate snapshot")
        );

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn startup_refresh_status_requests_semantic_bootstrap_for_valid_manifest_without_rows() {
        let workspace_root = temp_workspace_root("startup-semantic-bootstrap");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::write(workspace_root.join("src.rs"), "pub fn alpha() {}\n")
            .expect("source file should be writable");

        let db_path = init_storage(&workspace_root);
        reindex_repository_with_runtime_config(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
        )
        .expect("baseline lexical reindex should succeed");

        let repository = WatchedRepository {
            repository_id: "repo-001".to_owned(),
            canonical_root: workspace_root.canonicalize().ok(),
            root: workspace_root.clone(),
            db_path,
        };
        let status = startup_refresh_status(
            &repository,
            &SemanticRuntimeConfig {
                enabled: true,
                provider: Some(crate::settings::SemanticRuntimeProvider::OpenAi),
                model: None,
                strict_mode: false,
            },
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
        )
        .expect("startup refresh status should resolve");
        assert!(status.should_refresh);
        assert_eq!(status.reason, "semantic_snapshot_missing_for_active_model");

        cleanup_workspace(&workspace_root);
    }

    #[test]
    fn startup_refresh_status_skips_semantic_bootstrap_when_no_eligible_entries_exist() {
        let workspace_root = temp_workspace_root("startup-no-semantic-files");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::write(workspace_root.join("notes.bin"), "opaque")
            .expect("fixture file should be writable");

        let db_path = init_storage(&workspace_root);
        reindex_repository_with_runtime_config(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
            &SemanticRuntimeConfig::default(),
            &SemanticRuntimeCredentials::default(),
        )
        .expect("baseline lexical reindex should succeed");

        let repository = WatchedRepository {
            repository_id: "repo-001".to_owned(),
            canonical_root: workspace_root.canonicalize().ok(),
            root: workspace_root.clone(),
            db_path,
        };
        let status = startup_refresh_status(
            &repository,
            &SemanticRuntimeConfig {
                enabled: true,
                provider: Some(crate::settings::SemanticRuntimeProvider::OpenAi),
                model: None,
                strict_mode: false,
            },
            &SemanticRuntimeCredentials {
                openai_api_key: Some("test-openai-key".to_owned()),
                gemini_api_key: None,
            },
        )
        .expect("startup refresh status should resolve");
        assert!(!status.should_refresh);
        assert_eq!(status.reason, "manifest_valid_no_semantic_eligible_entries");

        cleanup_workspace(&workspace_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn watch_runtime_initial_sync_reindexes_when_manifest_missing() {
        let workspace_root = temp_workspace_root("initial-sync");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::write(workspace_root.join("src.rs"), "fn alpha() {}\n")
            .expect("source file should be writable");

        let db_path = init_storage(&workspace_root);
        let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from workspace root");
        config.watch = WatchConfig {
            mode: WatchMode::On,
            debounce_ms: 25,
            retry_ms: 100,
        };

        let runtime = maybe_start_watch_runtime(&config, RuntimeTransportKind::Stdio)
            .expect("watch runtime should start")
            .expect("watch runtime should be enabled");
        let snapshot_id = wait_for_snapshot_id(&db_path, "repo-001", Duration::from_secs(5))
            .await
            .expect("initial sync should create a manifest snapshot");
        assert!(snapshot_id.starts_with("snapshot-"));

        drop(runtime);
        tokio::time::sleep(Duration::from_millis(25)).await;
        cleanup_workspace(&workspace_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn watch_runtime_initial_sync_preserves_docs_visibility_and_target_exclusion() {
        let workspace_root = temp_workspace_root("docs-visible");
        fs::create_dir_all(workspace_root.join("docs/contracts"))
            .expect("docs directory should be creatable");
        fs::create_dir_all(workspace_root.join("target/debug"))
            .expect("target directory should be creatable");
        fs::write(workspace_root.join(".gitignore"), "docs/\n")
            .expect("gitignore should be writable");
        fs::write(
            workspace_root.join("docs/contracts/errors.md"),
            "# Errors\n",
        )
        .expect("docs contract file should be writable");
        fs::write(workspace_root.join("target/debug/app"), "binary")
            .expect("target artifact should be writable");

        let db_path = init_storage(&workspace_root);
        let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from workspace root");
        config.watch = WatchConfig {
            mode: WatchMode::On,
            debounce_ms: 25,
            retry_ms: 100,
        };

        let runtime = maybe_start_watch_runtime(&config, RuntimeTransportKind::Stdio)
            .expect("watch runtime should start")
            .expect("watch runtime should be enabled");
        wait_for_snapshot_id(&db_path, "repo-001", Duration::from_secs(5))
            .await
            .expect("initial sync should create a manifest snapshot");

        let manifest = Storage::new(&db_path)
            .load_latest_manifest_for_repository("repo-001")
            .expect("latest manifest query should succeed")
            .expect("manifest snapshot should exist");
        let paths = manifest
            .entries
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();
        assert!(
            paths.iter().any(|path| path.ends_with("docs/contracts/errors.md")),
            "docs contract path should remain indexed: {paths:?}"
        );
        assert!(
            paths.iter().all(|path| !path.starts_with("target/")),
            "target artifacts must stay excluded: {paths:?}"
        );

        drop(runtime);
        tokio::time::sleep(Duration::from_millis(25)).await;
        cleanup_workspace(&workspace_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn watch_runtime_startup_skips_initial_sync_for_valid_manifest() {
        let workspace_root = temp_workspace_root("startup-valid");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        fs::write(workspace_root.join("src.rs"), "fn alpha() {}\n")
            .expect("source file should be writable");

        let db_path = init_storage(&workspace_root);
        let summary = reindex_repository(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
        )
        .expect("baseline changed-only reindex should succeed");

        let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from workspace root");
        config.watch = WatchConfig {
            mode: WatchMode::On,
            debounce_ms: 25,
            retry_ms: 100,
        };

        let runtime = maybe_start_watch_runtime(&config, RuntimeTransportKind::Stdio)
            .expect("watch runtime should start")
            .expect("watch runtime should be enabled");
        tokio::time::sleep(Duration::from_millis(250)).await;

        let latest = Storage::new(&db_path)
            .load_latest_manifest_for_repository("repo-001")
            .expect("latest manifest query should succeed")
            .expect("baseline manifest should exist");
        assert_eq!(latest.snapshot_id, summary.snapshot_id);

        drop(runtime);
        tokio::time::sleep(Duration::from_millis(25)).await;
        cleanup_workspace(&workspace_root);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn watch_runtime_notify_backend_reindexes_after_real_file_change() {
        let workspace_root = temp_workspace_root("notify-reindex");
        fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
        let source_path = workspace_root.join("src.rs");
        fs::write(&source_path, "fn alpha() {}\n").expect("source file should be writable");

        let db_path = init_storage(&workspace_root);
        let summary = reindex_repository(
            "repo-001",
            &workspace_root,
            &db_path,
            ReindexMode::ChangedOnly,
        )
        .expect("baseline changed-only reindex should succeed");

        let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
            .expect("config should load from workspace root");
        config.watch = WatchConfig {
            mode: WatchMode::On,
            debounce_ms: 25,
            retry_ms: 100,
        };

        let runtime = maybe_start_watch_runtime(&config, RuntimeTransportKind::Stdio)
            .expect("watch runtime should start")
            .expect("watch runtime should be enabled");
        tokio::time::sleep(Duration::from_millis(250)).await;

        let created_path = workspace_root.join("added.rs");
        fs::write(&created_path, "pub fn watch_notify_beta() {}\n// watch-notify-beta\n")
            .expect("creating a new source file should trigger notify backend");

        let next_snapshot_id = wait_for_snapshot_id_change(
            &db_path,
            "repo-001",
            &summary.snapshot_id,
            Duration::from_secs(5),
        )
        .await
        .expect("watch-triggered reindex should advance the snapshot id");
        assert_ne!(next_snapshot_id, summary.snapshot_id);

        let searcher = TextSearcher::new(
            FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
                .expect("search config should load from workspace root"),
        );
        let matches = searcher
            .search_literal_with_filters(
                SearchTextQuery {
                    query: "watch-notify-beta".to_owned(),
                    path_regex: None,
                    limit: 5,
                },
                SearchFilters::default(),
            )
            .expect("literal search should succeed after watch-triggered reindex");
        assert!(
            matches
                .iter()
                .any(|entry| {
                    entry.path == "added.rs" && entry.excerpt.contains("watch-notify-beta")
                }),
            "query path should observe the post-reindex file contents: {:?}",
            matches
                .iter()
                .map(|entry| (entry.path.clone(), entry.excerpt.clone()))
                .collect::<Vec<_>>()
        );

        drop(runtime);
        tokio::time::sleep(Duration::from_millis(25)).await;
        cleanup_workspace(&workspace_root);
    }
}
