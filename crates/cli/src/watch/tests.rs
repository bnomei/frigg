use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{Event, EventKind};

use super::*;
use crate::indexer::{
    ManifestStore, ReindexMode, reindex_repository, reindex_repository_with_runtime_config,
};
use crate::manifest_validation::ValidatedManifestCandidateCache;
use crate::mcp::RuntimeTaskRegistry;
use crate::searcher::{SearchFilters, SearchTextQuery, TextSearcher};
use crate::settings::{
    FriggConfig, RuntimeTransportKind, SemanticRuntimeConfig, SemanticRuntimeCredentials,
    WatchConfig, WatchMode,
};
use crate::storage::{Storage, ensure_provenance_db_parent_dir};
use tokio::time::Instant;

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

fn test_runtime_task_registry() -> Arc<RwLock<RuntimeTaskRegistry>> {
    Arc::new(RwLock::new(RuntimeTaskRegistry::new()))
}

fn test_validated_manifest_candidate_cache() -> Arc<RwLock<ValidatedManifestCandidateCache>> {
    Arc::new(RwLock::new(ValidatedManifestCandidateCache::default()))
}

fn test_repository_cache_invalidation_log() -> Arc<RwLock<Vec<String>>> {
    Arc::new(RwLock::new(Vec::new()))
}

fn test_repository_cache_invalidation_callback(
    log: Arc<RwLock<Vec<String>>>,
) -> RepositoryCacheInvalidationCallback {
    Arc::new(move |repository_id: &str| {
        log.write()
            .expect("test repository cache invalidation log poisoned")
            .push(repository_id.to_owned());
    })
}

fn init_storage(workspace_root: &Path) -> PathBuf {
    let db_path =
        ensure_provenance_db_parent_dir(workspace_root).expect("db path should be creatable");
    Storage::new(&db_path)
        .initialize()
        .expect("storage should initialize");
    db_path
}

fn test_attached_workspace(
    workspace_root: &Path,
    db_path: &Path,
) -> crate::mcp::workspace_registry::AttachedWorkspace {
    crate::mcp::workspace_registry::AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("watch-test-workspace")
            .to_owned(),
        root: workspace_root.to_path_buf(),
        db_path: db_path.to_path_buf(),
    }
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

