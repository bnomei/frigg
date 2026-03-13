use std::sync::{Arc, RwLock};
use std::time::Duration;

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{Instant, MissedTickBehavior};
use tracing::{info, warn};

use crate::domain::{FriggError, FriggResult};
use crate::indexer::{
    ReindexMode, reindex_repository_with_runtime_config,
    reindex_repository_with_runtime_config_and_dirty_paths,
};
use crate::manifest_validation::ValidatedManifestCandidateCache;
use crate::mcp::RuntimeTaskRegistry;
use crate::mcp::types::{RuntimeTaskKind, RuntimeTaskStatus};
use crate::settings::{
    FriggConfig, RuntimeTransportKind, SemanticRuntimeConfig, SemanticRuntimeCredentials,
};

use super::repository::{
    WatchedRepository, build_watched_repositories, event_kind_is_relevant,
    repository_index_for_path, should_ignore_watch_path, startup_refresh_status,
};
use super::scheduler::{ScheduledRefresh, WatchRefreshClass, WatchSchedulerState};

const WATCH_TICK_MS: u64 = 50;

pub type RepositoryCacheInvalidationCallback = Arc<dyn Fn(&str) + Send + Sync + 'static>;

enum SupervisorCommand {
    Event(Event),
    InitialSync {
        root_idx: usize,
        class: WatchRefreshClass,
    },
    ReindexCompleted {
        root_idx: usize,
        class: WatchRefreshClass,
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
    task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<RepositoryCacheInvalidationCallback>,
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
        task_registry,
        validated_manifest_candidate_cache,
        repository_cache_invalidation_callback,
        command_rx,
        command_tx.clone(),
    ));

    for (root_idx, repository) in repositories.iter().enumerate() {
        let startup_status =
            startup_refresh_status(repository, &semantic_runtime, &semantic_credentials)?;
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
            refresh_class = %startup_status
                .refresh_class
                .unwrap_or(WatchRefreshClass::ManifestFast)
                .as_str(),
            startup_reason = %startup_status.reason,
            snapshot_id = startup_status.snapshot_id.as_deref().unwrap_or("-"),
            debounce_ms = watch_config.debounce_ms,
            "built-in watch mode queued initial refresh"
        );
        let _ = command_tx.send(SupervisorCommand::InitialSync {
            root_idx,
            class: startup_status
                .refresh_class
                .unwrap_or(WatchRefreshClass::ManifestFast),
        });
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
    task_registry: Arc<RwLock<RuntimeTaskRegistry>>,
    validated_manifest_candidate_cache: Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<RepositoryCacheInvalidationCallback>,
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
                    SupervisorCommand::Event(event) => handle_notify_event(
                        &repositories,
                        &mut scheduler,
                        &validated_manifest_candidate_cache,
                        repository_cache_invalidation_callback.as_ref(),
                        event,
                        now,
                        debounce,
                    ),
                    SupervisorCommand::InitialSync { root_idx, class } => {
                        scheduler.enqueue_initial_sync(root_idx, class, now)
                    }
                    SupervisorCommand::ReindexCompleted {
                        root_idx,
                        class,
                        result,
                    } => {
                        handle_reindex_completed(
                            &repositories,
                            &mut scheduler,
                            root_idx,
                            class,
                            result,
                            repository_cache_invalidation_callback.as_ref(),
                            now,
                            retry,
                            &semantic_runtime,
                            &semantic_credentials,
                        );
                    }
                }
            }
            _ = ticker.tick() => {}
        }

        let now = Instant::now();
        if let Some(ScheduledRefresh { root_idx, class }) = scheduler.next_ready_refresh(now) {
            let recent_paths = scheduler.mark_started(root_idx, class);
            let Some(repository) = repositories.get(root_idx).cloned() else {
                warn!(root_idx, "built-in watch mode resolved invalid root index");
                continue;
            };
            if class == WatchRefreshClass::SemanticFollowup
                && task_registry
                    .read()
                    .expect("watch runtime task registry poisoned")
                    .has_active_task_for_repository(
                        RuntimeTaskKind::SemanticRefresh,
                        &repository.repository_id,
                    )
            {
                scheduler.mark_succeeded(root_idx, class, now);
                continue;
            }
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                refresh_class = %class.as_str(),
                debounce_ms = watch_config.debounce_ms,
                "built-in watch mode starting refresh"
            );
            let task_id = task_registry
                .write()
                .expect("watch runtime task registry poisoned")
                .start_task(
                    watch_task_kind_for_class(class),
                    repository.repository_id.clone(),
                    watch_task_phase_for_class(class),
                    Some(format!(
                        "watch root {} class {}",
                        repository.root.display(),
                        class.as_str()
                    )),
                );
            let completion_tx = command_tx.clone();
            let semantic_runtime = semantic_runtime.clone();
            let semantic_credentials = semantic_credentials.clone();
            let task_registry: Arc<RwLock<RuntimeTaskRegistry>> = Arc::clone(&task_registry);
            let validated_manifest_candidate_cache =
                Arc::clone(&validated_manifest_candidate_cache);
            let recent_paths = recent_paths;
            tokio::task::spawn_blocking(move || {
                let result = match class {
                    WatchRefreshClass::ManifestFast => {
                        let mut lexical_only_runtime = semantic_runtime.clone();
                        lexical_only_runtime.enabled = false;
                        reindex_repository_with_runtime_config_and_dirty_paths(
                            &repository.repository_id,
                            &repository.root,
                            &repository.db_path,
                            ReindexMode::ChangedOnly,
                            &lexical_only_runtime,
                            &semantic_credentials,
                            &recent_paths,
                        )
                    }
                    WatchRefreshClass::SemanticFollowup => reindex_repository_with_runtime_config(
                        &repository.repository_id,
                        &repository.root,
                        &repository.db_path,
                        ReindexMode::Full,
                        &semantic_runtime,
                        &semantic_credentials,
                    ),
                }
                .map_err(|err| err.to_string());
                let detail = result.as_ref().err().cloned();
                let status = if result.is_ok() {
                    RuntimeTaskStatus::Succeeded
                } else {
                    RuntimeTaskStatus::Failed
                };
                if result.is_ok() && class == WatchRefreshClass::ManifestFast {
                    validated_manifest_candidate_cache
                        .write()
                        .expect("validated manifest candidate cache poisoned")
                        .invalidate_root(&repository.root);
                }
                task_registry
                    .write()
                    .expect("watch runtime task registry poisoned")
                    .finish_task(&task_id, status, detail);
                let _ = completion_tx.send(SupervisorCommand::ReindexCompleted {
                    root_idx,
                    class,
                    result,
                });
            });
        }
    }
}

