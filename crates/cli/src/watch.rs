#[path = "watch/repository.rs"]
mod repository;
#[path = "watch/scheduler.rs"]
mod scheduler;
#[path = "watch/supervisor.rs"]
mod supervisor;
#[cfg(test)]
#[path = "watch/tests.rs"]
mod tests;

pub use supervisor::{WatchRuntime, maybe_start_watch_runtime};

#[cfg(test)]
pub(crate) use crate::workspace_ignores::build_root_ignore_matcher;
#[cfg(test)]
use repository::{
    WatchedRepository, latest_manifest_is_valid, repository_relative_watch_path,
    should_ignore_watch_path, startup_refresh_status,
};
#[cfg(test)]
use scheduler::{ScheduledRefresh, WatchRefreshClass, WatchSchedulerState};