async fn wait_for_repository_invalidation_count(
    log: &Arc<RwLock<Vec<String>>>,
    expected_minimum: usize,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if log
            .read()
            .expect("test repository cache invalidation log poisoned")
            .len()
            >= expected_minimum
        {
            return true;
        }

        if tokio::time::Instant::now() >= deadline {
            return false;
        }

        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

fn delete_retrieval_projection_family(
    db_path: &Path,
    repository_id: &str,
    snapshot_id: &str,
    family: &str,
) {
    let conn = rusqlite::Connection::open(db_path)
        .expect("test db should open for retrieval projection family deletion");
    conn.execute(
        "DELETE FROM retrieval_projection_head WHERE repository_id = ?1 AND snapshot_id = ?2 AND family = ?3",
        (repository_id, snapshot_id, family),
    )
    .expect("retrieval projection head should delete for test setup");
    if family == "path_relation" {
        conn.execute(
            "DELETE FROM path_relation_projection WHERE repository_id = ?1 AND snapshot_id = ?2",
            (repository_id, snapshot_id),
        )
        .expect("path relation rows should delete for test setup");
    }
}

async fn wait_for_retrieval_projection_family(
    db_path: &Path,
    repository_id: &str,
    snapshot_id: &str,
    family: &str,
    timeout: Duration,
) -> bool {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let storage = Storage::new(db_path);
        if storage
            .load_retrieval_projection_head_for_repository_snapshot_family(
                repository_id,
                snapshot_id,
                family,
            )
            .expect("retrieval projection head query should succeed")
            .is_some()
        {
            return true;
        }

        if tokio::time::Instant::now() >= deadline {
            return false;
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
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(749)),
        None
    );
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(750)),
        Some(ScheduledRefresh {
            root_idx: 0,
            repository_id: "repo-000".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );

    let started_paths = scheduler.mark_started(0, WatchRefreshClass::ManifestFast);
    assert_eq!(started_paths, vec![PathBuf::from("one.rs")]);
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(750)),
        None
    );

    scheduler.mark_succeeded(
        0,
        WatchRefreshClass::ManifestFast,
        now + Duration::from_millis(760),
    );
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(760)),
        Some(ScheduledRefresh {
            root_idx: 1,
            repository_id: "repo-001".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
}

#[test]
fn scheduler_coalesces_rerun_when_event_arrives_in_flight() {
    let mut scheduler = WatchSchedulerState::new(1);
    let now = Instant::now();
    let debounce = Duration::from_millis(750);

    scheduler.record_path_change(0, PathBuf::from("one.rs"), now, debounce);
    let started_paths = scheduler.mark_started(0, WatchRefreshClass::ManifestFast);
    assert_eq!(started_paths, vec![PathBuf::from("one.rs")]);
    scheduler.record_path_change(
        0,
        PathBuf::from("one.rs"),
        now + Duration::from_millis(100),
        debounce,
    );
    assert!(scheduler.root_rerun_requested(0, WatchRefreshClass::ManifestFast));

    scheduler.mark_succeeded(
        0,
        WatchRefreshClass::ManifestFast,
        now + Duration::from_millis(200),
    );
    assert!(scheduler.root_pending(0, WatchRefreshClass::ManifestFast));
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(849)),
        None
    );
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(850)),
        Some(ScheduledRefresh {
            root_idx: 0,
            repository_id: "repo-000".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
}

#[test]
fn scheduler_failure_schedules_retry_without_parallel_restart() {
    let mut scheduler = WatchSchedulerState::new(1);
    let now = Instant::now();
    let retry = Duration::from_millis(5_000);

    scheduler.enqueue_initial_sync(0, WatchRefreshClass::ManifestFast, now);
    let started_paths = scheduler.mark_started(0, WatchRefreshClass::ManifestFast);
    assert!(started_paths.is_empty());
    scheduler.mark_failed(0, WatchRefreshClass::ManifestFast, now, retry);
    scheduler.record_path_change(
        0,
        PathBuf::from("retry.rs"),
        now + Duration::from_millis(100),
        Duration::from_millis(750),
    );

    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(4_999)),
        None
    );
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(5_000)),
        Some(ScheduledRefresh {
            root_idx: 0,
            repository_id: "repo-000".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
}

#[test]
fn scheduler_passes_only_current_batch_recent_paths_to_started_refresh() {
    let mut scheduler = WatchSchedulerState::new(1);
    let now = Instant::now();
    let debounce = Duration::from_millis(750);

    scheduler.record_path_change(0, PathBuf::from("one.rs"), now, debounce);
    scheduler.record_path_change(0, PathBuf::from("two.rs"), now, debounce);
    let first_started_paths = scheduler.mark_started(0, WatchRefreshClass::ManifestFast);
    assert_eq!(
        first_started_paths,
        vec![PathBuf::from("one.rs"), PathBuf::from("two.rs")]
    );

    scheduler.record_path_change(
        0,
        PathBuf::from("three.rs"),
        now + Duration::from_millis(100),
        debounce,
    );
    scheduler.mark_succeeded(
        0,
        WatchRefreshClass::ManifestFast,
        now + Duration::from_millis(200),
    );
    let second_started_paths = scheduler.mark_started(0, WatchRefreshClass::ManifestFast);
    assert_eq!(second_started_paths, vec![PathBuf::from("three.rs")]);
}

#[test]
fn scheduler_allows_manifest_fast_while_other_root_runs_semantic_followup() {
    let mut scheduler = WatchSchedulerState::new(2);
    let now = Instant::now();
    let debounce = Duration::from_millis(750);

    scheduler.enqueue_initial_sync(0, WatchRefreshClass::SemanticFollowup, now);
    scheduler.record_path_change(1, PathBuf::from("two.rs"), now, debounce);

    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(750)),
        Some(ScheduledRefresh {
            root_idx: 1,
            repository_id: "repo-001".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
    let _ = scheduler.mark_started(0, WatchRefreshClass::SemanticFollowup);
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(750)),
        Some(ScheduledRefresh {
            root_idx: 1,
            repository_id: "repo-001".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
}

#[test]
fn watch_runtime_fairness_allows_unrelated_manifest_fast_while_semantic_followup_is_active() {
    let mut scheduler = WatchSchedulerState::new(2);
    let now = Instant::now();
    let debounce = Duration::from_millis(750);

    scheduler.enqueue_initial_sync(0, WatchRefreshClass::SemanticFollowup, now);
    let started_paths = scheduler.mark_started(0, WatchRefreshClass::SemanticFollowup);
    assert!(started_paths.is_empty());

    scheduler.record_path_change(
        0,
        PathBuf::from("root-zero-during-semantic.rs"),
        now + Duration::from_millis(10),
        debounce,
    );
    scheduler.record_path_change(1, PathBuf::from("root-one.rs"), now, debounce);

    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(750)),
        Some(ScheduledRefresh {
            root_idx: 1,
            repository_id: "repo-001".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
}

#[test]
fn watch_runtime_fairness_noisy_root_rerun_does_not_starve_other_manifest_fast_work() {
    let mut scheduler = WatchSchedulerState::new(2);
    let now = Instant::now();
    let debounce = Duration::from_millis(750);

    scheduler.record_path_change(0, PathBuf::from("root-zero-first.rs"), now, debounce);
    scheduler.record_path_change(
        1,
        PathBuf::from("root-one.rs"),
        now + Duration::from_millis(10),
        debounce,
    );

    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(750)),
        Some(ScheduledRefresh {
            root_idx: 0,
            repository_id: "repo-000".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
    let started_paths = scheduler.mark_started(0, WatchRefreshClass::ManifestFast);
    assert_eq!(started_paths, vec![PathBuf::from("root-zero-first.rs")]);

    scheduler.record_path_change(
        0,
        PathBuf::from("root-zero-rerun.rs"),
        now + Duration::from_millis(100),
        debounce,
    );
    assert!(scheduler.root_rerun_requested(0, WatchRefreshClass::ManifestFast));

    scheduler.mark_succeeded(
        0,
        WatchRefreshClass::ManifestFast,
        now + Duration::from_millis(200),
    );
    assert!(scheduler.root_pending(0, WatchRefreshClass::ManifestFast));
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(759)),
        None
    );
    assert_eq!(
        scheduler.next_ready_refresh(now + Duration::from_millis(760)),
        Some(ScheduledRefresh {
            root_idx: 1,
            repository_id: "repo-001".to_owned(),
            class: WatchRefreshClass::ManifestFast,
        })
    );
}

#[test]
fn watch_path_filter_ignores_internal_roots_only() {
    let root = PathBuf::from("/tmp/frigg-root");
    let repository = WatchedRepository {
        repository_id: "repo-001".to_owned(),
        canonical_root: Some(root.clone()),
        root_ignore_matcher: build_root_ignore_matcher(&root),
        root: root.clone(),
        db_path: root.join(".frigg/storage.sqlite3"),
    };
    assert!(should_ignore_watch_path(
        &repository,
        &root.join(".frigg/storage.sqlite3")
    ));
    assert!(should_ignore_watch_path(
        &repository,
        &root.join(".git/index")
    ));
    assert!(should_ignore_watch_path(
        &repository,
        &root.join("target/debug/app")
    ));
    assert!(!should_ignore_watch_path(
        &repository,
        &root.join("contracts/errors.md")
    ));
    assert!(!should_ignore_watch_path(
        &repository,
        &root.join("crates/cli/src/main.rs")
    ));
}

#[test]
fn watch_path_filter_respects_root_gitignore_rules() {
    let root = temp_workspace_root("watch-gitignore-filter");
    fs::create_dir_all(root.join("contracts")).expect("contracts directory should exist");
    fs::create_dir_all(root.join("src")).expect("src directory should exist");
    fs::write(root.join(".gitignore"), "contracts/\n").expect("gitignore should be writable");
    fs::write(root.join("contracts/errors.md"), "# Errors\n")
        .expect("contract file should be writable");
    fs::write(root.join("src/lib.rs"), "pub fn alpha() {}\n")
        .expect("source file should be writable");

    let repository = WatchedRepository {
        repository_id: "repo-001".to_owned(),
        canonical_root: root.canonicalize().ok(),
        root_ignore_matcher: build_root_ignore_matcher(&root),
        root: root.clone(),
        db_path: root.join(".frigg/storage.sqlite3"),
    };

    assert!(should_ignore_watch_path(
        &repository,
        &root.join("contracts/errors.md")
    ));
    assert!(!should_ignore_watch_path(
        &repository,
        &root.join("src/lib.rs")
    ));

    cleanup_workspace(&root);
}

#[test]
fn repository_relative_watch_path_accepts_canonical_root_prefix() {
    let repository = WatchedRepository {
        repository_id: "repo-001".to_owned(),
        root: PathBuf::from("/var/folders/example/frigg-root"),
        canonical_root: Some(PathBuf::from("/private/var/folders/example/frigg-root")),
        root_ignore_matcher: build_root_ignore_matcher(Path::new(
            "/var/folders/example/frigg-root",
        )),
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
    store
        .initialize()
        .expect("manifest store should initialize");

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
        root_ignore_matcher: build_root_ignore_matcher(&workspace_root),
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
        root_ignore_matcher: build_root_ignore_matcher(&workspace_root),
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
    assert_eq!(
        status.refresh_class,
        Some(WatchRefreshClass::SemanticFollowup)
    );

    cleanup_workspace(&workspace_root);
}

#[test]
fn startup_refresh_status_skips_semantic_bootstrap_when_no_eligible_entries_exist() {
    let workspace_root = temp_workspace_root("startup-no-semantic-files");
    fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
    fs::write(workspace_root.join("notes.bin"), "opaque").expect("fixture file should be writable");

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
        root_ignore_matcher: build_root_ignore_matcher(&workspace_root),
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
    assert_eq!(status.refresh_class, None);

    cleanup_workspace(&workspace_root);
}

#[test]
fn startup_refresh_status_requests_manifest_refresh_for_missing_retrieval_projection_family() {
    let workspace_root = temp_workspace_root("startup-missing-retrieval-projection-family");
    fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("Cargo manifest should be writable");
    fs::create_dir_all(workspace_root.join("src")).expect("src directory should be creatable");
    fs::write(workspace_root.join("src/main.rs"), "fn main() {}\n")
        .expect("source file should be writable");
    fs::create_dir_all(workspace_root.join("tests")).expect("tests directory should be creatable");
    fs::write(
        workspace_root.join("tests/main_test.rs"),
        "#[test]\nfn main_test() {}\n",
    )
    .expect("test file should be writable");

    let db_path = init_storage(&workspace_root);
    let summary = reindex_repository_with_runtime_config(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
    )
    .expect("baseline lexical reindex should succeed");
    delete_retrieval_projection_family(&db_path, "repo-001", &summary.snapshot_id, "path_relation");

    let repository = WatchedRepository {
        repository_id: "repo-001".to_owned(),
        canonical_root: workspace_root.canonicalize().ok(),
        root_ignore_matcher: build_root_ignore_matcher(&workspace_root),
        root: workspace_root.clone(),
        db_path,
    };
    let status = startup_refresh_status(
        &repository,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
    )
    .expect("startup refresh status should resolve");
    assert!(status.should_refresh);
    assert_eq!(
        status.reason,
        "manifest_snapshot_missing_retrieval_projections"
    );
    assert_eq!(status.snapshot_id, Some(summary.snapshot_id));
    assert_eq!(status.refresh_class, Some(WatchRefreshClass::ManifestFast));

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

    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        test_runtime_task_registry(),
        test_validated_manifest_candidate_cache(),
        None,
    )
    .expect("watch runtime should start")
    .expect("watch runtime should be enabled");
    runtime
        .acquire_lease(&test_attached_workspace(&workspace_root, &db_path))
        .expect("watch lease should acquire");
    let snapshot_id = wait_for_snapshot_id(&db_path, "repo-001", Duration::from_secs(5))
        .await
        .expect("initial sync should create a manifest snapshot");
    assert!(snapshot_id.starts_with("snapshot-"));

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    cleanup_workspace(&workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn watch_runtime_initial_sync_respects_gitignored_contracts_and_excludes_target() {
    let workspace_root = temp_workspace_root("contracts-visible");
    fs::create_dir_all(workspace_root.join("contracts"))
        .expect("contracts directory should be creatable");
    fs::create_dir_all(workspace_root.join("target/debug"))
        .expect("target directory should be creatable");
    fs::write(workspace_root.join(".gitignore"), "contracts/\n")
        .expect("gitignore should be writable");
    fs::write(workspace_root.join("contracts/errors.md"), "# Errors\n")
        .expect("contract file should be writable");
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

    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        test_runtime_task_registry(),
        test_validated_manifest_candidate_cache(),
        None,
    )
    .expect("watch runtime should start")
    .expect("watch runtime should be enabled");
    runtime
        .acquire_lease(&test_attached_workspace(&workspace_root, &db_path))
        .expect("watch lease should acquire");
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
        paths
            .iter()
            .all(|path| !path.ends_with("contracts/errors.md")),
        "gitignored contract path should stay excluded: {paths:?}"
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

    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        test_runtime_task_registry(),
        test_validated_manifest_candidate_cache(),
        None,
    )
    .expect("watch runtime should start")
    .expect("watch runtime should be enabled");
    runtime
        .acquire_lease(&test_attached_workspace(&workspace_root, &db_path))
        .expect("watch lease should acquire");
    tokio::time::sleep(Duration::from_secs(1)).await;

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

    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        test_runtime_task_registry(),
        test_validated_manifest_candidate_cache(),
        None,
    )
    .expect("watch runtime should start")
    .expect("watch runtime should be enabled");
    runtime
        .acquire_lease(&test_attached_workspace(&workspace_root, &db_path))
        .expect("watch lease should acquire");

    let initial_snapshot_id = wait_for_snapshot_id(&db_path, "repo-001", Duration::from_secs(10))
        .await
        .expect("initial sync should create a manifest snapshot");

    fs::write(
        workspace_root.join("src.rs"),
        "fn alpha() {}\npub fn watch_notify_beta() {}\n// watch-notify-beta\n",
    )
    .expect("updating an existing source file should trigger notify backend");
    runtime.inject_test_event(Event::new(EventKind::Any).add_path(workspace_root.join("src.rs")));

    let next_snapshot_id = wait_for_snapshot_id_change(
        &db_path,
        "repo-001",
        &initial_snapshot_id,
        Duration::from_secs(10),
    )
    .await;
    let next_snapshot_id =
        next_snapshot_id.expect("watch-triggered reindex should advance the snapshot id");
    assert_ne!(next_snapshot_id, initial_snapshot_id);

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
            .any(|entry| { entry.path == "src.rs" && entry.excerpt.contains("watch-notify-beta") }),
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

#[tokio::test(flavor = "current_thread")]
async fn watch_runtime_invokes_repository_cache_invalidation_callback_for_initial_sync_and_notify()
{
    let workspace_root = temp_workspace_root("notify-cache-invalidation");
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

    let invalidation_log = test_repository_cache_invalidation_log();
    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        test_runtime_task_registry(),
        test_validated_manifest_candidate_cache(),
        Some(test_repository_cache_invalidation_callback(Arc::clone(
            &invalidation_log,
        ))),
    )
    .expect("watch runtime should start")
    .expect("watch runtime should be enabled");

    let attached_workspace = crate::mcp::workspace_registry::AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("notify-cache-invalidation")
            .to_owned(),
        root: workspace_root.clone(),
        db_path: db_path.clone(),
    };
    runtime
        .acquire_lease(&attached_workspace)
        .expect("watch lease should acquire");

    let initial_snapshot_id = wait_for_snapshot_id(&db_path, "repo-001", Duration::from_secs(10))
        .await
        .expect("initial sync should create a manifest snapshot");
    assert!(
        wait_for_repository_invalidation_count(&invalidation_log, 1, Duration::from_secs(10)).await,
        "initial sync should invalidate repository-scoped caches"
    );

    let initial_count = invalidation_log
        .read()
        .expect("test repository cache invalidation log poisoned")
        .len();
    fs::write(
        workspace_root.join("added.rs"),
        "pub fn watch_notify_cache_invalidation() {}\n",
    )
    .expect("creating a new source file should trigger notify backend");
    runtime.inject_test_event(Event::new(EventKind::Any).add_path(workspace_root.join("added.rs")));

    assert!(
        wait_for_repository_invalidation_count(
            &invalidation_log,
            initial_count + 1,
            Duration::from_secs(10),
        )
        .await,
        "notify-driven dirty or refresh transitions should invalidate repository-scoped caches"
    );

    let notify_count = invalidation_log
        .read()
        .expect("test repository cache invalidation log poisoned")
        .len();
    let next_snapshot_id = wait_for_snapshot_id_change(
        &db_path,
        "repo-001",
        &initial_snapshot_id,
        Duration::from_secs(10),
    )
    .await
    .expect("watch-triggered refresh should advance the snapshot id");
    assert_ne!(next_snapshot_id, initial_snapshot_id);
    assert!(
        wait_for_repository_invalidation_count(
            &invalidation_log,
            notify_count + 1,
            Duration::from_secs(10),
        )
        .await,
        "completed refresh transitions should also invalidate repository-scoped caches"
    );
    assert!(
        invalidation_log
            .read()
            .expect("test repository cache invalidation log poisoned")
            .iter()
            .all(|repository_id| repository_id == "repo-001"),
        "callback should only receive the affected repository id"
    );

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    cleanup_workspace(&workspace_root);
}

#[tokio::test(flavor = "current_thread")]
async fn watch_runtime_repairs_missing_retrieval_projection_family_and_invalidates_repository_cache()
 {
    let workspace_root = temp_workspace_root("startup-refreshes-missing-retrieval-projection");
    fs::create_dir_all(&workspace_root).expect("workspace root should be creatable");
    fs::write(
        workspace_root.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .expect("Cargo manifest should be writable");
    fs::create_dir_all(workspace_root.join("src")).expect("src directory should be creatable");
    fs::write(workspace_root.join("src/main.rs"), "fn main() {}\n")
        .expect("source file should be writable");
    fs::create_dir_all(workspace_root.join("tests")).expect("tests directory should be creatable");
    fs::write(
        workspace_root.join("tests/main_test.rs"),
        "#[test]\nfn main_test() {}\n",
    )
    .expect("test file should be writable");

    let db_path = init_storage(&workspace_root);
    let initial_summary = reindex_repository_with_runtime_config(
        "repo-001",
        &workspace_root,
        &db_path,
        ReindexMode::Full,
        &SemanticRuntimeConfig::default(),
        &SemanticRuntimeCredentials::default(),
    )
    .expect("baseline lexical reindex should succeed");

    let storage = Storage::new(&db_path);
    assert!(
        !storage
            .load_path_relation_projections_for_repository_snapshot(
                "repo-001",
                &initial_summary.snapshot_id,
            )
            .expect("path relation rows should load")
            .is_empty()
    );

    delete_retrieval_projection_family(
        &db_path,
        "repo-001",
        &initial_summary.snapshot_id,
        "path_relation",
    );
    assert_eq!(
        storage
            .missing_retrieval_projection_families_for_repository_snapshot(
                "repo-001",
                &initial_summary.snapshot_id,
            )
            .expect("missing retrieval projection family query should succeed"),
        vec!["path_relation".to_owned()]
    );

    let mut config = FriggConfig::from_workspace_roots(vec![workspace_root.clone()])
        .expect("config should load from workspace root");
    config.watch = WatchConfig {
        mode: WatchMode::On,
        debounce_ms: 25,
        retry_ms: 100,
    };

    let invalidation_log = test_repository_cache_invalidation_log();
    let runtime = maybe_start_watch_runtime(
        &config,
        RuntimeTransportKind::Stdio,
        test_runtime_task_registry(),
        test_validated_manifest_candidate_cache(),
        Some(test_repository_cache_invalidation_callback(Arc::clone(
            &invalidation_log,
        ))),
    )
    .expect("watch runtime should start")
    .expect("watch runtime should be enabled");

    let attached_workspace = crate::mcp::workspace_registry::AttachedWorkspace {
        repository_id: "repo-001".to_owned(),
        runtime_repository_id: "repo-001".to_owned(),
        display_name: workspace_root
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("startup-refreshes-missing-retrieval-projection")
            .to_owned(),
        root: workspace_root.clone(),
        db_path: db_path.clone(),
    };
    runtime
        .acquire_lease(&attached_workspace)
        .expect("watch lease should acquire");

    assert!(
        wait_for_retrieval_projection_family(
            &db_path,
            "repo-001",
            &initial_summary.snapshot_id,
            "path_relation",
            Duration::from_secs(10),
        )
        .await,
        "startup refresh should restore the missing retrieval projection head"
    );
    assert!(
        wait_for_repository_invalidation_count(&invalidation_log, 1, Duration::from_secs(10)).await,
        "startup refresh completion should invalidate repository-scoped caches"
    );

    let refreshed_snapshot_id = wait_for_snapshot_id(&db_path, "repo-001", Duration::from_secs(5))
        .await
        .expect("latest manifest snapshot should remain visible");
    assert_eq!(refreshed_snapshot_id, initial_summary.snapshot_id);
    assert!(
        !storage
            .load_path_relation_projections_for_repository_snapshot(
                "repo-001",
                &initial_summary.snapshot_id,
            )
            .expect("restored path relation rows should load")
            .is_empty()
    );
    assert!(
        storage
            .missing_retrieval_projection_families_for_repository_snapshot(
                "repo-001",
                &initial_summary.snapshot_id,
            )
            .expect("missing retrieval projection family query should succeed")
            .is_empty()
    );
    assert!(
        invalidation_log
            .read()
            .expect("test repository cache invalidation log poisoned")
            .iter()
            .all(|repository_id| repository_id == "repo-001"),
        "callback should only receive the affected repository id"
    );

    drop(runtime);
    tokio::time::sleep(Duration::from_millis(25)).await;
    cleanup_workspace(&workspace_root);
}