fn watch_task_kind_for_class(class: WatchRefreshClass) -> RuntimeTaskKind {
    match class {
        WatchRefreshClass::ManifestFast => RuntimeTaskKind::ChangedReindex,
        WatchRefreshClass::SemanticFollowup => RuntimeTaskKind::SemanticRefresh,
    }
}

fn watch_task_phase_for_class(class: WatchRefreshClass) -> &'static str {
    match class {
        WatchRefreshClass::ManifestFast => "watch_manifest_fast",
        WatchRefreshClass::SemanticFollowup => "watch_semantic_followup",
    }
}

fn handle_notify_event(
    repositories: &[WatchedRepository],
    scheduler: &mut WatchSchedulerState,
    validated_manifest_candidate_cache: &Arc<RwLock<ValidatedManifestCandidateCache>>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
    event: Event,
    now: Instant,
    debounce: Duration,
) {
    if !event_kind_is_relevant(&event.kind) {
        return;
    }

    let mut invalidated_root_indices = Vec::new();
    for path in event.paths {
        let Some(root_idx) = repository_index_for_path(repositories, &path) else {
            continue;
        };
        if should_ignore_watch_path(&repositories[root_idx], &path) {
            continue;
        }

        scheduler.record_path_change(root_idx, path.clone(), now, debounce);
        validated_manifest_candidate_cache
            .write()
            .expect("validated manifest candidate cache poisoned")
            .mark_dirty_root(&repositories[root_idx].root);
        if !invalidated_root_indices.contains(&root_idx) {
            if let Some(callback) = repository_cache_invalidation_callback {
                callback(&repositories[root_idx].repository_id);
            }
            invalidated_root_indices.push(root_idx);
        }
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
    class: WatchRefreshClass,
    result: Result<crate::indexer::ReindexSummary, String>,
    repository_cache_invalidation_callback: Option<&RepositoryCacheInvalidationCallback>,
    now: Instant,
    retry: Duration,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
) {
    let Some(repository) = repositories.get(root_idx) else {
        warn!(
            root_idx,
            "built-in watch mode completed for unknown root index"
        );
        return;
    };

    match result {
        Ok(summary) => {
            scheduler.mark_succeeded(root_idx, class, now);
            if let Some(callback) = repository_cache_invalidation_callback {
                callback(&repository.repository_id);
            }
            info!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                refresh_class = %class.as_str(),
                snapshot_id = %summary.snapshot_id,
                files_scanned = summary.files_scanned,
                files_changed = summary.files_changed,
                files_deleted = summary.files_deleted,
                duration_ms = summary.duration_ms,
                "built-in watch mode refresh succeeded"
            );
            if class == WatchRefreshClass::ManifestFast {
                queue_semantic_followup_if_needed(
                    repository,
                    scheduler,
                    root_idx,
                    now,
                    semantic_runtime,
                    semantic_credentials,
                );
            }
        }
        Err(error) => {
            scheduler.mark_failed(root_idx, class, now, retry);
            warn!(
                repository_id = %repository.repository_id,
                root = %repository.root.display(),
                refresh_class = %class.as_str(),
                retry_ms = retry.as_millis(),
                error = %error,
                "built-in watch mode refresh failed; retry scheduled"
            );
        }
    }
}

fn queue_semantic_followup_if_needed(
    repository: &WatchedRepository,
    scheduler: &mut WatchSchedulerState,
    root_idx: usize,
    now: Instant,
    semantic_runtime: &SemanticRuntimeConfig,
    semantic_credentials: &SemanticRuntimeCredentials,
) {
    let Ok(status) = startup_refresh_status(repository, semantic_runtime, semantic_credentials)
    else {
        return;
    };
    if status.refresh_class != Some(WatchRefreshClass::SemanticFollowup) {
        return;
    }

    scheduler.enqueue_semantic_followup(root_idx, now);
    info!(
        repository_id = %repository.repository_id,
        root = %repository.root.display(),
        startup_reason = %status.reason,
        snapshot_id = status.snapshot_id.as_deref().unwrap_or("-"),
        "built-in watch mode queued semantic follow-up after manifest refresh"
    );
}
